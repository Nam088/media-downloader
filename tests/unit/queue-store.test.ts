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
    useQueueStore.setState({ jobs: {} });
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

describe("queue store display order", () => {
  beforeEach(() => {
    useQueueStore.setState({ jobs: {} });
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
