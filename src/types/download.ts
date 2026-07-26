export type MediaType = "audio" | "video" | "gallery";

/** Only meaningful when `media_type === "gallery"`. Mirrors the three modes
 * the reference implementation offered for a TikTok slideshow post. */
export type GalleryMode = "files" | "audio_only" | "images_only" | "slideshow";

export type JobStatus =
  | "queued"
  | "fetching_metadata"
  | "downloading"
  | "paused"
  | "completed"
  | "failed"
  | "canceled";

/** `bitrate_kbps` is `null` when the source never exposed a bitrate at all
 * (e.g. TikTok's pre-muxed video+audio formats) — audio is still real and
 * extractable, there's just nothing to label a specific number with. */
export interface AudioFormatOption {
  bitrate_kbps: number | null;
  codec: string;
  filesize_bytes: number | null;
}

export interface VideoQualityOption {
  label: string;
  filesize_bytes: number | null;
}

/** One file gallery-dl found for a gallery-backed `MediaSource`. `is_audio`
 * is decided server-side from the file extension (mirrors the reference
 * implementation's own image/audio classification for a slideshow post). */
export interface GalleryItemPreview {
  url: string;
  extension: string | null;
  is_audio: boolean;
}

/** One entry in a playlist, as flattened by `yt-dlp --flat-playlist`. Empty
 * whenever the source isn't a playlist. `thumbnail_url` can be null when
 * yt-dlp reports no thumbnail for that specific entry. */
export interface PlaylistEntryPreview {
  url: string;
  title: string;
  duration_seconds: number | null;
  thumbnail_url: string | null;
}

/** Options are always populated from the real formats yt-dlp returned for
 * this specific link (FR-004, FR-019) — never a fixed list in this file.
 *
 * `is_gallery`/`gallery_items` are populated instead when the link was
 * resolved by gallery-dl rather than yt-dlp (yt-dlp had no extractor, or the
 * link is an image/gallery post yt-dlp can't represent — e.g. a TikTok
 * slideshow). `available_*` are always empty in that case. */
export interface MediaSource {
  source_url: string;
  title: string;
  thumbnail_url: string | null;
  duration_seconds: number | null;
  platform: string;
  is_playlist: boolean;
  playlist_item_count: number | null;
  available_video_qualities: VideoQualityOption[];
  available_audio_formats: AudioFormatOption[];
  is_gallery: boolean;
  gallery_items: GalleryItemPreview[];
  playlist_entries: PlaylistEntryPreview[];
}

export interface DownloadJob {
  id: string;
  source_url: string;
  platform: string;
  media_type: MediaType;
  audio_quality: string | null;
  video_quality: string | null;
  gallery_mode: GalleryMode | null;
  selected_gallery_indices: number[] | null;
  status: JobStatus;
  progress_percent: number;
  speed_bytes_per_sec: number | null;
  eta_seconds: number | null;
  error_message: string | null;
  output_directory: string;
  output_file_path: string | null;
  is_playlist_item: boolean;
  parent_playlist_id: string | null;
  retried_from_job_id: string | null;
  created_at: string;
  updated_at: string;
  /** This job's own display title, shown instead of the raw `source_url`
   * when available. `null` for jobs created before this field existed, or
   * paths where the backend never had a title to begin with. */
  title: string | null;
  /** The shared playlist's own title, duplicated onto every job fanned out
   * from the same submission (same value for every job sharing
   * `parent_playlist_id`), used as the queue's group header. `null` for
   * non-playlist jobs. */
  playlist_title: string | null;
  /** Run order in the waiting queue, using fractional indexing: lower runs
   * first, and dropping between two items just takes their midpoint so a
   * drag writes exactly one row. `created_at` still breaks ties. */
  queue_position: number;
  /** How many times this job has auto-retried after a transient failure.
   * Does not count the first run. */
  retry_count: number;
  /** When non-null and in the future, this job is waiting for its retry turn
   * and the dispatcher skips it until then. */
  next_retry_at: string | null;
}

export interface AppError {
  code: string;
  message: string;
}

export interface CreateJobInput {
  source_url: string;
  media_type: MediaType;
  /** `null` (or omitted) means "whatever the source actually has" — sent
   * instead of a made-up number when the source published no bitrate at all,
   * and for any job where the field doesn't apply. */
  audio_quality?: string | null;
  video_quality?: string | null;
  gallery_mode?: GalleryMode;
  /** Omitted means "everything", which is deliberately not the same as a full
   * list — see `DownloadJob.selected_gallery_indices`. */
  selected_gallery_indices?: number[];
  output_directory: string;
  playlist_scope?: "single_item" | "entire_playlist";
  /** This job's own display title (e.g. `MediaSource.title`). */
  title?: string;
}

/** One entry submitted to `create_playlist_download_jobs` — lets each video
 * in the playlist get its own media type and quality, per the "some video,
 * some audio" requirement. */
export interface PlaylistItemJobInput {
  source_url: string;
  media_type: MediaType;
  audio_quality?: string;
  video_quality?: string;
  /** This video's own title, from `MediaSource.playlist_entries[].title`. */
  title?: string;
}

export interface CreatePlaylistJobsInput {
  output_directory: string;
  items: PlaylistItemJobInput[];
  /** The playlist's own title (`MediaSource.title`), shared across every job
   * created from this submission as the queue's group header. */
  playlist_title?: string;
}

export interface JobProgressEvent {
  job_id: string;
  progress_percent: number;
  speed_bytes_per_sec: number | null;
  eta_seconds: number | null;
}

export interface JobStatusChangedEvent {
  job_id: string;
  status: JobStatus;
  error_message: string | null;
  output_file_path: string | null;
}
