import { useCallback, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { motion, AnimatePresence } from "framer-motion";
import { toast } from "sonner";
import {
  Clock,
  Globe,
  FolderOpen,
  Loader2,
  Download,
  Search,
  Images,
  FileUp,
  ExternalLink,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { openExternalUrl } from "@/lib/open-url";
import { Textarea } from "@/components/ui/textarea";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { Card, CardContent } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { ErrorBanner } from "@/components/ErrorBanner";
import { BatchPanel } from "@/components/BatchPanel";
import { GalleryItemPicker } from "@/components/GalleryItemPicker";
import { OutputOptionsPicker } from "@/components/OutputOptionsPicker";
import { PlaylistDetailPanel } from "@/components/PlaylistDetailPanel";
import { PlaylistScopeDialog } from "@/components/PlaylistScopeDialog";
import { useAppSettings } from "@/hooks/use-app-settings";
import { useBatchDownload, type BatchMediaType } from "@/hooks/use-batch-download";
import { URL_LIST_EXTENSIONS, useFileDrop } from "@/hooks/use-file-drop";
import { useQueueStore } from "@/stores/queue-store";
import {
  GENERIC_PLAYLIST_AUDIO_QUALITIES,
  GENERIC_PLAYLIST_VIDEO_QUALITIES,
  audioQualityValue,
} from "@/lib/generic-quality-options";
import { buildJobInput } from "@/lib/build-job-input";
import { trimErrorFor } from "@/lib/trim-input";
import { audioOutputDetail, videoOutputDetail } from "@/lib/output-format-labels";
import { CURATED_PLATFORMS, formatDuration, formatFileSize, formatPlatformLabel } from "@/lib/format";
import { dedupeUrls, extractUrlsFromText } from "@/lib/url-parsing";
import { NEW_JOB_OUTPUT_OPTIONS } from "@/types/download";
import type {
  AppError,
  AudioFormatOption,
  CreateJobInput,
  DownloadJob,
  GalleryMode,
  MediaSource,
  MediaType,
  OutputOptions,
  VideoQualityOption,
} from "@/types/download";

/** One row in the quality picker — mirrors the reference layout: radio +
 * bold quality label + codec detail + right-aligned estimated size. Options
 * are rendered exactly as the backend returned them (FR-004/FR-019); this
 * component never invents a quality tier that isn't in `options`.
 *
 * The detail column is derived from the source's own codec plus the current
 * output selection (FR-206). It used to be `MP3 / ${codec}` for audio and the
 * constant `"MP4 / H264 / AAC"` for video — the first printed impossible
 * pairs like "MP3 / OPUS", and the second kept claiming H.264/AAC/MP4 no
 * matter what the job would actually produce. */
function QualityOptionsList({
  qualityLabel,
  audioOptions,
  videoOptions,
  outputOptions,
  value,
  onChange,
}: {
  qualityLabel: string;
  audioOptions?: AudioFormatOption[];
  videoOptions?: VideoQualityOption[];
  outputOptions: OutputOptions;
  value: string | undefined;
  onChange: (value: string) => void;
}) {
  const { t } = useTranslation();
  const rows = audioOptions
    ? audioOptions.map((opt) => ({
        value: audioQualityValue(opt.bitrate_kbps),
        label: opt.bitrate_kbps == null ? t("downloadForm.best_available") : `${opt.bitrate_kbps}kbps`,
        detail: audioOutputDetail(t, opt.codec, outputOptions.audio),
        size: formatFileSize(opt.filesize_bytes),
      }))
    : (videoOptions ?? []).map((opt) => ({
        value: opt.label,
        label: opt.label,
        detail: videoOutputDetail(t, outputOptions),
        size: formatFileSize(opt.filesize_bytes),
      }));

  if (rows.length === 0) return null;

  return (
    <div className="flex flex-col gap-2">
      <Label className="text-xs font-semibold tracking-tight text-foreground/80">{qualityLabel}</Label>
      <RadioGroup
        value={value}
        onValueChange={onChange}
        className="gap-1.5"
      >
        {rows.map((row) => (
          <label
            key={row.value}
            htmlFor={`quality-${row.value}`}
            className={`flex cursor-pointer items-center gap-3 rounded-md border border-border/80 bg-card px-3.5 py-2.5 text-sm shadow-2xs transition-all hover:border-primary/40 hover:bg-accent/40 ${
              value === row.value ? "border-primary bg-primary/5 ring-1 ring-primary/30" : ""
            }`}
          >
            <RadioGroupItem value={row.value} id={`quality-${row.value}`} />
            <span className="w-16 shrink-0 font-semibold text-foreground">{row.label}</span>
            <span className="flex-1 text-xs text-muted-foreground">{row.detail}</span>
            <span className="shrink-0 text-xs font-mono font-medium text-muted-foreground">
              {row.size}
            </span>
          </label>
        ))}
      </RadioGroup>
    </div>
  );
}

/** Shown instead of `QualityOptionsList` when `preview.is_gallery` — a
 * gallery-dl-backed source has no video/audio quality tiers, just a
 * three-way choice of what to do with the files it found (see
 * `models::GalleryMode` on the backend). "Merged video" is disabled when the
 * source has no audio track at all, since there'd be nothing to sync a
 * slideshow to. */
function GalleryModeSelector({
  hasAudio,
  hasImages,
  value,
  onChange,
}: {
  hasAudio: boolean;
  hasImages: boolean;
  value: GalleryMode;
  onChange: (value: GalleryMode) => void;
}) {
  const { t } = useTranslation();
  const rows: { value: GalleryMode; label: string; hint: string; disabled?: boolean }[] = [
    { value: "files", label: t("downloadForm.gallery_mode_files"), hint: t("downloadForm.gallery_mode_files_hint") },
    {
      value: "images_only",
      label: t("downloadForm.gallery_mode_images_only"),
      hint: t("downloadForm.gallery_mode_images_only_hint"),
      disabled: !hasImages,
    },
    {
      value: "audio_only",
      label: t("downloadForm.gallery_mode_audio_only"),
      hint: t("downloadForm.gallery_mode_audio_only_hint"),
      disabled: !hasAudio,
    },
    {
      value: "slideshow",
      label: t("downloadForm.gallery_mode_slideshow"),
      hint:
        hasAudio && hasImages
          ? t("downloadForm.gallery_mode_slideshow_hint")
          : t("downloadForm.gallery_mode_slideshow_unavailable"),
      disabled: !hasAudio || !hasImages,
    },
  ];

  return (
    <div className="flex flex-col gap-2">
      <Label className="text-xs font-semibold tracking-tight text-foreground/80">
        {t("downloadForm.gallery_mode_label")}
      </Label>
      <RadioGroup value={value} onValueChange={(v) => onChange(v as GalleryMode)} className="gap-1.5">
        {rows.map((row) => (
          <label
            key={row.value}
            htmlFor={`gallery-mode-${row.value}`}
            className={`flex items-center gap-3 rounded-md border border-border/80 bg-card px-3.5 py-2.5 text-sm shadow-2xs transition-all ${
              row.disabled
                ? "cursor-not-allowed opacity-50"
                : "cursor-pointer hover:border-primary/40 hover:bg-accent/40"
            } ${value === row.value && !row.disabled ? "border-primary bg-primary/5 ring-1 ring-primary/30" : ""}`}
          >
            <RadioGroupItem value={row.value} id={`gallery-mode-${row.value}`} disabled={row.disabled} />
            <span className="w-28 shrink-0 font-semibold text-foreground">{row.label}</span>
            <span className="flex-1 text-xs text-muted-foreground">{row.hint}</span>
          </label>
        ))}
      </RadioGroup>
    </div>
  );
}

export function DownloadForm() {
  const { t } = useTranslation();
  const [rawInput, setRawInput] = useState("");
  const [preview, setPreview] = useState<MediaSource | null>(null);
  const [previewLoading, setPreviewLoading] = useState(false);
  const [previewingUrl, setPreviewingUrl] = useState<string | null>(null);
  const [mediaType, setMediaType] = useState<MediaType>("video");
  const [audioQuality, setAudioQuality] = useState<string | undefined>(undefined);
  const [videoQuality, setVideoQuality] = useState<string | undefined>(undefined);
  const [galleryMode, setGalleryMode] = useState<GalleryMode>("files");
  // Indices into `preview.gallery_items` (not URLs — TikTok's per-item CDN
  // URLs are short-lived and signed per-request, so the backend correlates
  // a selection against a fresh re-crawl by ordinal position instead; see
  // `models::DownloadJob.selected_gallery_indices`'s doc comment).
  const [selectedGalleryIndices, setSelectedGalleryIndices] = useState<Set<number>>(new Set());
  // FR-208/FR-209 start ON for a *new* job, which is why this is
  // `NEW_JOB_OUTPUT_OPTIONS` and not the Rust-side `OutputOptions::default()`
  // — the latter answers "what did a job created before Phase 2 mean?" and
  // has both embed flags off.
  const [outputOptions, setOutputOptions] = useState<OutputOptions>(NEW_JOB_OUTPUT_OPTIONS);
  const [outputDirectory, setOutputDirectory] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<AppError | null>(null);
  const [playlistDialogOpen, setPlaylistDialogOpen] = useState(false);
  // Held here, not in BatchPanel, so the shared output picker above the list
  // shows the controls that match what the batch will be downloaded as.
  // Audio is the common case and stays the default.
  const [batchMediaType, setBatchMediaType] = useState<BatchMediaType>("audio");
  const { settings } = useAppSettings();
  const batch = useBatchDownload();

  // Fall back to the persisted default (FR-008/Settings) so users don't have
  // to pick a folder on every single download; an explicit manual pick (via
  // "Choose folder" below, which sets `outputDirectory`) always takes
  // priority. Computed at render time rather than copied into state via an
  // effect, so there's exactly one source of truth for "what's selected".
  const effectiveOutputDirectory = outputDirectory ?? settings?.default_output_directory ?? null;

  const urls = extractUrlsFromText(rawInput);
  const isBatchMode = urls.length > 1;

  /** Merge an externally-sourced list (dropped file, imported file) into the
   * textarea without losing what is already typed there, and without adding a
   * link twice. */
  const mergeImportedUrls = useCallback(
    (imported: string[]) => {
      if (imported.length === 0) {
        toast.info(t("downloadForm.dropped_urls_none"));
        return;
      }
      setRawInput((current) => {
        const merged = dedupeUrls([...extractUrlsFromText(current), ...imported]);
        return merged.unique.join("\n");
      });
      toast.success(t("downloadForm.dropped_urls", { count: imported.length }));
    },
    [t],
  );

  useFileDrop(mergeImportedUrls);

  async function handleImportUrlList() {
    const selected = await openDialog({
      multiple: false,
      filters: [
        { name: t("downloadForm.url_list_filter_name"), extensions: [...URL_LIST_EXTENSIONS] },
      ],
    });
    if (typeof selected !== "string") return;
    setError(null);
    try {
      // Invoked directly rather than through `readUrlListFiles`: that helper
      // swallows a bad file so one dud can't kill a multi-file drop, but here
      // the user picked exactly one file and deserves to be told why it failed.
      mergeImportedUrls(await invoke<string[]>("read_url_list_file", { path: selected }));
    } catch (err) {
      setError(err as AppError);
    }
  }

  async function handlePreview() {
    if (urls.length !== 1) return;
    const url = urls[0];
    setPreviewingUrl(url);
    setPreviewLoading(true);
    setError(null);
    setPreview(null);
    setAudioQuality(undefined);
    setVideoQuality(undefined);
    try {
      const result = await invoke<MediaSource>("preview_media", { sourceUrl: url });
      setPreview(result);
      if (result.is_gallery) {
        setMediaType("gallery");
        setGalleryMode("files");
        // Default to everything selected — most people want all the images;
        // deselecting specific ones is the exception, not the starting point.
        setSelectedGalleryIndices(
          new Set(
            result.gallery_items.reduce<number[]>((indices, item, index) => {
              if (!item.is_audio) indices.push(index);
              return indices;
            }, []),
          ),
        );
      } else {
        // Leftover "gallery" selection from a previous preview on this
        // same form wouldn't make sense against a regular video/audio result.
        setMediaType((prev) => (prev === "gallery" ? "video" : prev));
        if (result.available_audio_formats.length > 0) {
          setAudioQuality(audioQualityValue(result.available_audio_formats[0].bitrate_kbps));
        }
        if (result.available_video_qualities.length > 0) {
          setVideoQuality(result.available_video_qualities[0].label);
        }
      }
    } catch (err) {
      const appError = err as AppError;
      // A user-initiated stop (handleStopPreview below) isn't a real error —
      // don't show it as one.
      if (appError.code !== "CANCELED") {
        setError(appError);
      }
    } finally {
      setPreviewLoading(false);
      setPreviewingUrl(null);
    }
  }

  async function handleStopPreview() {
    // Optimistic: the button disappears immediately regardless of whether
    // the backend confirms the kill in time, so the UI never feels stuck
    // waiting on the same slow request the user just asked to stop.
    setPreviewLoading(false);
    if (previewingUrl) {
      await invoke("cancel_preview_media", { sourceUrl: previewingUrl });
    }
  }

  async function handleChooseDirectory() {
    const selected = await openDialog({ directory: true, multiple: false });
    if (typeof selected === "string") {
      setOutputDirectory(selected);
    }
  }

  function resetForm() {
    setRawInput("");
    setPreview(null);
    setAudioQuality(undefined);
    setVideoQuality(undefined);
    batch.reset();
  }

  async function submitSingleJob(scope?: "single_item" | "entire_playlist") {
    if (!preview || !effectiveOutputDirectory) return;
    if (mediaType !== "gallery") {
      const quality = mediaType === "audio" ? audioQuality : videoQuality;
      if (!preview.is_playlist && !quality) return;
    }

    setSubmitting(true);
    setError(null);
    try {
      const input: CreateJobInput = buildJobInput({
        preview,
        mediaType,
        audioQuality,
        videoQuality,
        outputDirectory: effectiveOutputDirectory,
        galleryMode,
        selectedGalleryIndices: Array.from(selectedGalleryIndices),
        playlistScope: scope,
        outputOptions,
      });
      const createdJobs = await invoke<DownloadJob[]>("create_download_job", { input });
      useQueueStore.getState().upsertJobs(createdJobs);
      toast.success(t("downloadForm.added_to_queue"));
      // Deliberately not resetting the form here: queuing a download doesn't
      // mean the user is done with this link — they might want to queue the
      // same video again in another quality/format, or the download is still
      // in progress in the background (see QueueList) and clearing the
      // preview out from under them reads as if something went wrong.
    } catch (err) {
      setError(err as AppError);
    } finally {
      setSubmitting(false);
      setPlaylistDialogOpen(false);
    }
  }

  function handleDownloadClick() {
    if (preview?.is_playlist) {
      setPlaylistDialogOpen(true);
      return;
    }
    submitSingleJob();
  }

  /** Runs the whole pasted list with one shared media-type choice; per-link
   * quality still comes from each link's own preview (FR-019), and each link's
   * outcome is reported individually by `BatchPanel`. */
  async function handleRunBatch(runMediaType: BatchMediaType) {
    if (!effectiveOutputDirectory) return;
    setError(null);
    const summary = await batch.run({
      urls,
      mediaType: runMediaType,
      outputDirectory: effectiveOutputDirectory,
      // FR-232 — the same choices for every link in the paste. Without this
      // the batch path was the one that quietly reverted to MP3/MP4.
      outputOptions,
    });
    if (summary.failed > 0) {
      toast.warning(t("downloadForm.batch_partial_failure", { count: summary.failed }));
    } else if (summary.created > 0) {
      toast.success(t("downloadForm.added_to_queue"));
    }
  }

  const hasQualityOptions =
    mediaType === "audio"
      ? (preview?.available_audio_formats.length ?? 0) > 0
      : (preview?.available_video_qualities.length ?? 0) > 0;
  const selectedQuality = mediaType === "audio" ? audioQuality : videoQuality;
  // Files/ImagesOnly/Slideshow all need at least one selected image;
  // AudioOnly ignores the image selection entirely (it discards images
  // regardless), so it's the one gallery mode that's fine with zero picked.
  const galleryHasValidSelection = mediaType !== "gallery" || galleryMode === "audio_only" || selectedGalleryIndices.size > 0;
  // FR-223 — an unusable trim range blocks the job at the button, matching
  // the message the picker already shows at the field. The same check runs in
  // the backend, which is what actually enforces it (`TrimRange::validate`).
  const trimError = trimErrorFor(outputOptions, preview?.duration_seconds);
  const canDownloadSingle = Boolean(
    preview &&
      effectiveOutputDirectory &&
      !submitting &&
      galleryHasValidSelection &&
      trimError === null &&
      (mediaType === "gallery" || preview.is_playlist || selectedQuality),
  );
  // Guard on the raw value, not the formatted string: formatDuration always
  // returns a placeholder, so testing its result would show a `--:--` clock
  // badge on sources with no duration (live streams). `!= null` rather than a
  // truthiness check so a genuine zero-second source still renders.
  const duration =
    preview && preview.duration_seconds != null ? formatDuration(preview.duration_seconds) : null;

  return (
    <Card className="overflow-hidden rounded-lg border-border/80 bg-card shadow-2xs transition-all">
      <CardContent className="flex flex-col p-0">
        <div className="flex flex-col gap-4 p-6 pb-2">
          <ErrorBanner error={error} onDismiss={() => setError(null)} />
          
          <div className="flex flex-col gap-2.5">
            <div className="flex items-center justify-between">
              <Label htmlFor="source-url" className="text-sm font-bold tracking-wider uppercase text-foreground/80">
                {t("downloadForm.url_label")}
              </Label>
              <div className="flex items-center gap-2">
                <button
                  type="button"
                  onClick={handleImportUrlList}
                  className="inline-flex items-center gap-1.5 text-xs font-semibold text-primary transition-all hover:underline"
                >
                  <FileUp className="h-3.5 w-3.5" />
                  {t("downloadForm.import_url_list")}
                </button>
                {rawInput.trim().length > 0 && (
                  <button
                    type="button"
                    onClick={() => setRawInput(urls.join("\n"))}
                    className="text-xs font-semibold text-primary hover:underline transition-all"
                    title={t("downloadForm.clean_links_hint")}
                  >
                    {t("downloadForm.clean_links", { count: urls.length })}
                  </button>
                )}
                {urls.length > 0 && (
                  <span className="text-xs font-mono font-semibold text-primary">
                    {t("downloadForm.url_count", { count: urls.length })}
                  </span>
                )}
              </div>
            </div>

            {/* Full-width Stacked Input & Action Button Layout */}
            <div className="flex flex-col gap-3">
              <Textarea
                id="source-url"
                value={rawInput}
                onChange={(e) => setRawInput(e.target.value)}
                onPaste={(e) => {
                  const pastedText = e.clipboardData.getData("text");
                  const extracted = extractUrlsFromText(pastedText);
                  if (extracted.length > 0) {
                    e.preventDefault();
                    setRawInput(extracted.join("\n"));
                    toast.success(t("downloadForm.paste_filtered", { count: extracted.length }));
                  }
                }}
                placeholder={t("downloadForm.url_placeholder_multi")}
                rows={isBatchMode ? 4 : 2}
                className="w-full resize-y rounded-xl border-border/80 bg-muted/30 p-3.5 pl-4 text-sm font-medium transition-all focus-visible:bg-background focus-visible:ring-2 focus-visible:ring-primary/30 break-all leading-relaxed"
              />
              {!isBatchMode && urls.length === 1 && (
                <div>
                  {previewLoading ? (
                    <Button
                      onClick={handleStopPreview}
                      variant="outline"
                      className="w-full h-11 rounded-xl text-sm font-bold gap-2 border-border/80"
                    >
                      <Loader2 className="h-4.5 w-4.5 animate-spin text-primary" />
                      <span>{t("downloadForm.stop_preview_button")}</span>
                    </Button>
                  ) : (
                    <Button
                      onClick={handlePreview}
                      className="w-full h-11 rounded-xl text-sm font-bold shadow-xs gap-2 transition-all active:scale-98"
                    >
                      <Search className="h-4.5 w-4.5 stroke-[2.5]" />
                      <span>{t("downloadForm.preview_button")}</span>
                    </Button>
                  )}
                </div>
              )}
            </div>

            {isBatchMode && (
              <p className="text-sm font-medium text-muted-foreground mt-1">
                {t("downloadForm.batch_mode_hint", { count: urls.length })}
              </p>
            )}

            {/* Quick Supported Platform Tags */}
            {!isBatchMode && !preview && (
              <div className="flex flex-wrap items-center gap-2 pt-1">
                <span className="text-xs font-semibold text-muted-foreground mr-1">
                  {t("downloadForm.supported_label")}
                </span>
                {CURATED_PLATFORMS.map((platform) => (
                  <span key={platform} className="rounded-md bg-muted/70 px-2.5 py-1 text-xs font-semibold text-foreground/80 border border-border/50">
                    {formatPlatformLabel(platform)}
                  </span>
                ))}
              </div>
            )}
          </div>
        </div>

        <AnimatePresence>
          {!isBatchMode && previewLoading && (
            <motion.div
              initial={{ opacity: 0, y: 12, height: 0 }}
              animate={{ opacity: 1, y: 0, height: "auto" }}
              exit={{ opacity: 0, y: -8, height: 0 }}
              className="mx-6 flex flex-col gap-4 rounded-xl border border-border/70 bg-card/60 p-4 shadow-2xs"
            >
              <div className="flex flex-col sm:flex-row gap-4">
                <Skeleton className="h-24 w-full sm:w-36 shrink-0 rounded-lg" />
                <div className="flex flex-1 flex-col justify-between gap-2 py-0.5">
                  <div className="space-y-2">
                    <Skeleton className="h-4.5 w-4/5" />
                    <Skeleton className="h-3.5 w-1/2" />
                  </div>
                  <div className="flex items-center gap-2 pt-2">
                    <Skeleton className="h-6 w-20 rounded-md" />
                    <Skeleton className="h-6 w-24 rounded-md" />
                  </div>
                </div>
              </div>
              <div className="grid grid-cols-2 sm:grid-cols-4 gap-3 pt-2">
                <Skeleton className="h-9 w-full rounded-lg" />
                <Skeleton className="h-9 w-full rounded-lg" />
                <Skeleton className="h-9 w-full rounded-lg" />
                <Skeleton className="h-9 w-full rounded-lg" />
              </div>
            </motion.div>
          )}

          {!isBatchMode && preview && !previewLoading && (
            <motion.div
              initial={{ opacity: 0, y: 16, height: 0 }}
              animate={{ opacity: 1, y: 0, height: "auto" }}
              exit={{ opacity: 0, y: -10, height: 0 }}
              transition={{ type: "spring", stiffness: 350, damping: 28 }}
              className="flex flex-col gap-6"
            >
              {/* Header: thumbnail + title + duration/link */}
              <div className="mx-6 flex flex-col sm:flex-row gap-5 rounded-lg border border-border/70 bg-muted/40 p-4">
                {preview.thumbnail_url ? (
                  <img
                    src={preview.thumbnail_url}
                    alt=""
                    className="h-24 w-full sm:w-36 shrink-0 rounded-md border border-border/50 object-cover shadow-2xs"
                  />
                ) : (
                  <div className="h-24 w-full sm:w-36 shrink-0 rounded-md border border-border/50 bg-muted flex items-center justify-center text-muted-foreground">
                    <Globe className="h-8 w-8 opacity-40" />
                  </div>
                )}
                <div className="flex min-w-0 flex-1 flex-col justify-between gap-2">
                  <div className="flex flex-col gap-1.5">
                    <button
                      type="button"
                      onClick={() => void openExternalUrl(preview.source_url)}
                      className="line-clamp-2 text-base font-semibold leading-snug text-foreground hover:text-primary hover:underline transition-colors text-left flex items-start gap-1.5 cursor-pointer group/title"
                    >
                      <span className="flex-1">{preview.title}</span>
                      <ExternalLink className="h-4 w-4 shrink-0 opacity-0 group-hover/title:opacity-100 transition-opacity text-primary mt-0.5" />
                    </button>
                    <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-muted-foreground">
                      {duration && (
                        <span className="inline-flex items-center gap-1.5 font-mono font-medium">
                          <Clock className="h-4 w-4 text-primary" />
                          {duration}
                        </span>
                      )}
                      <button
                        type="button"
                        onClick={() => void openExternalUrl(preview.source_url)}
                        className="inline-flex min-w-0 items-center gap-1.5 hover:text-primary transition-colors cursor-pointer text-left"
                      >
                        <Globe className="h-4 w-4 shrink-0" />
                        <span className="truncate font-mono">{preview.source_url}</span>
                      </button>
                    </div>
                  </div>
                  <Badge variant="secondary" className="w-fit rounded-md font-semibold text-xs px-2.5 py-0.5">
                    {formatPlatformLabel(preview.platform)}
                  </Badge>
                </div>
              </div>

            {preview.is_playlist && preview.playlist_entries.length > 0 ? (
              <div className="px-6 pt-2 pb-4">
                <PlaylistDetailPanel
                  preview={preview}
                  outputDirectory={effectiveOutputDirectory}
                  outputOptions={outputOptions}
                  onOutputOptionsChange={setOutputOptions}
                />
              </div>
            ) : preview.is_gallery ? (
              <div className="flex flex-col gap-6 px-6 pt-2 pb-2">
                <GalleryItemPicker
                  items={preview.gallery_items}
                  selectedIndices={Array.from(selectedGalleryIndices)}
                  onChange={(indices) => setSelectedGalleryIndices(new Set(indices))}
                />
                <p className="flex items-center gap-1.5 text-xs font-medium text-muted-foreground">
                  <Images className="h-4 w-4" />
                  {t("downloadForm.gallery_item_count", { count: preview.gallery_items.length })}
                </p>
                <GalleryModeSelector
                  hasAudio={preview.gallery_items.some((item) => item.is_audio)}
                  hasImages={preview.gallery_items.some((item) => !item.is_audio)}
                  value={galleryMode}
                  onChange={setGalleryMode}
                />
              </div>
            ) : (
              <div className="flex flex-col gap-6 px-6 pt-2 pb-2">
                {/* Modern Separate Pill Toggle Buttons with Generous Spacing & Animations */}
                <div className="flex items-center gap-4">
                  <button
                    type="button"
                    onClick={() => setMediaType("video")}
                    className={`flex-1 flex items-center justify-center gap-2 rounded-lg py-3 px-4 text-sm font-semibold border transition-all duration-200 active:scale-98 shadow-2xs ${
                      mediaType === "video"
                        ? "border-primary bg-primary text-primary-foreground shadow-xs ring-2 ring-primary/20"
                        : "border-border/80 bg-muted/40 text-muted-foreground hover:bg-muted hover:text-foreground"
                    }`}
                  >
                    <span>{t("downloadForm.media_type_video")}</span>
                  </button>
                  <button
                    type="button"
                    onClick={() => setMediaType("audio")}
                    className={`flex-1 flex items-center justify-center gap-2 rounded-lg py-3 px-4 text-sm font-semibold border transition-all duration-200 active:scale-98 shadow-2xs ${
                      mediaType === "audio"
                        ? "border-primary bg-primary text-primary-foreground shadow-xs ring-2 ring-primary/20"
                        : "border-border/80 bg-muted/40 text-muted-foreground hover:bg-muted hover:text-foreground"
                    }`}
                  >
                    <span>{t("downloadForm.media_type_audio")}</span>
                  </button>
                </div>

                {mediaType === "audio" && (
                  <div className="mt-1 animate-in fade-in-50 slide-in-from-top-1 duration-200 ease-out">
                    <QualityOptionsList
                      qualityLabel={t("downloadForm.audio_quality_label")}
                      audioOptions={preview.is_playlist ? GENERIC_PLAYLIST_AUDIO_QUALITIES : preview.available_audio_formats}
                      outputOptions={outputOptions}
                      value={audioQuality}
                      onChange={setAudioQuality}
                    />
                  </div>
                )}
                {mediaType === "video" && (
                  <div className="mt-1 animate-in fade-in-50 slide-in-from-top-1 duration-200 ease-out">
                    <QualityOptionsList
                      qualityLabel={t("downloadForm.video_quality_label")}
                      videoOptions={preview.is_playlist ? GENERIC_PLAYLIST_VIDEO_QUALITIES : preview.available_video_qualities}
                      outputOptions={outputOptions}
                      value={videoQuality}
                      onChange={setVideoQuality}
                    />
                  </div>
                )}

                {!hasQualityOptions && !preview.is_playlist && (
                  <p className="text-base text-muted-foreground">
                    {mediaType === "audio"
                      ? t("downloadForm.no_audio_formats")
                      : t("downloadForm.no_video_qualities")}
                  </p>
                )}
              </div>
            )}

            {/* FR-201→FR-211. Rendered for gallery previews too: the picker
                itself decides that none of it applies there (FR-234), so the
                rule lives with the component rather than being re-derived by
                every caller. Skipped for the inline playlist panel, which
                submits through its own per-item path. */}
            {!(preview.is_playlist && preview.playlist_entries.length > 0) && (
              <div className="px-6 pt-3 pb-1">
                <OutputOptionsPicker
                  mediaType={mediaType}
                  value={outputOptions}
                  onChange={setOutputOptions}
                  source={preview}
                />
              </div>
            )}
            </motion.div>
          )}
        </AnimatePresence>

        {(preview || isBatchMode) && (
          <div className="flex flex-col gap-2.5 px-6 pb-5 mt-2">
            <Label className="text-sm font-semibold text-foreground/80">{t("downloadForm.output_directory_label")}</Label>
            <div className="flex items-center gap-3 rounded-lg border border-border/80 bg-muted/30 p-2.5 pl-3.5">
              <FolderOpen className="h-5 w-5 shrink-0 text-primary" />
              <span className="min-w-0 flex-1 truncate text-xs font-mono text-foreground/80" dir="rtl">
                {effectiveOutputDirectory ?? t("downloadForm.no_directory_chosen")}
              </span>
              <Button variant="outline" size="sm" onClick={handleChooseDirectory} className="rounded-md text-xs font-semibold h-9 px-3.5">
                {t("downloadForm.choose_directory_button")}
              </Button>
            </div>
          </div>
        )}

        {isBatchMode && (
          <>
            {/* FR-232 — one picker for the whole paste. `source` is null on
                purpose: several links have no single format list, no single
                subtitle list and no single chapter list, and the picker says
                so rather than describing one link as if it were all of them. */}
            <div className="px-6 pb-2">
              <OutputOptionsPicker
                mediaType={batchMediaType}
                value={outputOptions}
                onChange={setOutputOptions}
                source={null}
              />
            </div>
            <BatchPanel
              urls={urls}
              items={batch.items}
              running={batch.running}
              mediaType={batchMediaType}
              onMediaTypeChange={setBatchMediaType}
              onRun={(runMediaType) => void handleRunBatch(runMediaType)}
              disabled={!effectiveOutputDirectory || trimError !== null}
            />
          </>
        )}

        <div className="flex items-center justify-end gap-3 border-t border-border/60 bg-muted/20 px-6 py-4">
          {(preview || isBatchMode) && (
            <Button
              variant="ghost"
              onClick={resetForm}
              disabled={submitting || batch.running}
              className="rounded-lg text-sm font-medium h-10 px-4"
            >
              {t("common.cancel")}
            </Button>
          )}
          {!isBatchMode &&
            // Hidden once the playlist's own entries are showing inline
            // (PlaylistDetailPanel below). That panel has its own submit
            // button, since it queues a per-item selection this generic
            // button can't express.
            preview &&
            !(preview.is_playlist && preview.playlist_entries.length > 0) && (
              <Button onClick={handleDownloadClick} disabled={!canDownloadSingle} className="rounded-lg shadow-xs h-10 px-7 text-sm font-semibold gap-2">
                {submitting ? (
                  <>
                    <Loader2 className="h-4.5 w-4.5 animate-spin text-primary-foreground" />
                    <span>{t("common.loading")}</span>
                  </>
                ) : (
                  <>
                    <Download className="h-4.5 w-4.5" />
                    <span>
                      {mediaType === "gallery"
                        ? t("downloadForm.download_gallery_button")
                        : mediaType === "video"
                          ? t("downloadForm.download_video_button")
                          : t("downloadForm.download_audio_button")}
                    </span>
                  </>
                )}
              </Button>
            )}
        </div>
      </CardContent>

      <PlaylistScopeDialog
        preview={preview}
        open={playlistDialogOpen}
        onOpenChange={setPlaylistDialogOpen}
        onChoose={(scope) => submitSingleJob(scope)}
      />
    </Card>
  );
}
