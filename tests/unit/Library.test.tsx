import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";

import i18n from "@/lib/i18n";
import { Library } from "@/pages/Library";
import { stopActiveMedia } from "@/components/MediaPlayer";
import { DEFAULT_LIBRARY_PAGE_SIZE, useLibraryStore } from "@/stores/library-store";
import type { LibraryItem, LibraryQuery, LibraryStats } from "@/types/library";

/**
 * The Library page against the data this user actually has: 66 rows, not one
 * of which carries a thumbnail or a duration, and one of which is gone from
 * disk. Fixtures below keep those nulls rather than filling them in, because a
 * grid that only looks right with complete metadata looks broken on the real
 * database.
 */

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn().mockResolvedValue(undefined),
  convertFileSrc: vi.fn((path: string) => `asset://localhost/${encodeURIComponent(path)}`),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn().mockResolvedValue(null),
  save: vi.fn().mockResolvedValue(null),
}));

vi.mock("@tauri-apps/plugin-opener", () => ({
  openPath: vi.fn().mockResolvedValue(undefined),
}));

// The page must stop playback before the file underneath the player is moved
// or trashed (FR-316). Spying on the real implementation keeps that assertion
// about the page's ordering rather than about a stub.
vi.mock("@/components/MediaPlayer", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/components/MediaPlayer")>();
  return { ...actual, stopActiveMedia: vi.fn(actual.stopActiveMedia) };
});

interface Recorded {
  command: string;
  args: Record<string, unknown>;
}

let recorded: Recorded[] = [];
let backendItems: LibraryItem[] = [];
let backendStats: LibraryStats;

function makeItem(overrides: Partial<LibraryItem> = {}): LibraryItem {
  return {
    id: "item-1",
    file_path: "/downloads/song.mp3",
    title: "Song",
    media_type: "audio",
    file_format: "mp3",
    file_size_bytes: 4_194_304,
    // Every row in the real database is like this: the duration was never
    // measured and no thumbnail was ever saved.
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

function callsTo(command: string): Recorded[] {
  return recorded.filter((call) => call.command === command);
}

/** `Array.prototype.at` is ES2022 and `tsconfig.json` targets ES2020. */
function last<T>(values: T[]): T {
  return values[values.length - 1];
}

/** Every command name that reached `invoke`, including the ones a
 * `mockRejectedValueOnce` short-circuited before the fake backend saw them. */
function invokedCommands(): string[] {
  return vi.mocked(invoke).mock.calls.map(([command]) => command);
}

function lastQuery(command: string): LibraryQuery {
  return last(callsTo(command)).args.query as LibraryQuery;
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

/** Renders the page and waits for the first page of rows to land. */
async function renderLibrary() {
  const view = render(<Library active />);
  await waitFor(() => expect(callsTo("list_library").length).toBeGreaterThan(0));
  return view;
}

function rows() {
  return screen.queryAllByTestId("library-item");
}

beforeAll(() => {
  void i18n.changeLanguage("en");
  Object.defineProperty(HTMLMediaElement.prototype, "play", {
    configurable: true,
    value: () => Promise.resolve(),
  });
  Object.defineProperty(HTMLMediaElement.prototype, "pause", {
    configurable: true,
    value: () => {},
  });
});

beforeEach(() => {
  recorded = [];
  backendItems = [
    makeItem({ id: "a", title: "Alpha track" }),
    makeItem({
      id: "b",
      title: "Beta clip",
      media_type: "video",
      file_format: "mp4",
      platform: "tiktok",
    }),
    makeItem({ id: "c", title: "Gamma set" }),
  ];
  backendStats = statsFor(backendItems);
  localStorage.clear();
  resetStore();
  vi.mocked(openDialog).mockResolvedValue(null);
  vi.mocked(saveDialog).mockResolvedValue(null);
  vi.mocked(stopActiveMedia).mockClear();
  // `mockImplementation` alone leaves `mock.calls` from the previous test in
  // place, which would make every "was this called?" assertion below read a
  // neighbour's history instead of its own.
  vi.mocked(invoke).mockClear();

  vi.mocked(invoke).mockImplementation(async (command: string, args?: unknown) => {
    const parameters = (args ?? {}) as Record<string, unknown>;
    recorded.push({ command, args: parameters });

    if (command === "list_library") {
      const query = (parameters.query ?? {}) as LibraryQuery;
      const offset = query.offset ?? 0;
      const limit = query.limit ?? backendItems.length;
      return backendItems.slice(offset, offset + limit) as never;
    }
    if (command === "library_stats") return backendStats as never;
    if (command === "reconcile_library")
      return { checked: backendItems.length, missing: 0, changed_item_ids: [] } as never;
    if (command === "rename_library_item" || command === "relink_library_item") {
      return makeItem({ id: String(parameters.itemId) }) as never;
    }
    if (command === "move_library_items") return [] as never;
    if (command === "delete_library_items" || command === "remove_library_items") {
      return (parameters.itemIds as string[]).length as never;
    }
    return undefined as never;
  });
});

describe("Library — first open (FR-305)", () => {
  it("does not touch the backend until its tab is the one being shown", async () => {
    render(<Library active={false} />);
    // A few frames' worth of microtasks: enough for any mount effect to fire.
    await Promise.resolve();
    expect(recorded).toEqual([]);
  });

  it("reconciles once when opened, not once per render", async () => {
    const { rerender } = await renderLibrary();
    await waitFor(() => expect(callsTo("reconcile_library")).toHaveLength(1));

    rerender(<Library active />);
    rerender(<Library active />);

    expect(callsTo("reconcile_library")).toHaveLength(1);
  });
});

describe("Library — empty states (FR-311)", () => {
  it("tells someone with nothing downloaded something different from someone whose filters matched nothing", async () => {
    backendItems = [];
    backendStats = statsFor([]);
    await renderLibrary();

    const nothingDownloaded = await screen.findByTestId("library-empty-nothing-downloaded");
    expect(screen.queryByTestId("library-empty-no-results")).toBeNull();
    // Nothing to clear, so offering to clear filters here would be nonsense.
    expect(screen.queryByTestId("library-clear-filters-empty")).toBeNull();
    const firstStoryText = nothingDownloaded.textContent ?? "";

    // Now the same empty list, but because of a filter.
    await userEvent.click(screen.getByTestId("library-filter-media-type-audio"));

    const noResults = await screen.findByTestId("library-empty-no-results");
    expect(screen.queryByTestId("library-empty-nothing-downloaded")).toBeNull();
    const secondStoryText = noResults.textContent ?? "";

    expect(secondStoryText).not.toBe(firstStoryText);
    // …and the second one hands back a way out that the first cannot offer.
    expect(within(noResults).getByTestId("library-clear-filters-empty")).toBeInTheDocument();
  });

  it("clears the filters from the no-results state and asks again unfiltered", async () => {
    backendItems = [];
    backendStats = statsFor([]);
    await renderLibrary();

    await userEvent.click(screen.getByTestId("library-filter-media-type-video"));
    await screen.findByTestId("library-empty-no-results");
    await userEvent.click(screen.getByTestId("library-clear-filters-empty"));

    await waitFor(() => expect(lastQuery("list_library").media_types).toEqual([]));
    expect(await screen.findByTestId("library-empty-nothing-downloaded")).toBeInTheDocument();
  });
});

describe("Library — rows with no thumbnail and no duration", () => {
  it("draws a media-type placeholder instead of a broken image box", async () => {
    await renderLibrary();

    const placeholders = await screen.findAllByTestId("library-thumbnail-placeholder");
    expect(placeholders).toHaveLength(3);
    expect(placeholders.map((node) => node.dataset.mediaType)).toEqual(["audio", "video", "audio"]);
    expect(screen.queryAllByTestId("library-thumbnail-image")).toHaveLength(0);
  });

  it("shows a real thumbnail through the asset protocol when one exists", async () => {
    backendItems = [makeItem({ id: "a", thumbnail_path: "/covers/a.jpg" })];
    backendStats = statsFor(backendItems);
    await renderLibrary();

    const image = await screen.findByTestId("library-thumbnail-image");
    expect(image).toHaveAttribute("src", expect.stringContaining("asset://localhost/"));
    // FR-310: the browser decides which of 10.000 covers to fetch, not us.
    expect(image).toHaveAttribute("loading", "lazy");
  });

  // The asset protocol can refuse a path outside its allowed scope, or the
  // file on disk can simply be gone — either way the grid must not show the
  // browser's bare broken-image glyph (FR-301 acceptance #3: no empty cell).
  it("falls back to the media-type placeholder when the thumbnail file fails to load", async () => {
    backendItems = [makeItem({ id: "a", thumbnail_path: "/covers/a.jpg", media_type: "audio" })];
    backendStats = statsFor(backendItems);
    await renderLibrary();

    const image = await screen.findByTestId("library-thumbnail-image");
    fireEvent.error(image);

    const placeholder = await screen.findByTestId("library-thumbnail-placeholder");
    expect(placeholder.dataset.mediaType).toBe("audio");
    expect(screen.queryByTestId("library-thumbnail-image")).not.toBeInTheDocument();
  });

  it("leaves the duration out entirely rather than claiming the file is 0:00 long", async () => {
    await renderLibrary();

    expect(screen.queryAllByTestId("library-item-duration")).toHaveLength(0);
    expect(screen.getByTestId("library-items").textContent).not.toContain("0:00");
  });

  it("shows the duration once one is known", async () => {
    backendItems = [makeItem({ id: "a", duration_seconds: 754 })];
    backendStats = statsFor(backendItems);
    await renderLibrary();

    expect(await screen.findByTestId("library-item-duration")).toHaveTextContent("12:34");
  });
});

describe("Library — provider badge on music items (T043)", () => {
  it("names the provider that actually delivered a music file", async () => {
    backendItems = [
      makeItem({
        id: "m1",
        title: "Lossless one",
        media_type: "music",
        file_format: "flac",
        platform: "spotify",
        source_provider: "tidal",
      }),
    ];
    backendStats = statsFor(backendItems);
    await renderLibrary();

    const badge = await screen.findByTestId("library-provider-badge");
    expect(badge).toHaveTextContent("TIDAL");
    expect(badge.dataset.provider).toBe("tidal");
  });

  it("labels an extension provider as such instead of pretending it is a brand", async () => {
    backendItems = [
      makeItem({
        id: "m2",
        media_type: "music",
        file_format: "flac",
        platform: "spotify",
        source_provider: "ext:tidal-web",
      }),
    ];
    backendStats = statsFor(backendItems);
    await renderLibrary();

    const badge = await screen.findByTestId("library-provider-badge");
    expect(badge).toHaveTextContent("tidal-web (ext)");
  });

  // The Rust `LibraryItem` does not expose `source_provider` yet, so today
  // every payload arrives without the field. The grid must treat that exactly
  // like "no provider": render normally, show nothing.
  it("shows no badge when the field is absent from the payload", async () => {
    backendItems = [
      makeItem({ id: "m3", title: "Music sans provider", media_type: "music", platform: "spotify" }),
    ];
    backendStats = statsFor(backendItems);
    await renderLibrary();

    await screen.findAllByTestId("library-item");
    expect(screen.queryByTestId("library-provider-badge")).toBeNull();
  });

  it("never puts the badge on audio or video rows, whatever the backend sends", async () => {
    backendItems = [
      makeItem({ id: "a1", media_type: "audio", source_provider: "tidal" }),
      makeItem({ id: "v1", media_type: "video", file_format: "mp4", source_provider: "qobuz" }),
    ];
    backendStats = statsFor(backendItems);
    await renderLibrary();

    const items = await screen.findAllByTestId("library-item");
    expect(items).toHaveLength(2);
    expect(screen.queryByTestId("library-provider-badge")).toBeNull();
    // The rest of the row is untouched: platform, format and size still show.
    expect(items[0].textContent).toContain("YouTube");
  });

  it("offers music as a media-type filter and sends it through the query", async () => {
    await renderLibrary();

    await userEvent.click(await screen.findByTestId("library-filter-media-type-music"));

    await waitFor(() => expect(lastQuery("list_library").media_types).toEqual(["music"]));
    expect(screen.getByTestId("library-filter-media-type-music")).toHaveAttribute(
      "aria-pressed",
      "true",
    );
  });

  it("keeps the badge in the list view too", async () => {
    backendItems = [
      makeItem({
        id: "m4",
        media_type: "music",
        file_format: "flac",
        platform: "spotify",
        source_provider: "deezer",
      }),
    ];
    backendStats = statsFor(backendItems);
    await renderLibrary();
    await screen.findByTestId("library-provider-badge");

    await userEvent.click(screen.getByTestId("library-view-list"));

    expect(screen.getByTestId("library-items")).toHaveAttribute("data-view-mode", "list");
    expect(screen.getByTestId("library-provider-badge")).toHaveTextContent("Deezer");
  });
});

describe("Library — missing files (FR-323 → FR-326)", () => {
  it("marks a missing row and swaps in relink and re-download", async () => {
    backendItems = [
      makeItem({ id: "a", title: "Still here" }),
      makeItem({ id: "gone", title: "Vanished", is_missing: true }),
    ];
    backendStats = statsFor(backendItems);
    await renderLibrary();

    const missingRow = await waitFor(() => rows().find((row) => row.dataset.itemId === "gone")!);
    const presentRow = rows().find((row) => row.dataset.itemId === "a")!;

    expect(within(missingRow).getByTestId("library-missing-badge")).toBeInTheDocument();
    expect(within(missingRow).getByTestId("library-action-relink")).toBeInTheDocument();
    expect(within(missingRow).getByTestId("library-action-redownload")).toBeInTheDocument();

    // Playing or renaming a file that is not there is an offer that cannot be
    // honoured, so the missing row must not carry it.
    expect(within(missingRow).queryByTestId("library-action-play")).toBeNull();
    expect(within(missingRow).queryByTestId("library-action-rename")).toBeNull();

    expect(within(presentRow).queryByTestId("library-missing-badge")).toBeNull();
    expect(within(presentRow).queryByTestId("library-action-relink")).toBeNull();
    expect(within(presentRow).getByTestId("library-action-play")).toBeInTheDocument();
  });

  it("re-downloads a missing item from its own original job", async () => {
    backendItems = [makeItem({ id: "gone", is_missing: true, job_id: "job-9" })];
    backendStats = statsFor(backendItems);
    await renderLibrary();

    await userEvent.click(await screen.findByTestId("library-action-redownload"));

    await waitFor(() =>
      expect(callsTo("redownload_library_item")).toEqual([
        { command: "redownload_library_item", args: { itemId: "gone" } },
      ]),
    );
  });

  it("relinks a missing item to the file the user points at", async () => {
    backendItems = [makeItem({ id: "gone", is_missing: true })];
    backendStats = statsFor(backendItems);
    vi.mocked(openDialog).mockResolvedValue("/elsewhere/song.mp3" as never);
    await renderLibrary();

    await userEvent.click(await screen.findByTestId("library-action-relink"));

    await waitFor(() =>
      expect(callsTo("relink_library_item")[0].args).toEqual({
        itemId: "gone",
        newPath: "/elsewhere/song.mp3",
      }),
    );
    // Relinking is the alternative to downloading it all over again.
    expect(callsTo("redownload_library_item")).toHaveLength(0);
  });

  it("does nothing when the file picker is dismissed", async () => {
    backendItems = [makeItem({ id: "gone", is_missing: true })];
    backendStats = statsFor(backendItems);
    await renderLibrary();

    await userEvent.click(await screen.findByTestId("library-action-relink"));
    await Promise.resolve();

    expect(callsTo("relink_library_item")).toHaveLength(0);
  });
});

describe("Library — deleting (FR-318, FR-320, FR-322)", () => {
  it("asks first, and says the file goes to the trash rather than away", async () => {
    await renderLibrary();
    await screen.findAllByTestId("library-item");

    await userEvent.click(screen.getAllByTestId("library-action-delete")[0]);

    // Nothing has happened yet — the dialog is the point.
    expect(callsTo("delete_library_items")).toHaveLength(0);

    const description = await screen.findByTestId("library-confirm-description");
    expect(description.textContent?.toLowerCase()).toContain("trash");

    await userEvent.click(screen.getByTestId("library-confirm-submit"));

    await waitFor(() =>
      expect(callsTo("delete_library_items")[0].args).toEqual({ itemIds: ["a"] }),
    );
  });

  it("does not delete anything when the confirmation is dismissed", async () => {
    await renderLibrary();
    await screen.findAllByTestId("library-item");

    await userEvent.click(screen.getAllByTestId("library-action-delete")[0]);
    await userEvent.click(await screen.findByTestId("library-confirm-cancel"));

    expect(callsTo("delete_library_items")).toHaveLength(0);
    expect(screen.queryByTestId("library-confirm-submit")).toBeNull();
  });

  it("takes the whole selection through one confirmation (FR-320)", async () => {
    await renderLibrary();
    await screen.findAllByTestId("library-item");

    const checkboxes = screen.getAllByTestId("library-select");
    await userEvent.click(checkboxes[2]);
    await userEvent.click(checkboxes[0]);

    await userEvent.click(screen.getByTestId("library-bulk-delete"));
    await userEvent.click(await screen.findByTestId("library-confirm-submit"));

    await waitFor(() =>
      // Display order, not click order.
      expect(callsTo("delete_library_items")[0].args).toEqual({ itemIds: ["a", "c"] }),
    );
  });

  it("stops the player before the file it is playing is trashed (FR-316)", async () => {
    await renderLibrary();
    await screen.findAllByTestId("library-item");

    await userEvent.click(screen.getAllByTestId("library-action-play")[0]);
    expect(await screen.findByTestId("library-preview")).toBeInTheDocument();

    await userEvent.click(screen.getAllByTestId("library-action-delete")[0]);
    await userEvent.click(await screen.findByTestId("library-confirm-submit"));

    await waitFor(() => expect(callsTo("delete_library_items")).toHaveLength(1));
    expect(vi.mocked(stopActiveMedia)).toHaveBeenCalled();
    expect(vi.mocked(stopActiveMedia).mock.invocationCallOrder[0]).toBeLessThan(
      vi.mocked(invoke).mock.invocationCallOrder[
        vi.mocked(invoke).mock.calls.findIndex(([command]) => command === "delete_library_items")
      ],
    );
    expect(screen.queryByTestId("library-preview")).toBeNull();
  });

  it("leaves a player alone when a different item is being deleted", async () => {
    await renderLibrary();
    await screen.findAllByTestId("library-item");

    await userEvent.click(screen.getAllByTestId("library-action-play")[0]);
    await screen.findByTestId("library-preview");
    vi.mocked(stopActiveMedia).mockClear();

    await userEvent.click(screen.getAllByTestId("library-action-delete")[1]);
    await userEvent.click(await screen.findByTestId("library-confirm-submit"));

    await waitFor(() => expect(callsTo("delete_library_items")).toHaveLength(1));
    expect(vi.mocked(stopActiveMedia)).not.toHaveBeenCalled();
    expect(screen.getByTestId("library-preview")).toBeInTheDocument();
  });
});

describe("Library — a refused write (FR-322)", () => {
  it("shows FILE_EXISTS as an error and keeps the rename open instead of resolving it", async () => {
    await renderLibrary();
    await screen.findAllByTestId("library-item");

    vi.mocked(invoke).mockRejectedValueOnce({
      code: "FILE_EXISTS",
      message: "A file already exists at /downloads/taken.mp3",
    });

    await userEvent.click(screen.getAllByTestId("library-action-rename")[0]);
    const input = await screen.findByTestId("library-rename-input");
    await userEvent.clear(input);
    await userEvent.type(input, "taken.mp3");
    await userEvent.click(screen.getByTestId("library-confirm-submit"));

    // The user is told, in their own language, that nothing was overwritten.
    const banner = await screen.findByText(i18n.t("errors.FILE_EXISTS"));
    expect(banner).toBeInTheDocument();

    // Still open, still holding the name that was refused: the decision is the
    // user's to make, and a closed dialog would read as success.
    expect(screen.getByTestId("library-rename-input")).toHaveValue("taken.mp3");
    // And no second, quietly-renamed attempt went out behind their back.
    expect(invokedCommands().filter((command) => command === "rename_library_item")).toHaveLength(
      1,
    );
  });

  it("closes the rename and keeps the new name when it succeeds", async () => {
    await renderLibrary();
    await screen.findAllByTestId("library-item");

    vi.mocked(invoke).mockImplementationOnce(async (command: string, args?: unknown) => {
      recorded.push({ command, args: (args ?? {}) as Record<string, unknown> });
      return makeItem({
        id: "a",
        title: "Alpha track",
        file_path: "/downloads/fresh.mp3",
      }) as never;
    });

    await userEvent.click(screen.getAllByTestId("library-action-rename")[0]);
    const input = await screen.findByTestId("library-rename-input");
    await userEvent.clear(input);
    await userEvent.type(input, "fresh.mp3");
    await userEvent.click(screen.getByTestId("library-confirm-submit"));

    await waitFor(() => expect(screen.queryByTestId("library-rename-input")).toBeNull());
    expect(callsTo("rename_library_item")[0].args).toEqual({ itemId: "a", newName: "fresh.mp3" });
  });
});

describe("Library — statistics as filters (FR-328, FR-329)", () => {
  it("filters the library by the breakdown row that was clicked", async () => {
    await renderLibrary();
    const breakdown = await screen.findByTestId("library-breakdown-platform");

    await userEvent.click(within(breakdown).getByRole("button", { name: /tiktok/i }));

    await waitFor(() => expect(lastQuery("list_library").platforms).toEqual(["tiktok"]));
    // The active-filter strip has to say so, or the list looks arbitrarily short.
    expect(screen.getByTestId("library-active-filters").textContent).toContain("TikTok");
    expect(screen.getByTestId("library-filter-platform-tiktok")).toHaveAttribute(
      "aria-pressed",
      "true",
    );
  });

  it("filters by media type from the same panel", async () => {
    await renderLibrary();
    const breakdown = await screen.findByTestId("library-breakdown-media-type");

    await userEvent.click(within(breakdown).getByRole("button", { name: /Video/ }));

    await waitFor(() => expect(lastQuery("list_library").media_types).toEqual(["video"]));
  });

  it("reports the totals the backend gave it", async () => {
    await renderLibrary();

    expect(await screen.findByTestId("library-stats-total-items")).toHaveTextContent("3");
    expect(screen.getByTestId("library-stats-total-size")).toHaveTextContent("12.0 MB");
  });
});

describe("Library — playlist export (FR-330)", () => {
  it("exports the selection in the order it is displayed", async () => {
    vi.mocked(saveDialog).mockResolvedValue("/downloads/mix.m3u" as never);
    await renderLibrary();
    await screen.findAllByTestId("library-item");

    const checkboxes = screen.getAllByTestId("library-select");
    await userEvent.click(checkboxes[2]); // "c" first
    await userEvent.click(checkboxes[0]); // then "a"

    await userEvent.click(screen.getByTestId("library-export-playlist"));

    await waitFor(() =>
      expect(callsTo("export_library_playlist")[0].args).toEqual({
        itemIds: ["a", "c"],
        destinationPath: "/downloads/mix.m3u",
      }),
    );
  });

  it("exports everything on display when nothing is ticked", async () => {
    vi.mocked(saveDialog).mockResolvedValue("/downloads/all.m3u" as never);
    await renderLibrary();
    await screen.findAllByTestId("library-item");

    await userEvent.click(screen.getByTestId("library-export-playlist"));

    await waitFor(() =>
      expect((callsTo("export_library_playlist")[0].args as { itemIds: string[] }).itemIds).toEqual(
        ["a", "b", "c"],
      ),
    );
  });
});

describe("Library — numbered pages", () => {
  /** The library this user actually has: 66 files, no thumbnails, no
   * durations. Four pages at the default size — the size the page bar has to
   * look right at, not the twenty-page case. */
  function fillBackend(count: number) {
    backendItems = Array.from({ length: count }, (_unused, index) =>
      makeItem({ id: `i${index}`, title: `Track ${index}` }),
    );
    backendStats = statsFor(backendItems);
  }

  function pageOf(rows: HTMLElement[]) {
    return rows.map((row) => row.dataset.itemId);
  }

  it("shows one button per page and says where the user is", async () => {
    fillBackend(66);
    await renderLibrary();

    await screen.findByTestId("library-pagination");
    expect(screen.getByTestId("library-page-1")).toHaveAttribute("aria-current", "page");
    expect(screen.getByTestId("library-page-4")).toBeInTheDocument();
    // 66 files is four pages, not five: the last page holds the leftover six.
    expect(screen.queryByTestId("library-page-5")).toBeNull();
    expect(screen.getByTestId("library-page-label")).toHaveTextContent("Page 1 of 4");
    expect(rows()).toHaveLength(DEFAULT_LIBRARY_PAGE_SIZE);
    // Nowhere to go back to from the first page.
    expect(screen.getByTestId("library-prev-page")).toBeDisabled();
  });

  it("asks the backend for exactly the page that was clicked", async () => {
    fillBackend(66);
    await renderLibrary();

    await userEvent.click(await screen.findByTestId("library-page-3"));

    await waitFor(() => expect(lastQuery("list_library").offset).toBe(40));
    expect(lastQuery("list_library").limit).toBe(DEFAULT_LIBRARY_PAGE_SIZE);
    // The rows on screen are that page and nothing else — an infinite-scroll
    // grid would be showing all sixty by now.
    expect(pageOf(rows())[0]).toBe("i40");
    expect(rows()).toHaveLength(DEFAULT_LIBRARY_PAGE_SIZE);
    expect(screen.getByTestId("library-page-label")).toHaveTextContent("Page 3 of 4");
    expect(screen.getByTestId("library-page-3")).toHaveAttribute("aria-current", "page");
  });

  it("walks to the last page with Next and stops there", async () => {
    fillBackend(66);
    await renderLibrary();

    await userEvent.click(await screen.findByTestId("library-next-page"));
    await waitFor(() => expect(lastQuery("list_library").offset).toBe(20));
    await userEvent.click(screen.getByTestId("library-next-page"));
    await waitFor(() => expect(lastQuery("list_library").offset).toBe(40));
    await userEvent.click(screen.getByTestId("library-next-page"));
    await waitFor(() => expect(lastQuery("list_library").offset).toBe(60));

    // Six leftover rows, and no way to page past them.
    expect(rows()).toHaveLength(6);
    expect(screen.getByTestId("library-next-page")).toBeDisabled();
  });

  it("goes back to page one with a new page size", async () => {
    fillBackend(66);
    await renderLibrary();
    await userEvent.click(await screen.findByTestId("library-page-3"));
    await waitFor(() => expect(lastQuery("list_library").offset).toBe(40));

    await userEvent.selectOptions(screen.getByTestId("library-page-size"), "10");

    await waitFor(() => expect(lastQuery("list_library").limit).toBe(10));
    // "Page 3" of twenty-a-page is a different set of files from "page 3" of
    // ten-a-page, so staying on 3 would land the user somewhere arbitrary.
    expect(lastQuery("list_library").offset).toBe(0);
    expect(screen.getByTestId("library-page-label")).toHaveTextContent("Page 1 of 7");
    expect(rows()).toHaveLength(10);
  });

  it("returns to page one when a filter narrows the library", async () => {
    fillBackend(66);
    await renderLibrary();
    await userEvent.click(await screen.findByTestId("library-page-2"));
    await waitFor(() => expect(lastQuery("list_library").offset).toBe(20));

    await userEvent.click(screen.getByTestId("library-filter-media-type-audio"));

    await waitFor(() => expect(lastQuery("list_library").media_types).toEqual(["audio"]));
    expect(lastQuery("list_library").offset).toBe(0);
    expect(screen.getByTestId("library-page-1")).toHaveAttribute("aria-current", "page");
  });
});

describe("Library — view and sort (FR-306, FR-309)", () => {
  it("switches between grid and list and remembers the choice", async () => {
    await renderLibrary();

    expect(await screen.findByTestId("library-items")).toHaveAttribute("data-view-mode", "grid");

    await userEvent.click(screen.getByTestId("library-view-list"));

    expect(screen.getByTestId("library-items")).toHaveAttribute("data-view-mode", "list");
    expect(localStorage.getItem("library.view_mode")).toBe("list");
    // Switching how rows are drawn is not a reason to ask the backend again.
    // (2 calls: one from initial load, one from reconcile reloading after stats refresh)
    expect(callsTo("list_library")).toHaveLength(2);
  });

  it("re-queries with the chosen sort and direction", async () => {
    await renderLibrary();

    await userEvent.selectOptions(screen.getByTestId("library-sort"), "title");
    await waitFor(() => expect(lastQuery("list_library").sort).toBe("title"));
    expect(lastQuery("list_library").direction).toBe("desc");

    await userEvent.click(screen.getByTestId("library-sort-direction"));
    await waitFor(() => expect(lastQuery("list_library").direction).toBe("asc"));
    expect(localStorage.getItem("library.direction")).toBe("asc");
  });
});
