use std::sync::Arc;

use tauri::{AppHandle, State};
#[cfg(not(windows))]
use tauri_plugin_opener::OpenerExt;

use crate::db::Db;
use crate::error::AppError;
use crate::logging::log_event;
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

    reveal_item_in_dir(&app, &file_path)
}

#[cfg(windows)]
pub fn reveal_item_in_dir(app: &AppHandle, path: &str) -> Result<(), AppError> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    let path_obj = std::path::Path::new(path);
    if !path_obj.exists() {
        // The frontend only ever shows a localized one-liner ("This item
        // could not be found") for `NOT_FOUND` — it never had the actual
        // path to show. Logging it here means the reason (and which exact
        // path was checked) shows up in the same in-app log panel every
        // other failure in this app already reports through, instead of
        // only being visible by re-deriving it from scratch.
        log_event(app, "WARN", format!("Reveal failed: file does not exist on disk: {path}"));
        return Err(AppError::not_found("File"));
    }

    // `explorer /select,<path>` needs quotes ONLY around `<path>` (Microsoft's
    // documented syntax). `.arg(format!("/select,{path}"))` instead hands the
    // WHOLE string to Rust's own Windows argument quoting, which wraps the
    // entire thing (comma included) in one pair of quotes the moment `<path>`
    // contains a space — true for virtually every real download. Explorer
    // then can't find its own `/select,` token inside that single quoted
    // blob and silently opens a plain window at the default folder instead
    // of revealing the file — reproduced live: it opened Documents instead
    // of the actual download folder. `raw_arg` bypasses Rust's quoting
    // entirely so the command line matches Microsoft's syntax exactly.
    Command::new("explorer")
        .raw_arg(format!("/select,\"{path}\""))
        .spawn()
        .map_err(|e| {
            log_event(app, "ERROR", format!("Reveal failed: could not launch explorer for {path}: {e}"));
            AppError::internal(format!("Failed to launch explorer: {e}"))
        })?;
    Ok(())
}

#[cfg(not(windows))]
pub fn reveal_item_in_dir(app: &AppHandle, path: &str) -> Result<(), AppError> {
    app.opener()
        .reveal_item_in_dir(path)
        .map_err(AppError::internal)
}

/// Mở file bằng ứng dụng mặc định của hệ thống.
#[tauri::command]
pub fn open_file(path: String) -> Result<(), AppError> {
    use std::process::Command;

    let path = std::path::Path::new(&path);
    if !path.exists() {
        return Err(AppError::not_found("File"));
    }

    #[cfg(windows)]
    let cmd = "explorer";
    #[cfg(target_os = "macos")]
    let cmd = "open";
    #[cfg(target_os = "linux")]
    let cmd = "xdg-open";

    Command::new(cmd)
        .arg(path)
        .spawn()
        .map_err(|e| AppError::internal(format!("Failed to open file: {e}")))?;
    Ok(())
}
