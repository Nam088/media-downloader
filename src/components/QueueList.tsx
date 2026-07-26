import { useEffect, useState } from "react";
import { useShallow } from "zustand/react/shallow";
import { useTranslation } from "react-i18next";
import { ChevronDown, ChevronRight, Pause, Play, X, RotateCcw } from "lucide-react";
import { Progress } from "@/components/ui/progress";
import { Button } from "@/components/ui/button";
import { QueueToolbar } from "@/components/QueueToolbar";
import { ensureQueueListeners, useQueueStore } from "@/stores/queue-store";
import { formatSpeed } from "@/lib/format";
import type { DownloadJob } from "@/types/download";

const ACTIVE_STATUSES = new Set(["queued", "fetching_metadata", "downloading", "paused"]);
/** What "Pause all" would act on. `paused` is counted separately. */
const RUNNING_STATUSES = new Set(["queued", "fetching_metadata", "downloading"]);
const FINISHED_STATUSES = new Set(["completed", "failed", "canceled"]);
// A playlist group keeps showing a finished item instead of dropping it (see
// PlaylistGroup below). It only disappears once every one of its jobs
// reaches one of these fully-resolved states.
const RESOLVED_STATUSES = new Set(["completed", "canceled"]);

function JobControls({ job }: { job: DownloadJob }) {
  const { pauseJob, resumeJob, cancelJob, retryJob } = useQueueStore();

  if (job.status === "downloading" || job.status === "queued" || job.status === "fetching_metadata") {
    return (
      <div className="flex gap-1">
        <Button variant="ghost" size="icon" className="h-7 w-7" onClick={() => pauseJob(job.id)}>
          <Pause className="h-3.5 w-3.5" />
        </Button>
        <Button variant="ghost" size="icon" className="h-7 w-7" onClick={() => cancelJob(job.id)}>
          <X className="h-3.5 w-3.5" />
        </Button>
      </div>
    );
  }

  if (job.status === "paused") {
    return (
      <div className="flex gap-1">
        <Button variant="ghost" size="icon" className="h-7 w-7" onClick={() => resumeJob(job.id)}>
          <Play className="h-3.5 w-3.5" />
        </Button>
        <Button variant="ghost" size="icon" className="h-7 w-7" onClick={() => cancelJob(job.id)}>
          <X className="h-3.5 w-3.5" />
        </Button>
      </div>
    );
  }

  if (job.status === "failed") {
    return (
      <Button variant="ghost" size="icon" className="h-7 w-7" onClick={() => retryJob(job.id)}>
        <RotateCcw className="h-3.5 w-3.5" />
      </Button>
    );
  }

  return null;
}

/**
 * Seconds left until this job's next retry, or `null` when it is not waiting
 * for one. A job between attempts has no status of its own: it is `queued`
 * with `next_retry_at` in the future, and the dispatcher skips it until then.
 *
 * Ticks once a second so the number actually counts down (FR-122), and only
 * while there is something to count — an interval per queued job would
 * otherwise wake the app up forever.
 */
function useRetryCountdown(job: DownloadJob): number | null {
  const [now, setNow] = useState(() => Date.now());
  const waiting = job.status === "queued" && job.next_retry_at !== null;

  useEffect(() => {
    if (!waiting) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
    // Deliberately not keyed on `next_retry_at`: if the deadline moves while
    // the job is still waiting, the running interval already reports it.
  }, [waiting]);

  if (!waiting || !job.next_retry_at) return null;
  const remaining = Math.ceil((new Date(job.next_retry_at).getTime() - now) / 1000);
  // A deadline in the past means the dispatcher is about to pick the job up:
  // showing "in -3s" would be worse than showing the plain "Queued" status.
  return remaining > 0 ? remaining : null;
}

/** One job's row, shared by standalone jobs and playlist-group children.
 * `title` falls back to `source_url` for jobs created before that field
 * existed, or paths where the backend never had a title to begin with. */
function JobRow({ job }: { job: DownloadJob }) {
  const { t } = useTranslation();
  const retryCountdown = useRetryCountdown(job);
  return (
    <div className="rounded-md border border-border/80 bg-card p-3 shadow-2xs transition-all">
      <div className="flex items-center justify-between gap-3">
        <span className="truncate text-sm font-medium text-foreground">{job.title ?? job.source_url}</span>
        <div className="flex shrink-0 items-center gap-2">
          <span className="rounded-sm bg-muted/80 px-1.5 py-0.5 text-[11px] font-medium text-muted-foreground capitalize">
            {retryCountdown === null
              ? t(`queue.status.${job.status}`)
              : t("queue.retry_countdown", {
                  defaultValue: "Retrying in {{seconds}}s (attempt {{attempt}})",
                  seconds: retryCountdown,
                  attempt: job.retry_count + 1,
                })}
          </span>
          <JobControls job={job} />
        </div>
      </div>
      <Progress value={job.progress_percent} className="mt-3 h-2 rounded-full bg-muted" />
      <div className="mt-2 flex justify-between text-xs font-mono text-muted-foreground">
        <span>{formatSpeed(job.speed_bytes_per_sec)}</span>
        <span className="font-semibold text-foreground/80">{Math.round(job.progress_percent)}%</span>
      </div>
    </div>
  );
}

/** All jobs fanned out from one playlist submission, collapsed under a
 * single header (playlist title, completed count, aggregate progress)
 * instead of showing as N unrelated rows. Expanded by default; every child
 * keeps rendering with its own real status (including "Completed") for as
 * long as the group itself is visible, so the overall progress reads
 * correctly instead of shrinking as items finish. */
function PlaylistGroup({
  jobs,
  collapsed,
  onToggle,
}: {
  jobs: DownloadJob[];
  collapsed: boolean;
  onToggle: () => void;
}) {
  const { t } = useTranslation();
  const total = jobs.length;
  const completedCount = jobs.filter((job) => job.status === "completed").length;
  const averageProgress = jobs.reduce((sum, job) => sum + job.progress_percent, 0) / total;
  const title = jobs[0]?.playlist_title ?? t("queue.playlist_fallback_title");

  return (
    <div className="rounded-md border border-border/80 bg-card p-3 shadow-2xs transition-all">
      <button
        type="button"
        onClick={onToggle}
        aria-expanded={!collapsed}
        title={t(collapsed ? "queue.expand_group" : "queue.collapse_group")}
        className="flex w-full items-center justify-between gap-3 text-left"
      >
        <div className="flex min-w-0 items-center gap-2">
          {collapsed ? (
            <ChevronRight className="h-4 w-4 shrink-0 text-muted-foreground" />
          ) : (
            <ChevronDown className="h-4 w-4 shrink-0 text-muted-foreground" />
          )}
          <span className="truncate text-sm font-semibold text-foreground">{title}</span>
        </div>
        <span className="shrink-0 text-xs font-medium text-muted-foreground">
          {t("queue.playlist_progress", { completed: completedCount, total })}
        </span>
      </button>
      <Progress value={averageProgress} className="mt-3 h-2 rounded-full bg-muted" />
      {!collapsed && (
        <div className="mt-3 flex flex-col gap-2 border-t border-border/60 pt-3">
          {jobs.map((job) => (
            <JobRow key={job.id} job={job} />
          ))}
        </div>
      )}
    </div>
  );
}

export function QueueList() {
  const { t } = useTranslation();
  // Display order comes from the store so that it always equals run order.
  // `useShallow` is load-bearing: `orderedJobs()` builds a fresh array on every
  // call, and an unwrapped selector would hand `useSyncExternalStore` a new
  // reference each time and re-render forever.
  const allJobs = useQueueStore(useShallow((state) => state.orderedJobs()));
  const moveJob = useQueueStore((state) => state.moveJob);
  const [collapsedGroups, setCollapsedGroups] = useState<Set<string>>(new Set());
  const [draggingId, setDraggingId] = useState<string | null>(null);

  useEffect(() => {
    ensureQueueListeners();
    // The queue outlives the window: reload whatever the database still holds.
    void useQueueStore.getState().hydrate();
  }, []);

  const groupsMap = new Map<string, DownloadJob[]>();
  for (const job of allJobs) {
    if (job.is_playlist_item && job.parent_playlist_id) {
      const existing = groupsMap.get(job.parent_playlist_id);
      if (existing) existing.push(job);
      else groupsMap.set(job.parent_playlist_id, [job]);
    }
  }
  const visibleGroups = Array.from(groupsMap.entries()).filter(([, children]) =>
    children.some((job) => !RESOLVED_STATUSES.has(job.status)),
  );
  const standaloneJobs = allJobs.filter(
    (job) => !(job.is_playlist_item && job.parent_playlist_id) && ACTIVE_STATUSES.has(job.status),
  );

  function toggleGroup(playlistId: string) {
    setCollapsedGroups((prev) => {
      const next = new Set(prev);
      if (next.has(playlistId)) next.delete(playlistId);
      else next.add(playlistId);
      return next;
    });
  }

  /** Only jobs that have not started yet can be reordered — a running job has
   * already taken its slot, so moving it in the queue would change nothing
   * (FR-119). */
  function isDraggable(job: DownloadJob) {
    return job.status === "queued" || job.status === "paused";
  }

  function handleDrop(targetId: string) {
    if (!draggingId || draggingId === targetId) return;

    // Work out the list as it will look after the drop, purely to read off the
    // dragged job's two new neighbours. Only those three ids are sent — the
    // backend writes one row from them, so a job enqueued mid-drag keeps its
    // place instead of being renumbered from a stale snapshot.
    const ids = standaloneJobs.map((job) => job.id);
    const from = ids.indexOf(draggingId);
    const to = ids.indexOf(targetId);
    if (from === -1 || to === -1) return;

    ids.splice(to, 0, ids.splice(from, 1)[0]);
    const landed = ids.indexOf(draggingId);

    setDraggingId(null);
    void moveJob(draggingId, ids[landed - 1] ?? null, ids[landed + 1] ?? null);
  }

  return (
    <div className="flex flex-col gap-2">
      <QueueToolbar
        activeCount={allJobs.filter((job) => RUNNING_STATUSES.has(job.status)).length}
        pausedCount={allJobs.filter((job) => job.status === "paused").length}
        finishedCount={allJobs.filter((job) => FINISHED_STATUSES.has(job.status)).length}
      />
      {visibleGroups.length === 0 && standaloneJobs.length === 0 ? (
        <p className="text-sm text-muted-foreground">{t("queue.empty")}</p>
      ) : (
        <>
          {visibleGroups.map(([playlistId, children]) => (
            <PlaylistGroup
              key={playlistId}
              jobs={children}
              collapsed={collapsedGroups.has(playlistId)}
              onToggle={() => toggleGroup(playlistId)}
            />
          ))}
          <ul className="flex list-none flex-col gap-2 p-0">
            {standaloneJobs.map((job) => (
              <li
                key={job.id}
                draggable={isDraggable(job)}
                onDragStart={(event) => {
                  setDraggingId(job.id);
                  // Firefox refuses to start a drag without payload; the id is
                  // also what a drop from outside the list would carry.
                  event.dataTransfer?.setData("text/plain", job.id);
                }}
                onDragEnd={() => setDraggingId(null)}
                onDragOver={(event) => {
                  // Preventing the default is what marks this row as a valid
                  // drop target at all.
                  if (draggingId) event.preventDefault();
                }}
                onDrop={(event) => {
                  event.preventDefault();
                  handleDrop(job.id);
                }}
                className={
                  isDraggable(job)
                    ? `cursor-grab ${draggingId === job.id ? "opacity-50" : ""}`
                    : undefined
                }
              >
                <JobRow job={job} />
              </li>
            ))}
          </ul>
        </>
      )}
    </div>
  );
}
