import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";

import {
  DEFAULT_LIBRARY_PAGE_SIZE,
  SEARCH_DEBOUNCE_MS,
  hasActiveFilters,
  useLibraryStore,
} from "@/stores/library-store";
import type { LibraryItem, LibraryQuery, LibraryStats } from "@/types/library";

/**
 * What these tests hold the store to: the shape of what crosses IPC.
 *
 * The SQL that turns a `LibraryQuery` into rows is the backend's problem and is
 * covered by its own Rust tests. What can only break here is the *request*: a
 * query that drops a filter the user is still looking at, a page that asks for
 * all 10.000 rows at once, or one IPC round trip per keystroke.
 */

interface Recorded {
  command: string;
  args: Record<string, unknown>;
}

let recorded: Recorded[] = [];
/** Everything the fake backend "has"; `list_library` pages through it. */
let backendItems: LibraryItem[] = [];

function makeItem(overrides: Partial<LibraryItem> = {}): LibraryItem {
  return {
    id: "item-1",
    file_path: "/downloads/song.mp3",
    title: "Song",
    media_type: "audio",
    file_format: "mp3",
    file_size_bytes: 1024,
    duration_seconds: null,
    platform: "youtube",
    source_url: "https://youtube.com/watch?v=abc",
    thumbnail_path: null,
    downloaded_at: "2026-07-01T10:00:00Z",
    is_missing: false,
    job_id: "job-1",
    ...overrides,
  };
}

/** Statistics the way the backend computes them: *through* the query it was
 * handed. A fake that ignored the filter would make "the count describes the
 * page" impossible to get wrong. */
function statsFor(items: LibraryItem[]): LibraryStats {
  const group = (pick: (item: LibraryItem) => string) => {
    const totals = new Map<string, { item_count: number; total_size_bytes: number }>();
    for (const item of items) {
      const bucket = totals.get(pick(item)) ?? { item_count: 0, total_size_bytes: 0 };
      bucket.item_count += 1;
      bucket.total_size_bytes += item.file_size_bytes;
      totals.set(pick(item), bucket);
    }
    return [...totals].map(([key, bucket]) => ({ key, ...bucket }));
  };

  return {
    total_items: items.length,
    total_size_bytes: items.reduce((sum, item) => sum + item.file_size_bytes, 0),
    missing_items: items.filter((item) => item.is_missing).length,
    by_platform: group((item) => item.platform),
    by_media_type: group((item) => item.media_type),
    formats: [...new Set(items.map((item) => item.file_format))],
  };
}

/** The rows a query selects, before paging. Only the filters these tests
 * exercise are honoured — the rest is the backend's SQL and its own tests. */
function matching(query: LibraryQuery): LibraryItem[] {
  return backendItems.filter((item) => {
    if (query.platforms?.length && !query.platforms.includes(item.platform)) return false;
    if (query.media_types?.length && !query.media_types.includes(item.media_type)) return false;
    if (query.search && !item.title.toLowerCase().includes(query.search.toLowerCase()))
      return false;
    return true;
  });
}

/** `Array.prototype.at` is ES2022 and `tsconfig.json` targets ES2020. */
function last<T>(values: T[]): T {
  return values[values.length - 1];
}

/** Waits for a reload the store kicked off without handing back a promise
 * (`togglePlatform`, `setSort`, the debounced search). `loading` is set
 * synchronously by the caller, so it is already `true` by the time we get here
 * and this cannot pass on the *previous* load. */
async function settled(): Promise<void> {
  await vi.waitFor(() => expect(useLibraryStore.getState().loading).toBe(false));
}

function queriesFor(command: string): LibraryQuery[] {
  return recorded
    .filter((call) => call.command === command)
    .map((call) => call.args.query as LibraryQuery);
}

function resetStore() {
  useLibraryStore.setState({
    items: [],
    stats: null,
    facets: null,
    searchInput: "",
    filters: {
      search: "",
      media_types: [],
      platforms: [],
      formats: [],
      downloaded_from: null,
      downloaded_to: null,
      missing_only: false,
    },
    sort: "downloaded_at",
    direction: "desc",
    viewMode: "grid",
    selectedIds: [],
    loading: false,
    page: 1,
    pageSize: DEFAULT_LIBRARY_PAGE_SIZE,
    totalItems: 0,
    reconciling: false,
    error: null,
    initialized: false,
  });
}

beforeEach(() => {
  recorded = [];
  backendItems = [makeItem({ id: "a" }), makeItem({ id: "b" }), makeItem({ id: "c" })];
  localStorage.clear();
  resetStore();

  vi.mocked(invoke).mockImplementation(async (command: string, args?: unknown) => {
    const parameters = (args ?? {}) as Record<string, unknown>;
    recorded.push({ command, args: parameters });

    if (command === "list_library") {
      const query = (parameters.query ?? {}) as LibraryQuery;
      const rows = matching(query);
      const offset = query.offset ?? 0;
      const limit = query.limit ?? rows.length;
      return rows.slice(offset, offset + limit) as never;
    }
    if (command === "library_stats") {
      return statsFor(matching((parameters.query ?? {}) as LibraryQuery)) as never;
    }
    if (command === "delete_library_items" || command === "remove_library_items") {
      // A real delete makes the row stop existing; a fake that kept it would
      // let a page reload quietly resurrect it.
      const removed = new Set(parameters.itemIds as string[]);
      backendItems = backendItems.filter((item) => !removed.has(item.id));
      return removed.size as never;
    }
    return undefined as never;
  });
});

afterEach(() => {
  vi.useRealTimers();
});

describe("library store — searching (FR-307)", () => {
  it("asks the backend once for a burst of keystrokes, not once per key", async () => {
    vi.useFakeTimers();
    const { setSearch } = useLibraryStore.getState();

    for (const partial of ["p", "po", "pod", "podc", "podca", "podcas", "podcast"]) {
      setSearch(partial);
      // Faster than the debounce window, i.e. someone actually typing.
      await vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE_MS / 3);
    }

    // Still nothing on the wire: the user has not stopped typing yet.
    expect(queriesFor("list_library")).toHaveLength(0);
    // But the input is already showing every character.
    expect(useLibraryStore.getState().searchInput).toBe("podcast");

    await vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE_MS);

    const queries = queriesFor("list_library");
    expect(queries).toHaveLength(1);
    expect(queries[0].search).toBe("podcast");
    // The intermediate prefixes must never have been asked for.
    expect(queries.map((query) => query.search)).not.toContain("podc");
  });

  it("does not re-query when the debounced term ends up unchanged", async () => {
    vi.useFakeTimers();
    const { setSearch } = useLibraryStore.getState();

    setSearch("jazz");
    await vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE_MS);
    expect(queriesFor("list_library")).toHaveLength(1);

    setSearch("jazzy");
    setSearch("jazz");
    await vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE_MS);
    expect(queriesFor("list_library")).toHaveLength(1);
  });

  it("sends null rather than an empty search so the backend adds no LIKE clause", async () => {
    await useLibraryStore.getState().reload();
    expect(queriesFor("list_library")[0].search).toBeNull();
  });
});

describe("library store — filters (FR-308)", () => {
  it("accumulates every active filter into one query instead of replacing them", async () => {
    const store = useLibraryStore.getState();

    store.toggleMediaType("audio");
    store.togglePlatform("youtube");
    store.toggleFormat("mp3");
    store.setFilters({ downloaded_from: "2026-01-01", downloaded_to: "2026-06-30" });
    useLibraryStore.setState((state) => ({ filters: { ...state.filters, search: "live" } }));
    await useLibraryStore.getState().reload();

    const query = last(queriesFor("list_library"));
    expect(query.media_types).toEqual(["audio"]);
    expect(query.platforms).toEqual(["youtube"]);
    expect(query.formats).toEqual(["mp3"]);
    expect(query.search).toBe("live");
    // A date filter is a range over `completed_at`, not a day equal to a
    // timestamp: the upper bound has to reach the end of its day or "up to
    // today" excludes everything downloaded today.
    expect(query.downloaded_from).toBe("2026-01-01T00:00:00Z");
    expect(query.downloaded_to).toBe("2026-06-30T23:59:59Z");
  });

  it("adds a second value to a filter rather than swapping the first one out", async () => {
    const store = useLibraryStore.getState();
    store.togglePlatform("youtube");
    store.togglePlatform("tiktok");
    await useLibraryStore.getState().reload();

    expect(last(queriesFor("list_library")).platforms).toEqual(["youtube", "tiktok"]);

    useLibraryStore.getState().togglePlatform("youtube");
    await useLibraryStore.getState().reload();
    expect(last(queriesFor("list_library")).platforms).toEqual(["tiktok"]);
  });

  it("counts as filtering only when something is actually narrowed", () => {
    const none = useLibraryStore.getState().filters;
    expect(hasActiveFilters(none)).toBe(false);
    expect(hasActiveFilters({ ...none, search: "   " })).toBe(false);
    expect(hasActiveFilters({ ...none, search: "a" })).toBe(true);
    expect(hasActiveFilters({ ...none, formats: ["mp3"] })).toBe(true);
    expect(hasActiveFilters({ ...none, missing_only: true })).toBe(true);
    expect(hasActiveFilters({ ...none, downloaded_to: "2026-01-01" })).toBe(true);
  });

  it("measures the statistics through the same filters as the list (SC-307)", async () => {
    const unfiltered = statsFor(backendItems).by_platform;
    // The unfiltered first load is where the filter choices come from.
    await useLibraryStore.getState().reload();
    expect(useLibraryStore.getState().facets?.by_platform).toEqual(unfiltered);

    useLibraryStore.getState().togglePlatform("tiktok");
    await useLibraryStore.getState().reload();

    expect(last(queriesFor("library_stats")).platforms).toEqual(["tiktok"]);
    // Nothing in the fixture is from TikTok, so the *measured* breakdown is
    // now empty…
    expect(useLibraryStore.getState().stats?.by_platform).toEqual([]);
    // …but the filter *choices* stay the unfiltered snapshot, or picking one
    // platform would delete every other platform from the filter itself.
    expect(useLibraryStore.getState().facets?.by_platform).toEqual(unfiltered);
  });
});

describe("library store — numbered paging (FR-310)", () => {
  /** The user's real library, in the shape that makes the page bar interesting:
   * 66 rows is four pages at the default size, not one and not twenty. */
  function fillBackend(count: number) {
    backendItems = Array.from({ length: count }, (_unused, index) =>
      makeItem({ id: `i${index}`, title: `Track ${index}` }),
    );
  }

  it("never asks for the whole library in one call", async () => {
    fillBackend(10_000);

    await useLibraryStore.getState().reload();

    const first = queriesFor("list_library")[0];
    expect(first.limit).toBe(DEFAULT_LIBRARY_PAGE_SIZE);
    expect(first.offset).toBe(0);
    expect(useLibraryStore.getState().items).toHaveLength(DEFAULT_LIBRARY_PAGE_SIZE);
    expect(useLibraryStore.getState().page).toBe(1);
    // The whole point of a numbered bar: it knows how many pages there are
    // before the user has walked to the end of them.
    expect(useLibraryStore.getState().totalItems).toBe(10_000);
  });

  it("asks page N for the rows page N covers, not for everything up to it", async () => {
    fillBackend(66);

    await useLibraryStore.getState().reload();
    await useLibraryStore.getState().setPage(4);

    const query = last(queriesFor("list_library"));
    expect(query.offset).toBe(3 * DEFAULT_LIBRARY_PAGE_SIZE);
    expect(query.limit).toBe(DEFAULT_LIBRARY_PAGE_SIZE);
    // Page four of 66 rows is the leftover six, and it holds only those six —
    // an infinite-scroll store would be sitting on all 66 by now.
    expect(useLibraryStore.getState().items.map((item) => item.id)).toEqual([
      "i60",
      "i61",
      "i62",
      "i63",
      "i64",
      "i65",
    ]);
    expect(useLibraryStore.getState().page).toBe(4);
  });

  it("pages by the size the user picked", async () => {
    fillBackend(66);
    await useLibraryStore.getState().reload();

    await useLibraryStore.getState().setPageSize(10);
    await useLibraryStore.getState().setPage(3);

    const query = last(queriesFor("list_library"));
    expect(query.limit).toBe(10);
    expect(query.offset).toBe(20);
    expect(useLibraryStore.getState().items.map((item) => item.id)[0]).toBe("i20");
  });

  it("takes the count from the same query it took the page from", async () => {
    fillBackend(66);
    backendItems.push(
      makeItem({ id: "tt", title: "TikTok one", platform: "tiktok" }),
      makeItem({ id: "tt2", title: "TikTok two", platform: "tiktok" }),
    );
    await useLibraryStore.getState().reload();

    useLibraryStore.getState().togglePlatform("tiktok");
    await settled();

    const page = last(queriesFor("list_library"));
    const count = last(queriesFor("library_stats"));
    // Every filter that shaped the page also shaped the count: a total taken
    // through a different question would promise pages the list cannot fill.
    expect(count.platforms).toEqual(page.platforms);
    expect(count.media_types).toEqual(page.media_types);
    expect(count.search).toEqual(page.search);
    expect(count.downloaded_from).toEqual(page.downloaded_from);
    expect(count.downloaded_to).toEqual(page.downloaded_to);
    expect(count.is_missing).toEqual(page.is_missing);
    // …and only the window differs, because a limited count would count the
    // window instead of the library.
    expect(count.limit).toBeNull();
    expect(count.offset).toBeNull();
    // The filtered total is the two TikTok rows, not all 68.
    expect(useLibraryStore.getState().totalItems).toBe(2);
  });

  it("starts over at page one when the question changes", async () => {
    fillBackend(66);
    await useLibraryStore.getState().reload();
    await useLibraryStore.getState().setPage(3);
    expect(useLibraryStore.getState().page).toBe(3);

    useLibraryStore.getState().togglePlatform("youtube");
    await settled();

    expect(last(queriesFor("list_library")).platforms).toEqual(["youtube"]);
    // "Page 3 of the previous question" points at nothing in particular.
    expect(last(queriesFor("list_library")).offset).toBe(0);
    expect(useLibraryStore.getState().page).toBe(1);
  });

  it("starts over at page one when the sort order changes", async () => {
    fillBackend(66);
    await useLibraryStore.getState().reload();
    await useLibraryStore.getState().setPage(3);

    useLibraryStore.getState().setSort("title", "asc");
    await settled();

    expect(last(queriesFor("list_library")).sort).toBe("title");
    expect(last(queriesFor("list_library")).offset).toBe(0);
    expect(useLibraryStore.getState().page).toBe(1);
  });

  it("falls back to the last page that still exists when the list shrinks underneath it", async () => {
    fillBackend(66);
    await useLibraryStore.getState().reload();
    await useLibraryStore.getState().setPage(4);
    expect(useLibraryStore.getState().page).toBe(4);

    // Something else trimmed the library — another window, a cleanup, a
    // reconcile that dropped rows. Page 4 no longer exists.
    backendItems = backendItems.slice(0, 25);
    await useLibraryStore.getState().reload();

    expect(useLibraryStore.getState().page).toBe(2);
    expect(last(queriesFor("list_library")).offset).toBe(DEFAULT_LIBRARY_PAGE_SIZE);
    // And it landed on rows, not on the empty tail of a page that is gone.
    expect(useLibraryStore.getState().items).toHaveLength(5);
  });

  it("keeps the last page filled after a delete instead of leaving a hole", async () => {
    fillBackend(66);
    await useLibraryStore.getState().reload();
    await useLibraryStore.getState().setPage(2);

    await useLibraryStore.getState().deleteItems(["i20", "i21"]);

    // Still page 2, but refilled from what follows: 64 rows left, so a full
    // page of 20 rather than the 18 that were on screen a moment ago.
    expect(useLibraryStore.getState().page).toBe(2);
    expect(useLibraryStore.getState().items).toHaveLength(DEFAULT_LIBRARY_PAGE_SIZE);
    expect(useLibraryStore.getState().items.map((item) => item.id)).not.toContain("i20");
    expect(useLibraryStore.getState().totalItems).toBe(64);
  });

  it("drops the selection when the page changes so a bulk action cannot reach off-screen rows", async () => {
    fillBackend(66);
    await useLibraryStore.getState().reload();
    useLibraryStore.getState().toggleSelected("i0");

    await useLibraryStore.getState().setPage(2);

    expect(useLibraryStore.getState().selectedIds).toEqual([]);
    expect(
      useLibraryStore
        .getState()
        .selectionInDisplayOrder()
        .map((item) => item.id),
    ).not.toContain("i0");
  });
});

describe("library store — refused writes (FR-322)", () => {
  it("surfaces FILE_EXISTS as an error and leaves the item exactly as it was", async () => {
    await useLibraryStore.getState().reload();
    const before = useLibraryStore.getState().items;

    vi.mocked(invoke).mockRejectedValueOnce({
      code: "FILE_EXISTS",
      message: "A file already exists at /downloads/taken.mp3",
    });

    const renamed = await useLibraryStore.getState().renameItem("a", "taken.mp3");

    expect(renamed).toBe(false);
    expect(useLibraryStore.getState().error?.code).toBe("FILE_EXISTS");
    // Not "resolved" into taken (2).mp3, not applied anyway: nothing moved.
    expect(useLibraryStore.getState().items).toEqual(before);
  });

  it("keeps a rejected batch move out of the list entirely", async () => {
    await useLibraryStore.getState().reload();
    const before = useLibraryStore.getState().items;

    vi.mocked(invoke).mockRejectedValueOnce({ code: "FILE_EXISTS", message: "taken" });
    const moved = await useLibraryStore.getState().moveItems(["a", "b"], "/elsewhere");

    expect(moved).toBe(false);
    expect(useLibraryStore.getState().items).toEqual(before);
  });

  it("wraps a non-AppError rejection so the banner still has a code to translate", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("boom"));
    await useLibraryStore.getState().deleteItems(["a"]);
    expect(useLibraryStore.getState().error?.code).toBe("INTERNAL");
  });
});

describe("library store — reconciliation (FR-323, FR-327)", () => {
  it("repaints only the ids the reconcile pass reported as changed", async () => {
    backendItems = [
      makeItem({ id: "a", is_missing: false }),
      makeItem({ id: "b", is_missing: true }),
      makeItem({ id: "c", is_missing: false }),
    ];
    await useLibraryStore.getState().reload();

    useLibraryStore.getState().applyReconciled(["a", "b"]);

    const byId = Object.fromEntries(
      useLibraryStore.getState().items.map((item) => [item.id, item.is_missing]),
    );
    expect(byId).toEqual({ a: true, b: false, c: false });
  });

  it("does not scan the library as part of listing it", async () => {
    await useLibraryStore.getState().reload();
    expect(recorded.map((call) => call.command)).not.toContain("reconcile_library");
  });
});

describe("library store — selection and export order (FR-330)", () => {
  it("hands over the selection in display order, not in the order it was clicked", async () => {
    await useLibraryStore.getState().reload();
    const store = useLibraryStore.getState();

    store.toggleSelected("c");
    store.toggleSelected("a");

    expect(useLibraryStore.getState().selectedIds).toEqual(["c", "a"]);
    expect(
      useLibraryStore
        .getState()
        .selectionInDisplayOrder()
        .map((item) => item.id),
    ).toEqual(["a", "c"]);
  });

  it("falls back to everything on display when nothing is ticked", async () => {
    await useLibraryStore.getState().reload();
    expect(
      useLibraryStore
        .getState()
        .selectionInDisplayOrder()
        .map((item) => item.id),
    ).toEqual(["a", "b", "c"]);
  });

  it("drops deleted ids from the selection so a stale id cannot be exported", async () => {
    await useLibraryStore.getState().reload();
    useLibraryStore.getState().toggleSelected("b");

    await useLibraryStore.getState().deleteItems(["b"]);

    expect(useLibraryStore.getState().selectedIds).toEqual([]);
    expect(useLibraryStore.getState().items.map((item) => item.id)).toEqual(["a", "c"]);
  });
});

describe("library store — remembered preferences (FR-306, FR-309)", () => {
  it("writes the view mode and sort choice down", () => {
    useLibraryStore.getState().setViewMode("list");
    useLibraryStore.getState().setSort("title", "asc");

    expect(localStorage.getItem("library.view_mode")).toBe("list");
    expect(localStorage.getItem("library.sort")).toBe("title");
    expect(localStorage.getItem("library.direction")).toBe("asc");
  });

  it("starts the next session from what was written down", async () => {
    localStorage.setItem("library.view_mode", "list");
    localStorage.setItem("library.sort", "file_size");
    localStorage.setItem("library.direction", "asc");

    vi.resetModules();
    const fresh = await import("@/stores/library-store");

    expect(fresh.useLibraryStore.getState().viewMode).toBe("list");
    expect(fresh.useLibraryStore.getState().sort).toBe("file_size");
    expect(fresh.useLibraryStore.getState().direction).toBe("asc");
  });

  it("ignores a stored value that is not a sort the app has", async () => {
    localStorage.setItem("library.sort", "whatever");

    vi.resetModules();
    const fresh = await import("@/stores/library-store");

    expect(fresh.useLibraryStore.getState().sort).toBe("downloaded_at");
  });
});
