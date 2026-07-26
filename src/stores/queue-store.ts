import { create } from "zustand";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import i18n from "@/lib/i18n";
import type { DownloadJob, JobProgressEvent, JobStatusChangedEvent } from "@/types/download";

interface QueueState {
  jobs: Record<string, DownloadJob>;
  upsertJob: (job: DownloadJob) => void;
  upsertJobs: (jobs: DownloadJob[]) => void;
  applyProgress: (event: JobProgressEvent) => void;
  applyStatusChanged: (event: JobStatusChangedEvent) => void;
  hydrate: () => Promise<void>;
  orderedJobs: () => DownloadJob[];
  pauseJob: (jobId: string) => Promise<void>;
  resumeJob: (jobId: string) => Promise<void>;
  cancelJob: (jobId: string) => Promise<void>;
  retryJob: (jobId: string) => Promise<void>;
}

export const useQueueStore = create<QueueState>((set, get) => ({
  jobs: {},
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
            progress_percent: event.progress_percent,
            speed_bytes_per_sec: event.speed_bytes_per_sec,
            eta_seconds: event.eta_seconds,
          },
        },
      };
    }),
  applyStatusChanged: (event) =>
    set((state) => {
      const existing = state.jobs[event.job_id];
      if (!existing) return state;
      return {
        jobs: {
          ...state.jobs,
          [event.job_id]: {
            ...existing,
            status: event.status,
            error_message: event.error_message,
            output_file_path: event.output_file_path ?? existing.output_file_path,
          },
        },
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
