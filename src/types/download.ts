export type MediaType = "audio" | "video" | "gallery";

/** Only meaningful when `media_type === "gallery"`. Mirrors the three modes
 * the reference implementation offered for a TikTok slideshow post. */
export type GalleryMode = "files" | "audio_only" | "images_only" | "slideshow";

/** The audio output format the user picked (FR-201).
 *
 * A discriminated union on `format`, not a `{ format, bitrate }` pair, and
 * that is the whole point: FR-203 says a bitrate only means anything for a
 * lossy codec, so `bitrate_kbps` exists **only** on the lossy members. TypeScript
 * then rejects `{ format: "flac", bitrate_kbps: 320 }` outright — the invalid
 * combination cannot be built, rather than being built and then hidden by the
 * picker. The Rust enum (`models::AudioOutput`) has exactly this shape, so the
 * guarantee holds on both sides of the bridge.
 *
 * `bitrate_kbps` omitted/null on a lossy format means "no specific bitrate
 * chosen": the backend falls back to the quality label already validated
 * against the source's real format list (`DownloadJob.audio_quality`), and
 * failing that, lets yt-dlp pick its own best.
 *
 * `"source"` means keep the source's format untouched — no transcoding step
 * runs at all (FR-202). */
export type AudioOutput =
  | { format: "mp3"; bitrate_kbps?: number | null }
  | { format: "m4a"; bitrate_kbps?: number | null }
  | { format: "opus"; bitrate_kbps?: number | null }
  | { format: "wav" }
  | { format: "flac" }
  | { format: "source" };

/** Output container for video jobs (FR-204). `"source"` keeps whatever the
 * source served — no remux is forced. */
export type VideoContainer = "mp4" | "mkv" | "source";

/** FR-205. `"compatibility"` is today's H.264/AAC behaviour, playable
 * essentially everywhere, and stays the default. `"quality"` takes the best
 * the source offers including VP9/AV1 — better compression, but older players
 * and many TVs cannot decode it. */
export type CodecPreference = "compatibility" | "quality";

/** Every output choice attached to one job, stored with it so a retry
 * reproduces the original configuration (FR-235). */
export interface OutputOptions {
  audio: AudioOutput;
  video_container: VideoContainer;
  codec_preference: CodecPreference;
  /** FR-208 — embed title/artist/source/upload date into the file. */
  embed_metadata: boolean;
  /** FR-209 — embed the thumbnail as cover art. Silently skipped, with the
   * reason written to the log, when the target container cannot hold cover
   * art (WAV, or "keep source format" where the container is unknown ahead of
   * time). That is never a job failure (FR-210). */
  embed_thumbnail: boolean;
}

/** What a **new** job's picker should start on: metadata and cover art on, per
 * FR-208/FR-209.
 *
 * Deliberately NOT the same as Rust's `OutputOptions::default()`, which has
 * both embed flags off. The two answer different questions. Rust's default
 * answers "what did a job that predates this feature mean?" — and the only
 * correct answer is "exactly what it did when it ran", which is no embedding.
 * This constant answers "what should a new job start as?", which FR-208/209
 * govern. Collapsing them into one value is how a default change silently
 * rewrites the meaning of old rows: every previously-finished job would
 * retroactively claim it had asked for embedded metadata, and retrying it
 * would produce a different file than the original (FR-235).
 *
 * Send this explicitly from the picker; omitting `output_options` entirely
 * gets the legacy no-embed behaviour. */
export const NEW_JOB_OUTPUT_OPTIONS: OutputOptions = {
  audio: { format: "mp3" },
  video_container: "mp4",
  codec_preference: "compatibility",
  embed_metadata: true,
  embed_thumbnail: true,
};

/** Whether a bitrate control should be shown at all for this format (FR-203).
 * Lossless formats and "keep source" have no bitrate to set — the type has no
 * field for one either. */
export function supportsBitrate(audio: AudioOutput): boolean {
  return audio.format === "mp3" || audio.format === "m4a" || audio.format === "opus";
}

/** Whether the chosen output can hold embedded cover art (FR-209/FR-210), so
 * the UI can explain why the control is unavailable instead of silently
 * dropping the request (SC-209). Mirrors `queue::thumbnail_support` — keep the
 * two in step.
 *
 * `"source"` reports `false` on purpose: the real container is decided by the
 * source at download time and may be one yt-dlp cannot embed into (WebM), so
 * the backend skips embedding rather than risk failing the job. */
export function supportsCoverArt(
  mediaType: MediaType,
  options: OutputOptions,
): boolean {
  if (mediaType === "audio") {
    return options.audio.format !== "wav" && options.audio.format !== "source";
  }
  if (mediaType === "video") {
    return options.video_container !== "source";
  }
  return false;
}

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
  /** The output choices this job was created with (FR-235), so the queue can
   * show what a job will actually produce and a retry reproduces it exactly.
   *
   * Optional on the read side only: the backend always serialises it, but
   * jobs stored before this feature existed carry no choices of their own and
   * come back as the Rust default (MP3/MP4, compatibility, no embedding) —
   * which is precisely the behaviour they ran with. */
  output_options?: OutputOptions;
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
  /** Output format/metadata choices. Omitting it is a valid, supported call
   * and yields today's behaviour exactly (MP3/MP4, compatibility codecs, no
   * embedding) — so the picker can be adopted incrementally. Send
   * `NEW_JOB_OUTPUT_OPTIONS` as the starting point for a new job. */
  output_options?: OutputOptions;
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
  /** One set of output choices applied to every picked video (FR-232).
   * Omitted means today's behaviour. */
  output_options?: OutputOptions;
}

export interface JobProgressEvent {
  job_id: string;
  /** `null` means the percentage is **unknown**, not zero: yt-dlp reports no
   * total size for audio-only formats and HLS, so there is nothing to compute
   * a percentage from. Treating that as 0% is what pinned the progress bar at
   * 0% for entire downloads. Render an indeterminate bar plus
   * `downloaded_bytes`/`speed_bytes_per_sec` instead — those are true. */
  progress_percent: number | null;
  /** Bytes fetched so far. Reported by yt-dlp even when the total isn't, and
   * live-only: `DownloadJob` has no such field because the database has no
   * column for it. */
  downloaded_bytes: number | null;
  speed_bytes_per_sec: number | null;
  eta_seconds: number | null;
}

/** The parts of a `job:progress` event that only make sense for a run that is
 * currently in flight, kept beside the persisted `DownloadJob` rather than on
 * it.
 *
 * `DownloadJob.progress_percent` mirrors a `REAL NOT NULL` column and is
 * always a number — the last percentage that was actually known. "We don't
 * know the percentage *right now*" is a property of the live run, so it lives
 * here, and disappears when the run does. */
export interface LiveProgress {
  /** `null` when the current run has no computable percentage. */
  progress_percent: number | null;
  downloaded_bytes: number | null;
}

export interface JobStatusChangedEvent {
  job_id: string;
  status: JobStatus;
  error_message: string | null;
  output_file_path: string | null;
}
