mod commands;
mod db;
mod downloader;
mod error;
mod logging;
mod models;
mod platform;

use std::sync::Arc;

use commands::media::{ActivePreviews, PreviewCache};
use db::Db;
use downloader::queue::DownloadQueue;
use logging::LogBuffer;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            // Đăng ký sớm nhất có thể: mọi `log_event` từ đây trở đi (kể cả
            // của bước dọn dẹp bên dưới) mới vào được bộ đệm mà trang Logs đọc.
            app.manage(LogBuffer::default());

            let db = Arc::new(Db::open(&app_data_dir.join("media-downloader.db"))?);

            // Phải chạy TRƯỚC khi dựng hàng đợi: job còn ghi `downloading`/
            // `fetching_metadata` là tàn dư của một phiên bị đóng đột ngột,
            // tiến trình tải của chúng đã chết cùng ứng dụng. Nếu để nguyên,
            // chúng sẽ chiếm chỗ trong giao diện mãi mãi mà không ai chạy —
            // dispatcher chỉ chọn job `queued` (FR-115).
            let interrupted = db.reset_interrupted_jobs()?;
            if interrupted > 0 {
                logging::log_event(
                    app.handle(),
                    "INFO",
                    format!("Paused {interrupted} job(s) left running by a previous session"),
                );
            }

            let settings = db.get_settings()?;
            let queue = DownloadQueue::new(
                Arc::clone(&db),
                app.handle().clone(),
                settings.max_concurrent_downloads as usize,
            );

            app.manage(db);
            app.manage(queue);
            app.manage(PreviewCache::default());
            app.manage(ActivePreviews::default());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::settings::get_settings,
            commands::settings::update_settings,
            commands::media::preview_media,
            commands::media::cancel_preview_media,
            commands::download::create_download_job,
            commands::download::create_playlist_download_jobs,
            commands::download::pause_job,
            commands::download::resume_job,
            commands::download::cancel_job,
            commands::download::retry_job,
            commands::history::list_queue,
            commands::history::list_history,
            commands::history::open_containing_folder,
            logging::get_logs,
            logging::clear_logs,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
