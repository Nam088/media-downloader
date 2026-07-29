import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { motion, AnimatePresence } from "framer-motion";
import { FolderOpen, FileAudio, FileVideo, Images, ExternalLink } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Tooltip, TooltipTrigger, TooltipContent } from "@/components/ui/tooltip";
import { DownloadForm } from "@/components/DownloadForm";
import { QueueList } from "@/components/QueueList";
import { toast } from "sonner";
import { formatPlatformLabel } from "@/lib/format";
import { openExternalUrl } from "@/lib/open-url";
import { useQueueStore } from "@/stores/queue-store";

export function Home() {
  const { t } = useTranslation();
  const jobs = useQueueStore((state) => state.jobs);
  const completedJobs = Object.values(jobs).filter((job) => job.status === "completed");

  async function handleOpenFolder(jobId: string) {
    try {
      await invoke("open_containing_folder", { jobId });
    } catch (err: any) {
      toast.error(err?.message ?? "Failed to open folder");
    }
  }

  return (
    <div className="mx-auto flex max-w-5xl flex-col gap-8 px-6 py-6">
      <DownloadForm />

      <section className="flex flex-col gap-3">
        <div className="flex items-center gap-3">
          <h2 className="text-sm font-bold uppercase tracking-wider text-muted-foreground">{t("home.queue_heading")}</h2>
          <div className="h-px flex-1 bg-border/60" />
        </div>
        <QueueList />
      </section>

      {completedJobs.length > 0 && (
        <section className="flex flex-col gap-3">
          <div className="flex items-center gap-3">
            <h2 className="text-sm font-bold uppercase tracking-wider text-muted-foreground">
              {t("home.completed_heading")}
            </h2>
            <div className="h-px flex-1 bg-border/60" />
          </div>
          <ul className="flex flex-col gap-2.5">
            <AnimatePresence initial={false}>
              {completedJobs.map((job) => {
                const fileName = job.output_file_path
                  ? job.output_file_path.split("/").pop() ?? job.output_file_path
                  : (job.title ?? job.source_url);
                const Icon = job.media_type === "audio" ? FileAudio : job.media_type === "gallery" ? Images : FileVideo;

                return (
                  <motion.li
                    key={job.id}
                    layout
                    initial={{ opacity: 0, y: 12, scale: 0.98 }}
                    animate={{ opacity: 1, y: 0, scale: 1 }}
                    exit={{ opacity: 0, scale: 0.96 }}
                    transition={{ type: "spring", stiffness: 400, damping: 30 }}
                    className="group flex items-center justify-between gap-4 rounded-xl border border-border/70 bg-card p-3.5 shadow-2xs transition-colors duration-200 hover:border-primary/40 hover:shadow-xs"
                  >
                  <div className="flex items-center gap-3.5 min-w-0 flex-1">
                    <div className="flex h-10 w-10 shrink-0 items-center justify-center rounded-lg bg-primary/10 text-primary">
                      <Icon className="h-5 w-5" />
                    </div>
                    <div className="flex min-w-0 flex-col gap-0.5">
                      <div className="flex items-center gap-1.5 min-w-0">
                        <Tooltip>
                          <TooltipTrigger asChild>
                            <button
                              type="button"
                              onClick={() => void openExternalUrl(job.source_url)}
                              className="truncate font-semibold text-foreground text-sm hover:text-primary hover:underline transition-colors text-left flex items-center gap-1.5 group/link cursor-pointer"
                            >
                              <span className="truncate">{job.title ?? fileName}</span>
                              <ExternalLink className="h-3.5 w-3.5 shrink-0 opacity-0 group-hover/link:opacity-100 transition-opacity text-primary" />
                            </button>
                          </TooltipTrigger>
                          <TooltipContent>{t("common.open_in_browser")}</TooltipContent>
                        </Tooltip>
                      </div>
                      <div className="flex items-center gap-2 text-xs text-muted-foreground">
                        <span className="rounded bg-muted/60 px-1.5 py-0.5 text-[10px] font-medium text-foreground/80">{formatPlatformLabel(job.platform)}</span>
                        {job.output_file_path && (
                          <span className="truncate font-mono text-[11px] text-muted-foreground/70" title={job.output_file_path}>
                            {job.output_file_path}
                          </span>
                        )}
                      </div>
                    </div>
                  </div>
                  {job.output_file_path && (
                    <Tooltip>
                      <TooltipTrigger asChild>
                        <Button
                          variant="outline"
                          size="sm"
                          className="h-9 gap-1.5 shrink-0 rounded-lg text-xs hover:bg-primary/10 hover:text-primary hover:border-primary/40 transition-colors"
                          onClick={() => void handleOpenFolder(job.id)}
                        >
                          <FolderOpen className="h-4 w-4" />
                          <span className="hidden sm:inline">{t("history.open_folder")}</span>
                        </Button>
                      </TooltipTrigger>
                      <TooltipContent>{t("history.open_folder")}</TooltipContent>
                    </Tooltip>
                  )}
                </motion.li>
              );
            })}
          </AnimatePresence>
          </ul>
        </section>
      )}
    </div>
  );
}
