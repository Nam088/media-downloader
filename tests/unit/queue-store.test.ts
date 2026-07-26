import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { useQueueStore } from "@/stores/queue-store";
import type { DownloadJob } from "@/types/download";

function makeJob(overrides: Partial<DownloadJob> = {}): DownloadJob {
  return {
    id: "job-1",
    source_url: "https://example.com/v",
    platform: "youtube",
    media_type: "audio",
    audio_quality: "128kbps",
    video_quality: null,
    gallery_mode: null,
    selected_gallery_indices: null,
    status: "queued",
    progress_percent: 0,
    speed_bytes_per_sec: null,
    eta_seconds: null,
    error_message: null,
    output_directory: "/out",
    output_file_path: null,
    is_playlist_item: false,
    parent_playlist_id: null,
    retried_from_job_id: null,
    created_at: "2026-07-26T00:00:00Z",
    updated_at: "2026-07-26T00:00:00Z",
    title: null,
    playlist_title: null,
    queue_position: 0,
    retry_count: 0,
    next_retry_at: null,
    ...overrides,
  };
}

describe("queue store hydration (FR-114)", () => {
  beforeEach(() => {
    useQueueStore.setState({ jobs: {}, liveProgress: {} });
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockResolvedValue(undefined);
  });

  it("loads the unfinished queue back from the backend", async () => {
    vi.mocked(invoke).mockImplementation((cmd: string) => {
      if (cmd === "list_queue") {
        return Promise.resolve([makeJob({ id: "a" }), makeJob({ id: "b", status: "paused" })]);
      }
      return Promise.resolve(undefined);
    });

    await useQueueStore.getState().hydrate();

    const { jobs } = useQueueStore.getState();
    expect(invoke).toHaveBeenCalledWith("list_queue");
    expect(Object.keys(jobs).sort()).toEqual(["a", "b"]);
    // A job interrupted by the previous shutdown comes back as `paused`, which
    // is the whole point: the user can only resume what they can see.
    expect(jobs.b.status).toBe("paused");
  });

  it("keeps the store usable when the backend call fails", async () => {
    useQueueStore.setState({ jobs: { existing: makeJob({ id: "existing" }) } });
    vi.mocked(invoke).mockRejectedValue(new Error("db locked"));

    await expect(useQueueStore.getState().hydrate()).resolves.toBeUndefined();
    // A failed hydration must not blank the screen either.
    expect(Object.keys(useQueueStore.getState().jobs)).toEqual(["existing"]);
  });

  it("does not overwrite a live status update that landed mid-hydration", async () => {
    let deliverQueue!: (jobs: DownloadJob[]) => void;
    vi.mocked(invoke).mockImplementation((cmd: string) => {
      if (cmd === "list_queue") {
        return new Promise<DownloadJob[]>((resolve) => {
          deliverQueue = resolve;
        });
      }
      return Promise.resolve(undefined);
    });

    const hydration = useQueueStore.getState().hydrate();
    // `job:status_changed` arrives while the snapshot is still in flight.
    useQueueStore
      .getState()
      .upsertJob(makeJob({ id: "a", status: "downloading", progress_percent: 40 }));
    // ...and only then does the (now stale) database snapshot come back.
    deliverQueue([
      makeJob({ id: "a", status: "queued", progress_percent: 0 }),
      makeJob({ id: "b" }),
    ]);
    await hydration;

    const { jobs } = useQueueStore.getState();
    expect(jobs.a.status).toBe("downloading");
    expect(jobs.a.progress_percent).toBe(40);
    // Jobs the store had never heard of are still added.
    expect(jobs.b).toBeDefined();
  });
});

describe("queue store progress", () => {
  beforeEach(() => {
    useQueueStore.setState({ jobs: {}, liveProgress: {} });
  });

  it("records an unknown percentage as unknown rather than as zero", () => {
    useQueueStore.setState({
      jobs: { a: makeJob({ id: "a", status: "downloading", progress_percent: 12 }) },
    });

    // A real tick from an audio-only download: no total size, so no
    // percentage — but a byte count and a speed that are both true.
    useQueueStore.getState().applyProgress({
      job_id: "a",
      progress_percent: null,
      downloaded_bytes: 523_264,
      speed_bytes_per_sec: 367_853,
      eta_seconds: null,
    });

    const { jobs, liveProgress } = useQueueStore.getState();
    expect(liveProgress.a.progress_percent).toBeNull();
    expect(liveProgress.a.downloaded_bytes).toBe(523_264);
    expect(jobs.a.speed_bytes_per_sec).toBe(367_853);
    // The persisted half mirrors the database column (REAL NOT NULL): it
    // keeps the last percentage that was known instead of being reset to 0,
    // which would just move the original bug into the frontend.
    expect(jobs.a.progress_percent).toBe(12);
  });

  it("reads 100% once a job completes, whatever the last tick said", () => {
    // The completion event carries no progress of its own, so a job whose
    // ticks never had a percentage would otherwise sit at its stale value
    // forever — which is exactly how 37 rows in the real database ended up
    // looking like completed-but-0%.
    useQueueStore.setState({
      jobs: { a: makeJob({ id: "a", status: "downloading", progress_percent: 0 }) },
      liveProgress: { a: { progress_percent: null, downloaded_bytes: 523_264 } },
    });

    useQueueStore.getState().applyStatusChanged({
      job_id: "a",
      status: "completed",
      error_message: null,
      output_file_path: "/out/track.mp3",
    });

    const { jobs, liveProgress } = useQueueStore.getState();
    expect(jobs.a.progress_percent).toBe(100);
    // The live half describes a run that no longer exists — a finished job
    // must not keep rendering an indeterminate "still working" bar.
    expect(liveProgress.a).toBeUndefined();
  });

  it("leaves a failed job's partial progress alone", () => {
    // A job that failed at 43% really did fetch 43%. Forcing that to
    // anything else would destroy the one number the user has when deciding
    // whether a retry is worth it.
    useQueueStore.setState({
      jobs: { a: makeJob({ id: "a", status: "downloading", progress_percent: 43 }) },
      liveProgress: { a: { progress_percent: 43, downloaded_bytes: 4_300_000 } },
    });

    useQueueStore.getState().applyStatusChanged({
      job_id: "a",
      status: "failed",
      error_message: "network timeout",
      output_file_path: null,
    });

    expect(useQueueStore.getState().jobs.a.progress_percent).toBe(43);
    expect(useQueueStore.getState().liveProgress.a).toBeUndefined();
  });
});

describe("queue store display order", () => {
  beforeEach(() => {
    useQueueStore.setState({ jobs: {}, liveProgress: {} });
  });

  it("orders jobs by queue position rather than by insertion order", () => {
    // Fractional indexing: positions are f64 on the Rust side, so negative and
    // non-integer values are normal. Insertion order here is deliberately the
    // reverse of the expected order, so a no-op sort cannot pass.
    useQueueStore.setState({
      jobs: {
        last: makeJob({ id: "last", queue_position: 2.5 }),
        middle: makeJob({ id: "middle", queue_position: 1.5 }),
        first: makeJob({ id: "first", queue_position: -3.25 }),
      },
    });

    expect(
      useQueueStore
        .getState()
        .orderedJobs()
        .map((job) => job.id),
    ).toEqual(["first", "middle", "last"]);
  });

  it("breaks a queue position tie with the older job first", () => {
    useQueueStore.setState({
      jobs: {
        newer: makeJob({ id: "newer", queue_position: 1, created_at: "2026-07-26T10:00:00Z" }),
        older: makeJob({ id: "older", queue_position: 1, created_at: "2026-07-26T09:00:00Z" }),
      },
    });

    expect(
      useQueueStore
        .getState()
        .orderedJobs()
        .map((job) => job.id),
    ).toEqual(["older", "newer"]);
  });
});
