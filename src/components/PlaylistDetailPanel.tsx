import { useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { Clock, Download, Loader2, Music, Video } from "lucide-react";
import { Button } from "@/components/ui/button";
import { ErrorBanner } from "@/components/ErrorBanner";
import { OutputOptionsPicker } from "@/components/OutputOptionsPicker";
import { trimErrorFor } from "@/lib/trim-input";
import { useQueueStore } from "@/stores/queue-store";
import {
  GENERIC_PLAYLIST_AUDIO_QUALITIES,
  GENERIC_PLAYLIST_VIDEO_QUALITIES,
  BEST_AUDIO_QUALITY_VALUE,
  audioQualityValue,
} from "@/lib/generic-quality-options";
import { formatDuration } from "@/lib/format";
import type {
  AppError,
  CreatePlaylistJobsInput,
  DownloadJob,
  MediaSource,
  OutputOptions,
  PlaylistItemJobInput,
} from "@/types/download";

interface PlaylistDetailPanelProps {
  preview: MediaSource | null;
  outputDirectory: string | null;
  /**
   * The output choices for every video queued from this panel (FR-232).
   *
   * Owned by the form rather than by this panel, and shared with the
   * single-link picker, so switching a link between "one video" and "the whole
   * playlist" cannot quietly change the format the file comes out in.
   */
  outputOptions: OutputOptions;
  onOutputOptionsChange: (next: OutputOptions) => void;
}

/** The per-video type toggle, a compact two-way pill, same visual language
 * as the top-level video/audio switch in DownloadForm, just small enough to
 * sit inline in a list row. */
function ItemTypeToggle({
  value,
  onChange,
  disabled,
}: {
  value: "video" | "audio";
  onChange: (value: "video" | "audio") => void;
  disabled: boolean;
}) {
  const { t } = useTranslation();
  return (
    <div className="flex shrink-0 items-center gap-1 rounded-md border border-border/70 bg-muted/30 p-0.5">
      <button
        type="button"
        disabled={disabled}
        onClick={() => onChange("video")}
        title={t("playlistDetail.item_type_video")}
        className={`flex items-center gap-1 rounded px-2 py-1 text-xs font-semibold transition-all disabled:opacity-40 ${
          value === "video" ? "bg-primary text-primary-foreground shadow-2xs" : "text-muted-foreground hover:text-foreground"
        }`}
      >
        <Video className="h-3.5 w-3.5" />
      </button>
      <button
        type="button"
        disabled={disabled}
        onClick={() => onChange("audio")}
        title={t("playlistDetail.item_type_audio")}
        className={`flex items-center gap-1 rounded px-2 py-1 text-xs font-semibold transition-all disabled:opacity-40 ${
          value === "audio" ? "bg-primary text-primary-foreground shadow-2xs" : "text-muted-foreground hover:text-foreground"
        }`}
      >
        <Music className="h-3.5 w-3.5" />
      </button>
    </div>
  );
}

/** Keyed by `preview.source_url` from the parent so every field resets to a
 * fresh default whenever a different playlist is previewed, without an
 * effect-driven reset (mount = fresh state). */
function PlaylistDetailPanelInner({
  preview,
  outputDirectory,
  outputOptions,
  onOutputOptionsChange,
}: {
  preview: MediaSource;
  outputDirectory: string | null;
  outputOptions: OutputOptions;
  onOutputOptionsChange: (next: OutputOptions) => void;
}) {
  const { t } = useTranslation();
  const entries = preview.playlist_entries;
  const [selected, setSelected] = useState<Set<number>>(new Set(entries.map((_, i) => i)));
  const [itemType, setItemType] = useState<Record<number, "video" | "audio">>(() =>
    Object.fromEntries(entries.map((_, i) => [i, "video" as const])),
  );
  const [audioQuality, setAudioQuality] = useState(audioQualityValue(GENERIC_PLAYLIST_AUDIO_QUALITIES[0].bitrate_kbps));
  const [videoQuality, setVideoQuality] = useState(GENERIC_PLAYLIST_VIDEO_QUALITIES[0].label);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<AppError | null>(null);

  const selectedCount = selected.size;
  // FR-223 — the same block as the single-link form; a playlist submission is
  // still a job creation.
  const trimError = trimErrorFor(outputOptions, preview.duration_seconds);
  const hasVideoSelected = Array.from(selected).some((i) => itemType[i] === "video");
  const hasAudioSelected = Array.from(selected).some((i) => itemType[i] === "audio");

  function toggleSelected(index: number) {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(index)) next.delete(index);
      else next.add(index);
      return next;
    });
  }

  function setAllType(type: "video" | "audio") {
    setItemType(Object.fromEntries(entries.map((_, i) => [i, type])));
  }

  async function handleSubmit() {
    if (!outputDirectory || selectedCount === 0) return;
    setSubmitting(true);
    setError(null);
    try {
      const items: PlaylistItemJobInput[] = Array.from(selected)
        .sort((a, b) => a - b)
        .map((index) => {
          const entry = entries[index];
          const type = itemType[index];
          return {
            source_url: entry.url,
            media_type: type,
            audio_quality: type === "audio" && audioQuality !== BEST_AUDIO_QUALITY_VALUE ? audioQuality : undefined,
            video_quality: type === "video" ? videoQuality : undefined,
            title: entry.title,
          };
        });
      const input: CreatePlaylistJobsInput = {
        output_directory: outputDirectory,
        items,
        playlist_title: preview.title,
        // The field existed from the first day of this command and nothing
        // filled it, which made this the second path (with the batch) where
        // every output choice was dropped on the floor.
        output_options: outputOptions,
      };
      const createdJobs = await invoke<DownloadJob[]>("create_playlist_download_jobs", { input });
      useQueueStore.getState().upsertJobs(createdJobs);
      toast.success(t("playlistDetail.added_to_queue", { count: createdJobs.length }));
    } catch (err) {
      setError(err as AppError);
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <div className="flex flex-col gap-4 rounded-lg border border-border/70 bg-muted/20 p-4">
      <div className="flex flex-col gap-1">
        <span className="text-sm font-semibold text-foreground">{t("playlistDetail.title")}</span>
        <span className="text-xs text-muted-foreground">
          {t("playlistDetail.description_with_count", { count: entries.length })}
        </span>
      </div>

      <ErrorBanner error={error} onDismiss={() => setError(null)} />

      <div className="flex items-center justify-between border-b border-border/60 pb-2.5">
        <span className="text-xs font-medium text-muted-foreground">
          {t("playlistDetail.selected_count", { selected: selectedCount, total: entries.length })}
        </span>
        <div className="flex flex-wrap items-center justify-end gap-x-3 gap-y-1">
          <button type="button" onClick={() => setSelected(new Set(entries.map((_, i) => i)))} className="text-xs font-semibold text-primary hover:underline">
            {t("playlistDetail.select_all")}
          </button>
          <button type="button" onClick={() => setSelected(new Set())} className="text-xs font-semibold text-muted-foreground hover:underline">
            {t("playlistDetail.select_none")}
          </button>
          <span className="text-border/80">|</span>
          <button type="button" onClick={() => setAllType("video")} className="text-xs font-semibold text-primary hover:underline">
            {t("playlistDetail.set_all_video")}
          </button>
          <button type="button" onClick={() => setAllType("audio")} className="text-xs font-semibold text-primary hover:underline">
            {t("playlistDetail.set_all_audio")}
          </button>
        </div>
      </div>

      <div className="flex max-h-80 flex-col gap-1.5 overflow-y-auto pr-1">
        {entries.map((entry, index) => {
          const isSelected = selected.has(index);
          // Guard on the raw value so entries with no duration hide the clock
          // badge instead of showing the `--:--` placeholder.
          const duration = entry.duration_seconds != null ? formatDuration(entry.duration_seconds) : null;
          return (
            <div
              key={`${entry.url}-${index}`}
              className={`flex items-center gap-3 rounded-md border px-2.5 py-2 transition-all ${
                isSelected ? "border-border/80 bg-card" : "border-border/40 bg-muted/20 opacity-60"
              }`}
            >
              <button
                type="button"
                onClick={() => toggleSelected(index)}
                className={`flex h-5 w-5 shrink-0 items-center justify-center rounded border text-[10px] font-bold transition-colors ${
                  isSelected ? "border-primary bg-primary text-primary-foreground" : "border-border bg-background text-transparent"
                }`}
              >
                ✓
              </button>
              {entry.thumbnail_url ? (
                <img src={entry.thumbnail_url} alt="" className="h-10 w-16 shrink-0 rounded object-cover" />
              ) : (
                <div className="h-10 w-16 shrink-0 rounded bg-muted" />
              )}
              <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                <span className="line-clamp-1 text-sm font-medium text-foreground">{entry.title}</span>
                {duration && (
                  <span className="inline-flex items-center gap-1 text-xs text-muted-foreground">
                    <Clock className="h-3 w-3" />
                    {duration}
                  </span>
                )}
              </div>
              <ItemTypeToggle
                value={itemType[index]}
                onChange={(type) => setItemType((prev) => ({ ...prev, [index]: type }))}
                disabled={!isSelected}
              />
            </div>
          );
        })}
      </div>

      {(hasVideoSelected || hasAudioSelected) && (
        <div className="flex flex-col gap-3 border-t border-border/60 pt-3 sm:flex-row">
          {hasVideoSelected && (
            <div className="flex flex-1 flex-col gap-1.5">
              <span className="text-xs font-semibold text-foreground/80">{t("downloadForm.video_quality_label")}</span>
              <select
                value={videoQuality}
                onChange={(e) => setVideoQuality(e.target.value)}
                className="h-9 rounded-md border border-border/80 bg-card px-2.5 text-sm"
              >
                {GENERIC_PLAYLIST_VIDEO_QUALITIES.map((opt) => (
                  <option key={opt.label} value={opt.label}>
                    {opt.label}
                  </option>
                ))}
              </select>
            </div>
          )}
          {hasAudioSelected && (
            <div className="flex flex-1 flex-col gap-1.5">
              <span className="text-xs font-semibold text-foreground/80">{t("downloadForm.audio_quality_label")}</span>
              <select
                value={audioQuality}
                onChange={(e) => setAudioQuality(e.target.value)}
                className="h-9 rounded-md border border-border/80 bg-card px-2.5 text-sm"
              >
                {GENERIC_PLAYLIST_AUDIO_QUALITIES.map((opt) => (
                  <option key={opt.bitrate_kbps} value={audioQualityValue(opt.bitrate_kbps)}>
                    {opt.bitrate_kbps}kbps
                  </option>
                ))}
              </select>
            </div>
          )}
        </div>
      )}

      {/* One picker for the whole selection. Its media type follows what is
          actually selected: a list queued entirely as audio gets the audio
          format choices, anything with a video in it gets the container and
          codec ones. */}
      <OutputOptionsPicker
        mediaType={hasVideoSelected ? "video" : "audio"}
        value={outputOptions}
        onChange={onOutputOptionsChange}
        source={preview}
      />

      <div className="flex justify-end border-t border-border/60 pt-3">
        <Button
          onClick={handleSubmit}
          disabled={submitting || selectedCount === 0 || !outputDirectory || trimError !== null}
          className="gap-2"
        >
          {submitting ? <Loader2 className="h-4 w-4 animate-spin" /> : <Download className="h-4 w-4" />}
          {t("playlistDetail.submit_button", { count: selectedCount })}
        </Button>
      </div>
    </div>
  );
}

/** Inline replacement for the old all-or-nothing `PlaylistScopeDialog`, shown
 * directly in the form (no modal) for links yt-dlp flattens into real
 * per-entry data: lets the user pick exactly which videos to queue and
 * whether each becomes an audio or video download. Renders nothing when the
 * backend reported no real per-entry list (e.g. some flat-playlist sources
 * yt-dlp can enumerate a count for but not individual entries). `DownloadForm`
 * falls back to `PlaylistScopeDialog` for that case. */
export function PlaylistDetailPanel({
  preview,
  outputDirectory,
  outputOptions,
  onOutputOptionsChange,
}: PlaylistDetailPanelProps) {
  if (!preview || preview.playlist_entries.length === 0) return null;

  return (
    <PlaylistDetailPanelInner
      key={preview.source_url}
      preview={preview}
      outputDirectory={outputDirectory}
      outputOptions={outputOptions}
      onOutputOptionsChange={onOutputOptionsChange}
    />
  );
}
