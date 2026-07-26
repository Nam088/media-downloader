import type { AudioFormatOption, VideoQualityOption } from "@/types/download";

/** Radio value used when a link has no explicit audio bitrate at all (e.g.
 * TikTok's pre-muxed formats — see `commands::media::extract_format_options`
 * on the backend). Submitting this means "omit audio_quality" (let yt-dlp
 * extract whatever's actually there) rather than sending a made-up number. */
export const BEST_AUDIO_QUALITY_VALUE = "__best__";

export function audioQualityValue(bitrateKbps: number | null): string {
  return bitrateKbps == null ? BEST_AUDIO_QUALITY_VALUE : `${bitrateKbps}kbps`;
}

/** Flat-playlist previews carry no per-entry format list at all (see
 * `commands::media::build_media_source`'s own comment on the backend) — so
 * there's nothing real to render per FR-004/FR-019's "never invent options"
 * rule. But that shouldn't mean playlist downloads can never pick a
 * resolution/bitrate at all: this is a fixed, generic set of common labels
 * (not validated against any specific link) that the backend passes through
 * as-is to yt-dlp's own graceful per-video format selector for each fanned-out
 * entry (`queue::video_format_selector` already tolerates a quality that
 * doesn't exist for a given video by falling back to the nearest one). */
export const GENERIC_PLAYLIST_VIDEO_QUALITIES: VideoQualityOption[] = [
  { label: "2160p", filesize_bytes: null },
  { label: "1440p", filesize_bytes: null },
  { label: "1080p", filesize_bytes: null },
  { label: "720p", filesize_bytes: null },
  { label: "480p", filesize_bytes: null },
  { label: "360p", filesize_bytes: null },
];
export const GENERIC_PLAYLIST_AUDIO_QUALITIES: AudioFormatOption[] = [
  { bitrate_kbps: 320, codec: "mp3", filesize_bytes: null },
  { bitrate_kbps: 256, codec: "mp3", filesize_bytes: null },
  { bitrate_kbps: 192, codec: "mp3", filesize_bytes: null },
  { bitrate_kbps: 128, codec: "mp3", filesize_bytes: null },
];
