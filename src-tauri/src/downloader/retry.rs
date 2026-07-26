//! Chính sách thử lại: quyết định lỗi nào đáng thử lại và chờ bao lâu.
//!
//! Tách riêng khỏi `queue` để logic quyết định này kiểm thử được mà không cần
//! dựng cả một hàng đợi, và để cả yt-dlp lẫn gallery-dl dùng chung một chính
//! sách (FR-120, FR-121).

// Nhóm `MAX_BACKOFF_SECONDS`, `BASE_BACKOFF_SECONDS`, `is_transient`,
// `backoff_seconds` và `should_retry` dưới đây mang `#[allow(dead_code)]`: đây
// là chính sách thử lại, còn bộ điều phối gọi tới nó được thêm ở một bước sau.
// Tới lúc đó các attribute này phải được gỡ bỏ, không phải để lại — chúng chỉ
// tồn tại để chính sách được kiểm thử trọn vẹn trước khi có người gọi.
// (`NETWORK_ERROR_MARKERS` và `has_network_marker` đã có người dùng ngay:
// hai bộ phân loại lỗi của yt-dlp và gallery-dl.)

/// Khoảng chờ tối đa giữa hai lần thử. Vượt quá mức này thì việc chờ tiếp
/// không còn giúp gì mà chỉ làm người dùng tưởng ứng dụng bị treo.
#[allow(dead_code)]
const MAX_BACKOFF_SECONDS: u64 = 300;

/// Khoảng chờ cho lần thử đầu tiên. Đủ dài để một lần chập mạng kịp hồi phục,
/// đủ ngắn để không gây khó chịu.
#[allow(dead_code)]
const BASE_BACKOFF_SECONDS: u64 = 5;

/// Các dấu hiệu cho thấy lỗi đến từ đường truyền chứ không từ nội dung. Thử
/// lại chỉ có ý nghĩa với nhóm này (FR-120).
///
/// Đặt ở đây thay vì trong `ytdlp` để cả hai bộ phân loại (`ytdlp` và
/// `gallery_dl`) cùng tham chiếu một nguồn duy nhất mà không module nào phải
/// phụ thuộc vào module kia.
pub const NETWORK_ERROR_MARKERS: [&str; 12] = [
    "network",
    "timed out",
    "timeout",
    "connection reset",
    "connection refused",
    "connection aborted",
    "temporary failure",
    "name resolution",
    "unable to connect",
    "http error 429",
    "http error 502",
    "http error 503",
];

/// Lỗi có đáng thử lại không, dựa trên mã lỗi ổn định của `AppError`.
///
/// Chỉ mã `NETWORK_ERROR` được coi là tạm thời. Mọi mã khác — kể cả
/// `DOWNLOAD_FAILED` vốn là nhóm gom — đều bị coi là vĩnh viễn, vì thử lại một
/// lỗi vĩnh viễn chỉ làm chậm phản hồi mà không đổi được kết quả.
#[allow(dead_code)]
pub fn is_transient(error_code: &str) -> bool {
    error_code == "NETWORK_ERROR"
}

/// Số giây chờ trước lần thử thứ `retry_count + 1`. Tăng gấp đôi mỗi lần,
/// chặn trên ở `MAX_BACKOFF_SECONDS`.
#[allow(dead_code)]
pub fn backoff_seconds(retry_count: i64) -> u64 {
    let exponent = retry_count.clamp(0, 16) as u32;
    BASE_BACKOFF_SECONDS
        .saturating_mul(2u64.saturating_pow(exponent))
        .min(MAX_BACKOFF_SECONDS)
}

/// Có nên tự thử lại không: lỗi phải là tạm thời và chưa dùng hết số lượt.
#[allow(dead_code)]
pub fn should_retry(error_code: &str, retry_count: i64, max_retries: i64) -> bool {
    is_transient(error_code) && retry_count < max_retries
}

/// Stderr có mang dấu hiệu lỗi đường truyền không. Người gọi phải kiểm tra các
/// lỗi nội dung TRƯỚC khi hỏi hàm này (xem `classify_ytdlp_error`).
pub fn has_network_marker(lower_stderr: &str) -> bool {
    NETWORK_ERROR_MARKERS
        .iter()
        .any(|marker| lower_stderr.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_errors_are_transient() {
        assert!(is_transient("NETWORK_ERROR"));
    }

    #[test]
    fn content_errors_are_permanent() {
        assert!(!is_transient("ACCESS_DENIED"));
        assert!(!is_transient("UNSUPPORTED_PLATFORM"));
        assert!(!is_transient("INVALID_QUALITY_OPTION"));
    }

    #[test]
    fn generic_download_failures_are_permanent() {
        // DOWNLOAD_FAILED là nhóm gom mọi lỗi chưa nhận diện được. Coi nó là
        // vĩnh viễn: thà báo lỗi ngay còn hơn bắt người dùng chờ hết ba vòng
        // thử lại vô ích (SC-106). Lỗi mạng thật đã được tách ra thành
        // NETWORK_ERROR ở tầng phân loại rồi.
        assert!(!is_transient("DOWNLOAD_FAILED"));
    }

    #[test]
    fn backoff_grows_and_is_capped() {
        assert_eq!(backoff_seconds(0), 5);
        assert_eq!(backoff_seconds(1), 10);
        assert_eq!(backoff_seconds(2), 20);
        assert_eq!(backoff_seconds(3), 40);
        assert_eq!(backoff_seconds(20), 300, "chặn trên ở 5 phút");
    }

    #[test]
    fn should_retry_stops_at_the_configured_limit() {
        assert!(should_retry("NETWORK_ERROR", 0, 3));
        assert!(should_retry("NETWORK_ERROR", 2, 3));
        assert!(!should_retry("NETWORK_ERROR", 3, 3), "đã dùng hết lượt");
        assert!(!should_retry("ACCESS_DENIED", 0, 3), "lỗi vĩnh viễn không thử lại");
        assert!(!should_retry("NETWORK_ERROR", 0, 0), "người dùng tắt retry");
    }
}
