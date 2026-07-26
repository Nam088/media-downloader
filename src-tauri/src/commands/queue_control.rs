//! Các lệnh tác động lên cả hàng đợi thay vì một job đơn lẻ (FR-117, FR-118).
//!
//! Toàn bộ quyết định nằm trong `DownloadQueue`; ở đây chỉ là lớp vỏ mỏng để
//! Tauri gọi được từ giao diện.

use tauri::State;

use crate::downloader::queue::DownloadQueue;
use crate::error::AppError;

/// Trả về id của những job vừa bị tạm dừng, để giao diện cập nhật đúng chừng
/// đó dòng thay vì nạp lại cả hàng đợi.
#[tauri::command]
pub async fn pause_all_jobs(queue: State<'_, DownloadQueue>) -> Result<Vec<String>, AppError> {
    queue.pause_all().await
}

#[tauri::command]
pub async fn resume_all_jobs(queue: State<'_, DownloadQueue>) -> Result<Vec<String>, AppError> {
    queue.resume_all().await
}

#[tauri::command]
pub async fn cancel_all_jobs(queue: State<'_, DownloadQueue>) -> Result<Vec<String>, AppError> {
    queue.cancel_all().await
}

/// Đặt một tác vụ vào giữa hai hàng xóm của nó sau khi người dùng thả chuột.
///
/// Giao diện gửi id hai hàng xóm chứ không gửi cả danh sách đã sắp xếp: chỉ có
/// đúng một dòng bị ghi, nên một tác vụ được thêm vào trong lúc người dùng đang
/// kéo không bị ghi đè vị trí bởi một ảnh chụp danh sách đã cũ. `None` ở một
/// phía nghĩa là không có hàng xóm ở phía đó, tức thả vào đầu hoặc cuối danh
/// sách.
#[tauri::command]
pub fn reorder_queue(
    queue: State<'_, DownloadQueue>,
    job_id: String,
    before_job_id: Option<String>,
    after_job_id: Option<String>,
) -> Result<(), AppError> {
    queue.move_job(&job_id, before_job_id.as_deref(), after_job_id.as_deref())
}
