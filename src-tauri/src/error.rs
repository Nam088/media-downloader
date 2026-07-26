use serde::Serialize;

/// Shared error type returned by every Tauri command (contracts/tauri-commands.md).
///
/// `message` is only an English fallback for unmapped codes. Real
/// localization happens on the frontend: `code` is a stable, machine-readable
/// key that `ErrorBanner` maps to an i18next string per `AppSettings.language`
/// (FR-009). Keeping translation in one place (the JSON locale files) avoids
/// duplicating every error string in both Rust and JSON.
#[derive(Debug, Serialize)]
pub struct AppError {
    pub code: String,
    pub message: String,
}

/// Mã lỗi cho sự cố đường truyền. Khai báo một lần ở đây vì nó đi qua ba
/// module: hai bộ phân loại (`ytdlp`, `gallery_dl`) sinh ra nó, còn
/// `retry::is_transient` đọc nó để quyết định có thử lại hay không. Gõ sai ở
/// bất kỳ đầu nào cũng sẽ âm thầm biến lỗi mạng thành lỗi vĩnh viễn mà không
/// test nào bắt được.
pub const NETWORK_ERROR_CODE: &str = "NETWORK_ERROR";

/// Mã lỗi cho "người dùng chủ động dừng". Không phải một thất bại: khi
/// `run_job` trả về mã này, `pause`/`cancel` đã đặt trạng thái cuối cùng rồi,
/// nên bộ điều phối phải bỏ qua kết quả thay vì đánh dấu thất bại hay thử lại.
/// Khai báo một lần ở đây vì nó đi qua cả nơi sinh ra (mọi nhánh huỷ trong
/// `queue`) lẫn nơi đọc (`retry::decide_outcome`).
pub const CANCELED_ERROR_CODE: &str = "CANCELED";

/// Engine SpotiFLAC (specs/006): không provider nào có bài — khác hẳn lỗi
/// mạng (không đáng thử lại tự động) và message gợi ý tải thường qua yt-dlp.
pub const SPOTIFLAC_NO_SOURCE_CODE: &str = "SPOTIFLAC_NO_SOURCE";
/// Mọi provider đều từ chối vì giới hạn khu vực.
pub const SPOTIFLAC_REGION_BLOCKED_CODE: &str = "SPOTIFLAC_REGION_BLOCKED";
/// Job đứng ở `waiting_input` quá 15 phút hoặc nhập grant sai quá 3 lần.
pub const SPOTIFLAC_CHALLENGE_TIMEOUT_CODE: &str = "SPOTIFLAC_CHALLENGE_TIMEOUT";
/// Cảnh báo (không fail job): máy thiếu Node.js nên JS extensions fallback bị
/// tắt cho lần chạy này.
pub const SPOTIFLAC_NODE_MISSING_CODE: &str = "SPOTIFLAC_NODE_MISSING";

impl AppError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn unsupported_platform(source_url: &str) -> Self {
        Self::new(
            "UNSUPPORTED_PLATFORM",
            format!("Link is not from a supported platform: {source_url}"),
        )
    }

    pub fn access_denied(reason: impl Into<String>) -> Self {
        Self::new("ACCESS_DENIED", reason.into())
    }

    /// Lỗi đường truyền — nhóm duy nhất đáng thử lại (xem `downloader::retry`).
    pub fn network_error(reason: impl Into<String>) -> Self {
        Self::new(NETWORK_ERROR_CODE, reason.into())
    }

    pub fn invalid_quality_option() -> Self {
        Self::new(
            "INVALID_QUALITY_OPTION",
            "Requested quality does not match any option returned by preview_media",
        )
    }

    pub fn not_found(what: &str) -> Self {
        Self::new("NOT_FOUND", format!("{what} not found"))
    }

    pub fn spotiflac_challenge_timeout() -> Self {
        Self::new(
            SPOTIFLAC_CHALLENGE_TIMEOUT_CODE,
            "Cloudflare verification was not completed in time",
        )
    }

    pub fn internal(err: impl std::fmt::Display) -> Self {
        Self::new("INTERNAL", err.to_string())
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::error::Error for AppError {}

impl From<rusqlite::Error> for AppError {
    fn from(err: rusqlite::Error) -> Self {
        AppError::internal(err)
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        AppError::internal(err)
    }
}
