import { describe, expect, it, beforeEach, vi } from "vitest";
import { act, fireEvent, render, screen } from "@testing-library/react";
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

describe("QueueList", () => {
  beforeEach(() => {
    useQueueStore.setState({ jobs: {} });
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

  it("sends only the dropped job and its new neighbours (FR-117)", () => {
    seedThreeQueuedJobs();
    render(<QueueList />);

    const rows = screen.getAllByRole("listitem");
    // Drag the last row to the top: it ends up with no neighbour before it,
    // and "a" — the row it displaced — after it.
    fireEvent.dragStart(rows[2]);
    fireEvent.dragOver(rows[0]);
    fireEvent.drop(rows[0]);

    expect(invoke).toHaveBeenCalledWith("reorder_queue", {
      jobId: "c",
      beforeJobId: null,
      afterJobId: "a",
    });
  });

  it("passes both neighbours when dropping into the middle", () => {
    seedThreeQueuedJobs();
    render(<QueueList />);

    const rows = screen.getAllByRole("listitem");
    // Drag "a" down onto "b": it lands between "b" and "c", so neither
    // neighbour is null.
    fireEvent.dragStart(rows[0]);
    fireEvent.dragOver(rows[1]);
    fireEvent.drop(rows[1]);

    expect(invoke).toHaveBeenCalledWith("reorder_queue", {
      jobId: "a",
      beforeJobId: "b",
      afterJobId: "c",
    });
  });

  it("moves the row immediately instead of waiting for the backend", () => {
    seedThreeQueuedJobs();
    // Never resolves: the row must have moved on the strength of the local
    // guess alone, not because the command came back.
    vi.mocked(invoke).mockImplementation(() => new Promise(() => {}));
    render(<QueueList />);

    const rows = screen.getAllByRole("listitem");
    fireEvent.dragStart(rows[2]);
    fireEvent.dragOver(rows[0]);
    fireEvent.drop(rows[0]);

    expect(rowTitles()).toEqual(["Job C", "Job A", "Job B"]);
    // Head of the queue: one below the job it now sits in front of, the same
    // number the backend would have written.
    expect(useQueueStore.getState().jobs.c.queue_position).toBe(0);
  });

  it("gives a job dropped between two others the midpoint position", () => {
    seedThreeQueuedJobs();
    vi.mocked(invoke).mockImplementation(() => new Promise(() => {}));
    render(<QueueList />);

    const rows = screen.getAllByRole("listitem");
    fireEvent.dragStart(rows[0]);
    fireEvent.dragOver(rows[1]);
    fireEvent.drop(rows[1]);

    expect(useQueueStore.getState().jobs.a.queue_position).toBe(2.5);
    expect(rowTitles()).toEqual(["Job B", "Job A", "Job C"]);
  });

  it("does nothing when a job is dropped on itself", () => {
    seedThreeQueuedJobs();
    render(<QueueList />);

    const rows = screen.getAllByRole("listitem");
    fireEvent.dragStart(rows[1]);
    fireEvent.drop(rows[1]);

    expect(invoke).not.toHaveBeenCalledWith("reorder_queue", expect.anything());
    expect(rowTitles()).toEqual(["Job A", "Job B", "Job C"]);
  });

  it("does not let a running job be dragged", () => {
    useQueueStore.setState({
      jobs: {
        a: makeJob({ id: "a", status: "downloading", queue_position: 1 }),
        b: makeJob({ id: "b", status: "paused", queue_position: 2 }),
      },
    });
    render(<QueueList />);

    const rows = screen.getAllByRole("listitem");
    expect(rows[0]).toHaveAttribute("draggable", "false");
    // A paused job has not taken its slot yet, so it still moves.
    expect(rows[1]).toHaveAttribute("draggable", "true");
  });

  it("re-reads the queue when the backend rejects the move", async () => {
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

    const rows = screen.getAllByRole("listitem");
    fireEvent.dragStart(rows[2]);
    fireEvent.dragOver(rows[0]);
    await act(async () => {
      fireEvent.drop(rows[0]);
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
