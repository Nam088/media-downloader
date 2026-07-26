//! Thông báo hệ thống khi một tác vụ kết thúc (FR-128).
//!
//! Hai quyết định ở đây được tách thành hàm thuần (`is_in_front`,
//! `notification_for`) để kiểm thử được mà không cần `AppHandle`: phần còn lại
//! chỉ là vào-ra với cửa sổ và với plugin thông báo.
//!
//! Văn bản thông báo để tiếng Anh: tầng Rust không có ngữ cảnh ngôn ngữ của
//! giao diện. Muốn dịch thì phải đọc `language` từ cài đặt và mang cả bảng dịch
//! sang Rust — việc đó nằm ngoài phạm vi task này.

use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::models::JobStatus;

/// Trạng thái cửa sổ chính, ba câu hỏi độc lập. `None` = **không truy vấn
/// được** (cửa sổ đã đóng, backend trả lỗi), chứ không phải "false".
#[derive(Debug, Clone, Copy, Default)]
pub struct WindowState {
    pub visible: Option<bool>,
    pub minimized: Option<bool>,
    pub focused: Option<bool>,
}

/// Cửa sổ có đang thực sự trước mặt người dùng không.
///
/// Chỉ đúng khi cả ba điều đều được xác nhận: đang hiện, không thu nhỏ, và
/// đang có focus. Ẩn xuống khay, thu nhỏ, hay nằm sau cửa sổ khác đều tính là
/// không trước mặt.
///
/// Bất kỳ câu hỏi nào không trả lời được (`None`) đều khiến hàm trả `false`,
/// tức là **vẫn gửi** thông báo. Chọn như vậy có chủ đích: gửi thừa một thông
/// báo trong lúc người dùng đang nhìn màn hình là phiền một chút, còn im lặng
/// khi người dùng đang chờ là mất hẳn thông tin họ cần.
pub fn is_in_front(state: WindowState) -> bool {
    matches!(
        (state.visible, state.minimized, state.focused),
        (Some(true), Some(false), Some(true))
    )
}

/// Thông báo tương ứng với trạng thái **cuối cùng** mà một lần chạy để lại.
///
/// Đây là cái chốt duy nhất quyết định lần chạy nào đáng báo. Chỉ hai trạng
/// thái kết thúc thật sự mới sinh thông báo; mọi trạng thái khác trả `None`.
/// Quan trọng nhất là `Queued`: một job thất bại tạm thời được `finish_job`
/// đưa **về lại hàng chờ** để thử lại, và người dùng không được nhận thông báo
/// "tải thất bại" cho một tác vụ mà hệ thống vẫn đang tự lo. `Paused` và
/// `Canceled` cũng không báo — chính người dùng vừa bấm nút đó.
pub fn notification_for(
    status: &JobStatus,
    label: &str,
    error: Option<&str>,
) -> Option<(&'static str, String)> {
    match status {
        JobStatus::Completed => Some(("Download complete", label.to_string())),
        JobStatus::Failed => Some((
            "Download failed",
            error.unwrap_or(label).to_string(),
        )),
        JobStatus::Queued
        | JobStatus::FetchingMetadata
        | JobStatus::Downloading
        | JobStatus::Paused
        | JobStatus::Canceled => None,
    }
}

/// Đọc trạng thái cửa sổ chính. Không có cửa sổ nào tên `main` thì mọi câu hỏi
/// đều là `None` — và theo `is_in_front`, thông báo vẫn được gửi.
fn main_window_state(app: &AppHandle) -> WindowState {
    let Some(window) = app.get_webview_window("main") else {
        return WindowState::default();
    };
    WindowState {
        visible: window.is_visible().ok(),
        minimized: window.is_minimized().ok(),
        focused: window.is_focused().ok(),
    }
}

/// Gửi thông báo hệ thống cho một lần chạy vừa kết thúc, nếu trạng thái đó
/// đáng báo và cửa sổ không nằm trước mặt người dùng.
///
/// Mọi nhánh kết thúc của hàng đợi đều đi qua đây, kể cả nhánh xếp lại hàng để
/// thử lại: quyết định "cái này có đáng báo không" nằm gọn trong
/// `notification_for` chứ không nằm rải rác ở các lời gọi.
pub fn notify_job_finished(app: &AppHandle, status: &JobStatus, label: &str, error: Option<&str>) {
    let Some((title, body)) = notification_for(status, label, error) else {
        return;
    };
    if is_in_front(main_window_state(app)) {
        return;
    }
    let _ = app.notification().builder().title(title).body(body).show();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_that_is_visible_unminimised_and_focused_is_in_front() {
        assert!(is_in_front(WindowState {
            visible: Some(true),
            minimized: Some(false),
            focused: Some(true),
        }));
    }

    #[test]
    fn hidden_minimised_or_unfocused_windows_are_not_in_front() {
        // Ẩn xuống khay: người dùng không thấy hàng đợi.
        assert!(!is_in_front(WindowState {
            visible: Some(false),
            minimized: Some(false),
            focused: Some(true),
        }));
        // Thu nhỏ: cửa sổ vẫn "visible" theo backend nhưng nằm dưới taskbar.
        assert!(!is_in_front(WindowState {
            visible: Some(true),
            minimized: Some(true),
            focused: Some(true),
        }));
        // Mở nhưng đang làm việc khác — đúng lúc cần được báo nhất.
        assert!(!is_in_front(WindowState {
            visible: Some(true),
            minimized: Some(false),
            focused: Some(false),
        }));
    }

    #[test]
    fn an_unqueryable_window_counts_as_not_in_front() {
        // Không có cửa sổ nào, hoặc backend trả lỗi cho từng câu hỏi. Thà gửi
        // thừa còn hơn nuốt mất thông báo người dùng đang đợi.
        assert!(!is_in_front(WindowState::default()));
        assert!(!is_in_front(WindowState {
            visible: None,
            minimized: Some(false),
            focused: Some(true),
        }));
        assert!(!is_in_front(WindowState {
            visible: Some(true),
            minimized: None,
            focused: Some(true),
        }));
        assert!(!is_in_front(WindowState {
            visible: Some(true),
            minimized: Some(false),
            focused: None,
        }));
    }

    #[test]
    fn completion_and_permanent_failure_both_get_a_notification() {
        assert_eq!(
            notification_for(&JobStatus::Completed, "Holiday clip", None),
            Some(("Download complete", "Holiday clip".to_string()))
        );
        assert_eq!(
            notification_for(&JobStatus::Failed, "Holiday clip", Some("HTTP 403")),
            Some(("Download failed", "HTTP 403".to_string()))
        );
    }

    #[test]
    fn a_failure_with_no_message_still_names_the_job() {
        assert_eq!(
            notification_for(&JobStatus::Failed, "Holiday clip", None),
            Some(("Download failed", "Holiday clip".to_string()))
        );
    }

    #[test]
    fn a_job_going_back_to_the_queue_for_a_retry_does_not_notify() {
        // `finish_job` gọi thẳng hàm này ở nhánh thử lại. Thất bại tạm thời +
        // còn ngân sách thử lại = job quay về `queued`; báo "Download failed"
        // ở đó là nói dối người dùng về một tác vụ vẫn đang chạy.
        assert_eq!(
            notification_for(&JobStatus::Queued, "Holiday clip", Some("Network error")),
            None
        );
    }

    #[test]
    fn user_driven_stops_do_not_notify() {
        // Người dùng vừa tự bấm nút; báo lại cho họ điều họ vừa làm là nhiễu.
        assert_eq!(notification_for(&JobStatus::Paused, "Holiday clip", None), None);
        assert_eq!(notification_for(&JobStatus::Canceled, "Holiday clip", None), None);
        assert_eq!(
            notification_for(&JobStatus::Downloading, "Holiday clip", None),
            None
        );
    }
}
