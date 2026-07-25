import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { FolderOpen, RotateCcw, FileAudio, FileVideo, Images, Globe, CheckCircle2, XCircle, AlertCircle } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { useQueueStore } from "@/stores/queue-store";
import type { DownloadJob, JobStatus } from "@/types/download";

const TERMINAL_STATUSES = new Set<JobStatus>(["completed", "failed", "canceled"]);

export function HistoryList({
  searchTerm = "",
  filterStatus = "all",
}: {
  searchTerm?: string;
  filterStatus?: string;
}) {
  const { t } = useTranslation();
  const [jobs, setJobs] = useState<DownloadJob[] | null>(null);
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

  useEffect(() => {
    invoke<DownloadJob[]>("list_history").then(setJobs);
  }, [terminalSignature]);

  async function handleOpenFolder(jobId: string) {
    await invoke("open_containing_folder", { jobId });
  }

  async function handleRetry(jobId: string) {
    await retryJob(jobId);
    const refreshed = await invoke<DownloadJob[]>("list_history");
    setJobs(refreshed);
  }

  const filteredJobs = useMemo(() => {
    if (!jobs) return [];
    return jobs.filter((job) => {
      const matchStatus = filterStatus === "all" || job.status === filterStatus;
      const term = searchTerm.toLowerCase().trim();
      const matchSearch =
        !term ||
        job.source_url.toLowerCase().includes(term) ||
        (job.output_file_path && job.output_file_path.toLowerCase().includes(term)) ||
        job.platform.toLowerCase().includes(term);
      return matchStatus && matchSearch;
    });
  }, [jobs, filterStatus, searchTerm]);

  if (jobs === null) {
    return (
      <div className="flex h-40 items-center justify-center rounded-xl border border-dashed border-border/80 p-8">
        <p className="text-sm text-muted-foreground animate-pulse">{t("common.loading")}</p>
      </div>
    );
  }

  if (jobs.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center rounded-xl border border-dashed border-border/80 p-12 text-center">
        <Globe className="h-10 w-10 text-muted-foreground/40 mb-3" />
        <h3 className="text-sm font-bold text-foreground">No download history yet</h3>
        <p className="text-xs text-muted-foreground mt-1 max-w-sm">{t("history.empty")}</p>
      </div>
    );
  }

  if (filteredJobs.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center rounded-xl border border-dashed border-border/80 p-10 text-center">
        <p className="text-sm font-semibold text-muted-foreground">No results matching your filters</p>
      </div>
    );
  }

  return (
    <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
      {filteredJobs.map((job) => {
        const isAudio = job.media_type === "audio";
        const isGallery = job.media_type === "gallery";
        const fileName = job.output_file_path
          ? job.output_file_path.split(/[/\\]/).pop()
          : job.source_url;

        return (
          <div
            key={job.id}
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
                    Completed
                  </span>
                )}
                {job.status === "failed" && (
                  <span className="inline-flex items-center gap-1.5 rounded-md bg-destructive/10 px-2 py-0.5 font-semibold text-[11px] text-destructive">
                    <XCircle className="h-3 w-3" />
                    Failed
                  </span>
                )}
                {job.status === "canceled" && (
                  <span className="inline-flex items-center gap-1.5 rounded-md bg-muted px-2 py-0.5 font-semibold text-[11px] text-muted-foreground">
                    <AlertCircle className="h-3 w-3" />
                    Canceled
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
                    <span>Open Folder</span>
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
                    <span>Retry</span>
                  </Button>
                )}
              </div>
            </div>
          </div>
        );
      })}
    </div>
  );
}
