//! Phần kế toán slot của bộ điều phối hàng đợi tải.
//!
//! Thay cho cơ chế cũ (`enqueue` spawn task ngay rồi task đó chờ `Semaphore`),
//! mô hình mới tách hẳn "xếp hàng" khỏi "chạy": `enqueue` chỉ ghi DB, còn một
//! task dispatcher duy nhất (`queue::spawn_dispatcher`) quyết định khi nào job
//! nào được chạy.
//!
//! Ba thứ mà cơ chế cũ không làm được và mô hình này làm được:
//! - **Sắp xếp lại thứ tự**: thứ tự nằm ở cột `queue_position` trong DB, không
//!   phải ở thứ tự các task đã spawn xếp hàng trước semaphore.
//! - **Đổi số luồng lúc đang chạy**: `max_concurrent` là `AtomicUsize` được
//!   đọc lại mỗi vòng, thay vì số permit cố định lúc khởi tạo semaphore.
//! - **Chờ thử lại có thể huỷ**: job chờ retry là một dòng DB ở trạng thái
//!   `queued` với `next_retry_at` ở tương lai, không phải một task đang ngủ.
//!
//! Chỉ phần thuần tính toán nằm ở đây; vòng lặp thật sự nằm trong `queue` vì
//! nó cần `Db`, `AppHandle` và bảng các job đang chạy.

use std::sync::atomic::{AtomicUsize, Ordering};

/// Nhịp tick của dispatcher. Cần một nhịp cố định (chứ không chỉ dựa vào tín
/// hiệu đánh thức) vì job chờ thử lại đến hạn theo đồng hồ, không có ai đánh
/// thức hộ.
pub const TICK_INTERVAL_MS: u64 = 1000;

/// Số slot còn trống để khởi chạy job mới.
///
/// Trả 0 khi số đang chạy đã bằng hoặc vượt giới hạn — trường hợp "vượt" xảy
/// ra hợp lệ khi người dùng giảm số luồng lúc đang chạy: các job đang chạy
/// được chạy nốt (FR-113), chỉ không có job mới nào được khởi chạy thêm.
pub fn available_slots(running_count: usize, max_concurrent: &AtomicUsize) -> usize {
    let max = max_concurrent.load(Ordering::Relaxed);
    max.saturating_sub(running_count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_free_slots_when_below_the_limit() {
        let max = AtomicUsize::new(3);
        assert_eq!(available_slots(0, &max), 3);
        assert_eq!(available_slots(2, &max), 1);
    }

    #[test]
    fn reports_no_slots_when_at_the_limit() {
        let max = AtomicUsize::new(3);
        assert_eq!(available_slots(3, &max), 0);
    }

    #[test]
    fn reports_no_slots_when_the_limit_was_lowered_mid_flight() {
        // Người dùng hạ từ 5 xuống 2 trong khi 4 job đang chạy: không được trả
        // về số âm (usize sẽ tràn) và cũng không được khởi chạy thêm gì
        // (FR-113).
        let max = AtomicUsize::new(2);
        assert_eq!(available_slots(4, &max), 0);
    }

    #[test]
    fn picks_up_a_raised_limit_without_being_recreated() {
        // Đây là điều `Semaphore::new(3)` không làm được: số permit cố định từ
        // lúc khởi tạo, muốn đổi phải dựng lại cả hàng đợi.
        let max = AtomicUsize::new(1);
        assert_eq!(available_slots(1, &max), 0);
        max.store(4, Ordering::Relaxed);
        assert_eq!(available_slots(1, &max), 3, "đổi số luồng có hiệu lực ngay");
    }
}
