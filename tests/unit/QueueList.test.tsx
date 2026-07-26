import { describe, expect, it, beforeEach, vi } from "vitest";
import { act, render, screen, within } from "@testing-library/react";
import userEvent, { type UserEvent } from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { QueueList } from "@/components/QueueList";
import { useQueueStore } from "@/stores/queue-store";
import type { DownloadJob } from "@/types/download";

function makeJob(overrides: Partial<DownloadJob> = {}): DownloadJob {
  return {
    id: "job-1",
    source_url: "https://youtube.com/watch?v=abc",
    platform: "youtube",
    media_type: "audio",
    audio_quality: "128kbps",
    video_quality: null,
    // Audio job from yt-dlp: the gallery-dl fields never apply here.
    gallery_mode: null,
    selected_gallery_indices: null,
    status: "downloading",
    progress_percent: 42,
    speed_bytes_per_sec: 1_500_000,
    eta_seconds: 10,
    error_message: null,
    output_directory: "/tmp",
    output_file_path: null,
    is_playlist_item: false,
    parent_playlist_id: null,
    retried_from_job_id: null,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    title: null,
    playlist_title: null,
    queue_position: 0,
    retry_count: 0,
    next_retry_at: null,
    ...overrides,
  };
}

/** Three pending jobs whose insertion order (c, a, b) is deliberately not
 * their queue order (a, b, c): a component that just echoed `Object.values`
 * would render them backwards, so every row index below is a real assertion
 * about ordering rather than about the fixture. */
function seedThreeQueuedJobs() {
  useQueueStore.setState({
    jobs: {
      c: makeJob({ id: "c", title: "Job C", queue_position: 3, status: "queued" }),
      a: makeJob({ id: "a", title: "Job A", queue_position: 1, status: "queued" }),
      b: makeJob({ id: "b", title: "Job B", queue_position: 2, status: "queued" }),
    },
  });
}

function rowTitles() {
  return screen.getAllByRole("listitem").map((row) => row.querySelector("span")?.textContent);
}

/**
 * Reordering is driven by pointer events and hit-tests rows by geometry, but
 * jsdom lays nothing out — every `getBoundingClientRect()` returns zeros, so
 * every row would occupy the same point. Give the rows the stacked boxes a
 * real list has: row `i` spans y = i*ROW_HEIGHT … (i+1)*ROW_HEIGHT.
 */
const ROW_HEIGHT = 100;

function layoutRows(rows: HTMLElement[]) {
  rows.forEach((row, index) => {
    const top = index * ROW_HEIGHT;
    row.getBoundingClientRect = () =>
      ({
        top,
        bottom: top + ROW_HEIGHT,
        height: ROW_HEIGHT,
        left: 0,
        right: 320,
        width: 320,
        x: 0,
        y: top,
        toJSON: () => ({}),
      }) as DOMRect;
  });
}

/** Vertical centre of the row at `index`, in the layout `layoutRows` sets up. */
const rowCenterY = (index: number) => index * ROW_HEIGHT + ROW_HEIGHT / 2;

function dragHandle(row: HTMLElement) {
  return within(row).getByRole("button", { name: /^reorder/i });
}

/**
 * Presses the drag handle of the row at `fromIndex` and drags the pointer to
 * `toY` (a viewport coordinate — use `rowCenterY(i)` for "over row i").
 * Leaves the button held down when `release` is false, so a test can inspect
 * the drop indicator mid-drag or interrupt the gesture. `fromY` overrides
 * where the press lands, which is what decides how far the pointer travelled.
 */
async function dragRow(
  user: UserEvent,
  fromIndex: number,
  toY: number,
  { release = true, fromY = rowCenterY(fromIndex) } = {},
) {
  const rows = screen.getAllByRole("listitem");
  layoutRows(rows);
  const handle = dragHandle(rows[fromIndex]);
  await user.pointer([
    { keys: "[MouseLeft>]", target: handle, coords: { clientX: 8, clientY: fromY } },
    { target: handle, coords: { clientX: 8, clientY: toY } },
  ]);
  if (release) {
    await user.pointer({ keys: "[/MouseLeft]", target: handle, coords: { clientX: 8, clientY: toY } });
  }
  return handle;
}

describe("QueueList", () => {
  beforeEach(() => {
    useQueueStore.setState({ jobs: {}, liveProgress: {}, challenges: {} });
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockImplementation(() => Promise.resolve(undefined));
  });

  it("shows an empty state when there are no active jobs", () => {
    render(<QueueList />);
    expect(screen.getByText(/no downloads in progress/i)).toBeInTheDocument();
  });

  it("renders an active job with its progress percentage", () => {
    useQueueStore.setState({ jobs: { "job-1": makeJob() } });
    render(<QueueList />);
    expect(screen.getByText("https://youtube.com/watch?v=abc")).toBeInTheDocument();
    expect(screen.getByText("42%")).toBeInTheDocument();
  });

  it("shows bytes and speed instead of a false 0% when the percentage is unknown", () => {
    // The user-visible bug. yt-dlp reports no total size for audio-only
    // formats and HLS, so there is no percentage to show — but there IS a
    // byte count and a speed in the very same payload. The row must say
    // "511.0 KB downloaded · 359.2 KB/s", never "0%".
    useQueueStore.setState({
      // The stored row still reads 0 — the column is REAL NOT NULL and no
      // tick ever carried a percentage to put there. What must not happen is
      // that number reaching the screen as if it meant "0% done".
      jobs: { "job-1": makeJob({ progress_percent: 0, speed_bytes_per_sec: 367_853 }) },
      liveProgress: { "job-1": { progress_percent: null, downloaded_bytes: 523_264 } },
    });
    render(<QueueList />);

    expect(screen.queryByText("0%")).not.toBeInTheDocument();
    expect(screen.getByText("511.0 KB downloaded")).toBeInTheDocument();
    expect(screen.getByText("359.2 KB/s")).toBeInTheDocument();
    // ...and the bar itself has to say "unknown" rather than sit at a value.
    const bar = screen.getByRole("progressbar", { name: /progress unknown/i });
    expect(bar).not.toHaveAttribute("aria-valuenow");
  });

  it("shows an ordinary percentage bar as soon as a live tick carries one", () => {
    // The other half: a job whose source does report a total must keep the
    // plain determinate bar. A component that rendered the indeterminate
    // branch unconditionally would pass the test above on its own.
    useQueueStore.setState({
      jobs: { "job-1": makeJob({ progress_percent: 42, speed_bytes_per_sec: 1_500_000 }) },
      liveProgress: { "job-1": { progress_percent: 42, downloaded_bytes: 4_200_000 } },
    });
    render(<QueueList />);

    expect(screen.getByText("42%")).toBeInTheDocument();
    expect(screen.queryByText(/downloaded$/)).not.toBeInTheDocument();
    expect(
      screen.queryByRole("progressbar", { name: /progress unknown/i }),
    ).not.toBeInTheDocument();
  });

  // FR-009 — a music job says which provider it is actually pulling from.
  it("names the provider a running music job is downloading from", () => {
    useQueueStore.setState({
      jobs: { "job-1": makeJob({ media_type: "music", platform: "spotify" }) },
      liveProgress: { "job-1": { progress_percent: 42, downloaded_bytes: 100, provider: "tidal" } },
    });
    render(<QueueList />);

    // Through `formatPlatformLabel`, so the branded name shows rather than
    // the raw provider id the worker sends.
    expect(screen.getByText("Source: TIDAL")).toBeInTheDocument();
  });

  it("shows no provider badge for a non-music job that somehow has one", () => {
    useQueueStore.setState({
      jobs: { "job-1": makeJob({ media_type: "audio" }) },
      liveProgress: { "job-1": { progress_percent: 42, downloaded_bytes: 100, provider: "tidal" } },
    });
    render(<QueueList />);

    expect(screen.queryByText(/Source:/)).not.toBeInTheDocument();
  });

  // A job parked on a Cloudflare check is still active: hiding it would leave
  // the user with a download that never finishes and no way to unblock it.
  it("keeps a waiting_input job on screen with its own status and no pause button", () => {
    useQueueStore.setState({
      jobs: { "job-1": makeJob({ media_type: "music", status: "waiting_input" }) },
    });
    render(<QueueList />);

    expect(screen.getByText("Waiting for verification")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /verify now/i })).toBeInTheDocument();
    // Pausing would kill the worker holding the challenge open.
    expect(screen.queryByRole("button", { name: /^pause$/i })).not.toBeInTheDocument();
  });

  it("re-reads the pending challenge when the user asks to verify again", async () => {
    const user = userEvent.setup();
    useQueueStore.setState({
      jobs: { "job-1": makeJob({ media_type: "music", status: "waiting_input" }) },
      // Already dismissed once, so the dialog is closed and the row's button
      // is the only way back to it.
      challenges: {
        "job-1": { challengeUrl: "https://challenge.example/v", attempts: 1, dismissed: true },
      },
    });
    render(<QueueList />);

    await user.click(screen.getByRole("button", { name: /verify now/i }));

    expect(invoke).toHaveBeenCalledWith("get_pending_challenge", { jobId: "job-1" });
  });

  it("opens the grant dialog for an undismissed challenge", () => {
    useQueueStore.setState({
      jobs: { "job-1": makeJob({ media_type: "music", status: "waiting_input" }) },
      challenges: {
        "job-1": { challengeUrl: "https://challenge.example/v", attempts: 0, dismissed: false },
      },
    });
    render(<QueueList />);

    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(screen.getByText("https://challenge.example/v")).toBeInTheDocument();
  });

  it("does not list completed jobs among active downloads", () => {
    useQueueStore.setState({
      jobs: { "job-1": makeJob({ status: "completed", progress_percent: 100 }) },
    });
    render(<QueueList />);
    expect(screen.getByText(/no downloads in progress/i)).toBeInTheDocument();
  });

  it("shows a job's title instead of its raw source_url when a title is set", () => {
    useQueueStore.setState({ jobs: { "job-1": makeJob({ title: "Some Song (Official Video)" }) } });
    render(<QueueList />);
    expect(screen.getByText("Some Song (Official Video)")).toBeInTheDocument();
    expect(screen.queryByText("https://youtube.com/watch?v=abc")).not.toBeInTheDocument();
  });

  it("groups playlist jobs under one header instead of N separate rows", () => {
    useQueueStore.setState({
      jobs: {
        "job-1": makeJob({
          id: "job-1",
          is_playlist_item: true,
          parent_playlist_id: "pl-1",
          playlist_title: "My Playlist",
          title: "Video 1",
          status: "downloading",
          progress_percent: 100,
        }),
        "job-2": makeJob({
          id: "job-2",
          is_playlist_item: true,
          parent_playlist_id: "pl-1",
          playlist_title: "My Playlist",
          title: "Video 2",
          status: "downloading",
          progress_percent: 0,
        }),
      },
    });
    render(<QueueList />);
    expect(screen.getByText("My Playlist")).toBeInTheDocument();
    expect(screen.getByText("0/2 completed")).toBeInTheDocument();
    // Expanded by default: both children are visible under the group.
    expect(screen.getByText("Video 1")).toBeInTheDocument();
    expect(screen.getByText("Video 2")).toBeInTheDocument();
  });

  it("keeps a playlist group visible, with the finished child still shown, until every job in it is completed", () => {
    useQueueStore.setState({
      jobs: {
        "job-1": makeJob({
          id: "job-1",
          is_playlist_item: true,
          parent_playlist_id: "pl-1",
          playlist_title: "My Playlist",
          title: "Video 1",
          status: "completed",
          progress_percent: 100,
        }),
        "job-2": makeJob({
          id: "job-2",
          is_playlist_item: true,
          parent_playlist_id: "pl-1",
          playlist_title: "My Playlist",
          title: "Video 2",
          status: "downloading",
          progress_percent: 30,
        }),
      },
    });
    render(<QueueList />);
    // Regression guard: a standalone job disappears from this list once
    // completed (see the test above). A playlist group must not do the
    // same to individual children while siblings are still active, or the
    // completed count above would have nothing to count against.
    expect(screen.getByText("My Playlist")).toBeInTheDocument();
    expect(screen.getByText("Video 1")).toBeInTheDocument();
    expect(screen.getByText("Video 2")).toBeInTheDocument();
  });

  it("hides the playlist group once every job in it has completed", () => {
    useQueueStore.setState({
      jobs: {
        "job-1": makeJob({
          id: "job-1",
          is_playlist_item: true,
          parent_playlist_id: "pl-1",
          playlist_title: "My Playlist",
          status: "completed",
          progress_percent: 100,
        }),
        "job-2": makeJob({
          id: "job-2",
          is_playlist_item: true,
          parent_playlist_id: "pl-1",
          playlist_title: "My Playlist",
          status: "completed",
          progress_percent: 100,
        }),
      },
    });
    render(<QueueList />);
    expect(screen.queryByText("My Playlist")).not.toBeInTheDocument();
    expect(screen.getByText(/no downloads in progress/i)).toBeInTheDocument();
  });

  it("falls back to a generic label when a playlist group has no stored title", () => {
    useQueueStore.setState({
      jobs: {
        "job-1": makeJob({ id: "job-1", is_playlist_item: true, parent_playlist_id: "pl-1", playlist_title: null }),
      },
    });
    render(<QueueList />);
    expect(screen.getByText("Playlist")).toBeInTheDocument();
  });

  it("lists jobs in queue order rather than the order they arrived in", () => {
    seedThreeQueuedJobs();
    render(<QueueList />);
    expect(rowTitles()).toEqual(["Job A", "Job B", "Job C"]);
  });

  it("sends only the dragged job and its new neighbours (FR-117)", async () => {
    const user = userEvent.setup();
    seedThreeQueuedJobs();
    render(<QueueList />);

    // Drag the last row to the top: it ends up with no neighbour before it,
    // and "a" — the row it displaced — after it.
    await dragRow(user, 2, rowCenterY(0));

    expect(invoke).toHaveBeenCalledWith("reorder_queue", {
      jobId: "c",
      beforeJobId: null,
      afterJobId: "a",
    });
  });

  it("passes both neighbours when dropping into the middle", async () => {
    const user = userEvent.setup();
    seedThreeQueuedJobs();
    render(<QueueList />);

    // Drag "a" down onto "b": it lands between "b" and "c", so neither
    // neighbour is null.
    await dragRow(user, 0, rowCenterY(1));

    expect(invoke).toHaveBeenCalledWith("reorder_queue", {
      jobId: "a",
      beforeJobId: "b",
      afterJobId: "c",
    });
  });

  it("drops onto the nearest row when the pointer is released past the end of the list", async () => {
    const user = userEvent.setup();
    seedThreeQueuedJobs();
    render(<QueueList />);

    // Well below the last row: the drag still resolves to "c" rather than
    // being thrown away for landing on empty space.
    await dragRow(user, 0, rowCenterY(2) + 400);

    expect(invoke).toHaveBeenCalledWith("reorder_queue", {
      jobId: "a",
      beforeJobId: "c",
      afterJobId: null,
    });
  });

  it("moves the row immediately instead of waiting for the backend", async () => {
    const user = userEvent.setup();
    seedThreeQueuedJobs();
    // Never resolves: the row must have moved on the strength of the local
    // guess alone, not because the command came back.
    vi.mocked(invoke).mockImplementation(() => new Promise(() => {}));
    render(<QueueList />);

    await dragRow(user, 2, rowCenterY(0));

    expect(rowTitles()).toEqual(["Job C", "Job A", "Job B"]);
    // Head of the queue: one below the job it now sits in front of, the same
    // number the backend would have written.
    expect(useQueueStore.getState().jobs.c.queue_position).toBe(0);
  });

  it("gives a job dropped between two others the midpoint position", async () => {
    const user = userEvent.setup();
    seedThreeQueuedJobs();
    vi.mocked(invoke).mockImplementation(() => new Promise(() => {}));
    render(<QueueList />);

    await dragRow(user, 0, rowCenterY(1));

    expect(useQueueStore.getState().jobs.a.queue_position).toBe(2.5);
    expect(rowTitles()).toEqual(["Job B", "Job A", "Job C"]);
  });

  it("marks a row being dragged downwards as landing after the row under the pointer", async () => {
    const user = userEvent.setup();
    seedThreeQueuedJobs();
    render(<QueueList />);

    await dragRow(user, 0, rowCenterY(1), { release: false });

    const rows = screen.getAllByRole("listitem");
    expect(rows[0]).toHaveAttribute("data-dragging", "true");
    expect(rows[1]).toHaveAttribute("data-drop-position", "after");
    expect(rows[2]).not.toHaveAttribute("data-drop-position");
    // Nothing is committed until the pointer comes up.
    expect(invoke).not.toHaveBeenCalledWith("reorder_queue", expect.anything());
  });

  it("marks a row being dragged upwards as landing before the row under the pointer", async () => {
    const user = userEvent.setup();
    seedThreeQueuedJobs();
    render(<QueueList />);

    await dragRow(user, 2, rowCenterY(0), { release: false });

    const rows = screen.getAllByRole("listitem");
    expect(rows[0]).toHaveAttribute("data-drop-position", "before");
    expect(rows[1]).not.toHaveAttribute("data-drop-position");
    expect(rows[2]).toHaveAttribute("data-dragging", "true");
  });

  it("does nothing when a row is dropped back onto itself", async () => {
    const user = userEvent.setup();
    seedThreeQueuedJobs();
    render(<QueueList />);

    // A real drag by any measure — well past the threshold — but it never
    // leaves the row it started on.
    await dragRow(user, 1, rowCenterY(1) + 30);

    expect(invoke).not.toHaveBeenCalledWith("reorder_queue", expect.anything());
    expect(rowTitles()).toEqual(["Job A", "Job B", "Job C"]);
  });

  it("ignores a press that never travels far enough to be a drag", async () => {
    const user = userEvent.setup();
    seedThreeQueuedJobs();
    render(<QueueList />);

    // Two pixels of jitter, but across the boundary between "b" and "c": far
    // enough to change the row under the pointer, not far enough to be a drag.
    await dragRow(user, 1, 2 * ROW_HEIGHT + 1, { fromY: 2 * ROW_HEIGHT - 1 });

    expect(invoke).not.toHaveBeenCalledWith("reorder_queue", expect.anything());
    expect(rowTitles()).toEqual(["Job A", "Job B", "Job C"]);
  });

  it("abandons a drag when Escape is pressed before the pointer comes up", async () => {
    const user = userEvent.setup();
    seedThreeQueuedJobs();
    render(<QueueList />);

    const handle = await dragRow(user, 2, rowCenterY(0), { release: false });
    expect(screen.getAllByRole("listitem")[0]).toHaveAttribute("data-drop-position", "before");

    await user.keyboard("{Escape}");
    await user.pointer({ keys: "[/MouseLeft]", target: handle, coords: { clientX: 8, clientY: rowCenterY(0) } });

    expect(invoke).not.toHaveBeenCalledWith("reorder_queue", expect.anything());
    expect(rowTitles()).toEqual(["Job A", "Job B", "Job C"]);
    expect(screen.getAllByRole("listitem")[0]).not.toHaveAttribute("data-drop-position");
  });

  it("gives only reorderable rows a drag handle", () => {
    useQueueStore.setState({
      jobs: {
        a: makeJob({ id: "a", title: "Running", status: "downloading", queue_position: 1 }),
        b: makeJob({ id: "b", title: "Waiting", status: "paused", queue_position: 2 }),
      },
    });
    render(<QueueList />);

    const rows = screen.getAllByRole("listitem");
    // A running job has already taken its slot, so it has nowhere to move to
    // (FR-119) — and the missing handle is what tells the user that, rather
    // than leaving them to guess whether dragging is broken.
    expect(within(rows[0]).queryByRole("button", { name: /^reorder/i })).not.toBeInTheDocument();
    // A paused job has not taken its slot yet, so it still moves.
    expect(within(rows[1]).getByRole("button", { name: /^reorder/i })).toBeInTheDocument();
  });

  it("still lets a row's own buttons be clicked", async () => {
    const user = userEvent.setup();
    seedThreeQueuedJobs();
    render(<QueueList />);

    // The press that starts a drag must not swallow the controls inside the
    // row: pausing a job is a click, not a gesture.
    const rows = screen.getAllByRole("listitem");
    const buttons = within(rows[0]).getAllByRole("button");
    // [0] is the drag handle; the pause and cancel controls follow it.
    expect(buttons).toHaveLength(3);
    await user.click(buttons[1]);

    expect(invoke).toHaveBeenCalledWith("pause_job", { jobId: "a" });
  });

  it("re-reads the queue when the backend rejects the move", async () => {
    const user = userEvent.setup();
    seedThreeQueuedJobs();
    vi.mocked(invoke).mockImplementation((cmd: string) => {
      if (cmd === "reorder_queue") return Promise.reject(new Error("NOT_FOUND"));
      if (cmd === "list_queue") {
        // The truth after the failed move: "c" never went anywhere.
        return Promise.resolve([makeJob({ id: "c", title: "Job C", queue_position: 3, status: "queued" })]);
      }
      return Promise.resolve(undefined);
    });
    render(<QueueList />);

    await act(async () => {
      await dragRow(user, 2, rowCenterY(0));
    });

    // The optimistic 0 has been replaced by what the database actually holds,
    // rather than left behind as a position that never existed.
    expect(useQueueStore.getState().jobs.c.queue_position).toBe(3);
    expect(rowTitles()).toEqual(["Job A", "Job B", "Job C"]);
  });

  it("counts down the seconds until a job's next retry attempt (FR-122)", () => {
    vi.useFakeTimers();
    try {
      vi.setSystemTime(new Date("2026-01-01T00:00:00Z"));
      useQueueStore.setState({
        jobs: {
          "job-1": makeJob({
            status: "queued",
            retry_count: 2,
            next_retry_at: "2026-01-01T00:00:10Z",
          }),
        },
      });
      render(<QueueList />);

      expect(screen.getByText("Retrying in 10s (attempt 3)")).toBeInTheDocument();

      act(() => {
        vi.advanceTimersByTime(3000);
      });
      expect(screen.getByText("Retrying in 7s (attempt 3)")).toBeInTheDocument();
      expect(screen.queryByText("Retrying in 10s (attempt 3)")).not.toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it("shows the plain status once the retry deadline has passed", () => {
    vi.useFakeTimers();
    try {
      vi.setSystemTime(new Date("2026-01-01T00:00:00Z"));
      useQueueStore.setState({
        jobs: {
          "job-1": makeJob({
            status: "queued",
            retry_count: 2,
            // Already due: the dispatcher is about to pick this job up.
            next_retry_at: "2025-12-31T23:59:55Z",
          }),
        },
      });
      render(<QueueList />);

      expect(screen.queryByText(/retrying in/i)).not.toBeInTheDocument();
      expect(screen.getByText("Queued")).toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it("does not count down for a job that is not waiting on a retry", () => {
    vi.useFakeTimers();
    try {
      vi.setSystemTime(new Date("2026-01-01T00:00:00Z"));
      useQueueStore.setState({
        jobs: {
          // Same future deadline, but the job is running: it is not waiting.
          "job-1": makeJob({
            status: "downloading",
            retry_count: 1,
            next_retry_at: "2026-01-01T00:00:10Z",
          }),
        },
      });
      render(<QueueList />);

      expect(screen.queryByText(/retrying in/i)).not.toBeInTheDocument();
      expect(screen.getByText("Downloading")).toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it("stops ticking when the countdown leaves the screen", () => {
    vi.useFakeTimers();
    const setIntervalSpy = vi.spyOn(window, "setInterval");
    const clearIntervalSpy = vi.spyOn(window, "clearInterval");
    try {
      vi.setSystemTime(new Date("2026-01-01T00:00:00Z"));
      useQueueStore.setState({
        jobs: {
          "job-1": makeJob({ status: "queued", next_retry_at: "2026-01-01T00:01:00Z" }),
        },
      });
      const { unmount } = render(<QueueList />);
      expect(setIntervalSpy).toHaveBeenCalledTimes(1);
      const timerId: unknown = setIntervalSpy.mock.results[0].value;

      unmount();

      expect(clearIntervalSpy).toHaveBeenCalledWith(timerId);
    } finally {
      setIntervalSpy.mockRestore();
      clearIntervalSpy.mockRestore();
      vi.useRealTimers();
    }
  });

  it("counts jobs for the bulk toolbar by what each action can act on", () => {
    useQueueStore.setState({
      jobs: {
        running: makeJob({ id: "running", status: "downloading", queue_position: 1 }),
        waiting: makeJob({ id: "waiting", status: "paused", queue_position: 2 }),
        done: makeJob({ id: "done", status: "completed", queue_position: 3 }),
      },
    });
    render(<QueueList />);

    expect(screen.getByRole("button", { name: /pause all/i })).toBeEnabled();
    expect(screen.getByRole("button", { name: /resume all/i })).toBeEnabled();
    expect(screen.getByRole("button", { name: /clear finished/i })).toBeEnabled();
  });
});
