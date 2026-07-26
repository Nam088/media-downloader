import { BEST_AUDIO_QUALITY_VALUE } from "@/lib/generic-quality-options";
import type { CreateJobInput, GalleryMode, MediaSource, MediaType } from "@/types/download";

export interface BuildJobInputArgs {
  preview: MediaSource;
  /** Ignored when `preview.is_gallery` — a gallery-dl-backed source has no
   * audio/video split to choose between, so the job is always a gallery. */
  mediaType: MediaType;
  audioQuality?: string | null;
  videoQuality?: string | null;
  outputDirectory: string;
  galleryMode?: GalleryMode;
  /** 0-based indices into `preview.gallery_items`. Omit for "no selection was
   * made", which means everything. */
  selectedGalleryIndices?: number[];
  /** Only consulted when `preview.is_playlist`; defaults to `single_item`. */
  playlistScope?: "single_item" | "entire_playlist";
}

/**
 * The single place a `CreateJobInput` gets built.
 *
 * The single-URL flow and the batch flow each used to assemble one of these
 * inline, which meant the same user choice could produce two subtly different
 * jobs depending on whether one link or several were pasted. Everything that
 * decides the shape of a job now lives here.
 */
export function buildJobInput(args: BuildJobInputArgs): CreateJobInput {
  const {
    preview,
    mediaType,
    audioQuality,
    videoQuality,
    outputDirectory,
    galleryMode,
    selectedGalleryIndices,
    playlistScope,
  } = args;

  if (preview.is_gallery) {
    // Only the images are ever selectable — the audio track has no checkbox
    // in the grid, and the backend re-adds its index unconditionally (audio
    // inclusion is `gallery_mode`'s call, see
    // `models::DownloadJob.selected_gallery_indices`). So "everything is
    // selected" has to be measured against the image count, not against
    // `gallery_items.length`.
    const selectableCount = preview.gallery_items.filter((item) => !item.is_audio).length;
    // Send the index list only for a genuine subset. Sending the full list
    // instead of omitting it is *not* equivalent: the indices are positional
    // and the backend re-dumps the post immediately before downloading, so a
    // full list freezes the item count as it was at preview time, whereas
    // omitting it means "everything, as found now".
    const everythingSelected =
      !selectedGalleryIndices || selectedGalleryIndices.length >= selectableCount;

    return {
      source_url: preview.source_url,
      media_type: "gallery",
      audio_quality: null,
      video_quality: null,
      output_directory: outputDirectory,
      gallery_mode: galleryMode,
      ...(everythingSelected ? {} : { selected_gallery_indices: selectedGalleryIndices }),
      title: preview.title,
    };
  }

  return {
    source_url: preview.source_url,
    media_type: mediaType,
    // The sentinel means "this source published no bitrate at all" — it's a
    // radio value, not a quality. The backend reads a missing audio quality
    // as "extract whatever is actually there".
    audio_quality:
      mediaType === "audio" && audioQuality && audioQuality !== BEST_AUDIO_QUALITY_VALUE
        ? audioQuality
        : null,
    video_quality: mediaType === "video" ? (videoQuality ?? null) : null,
    output_directory: outputDirectory,
    playlist_scope: preview.is_playlist ? (playlistScope ?? "single_item") : undefined,
    title: preview.title,
  };
}
