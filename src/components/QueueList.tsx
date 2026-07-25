import { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Pause, Play, X, RotateCcw } from "lucide-react";
import { Progress } from "@/components/ui/progress";
import { Button } from "@/components/ui/button";
import { ensureQueueListeners, useQueueStore } from "@/stores/queue-store";
import type { DownloadJob } from "@/types/download";

const ACTIVE_STATUSES = new Set(["queued", "fetching_metadata", "downloading", "paused"]);

function formatSpeed(bytesPerSec: number | null): string {
  if (!bytesPerSec) return "";
  const mb = bytesPerSec / (1024 * 1024);
  return mb >= 1 ? `${mb.toFixed(1)} MB/s` : `${(bytesPerSec / 1024).toFixed(0)} KB/s`;
}

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

export function QueueList() {
  const { t } = useTranslation();
  const jobs = useQueueStore((state) => state.jobs);

  useEffect(() => {
    ensureQueueListeners();
  }, []);

  const activeJobs = Object.values(jobs).filter((job) => ACTIVE_STATUSES.has(job.status));

  if (activeJobs.length === 0) {
    return <p className="text-sm text-muted-foreground">{t("queue.empty")}</p>;
  }

  return (
    <div className="flex flex-col gap-2">
      {activeJobs.map((job) => (
        <div key={job.id} className="rounded-md border border-border/80 bg-card p-3 shadow-2xs transition-all">
          <div className="flex items-center justify-between gap-3">
            <span className="truncate text-sm font-medium text-foreground">{job.source_url}</span>
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
      ))}
    </div>
  );
}
