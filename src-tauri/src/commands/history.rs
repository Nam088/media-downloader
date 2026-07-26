use std::sync::Arc;

use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::db::Db;
use crate::error::AppError;
use crate::models::{DownloadJob, HistoryQuery, JobStatus};

const ACTIVE_STATUSES: [JobStatus; 4] = [
    JobStatus::Queued,
    JobStatus::FetchingMetadata,
    JobStatus::Downloading,
    JobStatus::Paused,
];

const HISTORY_STATUSES: [JobStatus; 3] =
    [JobStatus::Completed, JobStatus::Failed, JobStatus::Canceled];

#[tauri::command]
pub fn list_queue(db: State<Arc<Db>>) -> Result<Vec<DownloadJob>, AppError> {
    db.list_jobs_by_statuses(&ACTIVE_STATUSES)
}

/// Một trang Lịch sử: mới nhất trước (FR-007, data-model.md §4), lọc theo
/// tab trạng thái + từ khoá ngay ở backend nên `count_history` cùng bộ lọc
/// luôn khớp với đúng tập đang hiển thị — số trang không lệch khỏi thực tế
/// như khi lọc phía giao diện trên một trang dữ liệu thô.
#[tauri::command]
pub fn list_history(db: State<Arc<Db>>, query: HistoryQuery) -> Result<Vec<DownloadJob>, AppError> {
    db.list_history_page(&query)
}

/// Tổng số dòng khớp CÙNG bộ lọc của `list_history` — dùng để tính số trang.
#[tauri::command]
pub fn count_history(db: State<Arc<Db>>, query: HistoryQuery) -> Result<i64, AppError> {
    db.count_history(&query)
}

/// Xoá toàn bộ Lịch sử (đã xong/thất bại/đã huỷ). Chỉ xoá bản ghi tác vụ —
/// không đụng tới file đã tải hay chỉ mục Library, vốn là một khái niệm khác
/// với vòng đời quản lý riêng (xoá/di chuyển/xoá khỏi chỉ mục ở trang Library).
#[tauri::command]
pub fn clear_history(db: State<Arc<Db>>) -> Result<usize, AppError> {
    db.delete_jobs_by_statuses(&HISTORY_STATUSES)
}

#[tauri::command]
pub fn open_containing_folder(
    app: AppHandle,
    db: State<Arc<Db>>,
    job_id: String,
) -> Result<(), AppError> {
    let job = db
        .get_job(&job_id)?
        .ok_or_else(|| AppError::not_found("Job"))?;
    let file_path = job
        .output_file_path
        .ok_or_else(|| AppError::new("NOT_FOUND", "This job has no downloaded file yet"))?;

    app.opener()
        .reveal_item_in_dir(&file_path)
        .map_err(AppError::internal)
}
