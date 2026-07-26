import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import {
  FolderOpen,
  RotateCcw,
  FileAudio,
  FileVideo,
  Images,
  Globe,
  CheckCircle2,
  XCircle,
  AlertCircle,
  Trash2,
  ChevronLeft,
  ChevronRight,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { useQueueStore } from "@/stores/queue-store";
import type { DownloadJob, JobStatus } from "@/types/download";

const TERMINAL_STATUSES = new Set<JobStatus>(["completed", "failed", "canceled"]);

/** History is filtered and paged entirely on the backend (search + status
 * tab included), so the page count always matches what's actually on
 * screen — the same reasoning as `LibraryQuery`, just for a smaller table. */
const HISTORY_PAGE_SIZES = [10, 20, 50] as const;
const DEFAULT_PAGE_SIZE = 20;

interface HistoryQueryArgs {
  search: string | null;
  status: string | null;
  limit: number;
  offset: number;
}

function buildQuery(
  searchTerm: string,
  filterStatus: string,
  pageSize: number,
  page: number,
): HistoryQueryArgs {
  return {
    search: searchTerm.trim() || null,
    status: filterStatus === "all" ? null : filterStatus,
    limit: pageSize,
    offset: (page - 1) * pageSize,
  };
}

/** Page numbers to render, collapsing a long run into `1 … 4 5 6 … 20`
 * instead of one button per page. */
function pageNumbers(current: number, total: number): (number | "ellipsis")[] {
  if (total <= 7) {
    return Array.from({ length: total }, (_, i) => i + 1);
  }
  const keep = new Set([1, total, current - 1, current, current + 1]);
  const sorted = [...keep].filter((p) => p >= 1 && p <= total).sort((a, b) => a - b);
  const result: (number | "ellipsis")[] = [];
  let previous = 0;
  for (const p of sorted) {
    if (previous && p - previous > 1) result.push("ellipsis");
    result.push(p);
    previous = p;
  }
  return result;
}

export function HistoryList({
  searchTerm = "",
  filterStatus = "all",
}: {
  searchTerm?: string;
  filterStatus?: string;
}) {
  const { t } = useTranslation();
  const [jobs, setJobs] = useState<DownloadJob[] | null>(null);
  const [total, setTotal] = useState(0);
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState<number>(DEFAULT_PAGE_SIZE);
  const [loadError, setLoadError] = useState(false);
  const [clearing, setClearing] = useState(false);
  const [confirmingClear, setConfirmingClear] = useState(false);
  const { retryJob } = useQueueStore();
  const queuedJobs = useQueueStore((state) => state.jobs);

  const terminalSignature = useMemo(
    () =>
      Object.values(queuedJobs)
        .filter((job) => TERMINAL_STATUSES.has(job.status))
        .map((job) => `${job.id}:${job.status}`)
        .sort()
        .join(","),
    [queuedJobs],
  );

  // Asking a different question (filter, search, page size) starts back at
  // page one — "page 3" of the previous question doesn't mean anything here.
  // Adjusted during render (React's own recipe for "reset state when a prop
  // changes"), not in an effect: an effect would fetch once with the stale
  // page, then again once the reset commits.
  const [prevQuestion, setPrevQuestion] = useState({ searchTerm, filterStatus, pageSize });
  let effectivePage = page;
  if (
    prevQuestion.searchTerm !== searchTerm ||
    prevQuestion.filterStatus !== filterStatus ||
    prevQuestion.pageSize !== pageSize
  ) {
    setPrevQuestion({ searchTerm, filterStatus, pageSize });
    effectivePage = 1;
    setPage(1);
  }

  const totalPages = Math.max(1, Math.ceil(total / pageSize));

  // A retry or a clear can shrink the result set out from under the page the
  // user is looking at — land on the new last page during this same render
  // instead of fetching the empty one first and fixing it up afterwards.
  if (effectivePage > totalPages) {
    effectivePage = totalPages;
    setPage(totalPages);
  }

  const query = useMemo(
    () => buildQuery(searchTerm, filterStatus, pageSize, effectivePage),
    [searchTerm, filterStatus, pageSize, effectivePage],
  );

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const [items, count] = await Promise.all([
          invoke<DownloadJob[]>("list_history", { query }),
          invoke<number>("count_history", { query }),
        ]);
        if (cancelled) return;
        setJobs(items);
        setTotal(count);
        setLoadError(false);
      } catch {
        if (!cancelled) setLoadError(true);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [query, terminalSignature]);

  async function refetch(targetPage: number) {
    const q = buildQuery(searchTerm, filterStatus, pageSize, targetPage);
    try {
      const [items, count] = await Promise.all([
        invoke<DownloadJob[]>("list_history", { query: q }),
        invoke<number>("count_history", { query: q }),
      ]);
      setJobs(items);
      setTotal(count);
      setPage(targetPage);
      setLoadError(false);
    } catch {
      setLoadError(true);
    }
  }

  async function handleClearHistory() {
    setClearing(true);
    try {
      await invoke("clear_history");
      await refetch(1);
    } finally {
      setClearing(false);
      setConfirmingClear(false);
    }
  }

  async function handleOpenFolder(jobId: string) {
    await invoke("open_containing_folder", { jobId });
  }

  async function handleRetry(jobId: string) {
    await retryJob(jobId);
    await refetch(effectivePage);
  }

  const noFilterActive = searchTerm.trim() === "" && filterStatus === "all";
  const trulyEmpty = noFilterActive && total === 0;

  if (loadError) {
    return (
      <div className="flex flex-col items-center justify-center rounded-xl border border-dashed border-destructive/40 bg-destructive/5 p-12 text-center">
        <XCircle className="h-10 w-10 text-destructive/60 mb-3" />
        <h3 className="text-sm font-bold text-destructive">{t("history.load_failed")}</h3>
        <p className="text-xs text-muted-foreground mt-1 max-w-sm">{t("history.load_failed_hint")}</p>
      </div>
    );
  }

  if (jobs === null) {
    return (
      <div className="flex h-40 items-center justify-center rounded-xl border border-dashed border-border/80 p-8">
        <p className="text-sm text-muted-foreground animate-pulse">{t("common.loading")}</p>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-4">
      {/* Clearing removes ALL history, not just what the current filter
          shows, so this stays available whenever we can't be sure the whole
          table is empty — hiding it only once we know for certain. */}
      {!trulyEmpty && (
        <div className="flex justify-end">
          <Button
            variant="outline"
            size="sm"
            className="h-8 gap-1.5 rounded-lg text-xs font-semibold text-muted-foreground hover:text-destructive hover:border-destructive/40"
            onClick={() => setConfirmingClear(true)}
            data-testid="history-clear-button"
          >
            <Trash2 className="h-3.5 w-3.5" />
            {t("history.clear_button")}
          </Button>
        </div>
      )}

      {total === 0 ? (
        noFilterActive ? (
          <div className="flex flex-col items-center justify-center rounded-xl border border-dashed border-border/80 p-12 text-center">
            <Globe className="h-10 w-10 text-muted-foreground/40 mb-3" />
            <h3 className="text-sm font-bold text-foreground">{t("history.empty_title")}</h3>
            <p className="text-xs text-muted-foreground mt-1 max-w-sm">{t("history.empty")}</p>
          </div>
        ) : (
          <div className="flex flex-col items-center justify-center rounded-xl border border-dashed border-border/80 p-10 text-center">
            <p className="text-sm font-semibold text-muted-foreground">{t("history.empty_filtered")}</p>
          </div>
        )
      ) : (
        <>
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            {jobs.map((job) => {
              const isAudio = job.media_type === "audio";
              const isGallery = job.media_type === "gallery";
              const fileName = job.output_file_path
                ? job.output_file_path.split(/[/\\]/).pop()
                : job.source_url;

              return (
                <div
                  key={job.id}
                  data-testid="history-item"
                  className="group flex flex-col justify-between gap-3.5 rounded-xl border border-border/80 bg-card p-4 shadow-2xs transition-all duration-200 hover:border-primary/40 hover:shadow-xs"
                >
                  {/* Top Row: Icon + Title + Quality Badge */}
                  <div className="flex items-start gap-3">
                    <div className={`flex h-10 w-10 shrink-0 items-center justify-center rounded-lg shadow-2xs ${
                      isAudio
                        ? "bg-amber-500/10 text-amber-600 dark:text-amber-400"
                        : isGallery
                          ? "bg-violet-500/10 text-violet-600 dark:text-violet-400"
                          : "bg-primary/10 text-primary"
                    }`}>
                      {isAudio ? (
                        <FileAudio className="h-5 w-5" />
                      ) : isGallery ? (
                        <Images className="h-5 w-5" />
                      ) : (
                        <FileVideo className="h-5 w-5" />
                      )}
                    </div>

                    <div className="flex min-w-0 flex-1 flex-col gap-1">
                      <span className="truncate text-sm font-bold leading-tight text-foreground/90 group-hover:text-primary transition-colors" title={fileName}>
                        {fileName}
                      </span>
                      <span className="truncate text-xs text-muted-foreground font-mono" title={job.source_url}>
                        {job.source_url}
                      </span>
                    </div>
                  </div>

                  {/* Middle Meta Info: Status + Quality Spec + Time */}
                  <div className="flex items-center justify-between border-t border-border/50 pt-3 text-xs">
                    <div className="flex items-center gap-2">
                      {job.status === "completed" && (
                        <span className="inline-flex items-center gap-1.5 rounded-md bg-emerald-500/10 px-2 py-0.5 font-semibold text-[11px] text-emerald-600 dark:text-emerald-400">
                          <CheckCircle2 className="h-3 w-3" />
                          {t("queue.status.completed")}
                        </span>
                      )}
                      {job.status === "failed" && (
                        <span className="inline-flex items-center gap-1.5 rounded-md bg-destructive/10 px-2 py-0.5 font-semibold text-[11px] text-destructive">
                          <XCircle className="h-3 w-3" />
                          {t("queue.status.failed")}
                        </span>
                      )}
                      {job.status === "canceled" && (
                        <span className="inline-flex items-center gap-1.5 rounded-md bg-muted px-2 py-0.5 font-semibold text-[11px] text-muted-foreground">
                          <AlertCircle className="h-3 w-3" />
                          {t("queue.status.canceled")}
                        </span>
                      )}

                      {(job.video_quality || job.audio_quality) && (
                        <Badge variant="outline" className="text-[10px] font-mono px-1.5 py-0 rounded-md">
                          {job.video_quality ?? job.audio_quality}
                        </Badge>
                      )}
                    </div>

                    {/* Action Buttons */}
                    <div className="flex items-center gap-1">
                      {job.status === "completed" && job.output_file_path && (
                        <Button
                          variant="secondary"
                          size="sm"
                          className="h-8 gap-1.5 rounded-lg px-2.5 text-xs font-semibold hover:bg-primary/10 hover:text-primary transition-colors"
                          onClick={() => handleOpenFolder(job.id)}
                          title={t("history.open_folder")}
                        >
                          <FolderOpen className="h-3.5 w-3.5" />
                          <span>{t("history.open_folder_button")}</span>
                        </Button>
                      )}
                      {job.status === "failed" && (
                        <Button
                          variant="secondary"
                          size="sm"
                          className="h-8 gap-1.5 rounded-lg px-2.5 text-xs font-semibold hover:bg-destructive/10 hover:text-destructive transition-colors"
                          onClick={() => handleRetry(job.id)}
                          title={t("common.retry")}
                        >
                          <RotateCcw className="h-3.5 w-3.5" />
                          <span>{t("common.retry")}</span>
                        </Button>
                      )}
                    </div>
                  </div>
                </div>
              );
            })}
          </div>

          <div className="flex flex-wrap items-center justify-between gap-3 pt-1">
            <div className="flex items-center gap-2 text-xs text-muted-foreground">
              <label className="sr-only" htmlFor="history-page-size">
                {t("history.page_size_label")}
              </label>
              <select
                id="history-page-size"
                value={pageSize}
                onChange={(e) => setPageSize(Number(e.target.value))}
                data-testid="history-page-size"
                className="h-8 rounded-lg border border-border/80 bg-card px-2 text-xs shadow-2xs"
              >
                {HISTORY_PAGE_SIZES.map((size) => (
                  <option key={size} value={size}>
                    {size}
                  </option>
                ))}
              </select>
              <span>{t("history.page_label", { current: effectivePage, total: totalPages })}</span>
            </div>

            <div className="flex items-center gap-1">
              <Button
                variant="outline"
                size="icon"
                className="h-8 w-8"
                disabled={effectivePage <= 1}
                onClick={() => setPage(effectivePage - 1)}
                aria-label={t("history.prev_page")}
                data-testid="history-prev-page"
              >
                <ChevronLeft className="h-4 w-4" />
              </Button>
              {pageNumbers(effectivePage, totalPages).map((p, i) =>
                p === "ellipsis" ? (
                  <span key={`ellipsis-${i}`} className="px-1 text-xs text-muted-foreground">
                    …
                  </span>
                ) : (
                  <Button
                    key={p}
                    variant={p === effectivePage ? "default" : "outline"}
                    size="icon"
                    className="h-8 w-8 text-xs"
                    onClick={() => setPage(p)}
                    data-testid={`history-page-${p}`}
                  >
                    {p}
                  </Button>
                ),
              )}
              <Button
                variant="outline"
                size="icon"
                className="h-8 w-8"
                disabled={effectivePage >= totalPages}
                onClick={() => setPage(effectivePage + 1)}
                aria-label={t("history.next_page")}
                data-testid="history-next-page"
              >
                <ChevronRight className="h-4 w-4" />
              </Button>
            </div>
          </div>
        </>
      )}

      <Dialog open={confirmingClear} onOpenChange={setConfirmingClear}>
        <DialogContent data-testid="history-clear-dialog">
          <DialogHeader>
            <DialogTitle>{t("history.clear_confirm_title")}</DialogTitle>
            <DialogDescription>{t("history.clear_confirm_body")}</DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button
              variant="ghost"
              onClick={() => setConfirmingClear(false)}
              data-testid="history-clear-cancel"
            >
              {t("common.cancel")}
            </Button>
            <Button
              variant="destructive"
              onClick={handleClearHistory}
              disabled={clearing}
              data-testid="history-clear-confirm"
            >
              {t("history.clear_confirm_action")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
