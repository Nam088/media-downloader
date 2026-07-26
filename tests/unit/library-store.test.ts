import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";

import {
  LIBRARY_PAGE_SIZE,
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

const STATS: LibraryStats = {
  total_items: 3,
  total_size_bytes: 3072,
  missing_items: 1,
  by_platform: [{ key: "youtube", item_count: 3, total_size_bytes: 3072 }],
  by_media_type: [{ key: "audio", item_count: 3, total_size_bytes: 3072 }],
  formats: ["mp3"],
};

/** `Array.prototype.at` is ES2022 and `tsconfig.json` targets ES2020. */
function last<T>(values: T[]): T {
  return values[values.length - 1];
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
    loadingMore: false,
    hasMore: false,
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
      const offset = query.offset ?? 0;
      const limit = query.limit ?? backendItems.length;
      return backendItems.slice(offset, offset + limit) as never;
    }
    if (command === "library_stats") return STATS as never;
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
    // The unfiltered first load is where the filter choices come from.
    await useLibraryStore.getState().reload();
    expect(useLibraryStore.getState().facets?.by_platform).toEqual(STATS.by_platform);

    useLibraryStore.getState().togglePlatform("tiktok");
    await useLibraryStore.getState().reload();

    expect(last(queriesFor("library_stats")).platforms).toEqual(["tiktok"]);
    // …but the filter *choices* stay the unfiltered snapshot, or picking one
    // platform would delete every other platform from the filter itself.
    expect(useLibraryStore.getState().facets?.by_platform).toEqual(STATS.by_platform);
  });
});

describe("library store — paging (FR-310)", () => {
  it("never asks for the whole library in one call", async () => {
    backendItems = Array.from({ length: 10_000 }, (_unused, index) =>
      makeItem({ id: `item-${index}` }),
    );

    await useLibraryStore.getState().reload();

    const first = queriesFor("list_library")[0];
    expect(first.limit).toBe(LIBRARY_PAGE_SIZE);
    expect(first.offset).toBe(0);
    expect(useLibraryStore.getState().items).toHaveLength(LIBRARY_PAGE_SIZE);
    expect(useLibraryStore.getState().hasMore).toBe(true);
  });

  it("asks for the next page from where the loaded rows end", async () => {
    backendItems = Array.from({ length: 150 }, (_unused, index) => makeItem({ id: `i${index}` }));

    await useLibraryStore.getState().reload();
    await useLibraryStore.getState().loadMore();

    expect(last(queriesFor("list_library")).offset).toBe(LIBRARY_PAGE_SIZE);
    expect(useLibraryStore.getState().items).toHaveLength(LIBRARY_PAGE_SIZE * 2);
    expect(useLibraryStore.getState().items.map((item) => item.id)).toContain("i0");
  });

  it("stops paging once a short page proves the end was reached", async () => {
    await useLibraryStore.getState().reload();
    expect(useLibraryStore.getState().hasMore).toBe(false);

    await useLibraryStore.getState().loadMore();
    expect(queriesFor("list_library")).toHaveLength(1);
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
