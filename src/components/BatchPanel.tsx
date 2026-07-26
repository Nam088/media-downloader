import { useTranslation } from "react-i18next";
import { AlertCircle, CheckCircle2, Circle, Download, Loader2 } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import type { BatchItem, BatchItemStatus, BatchMediaType } from "@/hooks/use-batch-download";

interface BatchPanelProps {
  urls: string[];
  items: BatchItem[];
  running: boolean;
  /**
   * Controlled from the form rather than held here, because the shared output
   * picker above this panel needs the same answer: an audio batch must offer
   * audio formats and a video batch a container (FR-232). Two copies of this
   * choice would let the two disagree.
   */
  mediaType: BatchMediaType;
  onMediaTypeChange: (mediaType: BatchMediaType) => void;
  onRun: (mediaType: BatchMediaType) => void;
  /** Set when something outside this panel blocks the run (no output folder). */
  disabled?: boolean;
}

const STATUS_ICON: Record<BatchItemStatus, typeof Circle> = {
  pending: Circle,
  previewing: Loader2,
  created: CheckCircle2,
  error: AlertCircle,
};

export function BatchPanel({
  urls,
  items,
  running,
  mediaType,
  onMediaTypeChange,
  onRun,
  disabled = false,
}: BatchPanelProps) {
  const { t } = useTranslation();

  return (
    <div className="flex flex-col gap-4 px-6 pb-5">
      <div className="flex flex-col gap-2">
        <Label className="text-sm font-semibold text-foreground/80">
          {t("downloadForm.batch_media_type")}
        </Label>
        <RadioGroup
          value={mediaType}
          onValueChange={(value) => onMediaTypeChange(value as BatchMediaType)}
          className="flex flex-row gap-3"
          disabled={running}
        >
          <label
            htmlFor="batch-media-audio"
            className={`flex flex-1 cursor-pointer items-center gap-3 rounded-md border border-border/80 bg-card px-3.5 py-2.5 text-sm font-semibold shadow-2xs transition-all hover:border-primary/40 ${
              mediaType === "audio" ? "border-primary bg-primary/5 ring-1 ring-primary/30" : ""
            }`}
          >
            <RadioGroupItem value="audio" id="batch-media-audio" />
            {t("downloadForm.media_type_audio")}
          </label>
          <label
            htmlFor="batch-media-video"
            className={`flex flex-1 cursor-pointer items-center gap-3 rounded-md border border-border/80 bg-card px-3.5 py-2.5 text-sm font-semibold shadow-2xs transition-all hover:border-primary/40 ${
              mediaType === "video" ? "border-primary bg-primary/5 ring-1 ring-primary/30" : ""
            }`}
          >
            <RadioGroupItem value="video" id="batch-media-video" />
            {t("downloadForm.media_type_video")}
          </label>
        </RadioGroup>
        <p className="text-xs text-muted-foreground">{t("downloadForm.batch_quality_hint")}</p>
      </div>

      {items.length > 0 && (
        <ul className="flex max-h-64 flex-col gap-1 overflow-y-auto rounded-lg border border-border/70 bg-muted/30 p-2.5 text-sm">
          {items.map((item, index) => {
            const Icon = STATUS_ICON[item.status];
            return (
              <li key={`${item.url}-${index}`} className="flex items-center gap-2">
                <Icon
                  aria-hidden="true"
                  className={`h-4 w-4 shrink-0 ${item.status === "previewing" ? "animate-spin text-primary" : ""} ${
                    item.status === "error" ? "text-destructive" : ""
                  } ${item.status === "created" ? "text-primary" : ""}`}
                />
                <span className="sr-only">{t(`downloadForm.batch_status_${item.status}`)}</span>
                <span className="min-w-0 flex-1 truncate">{item.title ?? item.url}</span>
                {item.errorCode && (
                  <span className="ml-auto shrink-0 text-xs font-medium text-destructive">
                    {t(`errors.${item.errorCode}`, { defaultValue: item.errorCode })}
                  </span>
                )}
              </li>
            );
          })}
        </ul>
      )}

      <Button
        onClick={() => onRun(mediaType)}
        disabled={running || disabled || urls.length === 0}
        // Pinned so the button keeps the same accessible name while the batch
        // is in flight and its visible label is a spinner.
        aria-label={t("downloadForm.download_all", { count: urls.length })}
        className="h-10 gap-2 self-end rounded-lg px-6 text-sm font-semibold shadow-xs"
      >
        {running ? (
          <>
            <Loader2 className="h-4 w-4 animate-spin text-primary-foreground" />
            <span>{t("common.loading")}</span>
          </>
        ) : (
          <>
            <Download className="h-4 w-4" />
            <span>{t("downloadForm.download_all", { count: urls.length })}</span>
          </>
        )}
      </Button>
    </div>
  );
}
