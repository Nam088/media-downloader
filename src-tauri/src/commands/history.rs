use std::sync::Arc;

use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::db::Db;
use crate::error::AppError;
use crate::models::{DownloadJob, JobStatus};

const ACTIVE_STATUSES: [JobStatus; 4] = [
    JobStatus::Queued,
    JobStatus::FetchingMetadata,
    JobStatus::Downloading,
    JobStatus::Paused,
];

const HISTORY_STATUSES: [JobStatus; 3] = [
    JobStatus::Completed,
    JobStatus::Failed,
    JobStatus::Canceled,
];

#[tauri::command]
pub fn list_queue(db: State<Arc<Db>>) -> Result<Vec<DownloadJob>, AppError> {
    db.list_jobs_by_statuses(&ACTIVE_STATUSES)
}

#[tauri::command]
pub fn list_history(db: State<Arc<Db>>) -> Result<Vec<DownloadJob>, AppError> {
    let mut jobs = db.list_jobs_by_statuses(&HISTORY_STATUSES)?;
    // `list_jobs_by_statuses` orders by `created_at ASC` (queue order); the
    // history view wants most-recent-first (FR-007, data-model.md §4).
    jobs.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(jobs)
}

#[tauri::command]
pub fn open_containing_folder(
    app: AppHandle,
    db: State<Arc<Db>>,
    job_id: String,
) -> Result<(), AppError> {
    let job = db.get_job(&job_id)?.ok_or_else(|| AppError::not_found("Job"))?;
    let file_path = job
        .output_file_path
        .ok_or_else(|| AppError::new("NOT_FOUND", "This job has no downloaded file yet"))?;

    app.opener()
        .reveal_item_in_dir(&file_path)
        .map_err(AppError::internal)
}
