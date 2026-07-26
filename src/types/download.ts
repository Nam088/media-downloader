// Một hằng số, hai chỗ dùng: ô xem trước (FR-213) và giá trị khởi tạo của bộ
// chọn. Nhập lại từ `lib/filename-template` thay vì viết lại `"{title}"` ở đây
// để hai nơi không bao giờ lệch nhau.
import { DEFAULT_TEMPLATE as DEFAULT_FILENAME_TEMPLATE } from "../lib/filename-template";

export type MediaType = "audio" | "video" | "gallery" | "music";

/** The three lossless-music quality tiers the SpotiFLAC engine offers
 * (data-model.md §5). Stored in `DownloadJob.audio_quality` as-is for
 * `media_type === "music"` jobs; the backend validates the value against the
 * preview's `available_music_tiers`, so this list never overrides the source. */
export type MusicQualityTier = "flac16" | "flac24" | "mp3_320";

export const MUSIC_QUALITY_TIERS: readonly MusicQualityTier[] = [
  "flac16",
  "flac24",
  "mp3_320",
] as const;

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

/** Where the chosen subtitles end up (FR-219). `"separate_files"` writes
 * `.vtt`/`.srt` files next to the media and works for every output format;
 * `"embedded"` puts them inside the media file as selectable tracks and only
 * works for containers that can hold a subtitle track (see
 * `supportsEmbeddedSubtitles`). */
export type SubtitleDelivery = "separate_files" | "embedded";

/** FR-217→FR-221. `languages` holds codes taken from
 * `MediaSource.subtitles[].language` — the languages the source actually has.
 * There is deliberately no fixed language list anywhere in this file.
 *
 * An empty `languages` means "no subtitles", which is the default: a job that
 * predates this feature never starts pulling extra files.
 *
 * `include_auto_generated` is a single flag for the whole selection rather
 * than a per-language one, because that is exactly how yt-dlp takes it
 * (`--sub-langs` is a list, `--write-auto-subs` is one flag over that list).
 * Set it whenever any picked language is only offered as auto-generated
 * (`MediaSource.subtitles[].auto_generated`). */
export interface SubtitleOptions {
  languages: string[];
  delivery: SubtitleDelivery;
  include_auto_generated: boolean;
}

/** The slice of the content to download (FR-222→FR-224). At least one bound
 * must be set — a range with neither is just "the whole thing", which is
 * `{ mode: "whole" }`. Validate before submitting: the backend rejects an
 * end that is not after the start, a negative time, or an empty range with
 * `INVALID_TRIM_RANGE`. */
export interface TrimRange {
  /** Omitted/null means "from the beginning". */
  start_seconds?: number | null;
  /** Omitted/null means "to the end". */
  end_seconds?: number | null;
  /** FR-224 — cut exactly at the requested times by re-encoding around the
   * cut points. The UI MUST warn that this makes the job noticeably slower;
   * left off, cuts land on the nearest keyframe and cost nothing extra. */
  accurate_cut?: boolean;
}

/** How the download is divided into files (FR-222→FR-227).
 *
 * A discriminated union, not `{ trim, split_chapters }` side by side, and
 * that is the whole point: FR-226 says trimming and chapter-splitting are
 * mutually exclusive, and a union makes "both at once" unrepresentable —
 * TypeScript refuses it, and the Rust enum (`models::SegmentMode`) has the
 * same shape, so no runtime check has to remember the rule. Disabling one
 * control in the picker is a UX nicety on top, not the enforcement. */
export type SegmentMode =
  | { mode: "whole" }
  | ({ mode: "trim" } & TrimRange)
  | { mode: "split_chapters" };

/** Every output choice attached to one job, stored with it so a retry
 * reproduces the original configuration (FR-235).
 *
 * The fields added after the first release are optional on purpose: the whole
 * struct is `#[serde(default)]` on the Rust side, so any of them may be
 * omitted and the backend fills in the value that reproduces today's
 * behaviour (FR-233). */
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
  /** FR-212 — `{field}` filename template; see `src/lib/filename-template.ts`
   * for the renderer that powers the live preview (FR-213). Omitted means
   * `DEFAULT_TEMPLATE` (`"{title}"`), which reproduces today's names exactly.
   *
   * The extension is not part of it: the backend always appends the real one
   * from the downloaded format, and a trailing `{ext}` is stripped so
   * `"{title}.{ext}"` doesn't produce `Song.mp4.mp4`. */
  filename_template?: string;
  subtitles?: SubtitleOptions;
  /** Trim **or** chapter-split — never both (FR-226). */
  segment?: SegmentMode;
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

/** What the subtitle picker starts on: nothing selected. Spelled out as its
 * own constant rather than folded into `NEW_JOB_OUTPUT_OPTIONS`, because for
 * these later options "what a new job starts as" and "what the backend
 * assumes when the field is absent" are the *same* answer — so sending them
 * would be noise on the wire, and a picker that never opened would look
 * indistinguishable from one that was opened and left alone. */
export const NEW_JOB_SUBTITLE_OPTIONS: SubtitleOptions = {
  languages: [],
  delivery: "separate_files",
  include_auto_generated: false,
};

/** The whole content in one file — same reasoning as
 * `NEW_JOB_SUBTITLE_OPTIONS`. */
export const NEW_JOB_SEGMENT_MODE: SegmentMode = { mode: "whole" };

/** The filename template a new job starts on, i.e. today's names exactly. */
export const NEW_JOB_FILENAME_TEMPLATE = DEFAULT_FILENAME_TEMPLATE;

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

/** Whether the chosen output can hold an embedded subtitle track
 * (FR-220), so the picker can disable "embed" with an explanation instead of
 * letting the backend silently skip it. Mirrors `queue::subtitle_embed_support`
 * — keep the two in step.
 *
 * Audio outputs report `false`: there is no subtitle track inside an MP3.
 * `"source"` reports `false` for the same reason as cover art — the real
 * container is decided at download time and may be one that cannot hold
 * subtitles. Separate subtitle files always work, including for audio. */
export function supportsEmbeddedSubtitles(
  mediaType: MediaType,
  options: OutputOptions,
): boolean {
  return mediaType === "video" && options.video_container !== "source";
}

/** Why a trim range is unusable, or `null` when it is fine (FR-223). Mirrors
 * `models::TrimRange::validate` so the field-level error the user sees is the
 * same rule the backend enforces — with one addition the backend cannot make:
 * `durationSeconds`, which lives on the preview, not on the job.
 *
 * Returns a stable reason code, not a sentence: the message belongs in the
 * locale files. */
export function validateTrimRange(
  range: TrimRange,
  durationSeconds?: number | null,
): "empty" | "negative" | "end_before_start" | "beyond_duration" | null {
  const { start_seconds: start, end_seconds: end } = range;
  if (start == null && end == null) return "empty";
  for (const bound of [start, end]) {
    if (bound == null) continue;
    if (!Number.isFinite(bound) || bound < 0) return "negative";
  }
  if (start != null && end != null && end <= start) return "end_before_start";
  if (durationSeconds != null && durationSeconds > 0) {
    if ((start ?? 0) >= durationSeconds) return "beyond_duration";
  }
  return null;
}

export type JobStatus =
  | "queued"
  | "fetching_metadata"
  | "downloading"
  /** A music job blocked on a Cloudflare challenge: the worker is alive and
   * waiting for a grant code on stdin, so the job still holds its concurrency
   * slot (data-model.md §2). Only `media_type === "music"` jobs enter it. */
  | "waiting_input"
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

/** One subtitle language the source actually offers (FR-217).
 *
 * `auto_generated` separates the author's own subtitles from machine-made
 * ones — they come from two different maps in yt-dlp's output
 * (`subtitles` vs `automatic_captions`), so this is source data, not a guess.
 * When a language has both, only the author's is listed.
 *
 * `label` is the source's own human name (`"Vietnamese"`) and is `null` when
 * it doesn't provide one — no name is invented from the code (FR-211), so the
 * UI decides how to render a bare `"vi"`. */
export interface SubtitleTrackPreview {
  /** The code that goes into the download request, e.g. `"vi"`, `"en-US"`. */
  language: string;
  label: string | null;
  auto_generated: boolean;
}

/** One chapter of the content (FR-225). `title` is `null` when the source
 * gives the chapter no name — the UI supplies a placeholder, the backend
 * never invents one (FR-211). */
export interface ChapterPreview {
  title: string | null;
  start_seconds: number | null;
  end_seconds: number | null;
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
  /** The subtitle languages this source really has (FR-217). Three states,
   * three different meanings — which is why it is nullable rather than a
   * plain array:
   *   - `null`: **not checked**. A gallery-dl preview, or a flat-playlist
   *     preview (per-video metadata isn't fetched at that depth). Say
   *     "unknown"; do not render an empty picker that reads as "still
   *     loading".
   *   - `[]`: checked, and the source has **no** subtitles — hide or disable
   *     the subtitle section with an explanation (FR-221).
   *   - non-empty: the real list; use it as-is.
   *
   * Optional only for older fixtures — the backend always sends the field. */
  subtitles?: SubtitleTrackPreview[] | null;
  /** The chapter list (FR-225), with the same three states as `subtitles`:
   * `null` = not checked, `[]` = checked and there are none (disable the
   * chapter-split option with an explanation), non-empty = show the count and
   * enable it. */
  chapters?: ChapterPreview[] | null;
  /** Only set on a SpotiFLAC-backed preview (Spotify/Tidal/Apple Music/
   * Pandora links). Non-empty means "this is a music source": the form shows
   * the tier picker instead of the video/audio quality lists, and the job is
   * created with `media_type: "music"`. Always the full three-tier list in
   * this scope — the worker does not probe tiers ahead of time. */
  available_music_tiers?: MusicQualityTier[];
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
  /** Only set for music jobs: the provider the SpotiFLAC worker is currently
   * pulling from (`"tidal" | "qobuz" | "deezer" | "amazon" | "ext:<name>"`).
   * Live-only, like `downloaded_bytes` — the queue shows it while the run is
   * in flight; the final provider is persisted on the library row instead. */
  provider?: string;
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
  /** See `JobProgressEvent.provider` — carried here so the queue row can name
   * the source a music job is actually downloading from (FR-009). */
  provider?: string;
}

export interface JobStatusChangedEvent {
  job_id: string;
  status: JobStatus;
  error_message: string | null;
  output_file_path: string | null;
  /** FR-227 — how many files this run produced: the original **plus** one per
   * chapter, and only for a chapter-split job. `null`/absent means the usual
   * single file, not "no files".
   *
   * A count on one event for one job, never N new queue rows: a chapter split
   * stays exactly one entry in the queue and in history. */
  produced_file_count?: number | null;
}

/** `job:cloudflare_challenge` — a music job's worker hit a Cloudflare
 * challenge and the job just entered `waiting_input`. The frontend opens the
 * grant dialog on `challenge_url`; the job resumes once
 * `submit_cloudflare_grant` is accepted (contracts/tauri-interface.md §3). */
export interface JobCloudflareChallengeEvent {
  job_id: string;
  challenge_url: string;
}
