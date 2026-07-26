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

/// Định dạng audio đầu ra người dùng chọn (FR-201).
///
/// Bitrate nằm **bên trong** các biến thể nén mất dữ liệu, chứ không phải là
/// một trường riêng cạnh chúng. Đó là điểm mấu chốt của kiểu này: FR-203 nói
/// bitrate chỉ có nghĩa với định dạng nén mất dữ liệu, và cách duy nhất để
/// điều đó không bao giờ bị vi phạm là làm cho một `Flac` mang bitrate trở
/// nên **không biểu diễn được** — không có chỗ nào để đặt con số đó vào. Nếu
/// bitrate là một trường ngang hàng (`OutputOptions { audio_format, bitrate }`)
/// thì tổ hợp vô nghĩa vẫn dựng được, và việc nó không lọt xuống yt-dlp sẽ chỉ
/// còn phụ thuộc vào một câu `if` ai đó nhớ viết — hoặc vào giao diện chịu ẩn
/// ô nhập, thứ mà spec nói rõ là không được dựa vào.
///
/// `bitrate_kbps = None` trên một biến thể mất dữ liệu nghĩa là "không chọn
/// bitrate cụ thể": bộ dựng tham số khi đó rơi về nhãn chất lượng đã được đối
/// chiếu với danh sách format thật của nguồn (`DownloadJob.audio_quality`,
/// FR-019), và nếu cả nhãn đó cũng không có thì để yt-dlp tự chọn mức tốt
/// nhất. Nhờ vậy mặc định giữ nguyên đúng hành vi hiện tại.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "format", rename_all = "snake_case")]
pub enum AudioOutput {
    Mp3 {
        #[serde(default)]
        bitrate_kbps: Option<u32>,
    },
    M4a {
        #[serde(default)]
        bitrate_kbps: Option<u32>,
    },
    Opus {
        #[serde(default)]
        bitrate_kbps: Option<u32>,
    },
    /// Không nén mất dữ liệu — không có trường bitrate nào để đặt (FR-203).
    Wav,
    /// Không nén mất dữ liệu — không có trường bitrate nào để đặt (FR-203).
    Flac,
    /// Giữ nguyên định dạng nguồn: KHÔNG chạy bất kỳ bước chuyển mã nào
    /// (FR-202) — không `-x`, không `--audio-format`.
    Source,
}

impl Default for AudioOutput {
    /// MP3 không kèm bitrate rõ ràng — đúng bằng hành vi đang chạy hôm nay,
    /// nơi `--audio-format mp3` được viết cứng còn bitrate lấy từ
    /// `DownloadJob.audio_quality`.
    fn default() -> Self {
        AudioOutput::Mp3 { bitrate_kbps: None }
    }
}

impl AudioOutput {
    /// Tên định dạng mà `--audio-format` nhận, hoặc `None` khi lựa chọn là
    /// "giữ nguyên định dạng gốc" (lúc đó không có bước chuyển mã nào).
    pub fn ytdlp_audio_format(&self) -> Option<&'static str> {
        match self {
            AudioOutput::Mp3 { .. } => Some("mp3"),
            AudioOutput::M4a { .. } => Some("m4a"),
            AudioOutput::Opus { .. } => Some("opus"),
            AudioOutput::Wav => Some("wav"),
            AudioOutput::Flac => Some("flac"),
            AudioOutput::Source => None,
        }
    }

    /// Bitrate người dùng chọn, chỉ tồn tại trên các biến thể mất dữ liệu.
    ///
    /// Trả `None` cho WAV/FLAC/Source *về mặt kiểu dữ liệu*, không phải vì một
    /// nhánh `if` nào đó: các biến thể ấy không mang trường bitrate.
    pub fn bitrate_kbps(&self) -> Option<u32> {
        match self {
            AudioOutput::Mp3 { bitrate_kbps }
            | AudioOutput::M4a { bitrate_kbps }
            | AudioOutput::Opus { bitrate_kbps } => *bitrate_kbps,
            AudioOutput::Wav | AudioOutput::Flac | AudioOutput::Source => None,
        }
    }

    /// Định dạng này có nén mất dữ liệu không — tức `--audio-quality` có nghĩa
    /// gì với nó không (FR-203).
    pub fn is_lossy(&self) -> bool {
        matches!(
            self,
            AudioOutput::Mp3 { .. } | AudioOutput::M4a { .. } | AudioOutput::Opus { .. }
        )
    }
}

/// Container video đầu ra (FR-204).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum VideoContainer {
    #[default]
    Mp4,
    Mkv,
    /// Giữ nguyên container nguồn — không truyền `--merge-output-format`, để
    /// yt-dlp giữ đúng thứ nó tải về.
    Source,
}

impl VideoContainer {
    /// Giá trị cho `--merge-output-format`, hoặc `None` khi giữ nguyên gốc.
    pub fn merge_output_format(&self) -> Option<&'static str> {
        match self {
            VideoContainer::Mp4 => Some("mp4"),
            VideoContainer::Mkv => Some("mkv"),
            VideoContainer::Source => None,
        }
    }
}

/// Ưu tiên tương thích hay chất lượng khi chọn codec video (FR-205).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CodecPreference {
    /// H.264 + AAC như hiện nay: mở được ở gần như mọi trình phát. Là mặc
    /// định, nên người dùng sẵn có không thấy hành vi đổi.
    #[default]
    Compatibility,
    /// Lấy codec tốt nhất nguồn có, kể cả VP9/AV1 — nén tốt hơn nhưng nhiều
    /// trình phát cũ và TV không giải mã được.
    Quality,
}

/// Toàn bộ lựa chọn đầu ra gắn với một tác vụ (Key Entity "Tuỳ chọn đầu ra").
///
/// Lưu vào đúng MỘT cột JSON `download_jobs.output_options` chứ không phải mỗi
/// lựa chọn một cột: những giá trị này chỉ được bộ chạy tác vụ đọc, không bao
/// giờ được truy vấn, lọc hay sắp xếp theo, nên một cột riêng không đổi lấy
/// được gì từ SQLite; trong khi đó số lượng lựa chọn còn tăng tiếp trong chính
/// phase này (phụ đề, cắt đoạn, chapter, preset). Xem migration 0010.
///
/// `#[serde(default)]` ở cả struct lẫn từng trường là thứ hiện thực hoá FR-233:
/// một bản ghi (hoặc preset) lưu từ phiên bản trước, khi có tuỳ chọn mới được
/// thêm vào, vẫn đọc được và tuỳ chọn mới nhận giá trị mặc định thay vì làm
/// hỏng cả bản ghi.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct OutputOptions {
    pub audio: AudioOutput,
    pub video_container: VideoContainer,
    pub codec_preference: CodecPreference,
    /// FR-208. Xem chú thích của `Default` bên dưới về việc vì sao mặc định ở
    /// tầng Rust là `false` chứ không phải `true`.
    pub embed_metadata: bool,
    /// FR-209. Bị bỏ qua (có ghi nhật ký) khi container đích không chứa được
    /// ảnh bìa — FR-210 nói rõ đó không phải là lỗi của tác vụ.
    pub embed_thumbnail: bool,
}

// `OutputOptions::default()` trả lời câu hỏi "một tác vụ có TRƯỚC tính năng này
// thì có nghĩa là gì?" — và câu trả lời đúng duy nhất là "đúng hành vi mà nó đã
// chạy khi được tạo": MP3/MP4, ưu tiên tương thích, không nhúng gì cả. Vì thế
// hai cờ nhúng mặc định `false` ở đây.
//
// Đó là một câu hỏi KHÁC với "một tác vụ MỚI nên bắt đầu từ đâu?", vốn do
// FR-208/FR-209 quy định là bật sẵn cả hai. Câu hỏi thứ hai được trả lời ở
// `NEW_JOB_OUTPUT_OPTIONS` trong `src/types/download.ts` — nơi giao diện lấy
// giá trị khởi tạo cho bộ chọn.
//
// Gộp hai câu hỏi vào một giá trị chính là cách một lần thay mặc định âm thầm
// viết lại ý nghĩa của những dòng dữ liệu cũ: mọi tác vụ đã tải xong từ trước
// bỗng nhiên "đã từng được yêu cầu nhúng metadata", và một lần thử lại sẽ tái
// tạo ra thứ khác hẳn bản gốc — trái thẳng FR-235.

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DownloadJob {
    pub id: String,
    pub source_url: String,
    pub platform: String,
    pub media_type: MediaType,
    pub audio_quality: Option<String>,
    pub video_quality: Option<String>,
    pub gallery_mode: Option<GalleryMode>,
    /// A user-picked subset of `MediaSource.gallery_items` to actually
    /// download (checkbox grid in the gallery preview), as 0-based indices
    /// into that same `gallery_items` array — `None` means no selection was
    /// made, i.e. everything. Never restricts the audio track, only which
    /// images: audio inclusion is entirely governed by `gallery_mode`
    /// (`AudioOnly`/`Slideshow` need it, `Files`/`ImagesOnly` keep or drop it
    /// regardless of this field).
    ///
    /// Indices, not URLs: `downloader::queue::run_gallery_job` re-dumps the
    /// post right before the real download and applies these same ordinal
    /// positions to *that* fresh dump (via gallery-dl's own `--range`) —
    /// matching by position rather than by URL value, since a site's own
    /// item order for a given, unchanged post is stable across separate
    /// crawls even when its per-item URLs aren't (confirmed on TikTok: fresh,
    /// short-lived, signed CDN URLs every single crawl, but the same 2
    /// images + 1 audio track in the same order every time).
    pub selected_gallery_indices: Option<Vec<u32>>,
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
    /// This job's own display title (e.g. a video's title), shown in the
    /// queue instead of the raw `source_url` when available. `None` for
    /// rows created before this field existed, or paths where the backend
    /// never had a title to begin with (e.g. a flat-playlist fan-out where
    /// yt-dlp only enumerated URLs, not per-entry titles).
    pub title: Option<String>,
    /// The shared playlist's own title, duplicated onto every job fanned
    /// out from the same submission (same value for every row sharing
    /// `parent_playlist_id`) so the frontend can group them under one
    /// header without a separate playlist table. `None` for non-playlist
    /// jobs.
    pub playlist_title: Option<String>,
    /// Thứ tự chạy trong hàng đợi chờ, dùng fractional indexing: số nhỏ chạy
    /// trước, và chèn vào giữa hai mục chỉ cần lấy điểm giữa của chúng nên mỗi
    /// lần kéo-thả chỉ ghi đúng một dòng. `created_at` vẫn là tiêu chí phân
    /// định khi hai giá trị bằng nhau.
    pub queue_position: f64,
    /// Số lần đã tự thử lại vì lỗi tạm thời. Không tính lần chạy đầu tiên.
    pub retry_count: i64,
    /// Khi khác `None` và ở tương lai, job này đang chờ tới lượt thử lại và
    /// bộ điều phối sẽ bỏ qua nó cho tới thời điểm đó (FR-121).
    pub next_retry_at: Option<String>,
    /// Lựa chọn đầu ra đã dùng cho tác vụ này (FR-235): lưu cùng tác vụ nên
    /// một lần thử lại tái tạo đúng cấu hình ban đầu thay vì cấu hình đang
    /// hiển thị trên màn hình lúc bấm thử lại. Dòng tạo trước khi cột này tồn
    /// tại đọc ra `OutputOptions::default()`, tức đúng hành vi chúng đã chạy.
    #[serde(default)]
    pub output_options: OutputOptions,
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
    /// One entry per item in a flat-playlist preview (empty when
    /// `!is_playlist`) — lets the frontend list every video in a playlist
    /// individually (title/duration/thumbnail) instead of only offering an
    /// all-or-nothing "entire playlist" fetch, and pick a different
    /// media type/quality per item (`commands::download::PlaylistItemJobInput`).
    pub playlist_entries: Vec<PlaylistEntryPreview>,
}

/// One video in a flat-playlist preview. `title`/`duration_seconds`/
/// `thumbnail_url` come straight from yt-dlp's own `--flat-playlist` entry
/// metadata (no per-video format fetch — same reasoning as
/// `available_video_qualities`'s doc comment: fetching real formats for
/// every single playlist item up front doesn't scale, so quality choice for
/// playlist items stays a generic, unvalidated label).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaylistEntryPreview {
    pub url: String,
    pub title: String,
    pub duration_seconds: Option<i64>,
    pub thumbnail_url: Option<String>,
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
    /// Hidden by default — the Logs nav tab (job failures/retries/fallback
    /// decisions) is a debugging aid, not something most users need to see
    /// day to day.
    pub show_logs_tab: bool,
    /// Số tác vụ được chạy đồng thời (FR-112). Bộ điều phối đọc lại giá trị
    /// này mỗi vòng nên đổi lúc đang chạy có hiệu lực ngay.
    pub max_concurrent_downloads: u32,
    /// Giới hạn tốc độ cho **mỗi** tiến trình tải, tính bằng KB/s. 0 = không
    /// giới hạn. Là giới hạn theo tiến trình chứ không phải tổng băng thông —
    /// giao diện phải nói rõ điều này (xem phần Assumptions của spec).
    pub rate_limit_kbps: u32,
    /// Số lần tự thử lại tối đa cho lỗi tạm thời. 0 = tắt hẳn tự thử lại.
    pub max_retry_attempts: u32,
    /// Đóng cửa sổ thì thu về khay hệ thống thay vì thoát (FR-127).
    pub run_in_background: bool,
}
