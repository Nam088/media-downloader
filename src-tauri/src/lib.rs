mod commands;
mod db;
mod downloader;
mod error;
mod models;
mod platform;

use std::sync::Arc;

use commands::media::{ActivePreviews, PreviewCache};
use db::Db;
use downloader::queue::DownloadQueue;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            let db = Arc::new(Db::open(&app_data_dir.join("media-downloader.db"))?);
            let queue = DownloadQueue::new(Arc::clone(&db), app.handle().clone());

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
            commands::download::pause_job,
            commands::download::resume_job,
            commands::download::cancel_job,
            commands::download::retry_job,
            commands::history::list_queue,
            commands::history::list_history,
            commands::history::open_containing_folder,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
