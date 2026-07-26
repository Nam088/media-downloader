use serde::{Deserialize, Serialize};

use crate::downloader::filename;

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

/// Phụ đề được giao thành file riêng hay nhúng thẳng vào file media (FR-219).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SubtitleDelivery {
    /// Mặc định: file `.vtt`/`.srt` nằm cạnh file media. Chạy được với mọi
    /// định dạng đầu ra, kể cả audio.
    #[default]
    SeparateFiles,
    /// Nhúng thành track chọn được bên trong file media. Chỉ container video
    /// mới chứa được — xem `queue::subtitle_embed_support` (FR-220).
    Embedded,
}

/// Lựa chọn phụ đề của một tác vụ (FR-217→FR-221).
///
/// `languages` rỗng nghĩa là "không tải phụ đề", và đó là mặc định — nên một
/// tác vụ có trước tính năng này không đột nhiên kéo thêm file phụ đề về.
///
/// Mã ngôn ngữ ở đây PHẢI đến từ danh sách thật của nguồn
/// (`MediaSource.subtitles`, do `commands::media` đọc ra từ chính JSON yt-dlp
/// trả về). Không có danh sách ngôn ngữ cố định nào trong mã nguồn — FR-217
/// cấm đúng điều đó.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct SubtitleOptions {
    /// Nhiều ngôn ngữ cùng lúc (FR-218), theo đúng thứ tự người dùng thấy.
    pub languages: Vec<String>,
    pub delivery: SubtitleDelivery,
    /// Có lấy cả phụ đề máy sinh hay không.
    ///
    /// Tách khỏi `languages` chứ không nhét một cờ vào từng mã ngôn ngữ, vì
    /// đây đúng là cách yt-dlp nhận tham số: `--sub-langs` là một danh sách
    /// mã, còn "được phép dùng bản tự động sinh" là một cờ riêng
    /// (`--write-auto-subs`) áp cho cả danh sách. Giao diện biết mã nào chỉ có
    /// bản tự động (`MediaSource.subtitles[].auto_generated`) nên bật cờ này
    /// khi người dùng chọn một mã như vậy.
    pub include_auto_generated: bool,
}

impl SubtitleOptions {
    /// Danh sách mã ngôn ngữ đã bỏ trùng, giữ nguyên thứ tự — thứ thật sự đi
    /// vào `--sub-langs`.
    pub fn normalized_languages(&self) -> Vec<String> {
        let mut seen = Vec::with_capacity(self.languages.len());
        for language in &self.languages {
            let language = language.trim();
            if !language.is_empty() && !seen.iter().any(|kept: &String| kept == language) {
                seen.push(language.to_string());
            }
        }
        seen
    }
}

/// Mã ngôn ngữ hợp lệ: chữ/số ASCII, có thể kèm `-`, `_`, `.` ở giữa (`vi`,
/// `en-US`, `zh-Hans`, `en-orig`). Kiểm ở tầng lệnh chứ không chỉ ở giao diện
/// vì `create_download_job` gọi trực tiếp được, và giá trị này đi thẳng vào
/// đối số `--sub-langs` — nơi yt-dlp còn hiểu cả cú pháp biểu thức chính quy.
pub fn is_valid_language_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag.len() <= 32
        && tag.starts_with(|c: char| c.is_ascii_alphanumeric())
        && tag
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// Khoảng thời gian cần tải (FR-222→FR-224). Ít nhất một trong hai mốc phải có
/// mặt — một `TrimRange` không mốc nào chính là "tải cả video", vốn đã là
/// [`SegmentMode::Whole`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct TrimRange {
    /// `None` = từ đầu.
    pub start_seconds: Option<f64>,
    /// `None` = tới hết.
    pub end_seconds: Option<f64>,
    /// FR-224: cắt đúng tại thời điểm yêu cầu bằng cách mã hoá lại quanh điểm
    /// cắt (`--force-keyframes-at-cuts`). Giao diện PHẢI nói rõ tuỳ chọn này
    /// làm tăng thời gian xử lý; mặc định `false` để một lần cắt thường vẫn
    /// nhanh như cũ (chỉ cắt tại keyframe gần nhất).
    pub accurate_cut: bool,
}

/// Vì sao một khoảng thời gian bị từ chối. Là kiểu riêng chứ không phải một
/// chuỗi, để test chỉ ra được luật nào đã bắt lỗi.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrimRangeError {
    /// Không có mốc nào: đây không phải một khoảng, mà là "cả nội dung".
    NoBound,
    NotFinite,
    Negative,
    /// Kết thúc không nằm sau bắt đầu — bao gồm cả hai mốc bằng nhau, vốn cho
    /// ra một file rỗng chứ không phải một lỗi rõ ràng.
    EndNotAfterStart,
}

impl std::fmt::Display for TrimRangeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            TrimRangeError::NoBound => "a trim range needs a start time, an end time, or both",
            TrimRangeError::NotFinite => "trim times must be real numbers of seconds",
            TrimRangeError::Negative => "trim times cannot be negative",
            TrimRangeError::EndNotAfterStart => "the end time must come after the start time",
        };
        f.write_str(text)
    }
}

impl TrimRange {
    /// FR-223 ở tầng lệnh. Giao diện kiểm tra trước để báo lỗi ngay tại ô nhập,
    /// nhưng `create_download_job` gọi trực tiếp được nên phép kiểm tra thật
    /// phải nằm ở đây; giao diện chỉ là bản sao cho trải nghiệm.
    ///
    /// KHÔNG kiểm được ở đây: mốc vượt quá thời lượng nội dung. Thời lượng là
    /// thuộc tính của nguồn (`MediaSource.duration_seconds`), không nằm trên
    /// tác vụ, nên đó là phần việc của giao diện — và yt-dlp tự cắt cụt phần
    /// vượt quá chứ không hỏng.
    pub fn validate(&self) -> Result<(), TrimRangeError> {
        let bounds = [self.start_seconds, self.end_seconds];
        if bounds.iter().all(Option::is_none) {
            return Err(TrimRangeError::NoBound);
        }
        for bound in bounds.into_iter().flatten() {
            if !bound.is_finite() {
                return Err(TrimRangeError::NotFinite);
            }
            if bound < 0.0 {
                return Err(TrimRangeError::Negative);
            }
        }
        if let (Some(start), Some(end)) = (self.start_seconds, self.end_seconds) {
            if end <= start {
                return Err(TrimRangeError::EndNotAfterStart);
            }
        }
        Ok(())
    }
}

/// Phần nào của nội dung được tải, và nó ra thành mấy file (FR-222→FR-227).
///
/// FR-226 nói tách chương và cắt đoạn loại trừ lẫn nhau. Ở đây điều đó không
/// phải một câu `if` ai đó phải nhớ viết: hai lựa chọn là hai **biến thể của
/// cùng một enum**, nên một tác vụ vừa cắt vừa tách chương là thứ *không biểu
/// diễn được* — không có chỗ nào trên kiểu dữ liệu để đặt cả hai. Cùng một
/// nước đi với bitrate bên trong [`AudioOutput`] (FR-203).
///
/// Nếu để hai trường ngang hàng (`trim: Option<TrimRange>` + `split_chapters:
/// bool`) thì tổ hợp cấm vẫn dựng được, vẫn lưu xuống JSON được, và việc nó
/// không lọt tới yt-dlp sẽ chỉ còn phụ thuộc vào một phép kiểm tra lúc chạy —
/// hoặc tệ hơn, vào giao diện chịu vô hiệu hoá ô kia, thứ mà một lời gọi lệnh
/// trực tiếp bỏ qua hoàn toàn.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum SegmentMode {
    /// Cả nội dung, một file. Mặc định — đúng hành vi hôm nay.
    #[default]
    Whole,
    /// Chỉ một đoạn (FR-222).
    Trim(TrimRange),
    /// Mỗi chương một file (FR-225). Nguồn không có chương thì yt-dlp ghi một
    /// dòng "Chapter information is unavailable" rồi đi tiếp — không phải lỗi,
    /// nhưng giao diện vẫn phải chặn trước dựa trên `MediaSource.chapters`.
    SplitChapters,
}

impl SegmentMode {
    pub fn trim(&self) -> Option<&TrimRange> {
        match self {
            SegmentMode::Trim(range) => Some(range),
            _ => None,
        }
    }

    pub fn splits_chapters(&self) -> bool {
        matches!(self, SegmentMode::SplitChapters)
    }
}

/// Lựa chọn đầu ra không dùng được. Mang mã lỗi đi kèm để tầng lệnh dịch sang
/// `AppError` mà không phải tự đoán mã.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputOptionsError {
    Trim(TrimRangeError),
    /// Mã ngôn ngữ không phải một thẻ ngôn ngữ.
    SubtitleLanguage(String),
}

impl OutputOptionsError {
    pub fn code(&self) -> &'static str {
        match self {
            OutputOptionsError::Trim(_) => "INVALID_TRIM_RANGE",
            OutputOptionsError::SubtitleLanguage(_) => "INVALID_SUBTITLE_LANGUAGE",
        }
    }
}

impl std::fmt::Display for OutputOptionsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OutputOptionsError::Trim(err) => write!(f, "{err}"),
            OutputOptionsError::SubtitleLanguage(tag) => {
                write!(f, "not a subtitle language code: {tag}")
            }
        }
    }
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// Mẫu tên file dạng `{field}` (FR-212). Mặc định
    /// [`filename::DEFAULT_TEMPLATE`] = `"{title}"`, tức đúng cái tên mà hành
    /// vi cũ (`-o "%(title)s.%(ext)s"`) vẫn cho ra.
    ///
    /// Phần mở rộng KHÔNG nằm trong mẫu này: nó do yt-dlp quyết định tại thời
    /// điểm tải và luôn được nối vào cuối. `{ext}` ở cuối mẫu vì thế bị cắt bỏ
    /// (nếu không sẽ ra `Bài hát.mp3.mp3`) — xem
    /// `queue::strip_trailing_ext_field`.
    pub filename_template: String,
    pub subtitles: SubtitleOptions,
    /// Cắt đoạn HOẶC tách chương — không bao giờ cả hai (FR-226).
    pub segment: SegmentMode,
}

impl Default for OutputOptions {
    fn default() -> Self {
        Self {
            audio: AudioOutput::default(),
            video_container: VideoContainer::default(),
            codec_preference: CodecPreference::default(),
            embed_metadata: false,
            embed_thumbnail: false,
            filename_template: filename::DEFAULT_TEMPLATE.to_string(),
            subtitles: SubtitleOptions::default(),
            segment: SegmentMode::default(),
        }
    }
}

impl OutputOptions {
    /// Mẫu tên file thật sự dùng: một chuỗi rỗng (ô nhập bị xoá sạch) là "chưa
    /// chọn gì", không phải "muốn tên file rỗng", nên rơi về mẫu mặc định.
    pub fn effective_filename_template(&self) -> &str {
        if self.filename_template.trim().is_empty() {
            filename::DEFAULT_TEMPLATE
        } else {
            &self.filename_template
        }
    }

    /// Cửa chặn ở tầng lệnh cho những ràng buộc mà kiểu dữ liệu không tự giữ
    /// được. Loại trừ cắt/tách chương (FR-226) KHÔNG có ở đây — nó đã do
    /// [`SegmentMode`] đảm bảo, và một phép kiểm tra lúc chạy cho điều đó chỉ
    /// là thứ để quên.
    pub fn validate(&self) -> Result<(), OutputOptionsError> {
        if let Some(range) = self.segment.trim() {
            range.validate().map_err(OutputOptionsError::Trim)?;
        }
        for language in &self.subtitles.languages {
            let language = language.trim();
            if !is_valid_language_tag(language) {
                return Err(OutputOptionsError::SubtitleLanguage(language.to_string()));
            }
        }
        Ok(())
    }
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

/// Một ngôn ngữ phụ đề mà nguồn **thật sự** có (FR-217).
///
/// `auto_generated` là điểm chính: FR-217 đòi phân biệt rõ phụ đề do người tạo
/// cung cấp với phụ đề máy sinh, và hai thứ đó nằm ở hai bản đồ khác nhau
/// trong JSON của yt-dlp (`subtitles` và `automatic_captions`) chứ không phải
/// một cờ nào đó ta tự suy ra.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubtitleTrackPreview {
    /// Mã ngôn ngữ đúng như yt-dlp gọi (`vi`, `en`, `en-orig`, `zh-Hans`) —
    /// chính là thứ sẽ đi vào `--sub-langs`.
    pub language: String,
    /// Tên đọc được nếu nguồn có cung cấp (`"Vietnamese"`); `None` khi không —
    /// KHÔNG bịa ra từ mã ngôn ngữ (FR-211), giao diện tự chọn cách hiển thị.
    pub label: Option<String>,
    pub auto_generated: bool,
}

/// Một chương của nội dung (FR-225). `title` là `Option` vì nguồn có thể trả
/// về chương không tên; bịa "Chapter 3" ở tầng này là điền giá trị suy đoán
/// (FR-211), nên việc đặt nhãn thay thế thuộc về giao diện.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChapterPreview {
    pub title: Option<String>,
    pub start_seconds: Option<f64>,
    pub end_seconds: Option<f64>,
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
    /// Ngôn ngữ phụ đề nguồn thật sự có (FR-217).
    ///
    /// Ba giá trị, ba nghĩa khác nhau — và đó là lý do trường này là `Option`
    /// chứ không phải một `Vec` phẳng:
    ///   - `None`: **chưa kiểm tra**. Preview do gallery-dl trả về, hoặc một
    ///     playlist phẳng (yt-dlp không lấy metadata từng video ở bước này).
    ///     Giao diện phải nói "không rõ", KHÔNG được hiện một ô chọn rỗng quay
    ///     mãi như đang tải.
    ///   - `Some([])`: đã kiểm tra, nguồn **không có** phụ đề nào — phần chọn
    ///     phụ đề bị ẩn hoặc vô hiệu hoá kèm giải thích (FR-221).
    ///   - `Some([...])`: danh sách thật, dùng nguyên.
    pub subtitles: Option<Vec<SubtitleTrackPreview>>,
    /// Danh sách chương (FR-225). `None`/`Some([])` mang đúng hai nghĩa như
    /// `subtitles`: "chưa kiểm tra" khác hẳn "không có chương nào", và chỉ cái
    /// sau mới được phép vô hiệu hoá tuỳ chọn tách chương kèm giải thích.
    pub chapters: Option<Vec<ChapterPreview>>,
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

/// Một mục trong Thư viện (FR-301) — đúng một file trên đĩa do chính ứng dụng
/// tải về, cùng mọi thứ cần để hiện nó lên lưới mà không phải hỏi lại nguồn.
///
/// Đây là hình dạng đầy đủ của một dòng `downloaded_files` (`data-model.md`
/// §3). Struct `DownloadedFile` cũ — chỉ có đường dẫn, định dạng, dung lượng,
/// thời điểm — đã bị thay hẳn bằng struct này: nó là thứ duy nhất đọc bảng
/// ấy, và giữ lại một hình dạng thứ hai chỉ để mô tả nửa số cột là cách chắc
/// chắn để hai bên trôi khỏi nhau.
///
/// `downloaded_at` map thẳng vào cột `completed_at`: tên cột giữ nguyên (đổi
/// tên cột là một migration cho một thứ thuần hiển thị), nhưng tên trường
/// theo hợp đồng đã chốt với tầng giao diện.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LibraryItem {
    pub id: String,
    pub file_path: String,
    pub title: String,
    pub media_type: MediaType,
    pub file_format: String,
    pub file_size_bytes: i64,
    /// `None` = **không biết**, không phải 0. Mọi mục nạp lại từ lịch sử cũ
    /// (FR-303) đều như vậy: thời lượng chỉ đo được bằng cách mở chính file
    /// ấy, và làm việc đó cho cả thư viện lúc khởi động đúng là thứ FR-327
    /// cấm. Từ phase này trở đi, mỗi tác vụ hoàn tất đều đo một lần rồi ghi.
    pub duration_seconds: Option<i64>,
    pub platform: String,
    pub source_url: String,
    /// Đường dẫn ảnh đại diện **cục bộ** (FR-304), không phải URL. `None` khi
    /// nguồn không hề có ảnh, hoặc khi mục này được nạp lại từ lịch sử cũ —
    /// giao diện dùng ảnh thay thế theo `media_type` thay vì để ô trống.
    pub thumbnail_path: Option<String>,
    pub downloaded_at: String,
    pub is_missing: bool,
    pub job_id: String,
}

/// Tiêu chí sắp xếp thư viện (FR-309).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LibrarySort {
    #[default]
    DownloadedAt,
    Title,
    FileSize,
    Duration,
}

impl LibrarySort {
    /// Mệnh đề `ORDER BY`. Không bao giờ ghép từ chuỗi do người dùng gửi lên:
    /// tiêu chí sắp xếp là một enum đóng, nên câu lệnh sinh ra chỉ có thể là
    /// một trong bốn chuỗi hằng dưới đây.
    ///
    /// `NULLS LAST` cho thời lượng là chủ ý: mục chưa biết thời lượng
    /// (`NULL`) phải nằm cuối ở CẢ hai chiều, vì "không biết" không phải là
    /// "ngắn nhất". SQLite mặc định xếp NULL TRƯỚC ở chiều tăng dần, nên vế
    /// tăng dần cần nói rõ. Viết dưới dạng `NULLS LAST` chứ không phải mẹo
    /// `duration_seconds IS NULL ASC, ...` vì chỉ dạng thứ nhất dùng được chỉ
    /// mục — dạng thứ hai là một biểu thức, và nó kéo cả câu lệnh về `USE
    /// TEMP B-TREE FOR ORDER BY` (đã kiểm bằng `EXPLAIN QUERY PLAN`).
    ///
    /// Vế phụ `rowid` giữ cho phân trang ổn định khi hai mục bằng nhau ở tiêu
    /// chí chính — thiếu nó, cùng một mục có thể xuất hiện ở cả trang 1 lẫn
    /// trang 2. Chiều của vế phụ phải khớp với chiều quét chỉ mục, nếu không
    /// SQLite lại phải dựng b-tree tạm: `completed_at` có chỉ mục khai báo
    /// `DESC`, nên quét xuôi cho ra `completed_at DESC` kèm `rowid ASC` —
    /// ngược chiều nhau — trong khi các chỉ mục còn lại khai báo `ASC` nên hai
    /// vế cùng chiều. Cả tám tổ hợp dưới đây đều đã được kiểm là không sinh
    /// b-tree tạm (xem test `every_sort_option_is_served_by_an_index`).
    pub fn order_by(self, direction: SortDirection) -> &'static str {
        match (self, direction) {
            (LibrarySort::DownloadedAt, SortDirection::Asc) => "completed_at ASC, rowid DESC",
            (LibrarySort::DownloadedAt, SortDirection::Desc) => "completed_at DESC, rowid ASC",
            (LibrarySort::Title, SortDirection::Asc) => "title COLLATE NOCASE ASC, rowid ASC",
            (LibrarySort::Title, SortDirection::Desc) => "title COLLATE NOCASE DESC, rowid DESC",
            (LibrarySort::FileSize, SortDirection::Asc) => "file_size_bytes ASC, rowid ASC",
            (LibrarySort::FileSize, SortDirection::Desc) => "file_size_bytes DESC, rowid DESC",
            (LibrarySort::Duration, SortDirection::Asc) => {
                "duration_seconds ASC NULLS LAST, rowid ASC"
            }
            (LibrarySort::Duration, SortDirection::Desc) => {
                "duration_seconds DESC NULLS LAST, rowid DESC"
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SortDirection {
    Asc,
    #[default]
    Desc,
}

/// Trang Lịch sử, lọc ở backend nên số trang luôn khớp với kết quả đang lọc
/// (cùng lý do với `LibraryQuery` ngay dưới đây). `status` là `None` = cả ba
/// trạng thái kết thúc (tab "Tất cả"), `Some(x)` = chỉ tab đó — `x` luôn là
/// một trong ba trạng thái kết thúc, không phải giá trị nào khác của
/// `JobStatus`; lời gọi với một trạng thái đang-chạy đơn giản là không khớp
/// dòng nào, vì Lịch sử luôn ép thêm điều kiện thuộc ba trạng thái đó.
#[derive(Debug, Clone, PartialEq, Deserialize, Default)]
#[serde(default)]
pub struct HistoryQuery {
    /// Khớp URL nguồn, tên file đầu ra, hoặc tên nền tảng — không phân biệt
    /// hoa thường, giống hệt bộ lọc phía giao diện trước đây.
    pub search: Option<String>,
    pub status: Option<JobStatus>,
    pub limit: i64,
    pub offset: i64,
}

/// Trạng thái duyệt thư viện (FR-307 → FR-310). `#[serde(default)]` ở cả
/// struct: một lời gọi `list_library({})` là hợp lệ và có nghĩa "mọi thứ, mới
/// nhất trước".
///
/// Mọi bộ lọc kết hợp theo logic VÀ (FR-308). Bên trong MỘT bộ lọc nhiều giá
/// trị (ví dụ `platforms: ["youtube", "tiktok"]`) thì là HOẶC — đó là cách
/// một nhóm ô chọn hoạt động, và là cách duy nhất để `platforms` có nhiều hơn
/// một phần tử mà vẫn trả về dòng nào đó.
#[derive(Debug, Clone, PartialEq, Deserialize, Default)]
#[serde(default)]
pub struct LibraryQuery {
    /// Khớp với tiêu đề HOẶC tên file, không phân biệt hoa thường kể cả với
    /// tiếng Việt — xem cột `search_text` trong migration 0012.
    pub search: Option<String>,
    pub media_types: Vec<MediaType>,
    pub platforms: Vec<String>,
    pub formats: Vec<String>,
    /// Khoảng thời gian tải, so sánh trực tiếp trên chuỗi RFC 3339 (đã lưu ở
    /// dạng sắp xếp được theo thứ tự từ điển). Bao gồm cả hai đầu.
    pub downloaded_from: Option<String>,
    pub downloaded_to: Option<String>,
    /// `None` = không quan tâm; `Some(true)` = chỉ các mục đang thiếu (màn
    /// hình dọn dẹp của FR-324); `Some(false)` = chỉ các mục còn file.
    pub is_missing: Option<bool>,
    pub sort: LibrarySort,
    pub direction: SortDirection,
    /// Phân trang. FR-310: giao diện không bao giờ nhận cả 10.000 dòng qua
    /// cầu IPC trong một lần — nó xin từng trang khi người dùng cuộn tới.
    /// `None` = không giới hạn (dùng cho xuất danh sách phát theo bộ lọc).
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// Một dòng trong phân bố của FR-328 (theo nền tảng hoặc theo loại nội dung).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LibraryBreakdownEntry {
    pub key: String,
    pub item_count: i64,
    pub total_size_bytes: i64,
}

/// FR-328. Được tính bằng chính bộ lọc đang áp, nên con số luôn khớp với thứ
/// người dùng đang nhìn (SC-307) thay vì mô tả một tập khác.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LibraryStats {
    pub total_items: i64,
    pub total_size_bytes: i64,
    pub missing_items: i64,
    pub by_platform: Vec<LibraryBreakdownEntry>,
    pub by_media_type: Vec<LibraryBreakdownEntry>,
    /// Danh sách định dạng thật sự có trong thư viện, để bộ lọc FR-308 chào
    /// đúng những gì tồn tại chứ không phải một danh sách cứng.
    pub formats: Vec<String>,
}

/// Dữ liệu ghi vào chỉ mục khi một tác vụ tạo ra một file (FR-301, FR-302).
///
/// Là một struct chứ không phải chín tham số vị trí: `insert_downloaded_file`
/// từng nhận bốn tham số cùng kiểu chuỗi và đã đủ dễ ghi nhầm thứ tự; chín
/// thì chắc chắn có ngày lẫn `platform` với `source_url` mà vẫn biên dịch
/// được.
#[derive(Debug, Clone, PartialEq)]
pub struct NewLibraryFile {
    pub job_id: String,
    pub file_path: String,
    pub file_format: String,
    pub file_size_bytes: i64,
    pub title: String,
    pub media_type: MediaType,
    pub platform: String,
    pub source_url: String,
    pub duration_seconds: Option<i64>,
    pub thumbnail_path: Option<String>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trim_range_needs_at_least_one_bound() {
        assert_eq!(
            TrimRange::default().validate(),
            Err(TrimRangeError::NoBound)
        );
    }

    #[test]
    fn one_sided_trim_ranges_are_valid() {
        // FR-222 nói rõ "bắt đầu VÀ/HOẶC kết thúc": chỉ nhập mốc bắt đầu là
        // một yêu cầu hợp lệ ("từ phút 12 tới hết"), không phải lỗi.
        let from_start = TrimRange {
            start_seconds: Some(750.0),
            ..TrimRange::default()
        };
        assert_eq!(from_start.validate(), Ok(()));

        let until_end = TrimRange {
            end_seconds: Some(900.0),
            ..TrimRange::default()
        };
        assert_eq!(until_end.validate(), Ok(()));
    }

    #[test]
    fn an_end_that_does_not_come_after_the_start_is_rejected() {
        let backwards = TrimRange {
            start_seconds: Some(900.0),
            end_seconds: Some(750.0),
            ..TrimRange::default()
        };
        assert_eq!(backwards.validate(), Err(TrimRangeError::EndNotAfterStart));

        // Hai mốc bằng nhau cho ra một file rỗng chứ không phải một lỗi nhìn
        // thấy được, nên phải bị chặn ở đây.
        let empty = TrimRange {
            start_seconds: Some(750.0),
            end_seconds: Some(750.0),
            ..TrimRange::default()
        };
        assert_eq!(empty.validate(), Err(TrimRangeError::EndNotAfterStart));
    }

    #[test]
    fn negative_and_non_finite_times_are_rejected() {
        for value in [-1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let range = TrimRange {
                start_seconds: Some(value),
                end_seconds: Some(900.0),
                ..TrimRange::default()
            };
            assert!(
                range.validate().is_err(),
                "{value} không được coi là mốc hợp lệ"
            );
        }
    }

    #[test]
    fn validation_reaches_the_trim_range_through_the_output_options() {
        // `create_download_job` gọi trực tiếp được, nên phép kiểm tra phải nằm
        // trên chính bộ lựa chọn được lưu cùng tác vụ, chứ không chỉ trên
        // `TrimRange` mà một người gọi khác có thể không bao giờ chạm tới.
        let options = OutputOptions {
            segment: SegmentMode::Trim(TrimRange {
                start_seconds: Some(900.0),
                end_seconds: Some(750.0),
                accurate_cut: false,
            }),
            ..OutputOptions::default()
        };
        assert_eq!(
            options.validate(),
            Err(OutputOptionsError::Trim(TrimRangeError::EndNotAfterStart))
        );
        assert_eq!(options.validate().unwrap_err().code(), "INVALID_TRIM_RANGE");
    }

    #[test]
    fn splitting_chapters_has_nowhere_to_put_a_trim_range() {
        // FR-226 ở tầng kiểu dữ liệu. Một payload cố tình mang cả hai (giao
        // diện lỗi, hoặc một lời gọi lệnh viết tay) không dựng nổi một tác vụ
        // vừa cắt vừa tách: dữ liệu cắt không có trường nào để rơi vào.
        let raw = r#"{"mode":"split_chapters","start_seconds":10.0,"end_seconds":20.0}"#;
        let segment: SegmentMode = serde_json::from_str(raw).expect("vẫn đọc được");

        assert_eq!(segment, SegmentMode::SplitChapters);
        assert!(
            segment.trim().is_none(),
            "không được mang theo khoảng cắt nào"
        );
        assert!(segment.splits_chapters());
    }

    #[test]
    fn a_trim_range_sits_beside_its_tag_in_json() {
        // Hình dạng JSON ở đây LÀ hợp đồng với giao diện
        // (`SegmentMode` trong `src/types/download.ts`): thẻ `mode` nằm cùng
        // cấp với ba trường của khoảng cắt. Đổi cách gắn thẻ mà không đổi bản
        // TypeScript thì mọi lần cắt gửi lên sẽ lặng lẽ thành "tải cả video".
        let json = serde_json::to_value(SegmentMode::Trim(TrimRange {
            start_seconds: Some(750.0),
            end_seconds: None,
            accurate_cut: true,
        }))
        .unwrap();

        assert_eq!(json["mode"], "trim");
        assert_eq!(json["start_seconds"], 750.0);
        assert_eq!(json["end_seconds"], serde_json::Value::Null);
        assert_eq!(json["accurate_cut"], true);

        // Và chiều ngược lại: trường nào giao diện không gửi thì nhận mặc định
        // thay vì làm hỏng cả bản ghi.
        let parsed: SegmentMode =
            serde_json::from_str(r#"{"mode":"trim","start_seconds":750.0}"#).unwrap();
        assert_eq!(
            parsed,
            SegmentMode::Trim(TrimRange {
                start_seconds: Some(750.0),
                end_seconds: None,
                accurate_cut: false,
            })
        );
    }

    #[test]
    fn a_trim_job_is_never_also_a_chapter_split_job() {
        let segment = SegmentMode::Trim(TrimRange {
            start_seconds: Some(750.0),
            end_seconds: Some(900.0),
            accurate_cut: true,
        });
        assert!(!segment.splits_chapters());
        assert!(segment.trim().is_some());
    }

    #[test]
    fn options_stored_before_these_choices_existed_still_load() {
        // FR-233: một bản ghi (hoặc preset) lưu ở phiên bản trước chỉ có năm
        // trường cũ. Nó phải đọc được nguyên vẹn, và các lựa chọn mới nhận
        // đúng giá trị mặc định — tức không phụ đề, không cắt, mẫu tên file
        // mặc định.
        let raw = r#"{
            "audio": {"format":"opus","bitrate_kbps":192},
            "video_container":"mkv",
            "codec_preference":"quality",
            "embed_metadata":true,
            "embed_thumbnail":true
        }"#;
        let options: OutputOptions = serde_json::from_str(raw).expect("bản ghi cũ phải đọc được");

        assert_eq!(
            options.audio,
            AudioOutput::Opus {
                bitrate_kbps: Some(192)
            }
        );
        assert_eq!(options.filename_template, filename::DEFAULT_TEMPLATE);
        assert_eq!(options.subtitles, SubtitleOptions::default());
        assert!(options.subtitles.languages.is_empty());
        assert_eq!(options.segment, SegmentMode::Whole);
    }

    #[test]
    fn the_default_filename_template_is_the_one_that_reproduces_todays_names() {
        assert_eq!(
            OutputOptions::default().filename_template,
            filename::DEFAULT_TEMPLATE
        );
        assert_eq!(
            OutputOptions::default().effective_filename_template(),
            "{title}"
        );
    }

    #[test]
    fn an_emptied_out_template_box_falls_back_instead_of_naming_a_file_nothing() {
        let options = OutputOptions {
            filename_template: "   ".to_string(),
            ..OutputOptions::default()
        };
        assert_eq!(
            options.effective_filename_template(),
            filename::DEFAULT_TEMPLATE
        );
    }

    #[test]
    fn subtitle_languages_are_deduplicated_but_keep_their_order() {
        let options = SubtitleOptions {
            languages: vec![
                "vi".into(),
                " en ".into(),
                "vi".into(),
                "  ".into(),
                "en".into(),
            ],
            ..SubtitleOptions::default()
        };
        assert_eq!(options.normalized_languages(), vec!["vi", "en"]);
    }

    #[test]
    fn only_real_looking_language_tags_reach_the_subtitle_flag() {
        for good in ["vi", "en", "en-US", "zh-Hans", "en-orig", "pt_BR"] {
            assert!(is_valid_language_tag(good), "{good} phải hợp lệ");
        }
        // `--sub-langs` của yt-dlp còn hiểu cả biểu thức chính quy và tiền tố
        // phủ định, nên một chuỗi tuỳ ý ở đây không phải chuyện vô hại.
        for bad in [
            "",
            "en us",
            ".*",
            "-live_chat",
            "en,vi",
            "en;rm -rf",
            &"a".repeat(33),
        ] {
            assert!(
                !is_valid_language_tag(bad),
                "{bad:?} không được coi là hợp lệ"
            );
        }
    }

    #[test]
    fn a_bogus_subtitle_language_stops_the_job_instead_of_reaching_ytdlp() {
        let options = OutputOptions {
            subtitles: SubtitleOptions {
                languages: vec!["vi".into(), ".*".into()],
                ..SubtitleOptions::default()
            },
            ..OutputOptions::default()
        };
        assert_eq!(
            options.validate(),
            Err(OutputOptionsError::SubtitleLanguage(".*".to_string()))
        );
        assert_eq!(
            options.validate().unwrap_err().code(),
            "INVALID_SUBTITLE_LANGUAGE"
        );
    }
}
