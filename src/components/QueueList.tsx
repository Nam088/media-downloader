import { useEffect, useState } from "react";
import { useShallow } from "zustand/react/shallow";
import { useTranslation } from "react-i18next";
import { ChevronDown, ChevronRight, Pause, Play, X, RotateCcw } from "lucide-react";
import { Progress } from "@/components/ui/progress";
import { Button } from "@/components/ui/button";
import { ensureQueueListeners, useQueueStore } from "@/stores/queue-store";
import { formatSpeed } from "@/lib/format";
import type { DownloadJob } from "@/types/download";

const ACTIVE_STATUSES = new Set(["queued", "fetching_metadata", "downloading", "paused"]);
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

/** One job's row, shared by standalone jobs and playlist-group children.
 * `title` falls back to `source_url` for jobs created before that field
 * existed, or paths where the backend never had a title to begin with. */
function JobRow({ job }: { job: DownloadJob }) {
  const { t } = useTranslation();
  return (
    <div className="rounded-md border border-border/80 bg-card p-3 shadow-2xs transition-all">
      <div className="flex items-center justify-between gap-3">
        <span className="truncate text-sm font-medium text-foreground">{job.title ?? job.source_url}</span>
        <div className="flex shrink-0 items-center gap-2">
          <span className="rounded-sm bg-muted/80 px-1.5 py-0.5 text-[11px] font-medium text-muted-foreground capitalize">
            {t(`queue.status.${job.status}`)}
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
  const [collapsedGroups, setCollapsedGroups] = useState<Set<string>>(new Set());

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

  if (visibleGroups.length === 0 && standaloneJobs.length === 0) {
    return <p className="text-sm text-muted-foreground">{t("queue.empty")}</p>;
  }

  function toggleGroup(playlistId: string) {
    setCollapsedGroups((prev) => {
      const next = new Set(prev);
      if (next.has(playlistId)) next.delete(playlistId);
      else next.add(playlistId);
      return next;
    });
  }

  return (
    <div className="flex flex-col gap-2">
      {visibleGroups.map(([playlistId, children]) => (
        <PlaylistGroup
          key={playlistId}
          jobs={children}
          collapsed={collapsedGroups.has(playlistId)}
          onToggle={() => toggleGroup(playlistId)}
        />
      ))}
      {standaloneJobs.map((job) => (
        <JobRow key={job.id} job={job} />
      ))}
    </div>
  );
}
