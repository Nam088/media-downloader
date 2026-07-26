import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ensureQueueListeners, useQueueStore } from "@/stores/queue-store";
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

  it("keeps the provider a music job's progress tick names (T021)", () => {
    useQueueStore.setState({
      jobs: { a: makeJob({ id: "a", media_type: "music", status: "downloading" }) },
    });

    useQueueStore.getState().applyProgress({
      job_id: "a",
      progress_percent: 12,
      downloaded_bytes: 1_000_000,
      speed_bytes_per_sec: 500_000,
      eta_seconds: 30,
      provider: "tidal",
    });

    expect(useQueueStore.getState().liveProgress.a.provider).toBe("tidal");

    // A later tick without a provider keeps the last one that was named —
    // the worker only repeats the field when it actually switches source.
    useQueueStore.getState().applyProgress({
      job_id: "a",
      progress_percent: 20,
      downloaded_bytes: 2_000_000,
      speed_bytes_per_sec: 500_000,
      eta_seconds: 25,
    });

    expect(useQueueStore.getState().liveProgress.a.provider).toBe("tidal");

    // ...and a tick that does name a new provider replaces it.
    useQueueStore.getState().applyProgress({
      job_id: "a",
      progress_percent: 21,
      downloaded_bytes: 2_100_000,
      speed_bytes_per_sec: 400_000,
      eta_seconds: 24,
      provider: "qobuz",
    });

    expect(useQueueStore.getState().liveProgress.a.provider).toBe("qobuz");
  });

  it("applies a waiting_input status change without discarding the live run (T021)", () => {
    // A music job blocked on a Cloudflare challenge is still *running* — the
    // worker process is alive, holding its slot, waiting on stdin. The live
    // half must survive so the row keeps its provider/bytes while waiting.
    useQueueStore.setState({
      jobs: { a: makeJob({ id: "a", media_type: "music", status: "downloading" }) },
      liveProgress: { a: { progress_percent: 55, downloaded_bytes: 5_000_000, provider: "tidal" } },
    });

    useQueueStore.getState().applyStatusChanged({
      job_id: "a",
      status: "waiting_input",
      error_message: null,
      output_file_path: null,
    });

    const { jobs, liveProgress } = useQueueStore.getState();
    expect(jobs.a.status).toBe("waiting_input");
    expect(liveProgress.a).toEqual({
      progress_percent: 55,
      downloaded_bytes: 5_000_000,
      provider: "tidal",
    });
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

describe("queue store cloudflare challenges (T036)", () => {
  const CHALLENGE = {
    challengeUrl: "https://challenge.example/verify",
    attempts: 0,
    dismissed: false,
  };

  beforeEach(() => {
    useQueueStore.setState({ jobs: {}, liveProgress: {}, challenges: {} });
    vi.mocked(invoke).mockReset();
    vi.mocked(invoke).mockResolvedValue(undefined);
  });

  it("records the challenge url a waiting job is blocked on", () => {
    useQueueStore.setState({
      jobs: { a: makeJob({ id: "a", media_type: "music", status: "downloading" }) },
    });

    useQueueStore.getState().applyChallenge({
      job_id: "a",
      challenge_url: "https://challenge.example/verify?token=xyz",
    });

    expect(useQueueStore.getState().challenges.a).toEqual({
      challengeUrl: "https://challenge.example/verify?token=xyz",
      attempts: 0,
      dismissed: false,
    });
  });

  it("keeps a challenge that arrives before the job row does", () => {
    // The challenge event can beat the status change that introduces the row;
    // dropping it here would lose the only copy of the URL the UI has.
    useQueueStore.getState().applyChallenge({
      job_id: "not-yet-known",
      challenge_url: "https://challenge.example/verify",
    });

    expect(useQueueStore.getState().challenges["not-yet-known"]?.challengeUrl).toBe(
      "https://challenge.example/verify",
    );
  });

  it("counts a re-emitted challenge as a burned attempt and re-surfaces it", () => {
    // The worker rejecting a grant re-emits the event for the same job — that
    // repeat *is* the attempt counter (data-model.md §6). It also clears a
    // dismissal: a fresh rejection is new news the user has to see.
    useQueueStore.setState({
      challenges: { a: { ...CHALLENGE, dismissed: true } },
    });

    useQueueStore.getState().applyChallenge({
      job_id: "a",
      challenge_url: "https://challenge.example/verify?fresh=1",
    });

    expect(useQueueStore.getState().challenges.a).toEqual({
      challengeUrl: "https://challenge.example/verify?fresh=1",
      attempts: 1,
      dismissed: false,
    });
  });

  it("drops the challenge as soon as the job leaves waiting_input", () => {
    useQueueStore.setState({
      jobs: { a: makeJob({ id: "a", media_type: "music", status: "waiting_input" }) },
      challenges: { a: CHALLENGE },
    });

    // The grant was accepted and the worker started streaming again.
    useQueueStore.getState().applyStatusChanged({
      job_id: "a",
      status: "downloading",
      error_message: null,
      output_file_path: null,
    });

    expect(useQueueStore.getState().challenges.a).toBeUndefined();
  });

  it("keeps the challenge while the job is still waiting on it", () => {
    useQueueStore.setState({
      jobs: { a: makeJob({ id: "a", media_type: "music", status: "downloading" }) },
      challenges: { a: CHALLENGE },
    });

    useQueueStore.getState().applyStatusChanged({
      job_id: "a",
      status: "waiting_input",
      error_message: null,
      output_file_path: null,
    });

    expect(useQueueStore.getState().challenges.a).toEqual(CHALLENGE);
  });

  it("drops the challenge when the wait times out into a failure", () => {
    useQueueStore.setState({
      jobs: { a: makeJob({ id: "a", media_type: "music", status: "waiting_input" }) },
      challenges: { a: CHALLENGE },
    });

    useQueueStore.getState().applyStatusChanged({
      job_id: "a",
      status: "failed",
      error_message: "SPOTIFLAC_CHALLENGE_TIMEOUT",
      output_file_path: null,
    });

    expect(useQueueStore.getState().challenges.a).toBeUndefined();
  });

  it("flags a dismissed challenge instead of deleting it", () => {
    // Deleting would let the reload-recovery path immediately re-fetch and
    // reopen the dialog the user just closed.
    useQueueStore.setState({ challenges: { a: CHALLENGE } });

    useQueueStore.getState().dismissChallenge("a");

    expect(useQueueStore.getState().challenges.a).toEqual({ ...CHALLENGE, dismissed: true });
  });

  it("sends the grant code to the waiting worker", async () => {
    await useQueueStore.getState().submitGrant("a", "grant-code-123");

    expect(invoke).toHaveBeenCalledWith("submit_cloudflare_grant", {
      jobId: "a",
      grant: "grant-code-123",
    });
  });

  it("restores a lost challenge from the backend after a reload", async () => {
    vi.mocked(invoke).mockImplementation((cmd: string) =>
      Promise.resolve(
        cmd === "get_pending_challenge"
          ? { challenge_url: "https://challenge.example/restored" }
          : undefined,
      ),
    );

    await useQueueStore.getState().restorePendingChallenge("a");

    expect(invoke).toHaveBeenCalledWith("get_pending_challenge", { jobId: "a" });
    expect(useQueueStore.getState().challenges.a).toEqual({
      challengeUrl: "https://challenge.example/restored",
      attempts: 0,
      dismissed: false,
    });
  });

  it("does not ask the backend when the challenge is already live", async () => {
    useQueueStore.setState({ challenges: { a: CHALLENGE } });

    await useQueueStore.getState().restorePendingChallenge("a");

    expect(invoke).not.toHaveBeenCalled();
    expect(useQueueStore.getState().challenges.a).toEqual(CHALLENGE);
  });

  it("reopens a dismissed challenge with a re-read url and its attempts intact", async () => {
    // "Verify now" on the queue row: the URL is re-read from the backend (the
    // only authoritative copy), the dismissal is cleared so the dialog opens
    // again, and the burned attempts carry over — dismissing was never a free
    // retry.
    useQueueStore.setState({ challenges: { a: { ...CHALLENGE, attempts: 2, dismissed: true } } });
    vi.mocked(invoke).mockImplementation((cmd: string) =>
      Promise.resolve(
        cmd === "get_pending_challenge"
          ? { challenge_url: "https://challenge.example/fresh" }
          : undefined,
      ),
    );

    await useQueueStore.getState().restorePendingChallenge("a");

    expect(useQueueStore.getState().challenges.a).toEqual({
      challengeUrl: "https://challenge.example/fresh",
      attempts: 2,
      dismissed: false,
    });
  });

  it("stores nothing when the backend has no pending challenge either", async () => {
    vi.mocked(invoke).mockResolvedValue(null);

    await useQueueStore.getState().restorePendingChallenge("a");

    expect(useQueueStore.getState().challenges.a).toBeUndefined();
  });

  it("routes the job:cloudflare_challenge event into the store", async () => {
    ensureQueueListeners();
    const registration = vi
      .mocked(listen)
      .mock.calls.find(([eventName]) => eventName === "job:cloudflare_challenge");
    expect(registration).toBeDefined();

    const handler = registration![1] as (event: {
      payload: { job_id: string; challenge_url: string };
    }) => void;
    handler({ payload: { job_id: "a", challenge_url: "https://challenge.example/verify" } });

    expect(useQueueStore.getState().challenges.a?.challengeUrl).toBe(
      "https://challenge.example/verify",
    );
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
