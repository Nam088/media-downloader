//! Lỗi nào đáng thử lại, và chờ bao lâu trước khi thử.
//!
//! Module gom hai mảnh của cùng một quyết định: nhận ra stderr nào là sự cố
//! đường truyền (`has_network_marker`, dùng bởi hai bộ phân loại lỗi), và mã
//! lỗi nào thì đáng thử lại cùng khoảng chờ giữa các lần (`is_transient`,
//! `backoff_seconds`, `should_retry`, dùng bởi bộ điều phối hàng đợi).
//!
//! Tách riêng khỏi `queue` để logic quyết định này kiểm thử được mà không cần
//! dựng cả một hàng đợi, và để cả yt-dlp lẫn gallery-dl dùng chung một chính
//! sách (FR-120, FR-121).

use crate::error::NETWORK_ERROR_CODE;

// Nhóm `MAX_BACKOFF_SECONDS`, `BASE_BACKOFF_SECONDS`, `is_transient`,
// `backoff_seconds` và `should_retry` dưới đây mang `#[allow(dead_code)]`: đây
// là chính sách thử lại, còn bộ điều phối gọi tới nó được thêm ở một bước sau.
// Tới lúc đó các attribute này phải được gỡ bỏ, không phải để lại — chúng chỉ
// tồn tại để chính sách được kiểm thử trọn vẹn trước khi có người gọi.
// (`has_network_marker` đã có người dùng ngay: hai bộ phân loại lỗi của yt-dlp
// và gallery-dl.)

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
/// phụ thuộc vào module kia. Giữ ở mức riêng tư: `has_network_marker` mới là
/// API dành cho bên ngoài, vì nó đi kèm ràng buộc về thứ tự kiểm tra.
///
/// Mỗi dấu hiệu phải là một cụm đủ đặc trưng để không khớp nhầm với văn xuôi
/// bình thường. Chỉ dùng `"network"` hay `"timeout"` trần là quá rộng: tên đài
/// truyền hình "Network Ten" trong thông báo chặn bản quyền sẽ bị hiểu thành sự
/// cố đường truyền và bị thử lại vô ích, đúng cái mà SC-106 cấm.
const NETWORK_ERROR_MARKERS: [&str; 15] = [
    "network is unreachable",
    "network unreachable",
    "network error",
    "timed out",
    "socket timeout",
    "read timeout",
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
    error_code == NETWORK_ERROR_CODE
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

/// Stderr có mang dấu hiệu lỗi đường truyền không. Nhận stderr thô, tự chuyển
/// về chữ thường — người gọi không phải nhớ tiền điều kiện nào cả.
///
/// Người gọi PHẢI kiểm tra các lỗi nội dung TRƯỚC khi hỏi hàm này: một thông
/// báo "private video" đôi khi cũng chứa từ "connection reset", và lỗi nội dung
/// phải thắng để không bị thử lại vô ích (xem `classify_ytdlp_error`).
pub fn has_network_marker(stderr: &str) -> bool {
    let lower = stderr.to_lowercase();
    NETWORK_ERROR_MARKERS
        .iter()
        .any(|marker| lower.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Đúng một mẫu stderr tối giản cho MỖI dấu hiệu trong
    /// `NETWORK_ERROR_MARKERS`, và mỗi mẫu chỉ được chạm đúng một dấu hiệu.
    ///
    /// Ràng buộc "đúng một" là có chủ đích: bộ mẫu trước đây chồng lấn nhau
    /// ("network timeout" chạm cả `network` lẫn `timeout`) nên xoá 8 trong 12
    /// dấu hiệu mà bộ test vẫn xanh. Với bộ mẫu này, xoá bất kỳ dấu hiệu nào
    /// cũng làm đỏ.
    const NETWORK_ERROR_SAMPLES: [&str; 15] = [
        "ERROR: [Errno 101] Network is unreachable",
        "ERROR: send failed: network unreachable",
        "ERROR: urlopen: Network error while fetching the manifest",
        "ERROR: [Errno 110] Connection timed out",
        "ERROR: giving up after the socket timeout elapsed",
        "ERROR: the read timeout expired before any data arrived",
        "ERROR: Connection reset by peer",
        "ERROR: [Errno 111] Connection refused",
        "ERROR: ('Connection aborted.', RemoteDisconnected('Remote end closed'))",
        "ERROR: <urlopen error [Errno -3] Temporary failure resolving the host>",
        "ERROR: name resolution failed for the media host",
        "ERROR: unable to connect to the fragment server",
        "ERROR: Unable to download webpage: HTTP Error 429: Too Many Requests",
        "ERROR: Unable to download webpage: HTTP Error 502: Bad Gateway",
        "ERROR: Unable to download webpage: HTTP Error 503: Service Unavailable",
    ];

    #[test]
    fn every_marker_is_pinned_by_exactly_one_sample() {
        assert_eq!(
            NETWORK_ERROR_SAMPLES.len(),
            NETWORK_ERROR_MARKERS.len(),
            "thêm dấu hiệu mới thì phải thêm một mẫu tương ứng"
        );

        for marker in NETWORK_ERROR_MARKERS {
            let hits = NETWORK_ERROR_SAMPLES
                .iter()
                .filter(|sample| sample.to_lowercase().contains(marker))
                .count();
            assert_eq!(hits, 1, "dấu hiệu `{marker}` phải được đúng một mẫu ghim");
        }

        for sample in NETWORK_ERROR_SAMPLES {
            let lower = sample.to_lowercase();
            let hits = NETWORK_ERROR_MARKERS
                .iter()
                .filter(|marker| lower.contains(*marker))
                .count();
            assert_eq!(hits, 1, "mẫu `{sample}` chỉ được chạm đúng một dấu hiệu");
        }
    }

    #[test]
    fn recognizes_every_network_marker() {
        for sample in NETWORK_ERROR_SAMPLES {
            assert!(has_network_marker(sample), "phải nhận ra là lỗi mạng: {sample}");
        }
    }

    #[test]
    fn marker_matching_is_case_insensitive() {
        // Hàm tự hạ chữ thường, nên người gọi truyền stderr thô vẫn đúng.
        assert!(has_network_marker("ERROR: Connection Reset By Peer"));
    }

    #[test]
    fn incidental_prose_is_not_a_network_marker() {
        // "Network Ten" là tên một đài truyền hình, không phải sự cố đường
        // truyền: đây là chặn theo bản quyền, một lỗi vĩnh viễn.
        assert!(
            !has_network_marker(
                "ERROR: Video unavailable. This video contains content from Network Ten, who has blocked it"
            ),
            "tên riêng chứa chữ Network không phải lỗi mạng"
        );
        // Tên tuỳ chọn của yt-dlp có dấu gạch nối nên không khớp cụm
        // "socket timeout".
        assert!(
            !has_network_marker("ERROR: Requested format is not available. Use --socket-timeout 30"),
            "nhắc tên tuỳ chọn không phải lỗi mạng"
        );
    }

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
