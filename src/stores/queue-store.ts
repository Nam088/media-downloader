import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import i18n from "@/lib/i18n";
import type {
  DownloadJob,
  JobProgressEvent,
  JobStatusChangedEvent,
  LiveProgress,
} from "@/types/download";

/** Statuses a job never leaves on its own — the ones `clearFinished` hides. */
const FINISHED_STATUSES = new Set(["completed", "failed", "canceled"]);

interface QueueState {
  jobs: Record<string, DownloadJob>;
  /** Keyed by job id, and only present while that job is actually running.
   * Holds what the database has no column for — see `LiveProgress`. */
  liveProgress: Record<string, LiveProgress>;
  upsertJob: (job: DownloadJob) => void;
  upsertJobs: (jobs: DownloadJob[]) => void;
  applyProgress: (event: JobProgressEvent) => void;
  applyStatusChanged: (event: JobStatusChangedEvent) => void;
  hydrate: () => Promise<void>;
  orderedJobs: () => DownloadJob[];
  pauseAll: () => Promise<void>;
  resumeAll: () => Promise<void>;
  cancelAll: () => Promise<void>;
  clearFinished: () => void;
  moveJob: (jobId: string, beforeJobId: string | null, afterJobId: string | null) => Promise<void>;
  pauseJob: (jobId: string) => Promise<void>;
  resumeJob: (jobId: string) => Promise<void>;
  cancelJob: (jobId: string) => Promise<void>;
  retryJob: (jobId: string) => Promise<void>;
}

export const useQueueStore = create<QueueState>((set, get) => ({
  jobs: {},
  liveProgress: {},
  upsertJob: (job) => set((state) => ({ jobs: { ...state.jobs, [job.id]: job } })),
  upsertJobs: (jobs) =>
    set((state) => ({
      jobs: { ...state.jobs, ...Object.fromEntries(jobs.map((job) => [job.id, job])) },
    })),
  applyProgress: (event) =>
    set((state) => {
      const existing = state.jobs[event.job_id];
      if (!existing) return state;
      return {
        jobs: {
          ...state.jobs,
          [event.job_id]: {
            ...existing,
            // A tick with no percentage keeps the last one that was known,
            // mirroring what the database row does (`COALESCE`). It must not
            // fall back to 0: that is exactly the "unknown means zero" bug,
            // just moved into the frontend.
            progress_percent: event.progress_percent ?? existing.progress_percent,
            speed_bytes_per_sec: event.speed_bytes_per_sec,
            eta_seconds: event.eta_seconds,
          },
        },
        liveProgress: {
          ...state.liveProgress,
          [event.job_id]: {
            progress_percent: event.progress_percent,
            downloaded_bytes: event.downloaded_bytes,
          },
        },
      };
    }),
  applyStatusChanged: (event) =>
    set((state) => {
      const existing = state.jobs[event.job_id];
      if (!existing) return state;
      // A job that has stopped has no live run to describe any more, so the
      // ephemeral half goes away and the row falls back to its stored
      // numbers. For a completed job that means 100%: the backend forces it
      // on the row, and this event carries no progress of its own, so
      // without mirroring it here the queue would keep showing whatever the
      // last tick happened to say until the next hydrate.
      const finished = FINISHED_STATUSES.has(event.status);
      const liveProgress = { ...state.liveProgress };
      if (finished) delete liveProgress[event.job_id];
      return {
        jobs: {
          ...state.jobs,
          [event.job_id]: {
            ...existing,
            status: event.status,
            error_message: event.error_message,
            output_file_path: event.output_file_path ?? existing.output_file_path,
            progress_percent:
              event.status === "completed" ? 100 : existing.progress_percent,
          },
        },
        liveProgress,
      };
    }),
  /**
   * Reloads the queue from the database. The store used to live only in
   * memory, so closing the app threw away the entire visible queue even though
   * SQLite had kept every row and `list_queue` had existed, uncalled, the whole
   * time (FR-114). Jobs interrupted by the last shutdown come back as `paused`
   * and resume from their partial file — but only if the user can see them.
   *
   * Never throws: an empty queue is still a usable app, an exception during
   * startup that blanks the screen is not.
   */
  hydrate: async () => {
    try {
      const rows = await invoke<DownloadJob[]>("list_queue");
      if (!Array.isArray(rows)) return;
      set((state) => {
        const jobs = { ...state.jobs };
        for (const row of rows) {
          // Merge rather than replace, and let whatever is already in the
          // store win: a `job:status_changed` event can land while this
          // snapshot is in flight, and the event is by definition newer than
          // the rows the database returned before it fired. At startup the
          // store is empty anyway, so in practice this just adds everything.
          if (!jobs[row.id]) jobs[row.id] = row;
        }
        return { jobs };
      });
    } catch (error) {
      console.error("failed to restore the download queue", error);
    }
  },
  /** Display order must equal run order: `queue_position` first (an f64 using
   * fractional indexing, so 1.5 and -3.25 are ordinary values), then
   * `created_at` to break ties. */
  orderedJobs: () =>
    Object.values(get().jobs).sort(
      (a, b) => a.queue_position - b.queue_position || a.created_at.localeCompare(b.created_at),
    ),
  /* The three bulk commands below return the ids they actually changed, but we
   * deliberately drop that list: the backend already emits one
   * `job:status_changed` per affected job before returning, and
   * `applyStatusChanged` has written the new status by the time the promise
   * resolves. Re-applying the returned ids here would either be a no-op or,
   * worse, overwrite a newer status that landed in between. */
  pauseAll: async () => {
    await invoke("pause_all_jobs");
  },
  resumeAll: async () => {
    await invoke("resume_all_jobs");
  },
  cancelAll: async () => {
    await invoke("cancel_all_jobs");
  },
  /**
   * Drops finished jobs from the queue view only. Nothing is deleted: those
   * rows are exactly what the History page reads, so this must never reach the
   * database (FR-118). A later `hydrate()` would bring them back, which is
   * fine — this is a "tidy up what I'm looking at" action, not a delete.
   */
  clearFinished: () => {
    set((state) => ({
      jobs: Object.fromEntries(
        Object.entries(state.jobs).filter(([, job]) => !FINISHED_STATUSES.has(job.status)),
      ),
    }));
  },
  /**
   * Puts a job between two new neighbours (either may be null for head/tail).
   *
   * Only the moved job and its two neighbour ids go over the wire — never a
   * full ordered list. A job enqueued while the drag was in flight would
   * otherwise have its position clobbered by our stale snapshot.
   *
   * The optimistic position uses the same formula the backend does (midpoint,
   * or ±1.0 at the ends) so the row lands where it was dropped instead of
   * snapping back for the length of the round-trip. If the command fails —
   * `NOT_FOUND` when a neighbour finished mid-drag, `INVALID_ARGUMENT` when the
   * ids collide — the guess is thrown away and real positions are re-read,
   * because at that point we have no idea whether the move landed.
   */
  moveJob: async (jobId, beforeJobId, afterJobId) => {
    const { jobs } = get();
    const before = beforeJobId ? jobs[beforeJobId]?.queue_position : undefined;
    const after = afterJobId ? jobs[afterJobId]?.queue_position : undefined;

    const optimisticPosition =
      before !== undefined && after !== undefined
        ? (before + after) / 2
        : before !== undefined
          ? before + 1
          : after !== undefined
            ? after - 1
            : 0;

    set((state) =>
      state.jobs[jobId]
        ? {
            jobs: {
              ...state.jobs,
              [jobId]: { ...state.jobs[jobId], queue_position: optimisticPosition },
            },
          }
        : state,
    );

    try {
      await invoke("reorder_queue", { jobId, beforeJobId, afterJobId });
    } catch (error) {
      console.error("failed to reorder the queue", error);
      toast.error(i18n.t("queue.reorder_failed", { defaultValue: "Could not reorder the queue." }));
      // Not a rollback to the remembered position: the failure may itself have
      // been caused by the queue moving underneath us, so re-read instead of
      // guessing. `upsertJobs`, not `hydrate` — hydrate only adds jobs it has
      // never seen and would leave the bad position in place.
      try {
        const rows = await invoke<DownloadJob[]>("list_queue");
        if (Array.isArray(rows)) get().upsertJobs(rows);
      } catch (resyncError) {
        console.error("failed to re-read the queue after a rejected reorder", resyncError);
      }
    }
  },
  pauseJob: async (jobId) => {
    await invoke("pause_job", { jobId });
  },
  resumeJob: async (jobId) => {
    await invoke("resume_job", { jobId });
  },
  cancelJob: async (jobId) => {
    await invoke("cancel_job", { jobId });
  },
  retryJob: async (jobId) => {
    const job = await invoke<DownloadJob>("retry_job", { jobId });
    get().upsertJob(job);
  },
}));

let listenersInitialized = false;

/** Registers the `job:progress`/`job:status_changed` listeners exactly once
 * per app session (contracts/tauri-commands.md). Safe to call from multiple
 * components — only the first call actually subscribes. */
export function ensureQueueListeners() {
  if (listenersInitialized) return;
  listenersInitialized = true;

  listen<JobProgressEvent>("job:progress", (event) => {
    useQueueStore.getState().applyProgress(event.payload);
  });
  listen<JobStatusChangedEvent>("job:status_changed", (event) => {
    useQueueStore.getState().applyStatusChanged(event.payload);

    // Completing a download shouldn't require a trip to the History tab
    // just to find the file — offer to open its folder right from here.
    if (event.payload.status === "completed") {
      const jobId = event.payload.job_id;
      toast.success(i18n.t("queue.download_completed"), {
        action: {
          label: i18n.t("history.open_folder"),
          onClick: () => {
            invoke("open_containing_folder", { jobId }).catch(() => {
              toast.error(i18n.t("errors.NOT_FOUND"));
            });
          },
        },
      });
    }
  });
}
