use serde::{Deserialize, Serialize};

/// Mirrors `data-model.md` §1 (DownloadJob). `status` values are also enforced
/// by a CHECK constraint in `db/migrations/0001_init.sql`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaType {
    Audio,
    Video,
    /// Backed by gallery-dl instead of yt-dlp — set when `preview_media`
    /// falls back to gallery-dl (yt-dlp has no extractor for the URL, or the
    /// URL resolves to an image/gallery post yt-dlp can't represent, e.g. a
    /// TikTok slideshow). `DownloadJob.gallery_mode` picks what to actually
    /// do with the gallery's files.
    Gallery,
}

/// Only meaningful when `DownloadJob.media_type == MediaType::Gallery`.
/// Mirrors the three modes the reference implementation
/// (`ytb-download-ui`'s `post_process_gallery`) offered for a TikTok
/// slideshow post, generalized to any gallery-dl-backed source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GalleryMode {
    /// Download every file gallery-dl finds, as-is (images, audio, whatever
    /// the source actually has).
    Files,
    /// Keep only the audio track(s); delete downloaded images.
    AudioOnly,
    /// Keep only the image(s); delete the downloaded audio track.
    ImagesOnly,
    /// Merge downloaded images + audio into one slideshow video via ffmpeg
    /// (concat demuxer, each image shown for a fixed duration, muxed against
    /// the audio track — see `downloader::queue`'s gallery job handling).
    Slideshow,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    FetchingMetadata,
    Downloading,
    Paused,
    Completed,
    Failed,
    Canceled,
}

impl JobStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            JobStatus::Queued => "queued",
            JobStatus::FetchingMetadata => "fetching_metadata",
            JobStatus::Downloading => "downloading",
            JobStatus::Paused => "paused",
            JobStatus::Completed => "completed",
            JobStatus::Failed => "failed",
            JobStatus::Canceled => "canceled",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "queued" => Some(JobStatus::Queued),
            "fetching_metadata" => Some(JobStatus::FetchingMetadata),
            "downloading" => Some(JobStatus::Downloading),
            "paused" => Some(JobStatus::Paused),
            "completed" => Some(JobStatus::Completed),
            "failed" => Some(JobStatus::Failed),
            "canceled" => Some(JobStatus::Canceled),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadJob {
    pub id: String,
    pub source_url: String,
    pub platform: String,
    pub media_type: MediaType,
    pub audio_quality: Option<String>,
    pub video_quality: Option<String>,
    pub gallery_mode: Option<GalleryMode>,
    pub status: JobStatus,
    pub progress_percent: f64,
    pub speed_bytes_per_sec: Option<i64>,
    pub eta_seconds: Option<i64>,
    pub error_message: Option<String>,
    pub output_directory: String,
    pub output_file_path: Option<String>,
    pub is_playlist_item: bool,
    pub parent_playlist_id: Option<String>,
    pub retried_from_job_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Mirrors `data-model.md` §2 (MediaSource). `available_audio_formats` /
/// `available_video_qualities` MUST be populated from the real formats
/// returned by yt-dlp for this specific link — never a hard-coded list
/// (FR-004, FR-019). `filesize_bytes` is likewise whatever yt-dlp reports
/// (often an estimate for adaptive streams) — `None` when the source
/// doesn't expose it, never a guessed number.
///
/// `bitrate_kbps` is `None` when the source doesn't expose a bitrate at all
/// (e.g. TikTok serves pre-muxed video+audio without a separate,
/// bitrate-labeled audio track) — this represents "extract audio at
/// whatever quality is actually there" rather than a fabricated number.
/// There is always at most one such entry per link.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioFormatOption {
    pub bitrate_kbps: Option<u32>,
    pub codec: String,
    pub filesize_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoQualityOption {
    pub label: String,
    pub filesize_bytes: Option<u64>,
}

/// One file gallery-dl found for a gallery-backed `MediaSource`. `is_audio`
/// is decided from `extension` (mirrors the reference implementation's own
/// `post_process_gallery` file-classification: `.mp3/.m4a/.wav/.aac/.ogg` is
/// the post's background audio, everything else is an image) so the
/// frontend can render images as a grid and surface the audio track
/// separately without needing its own copy of that classification logic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GalleryItemPreview {
    pub url: String,
    pub extension: Option<String>,
    pub is_audio: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaSource {
    pub source_url: String,
    pub title: String,
    pub thumbnail_url: Option<String>,
    pub duration_seconds: Option<i64>,
    pub platform: String,
    pub is_playlist: bool,
    pub playlist_item_count: Option<i64>,
    pub available_video_qualities: Vec<VideoQualityOption>,
    pub available_audio_formats: Vec<AudioFormatOption>,
    /// `true` when this preview came from gallery-dl (yt-dlp had no
    /// extractor, or the link resolves to an image/gallery post yt-dlp
    /// can't represent) rather than yt-dlp — see `research.md` §2's
    /// gallery-dl amendment. When `true`, `gallery_items` is the
    /// authoritative content list and the `available_*` fields above are
    /// always empty.
    pub is_gallery: bool,
    pub gallery_items: Vec<GalleryItemPreview>,
}

/// Mirrors `data-model.md` §3 (DownloadedFile).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadedFile {
    pub id: String,
    pub job_id: String,
    pub file_path: String,
    pub file_format: String,
    pub file_size_bytes: i64,
    pub completed_at: String,
}

/// Mirrors `data-model.md` §5 (AppSettings).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub theme: String,
    pub language: String,
    pub default_output_directory: String,
}
