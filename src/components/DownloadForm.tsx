import { useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { toast } from "sonner";
import { Clock, Globe, FolderOpen, Loader2, Download, Search, Images } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { Label } from "@/components/ui/label";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { Card, CardContent } from "@/components/ui/card";
import { Badge } from "@/components/ui/badge";
import { ErrorBanner } from "@/components/ErrorBanner";
import { PlaylistDetailPanel } from "@/components/PlaylistDetailPanel";
import { PlaylistScopeDialog } from "@/components/PlaylistScopeDialog";
import { useAppSettings } from "@/hooks/use-app-settings";
import { useQueueStore } from "@/stores/queue-store";
import {
  GENERIC_PLAYLIST_AUDIO_QUALITIES,
  GENERIC_PLAYLIST_VIDEO_QUALITIES,
  audioQualityValue,
} from "@/lib/generic-quality-options";
import { buildJobInput } from "@/lib/build-job-input";
import { formatDuration, formatFileSize } from "@/lib/format";
import { extractUrlsFromText } from "@/lib/url-parsing";
import type {
  AppError,
  AudioFormatOption,
  CreateJobInput,
  DownloadJob,
  GalleryMode,
  MediaSource,
  MediaType,
  VideoQualityOption,
} from "@/types/download";

const PLATFORM_DISPLAY_NAMES: Record<string, string> = {
  youtube: "YouTube",
  tiktok: "TikTok",
  facebook: "Facebook",
  instagram: "Instagram",
  twitter_x: "X (Twitter)",
  soundcloud: "SoundCloud",
};

/** `preview.platform` is whatever the backend resolved it to — one of the 6
 * required platforms, or yt-dlp's own extractor name for anything else (see
 * `commands::media::resolve_platform_label`) — so this only prettifies the
 * label, it never decides what's allowed. */
function platformDisplayName(platform: string): string {
  return PLATFORM_DISPLAY_NAMES[platform] ?? platform.charAt(0).toUpperCase() + platform.slice(1);
}

/** One row in the quality picker — mirrors the reference layout: radio +
 * bold quality label + codec detail + right-aligned estimated size. Options
 * are rendered exactly as the backend returned them (FR-004/FR-019); this
 * component never invents a quality tier that isn't in `options`. */
function QualityOptionsList({
  qualityLabel,
  audioOptions,
  videoOptions,
  value,
  onChange,
}: {
  qualityLabel: string;
  audioOptions?: AudioFormatOption[];
  videoOptions?: VideoQualityOption[];
  value: string | undefined;
  onChange: (value: string) => void;
}) {
  const { t } = useTranslation();
  const rows = audioOptions
    ? audioOptions.map((opt) => ({
        value: audioQualityValue(opt.bitrate_kbps),
        label: opt.bitrate_kbps == null ? t("downloadForm.best_available") : `${opt.bitrate_kbps}kbps`,
        detail: `MP3 / ${opt.codec.toUpperCase()}`,
        size: formatFileSize(opt.filesize_bytes),
      }))
    : (videoOptions ?? []).map((opt) => ({
        value: opt.label,
        label: opt.label,
        detail: "MP4 / H264 / AAC",
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
  const [outputDirectory, setOutputDirectory] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<AppError | null>(null);
  const [playlistDialogOpen, setPlaylistDialogOpen] = useState(false);
  const [batchErrors, setBatchErrors] = useState<{ url: string; message: string }[]>([]);
  const { settings } = useAppSettings();

  // Fall back to the persisted default (FR-008/Settings) so users don't have
  // to pick a folder on every single download; an explicit manual pick (via
  // "Choose folder" below, which sets `outputDirectory`) always takes
  // priority. Computed at render time rather than copied into state via an
  // effect, so there's exactly one source of truth for "what's selected".
  const effectiveOutputDirectory = outputDirectory ?? settings?.default_output_directory ?? null;

  const urls = extractUrlsFromText(rawInput);
  const isBatchMode = urls.length > 1;

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
        // Leftover "gallery" selection from a previous preview on this same
        // form wouldn't make sense against a regular video/audio result.
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

  async function handleDownloadAllBatch() {
    if (!effectiveOutputDirectory) return;
    setSubmitting(true);
    setError(null);
    setBatchErrors([]);
    const errors: { url: string; message: string }[] = [];

    for (const url of urls) {
      try {
        const previewResult = await invoke<MediaSource>("preview_media", { sourceUrl: url });
        // Batch mode's whole point is "grab audio from each link with no
        // per-item decisions" — for a gallery-backed link (no audio/video
        // quality concept at all), the closest equivalent is "keep just the
        // audio track", not a plain audio download that link doesn't have.
        const input: CreateJobInput = buildJobInput({
          preview: previewResult,
          mediaType: "audio",
          audioQuality:
            previewResult.available_audio_formats.length > 0
              ? audioQualityValue(previewResult.available_audio_formats[0].bitrate_kbps)
              : null,
          videoQuality: null,
          outputDirectory: effectiveOutputDirectory,
          // Only consulted for a gallery-backed link, which has no
          // audio/video quality concept at all — "keep just the audio track"
          // is the closest equivalent to batch mode's plain audio download.
          galleryMode: "audio_only",
        });
        const createdJobs = await invoke<DownloadJob[]>("create_download_job", { input });
        useQueueStore.getState().upsertJobs(createdJobs);
      } catch (err) {
        const appError = err as AppError;
        errors.push({ url, message: appError.message });
      }
    }

    setBatchErrors(errors);
    setSubmitting(false);
    if (errors.length === 0) {
      toast.success(t("downloadForm.added_to_queue"));
      resetForm();
    } else {
      toast.warning(t("downloadForm.batch_partial_failure", { count: errors.length }));
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
  const canDownloadSingle = Boolean(
    preview &&
      effectiveOutputDirectory &&
      !submitting &&
      galleryHasValidSelection &&
      (mediaType === "gallery" || preview.is_playlist || selectedQuality),
  );
  const canDownloadBatch = Boolean(effectiveOutputDirectory && !submitting && urls.length > 0);
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
                {rawInput.trim().length > 0 && (
                  <button
                    type="button"
                    onClick={() => setRawInput(urls.join("\n"))}
                    className="text-xs font-semibold text-primary hover:underline transition-all"
                    title="Loại bỏ ký tự thừa & giữ lại link hợp lệ"
                  >
                    Làm sạch link ({urls.length} link hợp lệ)
                  </button>
                )}
                {urls.length > 0 && (
                  <span className="text-xs font-mono font-semibold text-primary">
                    {isBatchMode ? `${urls.length} URLs` : "Single URL"}
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
                    toast.success(`Đã tự động lọc ${extracted.length} link hợp lệ`);
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
                <span className="text-xs font-semibold text-muted-foreground mr-1">Supported:</span>
                {Object.values(PLATFORM_DISPLAY_NAMES).map((plat) => (
                  <span key={plat} className="rounded-md bg-muted/70 px-2.5 py-1 text-xs font-semibold text-foreground/80 border border-border/50">
                    {plat}
                  </span>
                ))}
              </div>
            )}
          </div>
        </div>

        {!isBatchMode && preview && (
          <>
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
                  <span className="line-clamp-2 text-base font-semibold leading-snug text-foreground">
                    {preview.title}
                  </span>
                  <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-muted-foreground">
                    {duration && (
                      <span className="inline-flex items-center gap-1.5 font-mono font-medium">
                        <Clock className="h-4 w-4 text-primary" />
                        {duration}
                      </span>
                    )}
                    <span className="inline-flex min-w-0 items-center gap-1.5">
                      <Globe className="h-4 w-4 shrink-0" />
                      <span className="truncate">{preview.source_url}</span>
                    </span>
                  </div>
                </div>
                <Badge variant="secondary" className="w-fit rounded-md font-semibold text-xs px-2.5 py-0.5">
                  {platformDisplayName(preview.platform)}
                </Badge>
              </div>
            </div>

            {preview.is_playlist && preview.playlist_entries.length > 0 ? (
              <div className="px-6 pt-2 pb-4">
                <PlaylistDetailPanel preview={preview} outputDirectory={effectiveOutputDirectory} />
              </div>
            ) : preview.is_gallery ? (
              <div className="flex flex-col gap-6 px-6 pt-2 pb-2">
                {(() => {
                  const imageItems = preview.gallery_items
                    .map((item, index) => ({ item, index }))
                    .filter(({ item }) => !item.is_audio);
                  if (imageItems.length === 0) return null;
                  const selectedCount = imageItems.filter(({ index }) => selectedGalleryIndices.has(index)).length;
                  return (
                    <div className="flex flex-col gap-2">
                      <div className="flex items-center justify-between">
                        <span className="text-xs font-medium text-muted-foreground">
                          {t("downloadForm.gallery_selected_count", { selected: selectedCount, total: imageItems.length })}
                        </span>
                        <div className="flex items-center gap-3">
                          <button
                            type="button"
                            onClick={() => setSelectedGalleryIndices(new Set(imageItems.map(({ index }) => index)))}
                            className="text-xs font-semibold text-primary hover:underline"
                          >
                            {t("downloadForm.gallery_select_all")}
                          </button>
                          <button
                            type="button"
                            onClick={() => setSelectedGalleryIndices(new Set())}
                            className="text-xs font-semibold text-muted-foreground hover:underline"
                          >
                            {t("downloadForm.gallery_select_none")}
                          </button>
                        </div>
                      </div>
                      <div className="grid grid-cols-4 gap-2 sm:grid-cols-6">
                        {imageItems.slice(0, 24).map(({ item, index }) => {
                          const isSelected = selectedGalleryIndices.has(index);
                          return (
                            <button
                              type="button"
                              key={index}
                              onClick={() =>
                                setSelectedGalleryIndices((prev) => {
                                  const next = new Set(prev);
                                  if (next.has(index)) {
                                    next.delete(index);
                                  } else {
                                    next.add(index);
                                  }
                                  return next;
                                })
                              }
                              className="group relative aspect-square overflow-hidden rounded-md border border-border/50 shadow-2xs"
                            >
                              <img
                                src={item.url}
                                alt=""
                                className={`h-full w-full object-cover transition-opacity ${isSelected ? "" : "opacity-35"}`}
                              />
                              <span
                                className={`absolute right-1 top-1 flex h-5 w-5 items-center justify-center rounded-full border text-[10px] font-bold transition-colors ${
                                  isSelected
                                    ? "border-primary bg-primary text-primary-foreground"
                                    : "border-white/80 bg-black/30 text-transparent"
                                }`}
                              >
                                ✓
                              </span>
                            </button>
                          );
                        })}
                      </div>
                    </div>
                  );
                })()}
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
          </>
        )}

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

        {batchErrors.length > 0 && (
          <ul className="flex flex-col gap-1 px-6 text-sm text-destructive">
            {batchErrors.map((e) => (
              <li key={e.url} className="truncate">
                {e.url}: {e.message}
              </li>
            ))}
          </ul>
        )}

        <div className="flex items-center justify-end gap-3 border-t border-border/60 bg-muted/20 px-6 py-4">
          {(preview || isBatchMode) && (
            <Button variant="ghost" onClick={resetForm} disabled={submitting} className="rounded-lg text-sm font-medium h-10 px-4">
              {t("common.cancel")}
            </Button>
          )}
          {isBatchMode ? (
            <Button onClick={handleDownloadAllBatch} disabled={!canDownloadBatch} className="rounded-lg shadow-xs h-10 px-6 text-sm font-semibold gap-2">
              {submitting ? (
                <>
                  <Loader2 className="h-4 w-4 animate-spin text-primary-foreground" />
                  <span>{t("common.loading")}</span>
                </>
              ) : (
                <>
                  <Download className="h-4 w-4" />
                  <span>{t("downloadForm.download_all_button", { count: urls.length })}</span>
                </>
              )}
            </Button>
          ) : (
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
                        : mediaType === "audio"
                          ? t("downloadForm.download_audio_button")
                          : t("downloadForm.download_video_button")}
                    </span>
                  </>
                )}
              </Button>
            )
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
