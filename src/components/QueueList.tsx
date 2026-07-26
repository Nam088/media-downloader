import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { useShallow } from "zustand/react/shallow";
import { useTranslation } from "react-i18next";
import { ChevronDown, ChevronRight, GripVertical, Pause, Play, X, RotateCcw } from "lucide-react";
import { Progress } from "@/components/ui/progress";
import { Button } from "@/components/ui/button";
import { QueueToolbar } from "@/components/QueueToolbar";
import { CloudflareGrantDialog } from "@/components/CloudflareGrantDialog";
import { ensureQueueListeners, useQueueStore } from "@/stores/queue-store";
import { formatFileSize, formatPlatformLabel, formatSpeed } from "@/lib/format";
import type { DownloadJob } from "@/types/download";

const ACTIVE_STATUSES = new Set([
  "queued",
  "fetching_metadata",
  "downloading",
  "waiting_input",
  "paused",
]);
/** What "Pause all" would act on. `paused` is counted separately. */
const RUNNING_STATUSES = new Set(["queued", "fetching_metadata", "downloading"]);
const FINISHED_STATUSES = new Set(["completed", "failed", "canceled"]);
// A playlist group keeps showing a finished item instead of dropping it (see
// PlaylistGroup below). It only disappears once every one of its jobs
// reaches one of these fully-resolved states.
const RESOLVED_STATUSES = new Set(["completed", "canceled"]);

/**
 * How far the pointer must travel before a press on a drag handle becomes a
 * reorder. Without it, the tiny movement between pressing and releasing a
 * mouse button would be read as a drag and swallow ordinary clicks.
 */
const DRAG_THRESHOLD_PX = 4;

/**
 * The press that may or may not become a drag. Kept in a ref rather than
 * state because every pointer event has to read the *current* value
 * synchronously — a stale closure here would drop the gesture.
 */
type DragOrigin = {
  jobId: string;
  pointerId: number;
  startX: number;
  startY: number;
  /** Set once the pointer has travelled past `DRAG_THRESHOLD_PX`. */
  started: boolean;
  /** Id of the row the pointer was last over, i.e. what the indicator shows. */
  overId: string | null;
};

function JobControls({ job, onVerify }: { job: DownloadJob; onVerify: (jobId: string) => void }) {
  const { t } = useTranslation();
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

  // A music job waiting on a Cloudflare grant cannot be paused — that would
  // kill the worker holding the challenge open (data-model.md §2). The way
  // forward is the grant dialog; the way out is cancelling.
  //
  // "Verify now" is the only way back to a challenge the user dismissed, so
  // the row must keep offering it for as long as the job is blocked.
  if (job.status === "waiting_input") {
    return (
      <div className="flex gap-1">
        <Button
          variant="outline"
          size="sm"
          className="h-7 px-2 text-xs font-semibold"
          onClick={() => onVerify(job.id)}
        >
          {t("queue.openChallenge")}
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

/** Progress bar for a job whose percentage is genuinely unknown: yt-dlp
 * reported no total size, so there is no fraction to fill. A sliding bar says
 * "working, amount unknown" — which is the truth — where a 0%-wide bar said
 * "nothing has happened yet" for the whole download.
 *
 * Deliberately not `<Progress>`: Radix's progress bar is built to express a
 * value out of a maximum, and this state has neither. It also carries no
 * `aria-valuenow`, so assistive tech reads it as indeterminate rather than as
 * some specific number. */
function IndeterminateProgress({ label }: { label: string }) {
  return (
    <div
      role="progressbar"
      aria-label={label}
      aria-valuetext={label}
      className="mt-3 h-2 w-full overflow-hidden rounded-full bg-muted"
    >
      <div className="h-full w-1/3 rounded-full bg-primary animate-progress-indeterminate" />
    </div>
  );
}

/** One job's row, shared by standalone jobs and playlist-group children.
 * `title` falls back to `source_url` for jobs created before that field
 * existed, or paths where the backend never had a title to begin with. */
function JobRow({ job, onVerify }: { job: DownloadJob; onVerify: (jobId: string) => void }) {
  const { t } = useTranslation();
  const retryCountdown = useRetryCountdown(job);
  // Present only while this job is actually running. `progress_percent: null`
  // in it means the source never reported a total size (audio-only formats,
  // HLS) — a state the persisted row cannot express, since its column is
  // `REAL NOT NULL` and holds the last percentage that *was* known.
  const live = useQueueStore((state) => state.liveProgress[job.id]);
  const percentUnknown = live !== undefined && live.progress_percent === null;
  return (
    <div className="rounded-md border border-border/80 bg-card p-3 shadow-2xs transition-all">
      <div className="flex items-center justify-between gap-3">
        <span className="truncate text-sm font-medium text-foreground">{job.title ?? job.source_url}</span>
        <div className="flex shrink-0 items-center gap-2">
          {/* `waiting_input` gets the warning treatment: it is the one status
              where nothing moves until the user acts, so it must not read
              like an ordinary passive state such as "Paused". */}
          <span
            className={`rounded-sm px-1.5 py-0.5 text-[11px] font-medium capitalize ${
              job.status === "waiting_input"
                ? "bg-amber-500/10 text-amber-600 dark:text-amber-400"
                : "bg-muted/80 text-muted-foreground"
            }`}
          >
            {retryCountdown !== null
              ? t("queue.retry_countdown", {
                  seconds: retryCountdown,
                  attempt: job.retry_count + 1,
                })
              : job.status === "waiting_input"
                ? t("queue.waitingInput")
                : t(`queue.status.${job.status}`)}
          </span>
          <JobControls job={job} onVerify={onVerify} />
        </div>
      </div>
      {percentUnknown ? (
        <IndeterminateProgress label={t("queue.progress_unknown")} />
      ) : (
        <Progress value={job.progress_percent} className="mt-3 h-2 rounded-full bg-muted" />
      )}
      <div className="mt-2 flex justify-between text-xs font-mono text-muted-foreground">
        <span className="flex items-center gap-2">
          <span>{formatSpeed(job.speed_bytes_per_sec)}</span>
          {/* FR-009 — which provider a music job is actually pulling from
              right now. Live-only, like the speed beside it: the value comes
              with the progress ticks and disappears with the run. */}
          {job.media_type === "music" && live?.provider && (
            <span className="rounded-sm bg-muted/80 px-1.5 py-0.5 font-sans font-medium">
              {t("queue.provider", { provider: formatPlatformLabel(live.provider) })}
            </span>
          )}
        </span>
        {/* With no percentage to show, show what is actually known instead of
            a "0%" that is simply false — the bytes fetched so far come in the
            same payload as the missing total. */}
        <span className="font-semibold text-foreground/80">
          {percentUnknown
            ? t("queue.downloaded_so_far", { size: formatFileSize(live.downloaded_bytes) })
            : `${Math.round(job.progress_percent)}%`}
        </span>
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
  onVerify,
}: {
  jobs: DownloadJob[];
  collapsed: boolean;
  onToggle: () => void;
  onVerify: (jobId: string) => void;
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
            <JobRow key={job.id} job={job} onVerify={onVerify} />
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
  // One dialog for the whole queue, not one per row: two music jobs can be
  // challenged at once, and stacking two modals over each other would leave
  // the user unable to tell which download they are verifying. The first
  // undismissed challenge wins; solving or dismissing it reveals the next.
  const challenges = useQueueStore((state) => state.challenges);
  const dismissChallenge = useQueueStore((state) => state.dismissChallenge);
  const restorePendingChallenge = useQueueStore((state) => state.restorePendingChallenge);
  // Reopening a dismissed challenge is a view-level decision, so it stays
  // here rather than un-setting the store's `dismissed` flag: the store
  // records what the *backend* is waiting on, not which modal is on screen.
  const [reopenedJobId, setReopenedJobId] = useState<string | null>(null);
  const openChallenge =
    Object.entries(challenges).find(([, entry]) => !entry.dismissed) ??
    (reopenedJobId && challenges[reopenedJobId]
      ? ([reopenedJobId, challenges[reopenedJobId]] as const)
      : undefined);

  /** "Verify now" on a blocked row. The store call is what recovers a
   * challenge this window never saw (a reload drops the URL, which lives only
   * in the Rust process); when the entry is already known it is a no-op and
   * the local flag alone reopens the dialog. */
  function handleVerify(jobId: string) {
    setReopenedJobId(jobId);
    void restorePendingChallenge(jobId);
  }
  const [collapsedGroups, setCollapsedGroups] = useState<Set<string>>(new Set());
  // Reordering runs on pointer events, not the HTML5 drag-and-drop API: this
  // is a Tauri v2 webview, where `dragDropEnabled` defaults to true and the
  // OS-level drag handler swallows `dragstart`/`drop` before the page sees
  // them. Turning that flag off would fix reordering but also kill the
  // `tauri://drag-drop` event that dropping a .txt of URLs onto the window
  // needs (FR-104/FR-105) — one switch governs both. Pointer events are never
  // routed through that handler, so they work either way.
  const [draggingId, setDraggingId] = useState<string | null>(null);
  const [dropTarget, setDropTarget] = useState<{ id: string; position: "before" | "after" } | null>(null);
  const rowRefs = useRef(new Map<string, HTMLLIElement>());
  const dragOrigin = useRef<DragOrigin | null>(null);

  useEffect(() => {
    ensureQueueListeners();
    // The queue outlives the window: reload whatever the database still holds.
    void useQueueStore.getState().hydrate();
  }, []);

  // Escape abandons a drag in progress without committing it — the usual way
  // out of a gesture the user started by accident. Only listening while a drag
  // is actually running keeps the handler off every other keystroke.
  useEffect(() => {
    if (!draggingId) return;
    function onKeyDown(event: KeyboardEvent) {
      if (event.key !== "Escape") return;
      dragOrigin.current = null;
      setDraggingId(null);
      setDropTarget(null);
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [draggingId]);

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

  function resetDrag() {
    dragOrigin.current = null;
    setDraggingId(null);
    setDropTarget(null);
  }

  /** Which row the pointer is over, found by geometry rather than by event
   * target: once a row's handle has captured the pointer, every move and up
   * event is delivered to that handle no matter where the cursor actually is.
   * A pointer past either end of the list snaps to the nearest row. */
  function rowUnderPointer(clientY: number): string | null {
    let nearestId: string | null = null;
    let nearestDistance = Number.POSITIVE_INFINITY;
    for (const job of standaloneJobs) {
      const element = rowRefs.current.get(job.id);
      if (!element) continue;
      const rect = element.getBoundingClientRect();
      if (clientY >= rect.top && clientY <= rect.bottom) return job.id;
      const distance = clientY < rect.top ? rect.top - clientY : clientY - rect.bottom;
      if (distance < nearestDistance) {
        nearestDistance = distance;
        nearestId = job.id;
      }
    }
    return nearestId;
  }

  /** Where the dragged row would land relative to the row under the pointer.
   * Dragging downwards puts it after that row, upwards before it — the same
   * rule the drop-on-a-row mechanism this replaced followed. */
  function dropIndicatorFor(draggedId: string, overId: string | null) {
    if (!overId || overId === draggedId) return null;
    const ids = standaloneJobs.map((job) => job.id);
    const from = ids.indexOf(draggedId);
    const to = ids.indexOf(overId);
    if (from === -1 || to === -1) return null;
    return { id: overId, position: from < to ? ("after" as const) : ("before" as const) };
  }

  function commitMove(draggedId: string, overId: string | null) {
    if (!overId || draggedId === overId) return;

    // Work out the list as it will look after the drop, purely to read off the
    // dragged job's two new neighbours. Only those three ids are sent — the
    // backend writes one row from them, so a job enqueued mid-drag keeps its
    // place instead of being renumbered from a stale snapshot.
    const ids = standaloneJobs.map((job) => job.id);
    const from = ids.indexOf(draggedId);
    const to = ids.indexOf(overId);
    if (from === -1 || to === -1) return;

    ids.splice(to, 0, ids.splice(from, 1)[0]);
    const landed = ids.indexOf(draggedId);

    void moveJob(draggedId, ids[landed - 1] ?? null, ids[landed + 1] ?? null);
  }

  function handlePointerDown(job: DownloadJob, event: ReactPointerEvent<HTMLElement>) {
    // `button` is 0 for touch and pen too; this only rejects right/middle click.
    if (!isDraggable(job) || event.button !== 0) return;
    dragOrigin.current = {
      jobId: job.id,
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      started: false,
      overId: null,
    };
    try {
      event.currentTarget.setPointerCapture(event.pointerId);
    } catch {
      // Capture is an optimisation, not a requirement: without it the drag
      // still works as long as the pointer stays over the handle.
    }
    // Keeps the press from starting a text selection or a native drag.
    event.preventDefault();
  }

  function handlePointerMove(event: ReactPointerEvent<HTMLElement>) {
    const origin = dragOrigin.current;
    if (!origin || origin.pointerId !== event.pointerId) return;

    if (!origin.started) {
      const travelled = Math.hypot(event.clientX - origin.startX, event.clientY - origin.startY);
      if (travelled < DRAG_THRESHOLD_PX) return;
      origin.started = true;
      setDraggingId(origin.jobId);
    }

    origin.overId = rowUnderPointer(event.clientY);
    setDropTarget(dropIndicatorFor(origin.jobId, origin.overId));
  }

  function handlePointerUp(event: ReactPointerEvent<HTMLElement>) {
    const origin = dragOrigin.current;
    if (!origin || origin.pointerId !== event.pointerId) return;
    try {
      event.currentTarget.releasePointerCapture(event.pointerId);
    } catch {
      // The pointer is already gone; nothing left to release.
    }

    const { jobId, started, overId } = origin;
    resetDrag();
    // A press that never passed the threshold was a click, not a drag.
    if (!started) return;
    // Commit exactly what the indicator was showing, rather than re-deriving
    // it from this event: a pointerup carries no movement of its own.
    commitMove(jobId, overId);
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
              onVerify={handleVerify}
            />
          ))}
          <ul className="flex list-none flex-col gap-2 p-0">
            {standaloneJobs.map((job) => (
              <li
                key={job.id}
                ref={(element) => {
                  if (element) rowRefs.current.set(job.id, element);
                  else rowRefs.current.delete(job.id);
                }}
                data-dragging={draggingId === job.id ? "true" : undefined}
                data-drop-position={dropTarget?.id === job.id ? dropTarget.position : undefined}
                className={`relative flex items-stretch gap-2 ${draggingId === job.id ? "opacity-50" : ""}`}
              >
                {dropTarget?.id === job.id && (
                  <div
                    aria-hidden
                    className={`pointer-events-none absolute inset-x-0 h-0.5 rounded-full bg-primary ${
                      dropTarget.position === "before" ? "-top-1" : "-bottom-1"
                    }`}
                  />
                )}
                {/* The handle is the whole discoverability story for FR-119:
                    rows that cannot be reordered (a running job already holds
                    its slot) get an empty spacer instead, so "this row will
                    not move" reads differently from "dragging is broken". */}
                {isDraggable(job) ? (
                  <button
                    type="button"
                    aria-label={t("queue.drag_handle", {
                      title: job.title ?? job.source_url,
                    })}
                    title={t("queue.drag_handle_hint")}
                    // `touch-none`: without it the browser claims the gesture
                    // for scrolling and never sends the moves.
                    className="flex w-7 shrink-0 cursor-grab touch-none items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground active:cursor-grabbing"
                    onPointerDown={(event) => handlePointerDown(job, event)}
                    onPointerMove={handlePointerMove}
                    onPointerUp={handlePointerUp}
                    onPointerCancel={resetDrag}
                  >
                    <GripVertical className="h-4 w-4" />
                  </button>
                ) : (
                  <div aria-hidden className="w-7 shrink-0" />
                )}
                <div className="min-w-0 flex-1">
                  <JobRow job={job} onVerify={handleVerify} />
                </div>
              </li>
            ))}
          </ul>
        </>
      )}
      <CloudflareGrantDialog
        jobId={openChallenge?.[0] ?? null}
        challengeUrl={openChallenge?.[1].challengeUrl ?? null}
        open={Boolean(openChallenge)}
        onOpenChange={(open) => {
          if (open || !openChallenge) return;
          // Closing means "not now", not "solved": the entry stays so the row
          // keeps its way back in, and the local reopen flag is cleared so the
          // dialog does not spring straight back up.
          dismissChallenge(openChallenge[0]);
          setReopenedJobId(null);
        }}
      />
    </div>
  );
}
