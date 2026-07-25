import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { FolderOpen } from "lucide-react";
import { Button } from "@/components/ui/button";
import { DownloadForm } from "@/components/DownloadForm";
import { QueueList } from "@/components/QueueList";
import { useQueueStore } from "@/stores/queue-store";

export function Home() {
  const { t } = useTranslation();
  const jobs = useQueueStore((state) => state.jobs);
  const completedJobs = Object.values(jobs).filter((job) => job.status === "completed");

  async function handleOpenFolder(jobId: string) {
    await invoke("open_containing_folder", { jobId });
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
            {completedJobs.map((job) => (
              <li
                key={job.id}
                className="flex items-center justify-between gap-3 rounded-lg border border-border/80 bg-card p-4 text-base shadow-2xs transition-all hover:border-primary/30"
              >
                <span className="min-w-0 flex-1 truncate font-medium text-foreground/90" title={job.output_file_path ?? undefined}>
                  {job.output_file_path ?? job.source_url}
                </span>
                {job.output_file_path && (
                  <Button
                    variant="secondary"
                    size="icon"
                    className="h-9 w-9 shrink-0 rounded-md hover:bg-primary/10 hover:text-primary transition-colors"
                    onClick={() => handleOpenFolder(job.id)}
                    title={t("history.open_folder")}
                  >
                    <FolderOpen className="h-4.5 w-4.5" />
                  </Button>
                )}
              </li>
            ))}
          </ul>
        </section>
      )}
    </div>
  );
}
