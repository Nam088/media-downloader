//! Lỗi nào đáng thử lại, và chờ bao lâu trước khi thử.
//!
//! Module gom hai mảnh của cùng một quyết định: nhận ra stderr nào là sự cố
//! đường truyền (`has_network_marker`, dùng bởi hai bộ phân loại lỗi), và một
//! lần chạy thất bại thì phải làm gì (`decide_outcome`, dùng bởi bộ điều phối
//! hàng đợi).
//!
//! Tách riêng khỏi `queue` để logic quyết định này kiểm thử được mà không cần
//! dựng cả một hàng đợi, và để cả yt-dlp lẫn gallery-dl dùng chung một chính
//! sách (FR-120, FR-121).

use crate::error::{CANCELED_ERROR_CODE, NETWORK_ERROR_CODE};

/// Khoảng chờ tối đa giữa hai lần thử. Vượt quá mức này thì việc chờ tiếp
/// không còn giúp gì mà chỉ làm người dùng tưởng ứng dụng bị treo.
const MAX_BACKOFF_SECONDS: u64 = 300;

/// Khoảng chờ cho lần thử đầu tiên. Đủ dài để một lần chập mạng kịp hồi phục,
/// đủ ngắn để không gây khó chịu.
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
const NETWORK_ERROR_MARKERS: [&str; 17] = [
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
    // Not a network error in the literal sense, but the same "retry and it
    // often just works" shape: TikTok's own webpage/API structure shifts
    // under yt-dlp intermittently, and yt-dlp has no fix for it as of the
    // latest release (2026.07.04, confirmed still the current stable release
    // as of 2026-07-29 — this is an open, unresolved upstream issue, not
    // something an outdated bundled binary would fix). Reported across many
    // yt-dlp GitHub issues (#10919, #12574, #14508, #15418, #15506, #15566,
    // #15629) as intermittent — the same request commonly succeeds on a
    // later attempt — and gallery-dl's own fix for the equivalent failure
    // (mikf/gallery-dl#7191) is literally a retry loop, not a parsing fix.
    "unable to extract universal data for rehydration",
    "unable to extract webpage video data",
];

/// Lỗi có đáng thử lại không, dựa trên mã lỗi ổn định của `AppError`.
///
/// Chỉ mã `NETWORK_ERROR` được coi là tạm thời. Mọi mã khác — kể cả
/// `DOWNLOAD_FAILED` vốn là nhóm gom — đều bị coi là vĩnh viễn, vì thử lại một
/// lỗi vĩnh viễn chỉ làm chậm phản hồi mà không đổi được kết quả.
fn is_transient(error_code: &str) -> bool {
    error_code == NETWORK_ERROR_CODE
}

/// Số giây chờ trước lần thử thứ `retry_count + 1`. Tăng gấp đôi mỗi lần,
/// chặn trên ở `MAX_BACKOFF_SECONDS`.
fn backoff_seconds(retry_count: i64) -> u64 {
    let exponent = retry_count.clamp(0, 16) as u32;
    BASE_BACKOFF_SECONDS
        .saturating_mul(2u64.saturating_pow(exponent))
        .min(MAX_BACKOFF_SECONDS)
}

/// Có nên tự thử lại không: lỗi phải là tạm thời và chưa dùng hết số lượt.
fn should_retry(error_code: &str, retry_count: i64, max_retries: i64) -> bool {
    is_transient(error_code) && retry_count < max_retries
}

/// Việc phải làm sau một lần chạy thất bại.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Xếp lại vào hàng chờ, chỉ được phép chạy lại sau `delay_seconds` giây.
    Retry { delay_seconds: u64 },
    /// Thất bại vĩnh viễn: đánh dấu `failed` và dừng hẳn.
    Fail,
    /// Không đụng gì tới job cả.
    Ignore,
}

/// Quyết định duy nhất của chính sách thử lại, tách khỏi phần vào/ra để kiểm
/// thử được: `queue::finish_job` chỉ còn là lớp bọc mỏng đọc `retry_count` từ
/// DB, gọi hàm này, rồi thi hành kết quả.
///
/// `Ignore` dành riêng cho mã `CANCELED`: người dùng chủ động tạm dừng hoặc
/// huỷ thì `pause`/`cancel` đã đặt trạng thái cuối cùng rồi. Ghi đè nó thành
/// `failed` sẽ xoá mất lựa chọn của người dùng, còn thử lại thì lại càng
/// ngược ý họ hơn (FR-123).
pub fn decide_outcome(error_code: &str, retry_count: i64, max_retries: i64) -> Outcome {
    if error_code == CANCELED_ERROR_CODE {
        return Outcome::Ignore;
    }
    if should_retry(error_code, retry_count, max_retries) {
        return Outcome::Retry {
            delay_seconds: backoff_seconds(retry_count),
        };
    }
    Outcome::Fail
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
    const NETWORK_ERROR_SAMPLES: [&str; 17] = [
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
        "ERROR: [TikTok] 7667565735589334290: Unable to extract universal data for rehydration; please report this issue",
        "ERROR: [TikTok] 7524203120394554629: Unable to extract webpage video data",
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

    #[test]
    fn a_transient_failure_is_scheduled_for_another_attempt() {
        assert_eq!(
            decide_outcome(NETWORK_ERROR_CODE, 0, 3),
            Outcome::Retry { delay_seconds: 5 }
        );
        // Khoảng chờ phải lớn dần theo số lần đã thử, chứ không phải một hằng
        // số: hai lần thử liên tiếp cách nhau đúng 5 giây thì lần thử thứ ba
        // gần như chắc chắn gặp lại đúng sự cố mạng đó.
        assert_eq!(
            decide_outcome(NETWORK_ERROR_CODE, 2, 5),
            Outcome::Retry { delay_seconds: 20 }
        );
    }

    #[test]
    fn a_transient_failure_becomes_permanent_once_the_attempts_run_out() {
        assert_eq!(decide_outcome(NETWORK_ERROR_CODE, 3, 3), Outcome::Fail);
        assert_eq!(
            decide_outcome(NETWORK_ERROR_CODE, 0, 0),
            Outcome::Fail,
            "người dùng tắt hẳn tự thử lại"
        );
    }

    #[test]
    fn a_permanent_failure_never_waits_for_a_retry() {
        // Bắt người dùng chờ 5 + 10 + 20 giây rồi mới báo "video riêng tư" là
        // vô ích: kết quả không thể khác đi (SC-106).
        assert_eq!(decide_outcome("ACCESS_DENIED", 0, 3), Outcome::Fail);
        assert_eq!(decide_outcome("DOWNLOAD_FAILED", 0, 3), Outcome::Fail);
    }

    #[test]
    fn a_user_cancellation_is_neither_retried_nor_marked_failed() {
        // Người dùng bấm Tạm dừng/Huỷ: trạng thái cuối cùng đã do chính thao
        // tác đó đặt. Thử lại sẽ khởi động lại thứ họ vừa dừng, còn đánh dấu
        // thất bại sẽ hiện một lỗi mà không hề có lỗi nào (FR-123).
        assert_eq!(decide_outcome(CANCELED_ERROR_CODE, 0, 3), Outcome::Ignore);
        assert_eq!(
            decide_outcome(CANCELED_ERROR_CODE, 9, 3),
            Outcome::Ignore,
            "đã hết lượt thử lại cũng không được biến thành thất bại"
        );
    }
}
