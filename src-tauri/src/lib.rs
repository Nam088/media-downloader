mod commands;
mod db;
mod downloader;
mod error;
mod logging;
mod models;
mod notify;
mod platform;
mod tray;

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
        .plugin(tauri_plugin_notification::init())
        // Nút đóng cửa sổ: giấu xuống khay hay thoát hẳn (FR-129).
        //
        // Cài đặt được đọc lại ở ĐÂY, mỗi lần đóng, chứ không cache lúc khởi
        // động — người dùng vừa bật/tắt chạy nền trong Cài đặt là có hiệu lực
        // ngay, không phải khởi động lại ứng dụng.
        //
        // Không đọc được cài đặt (kể cả khi state chưa kịp đăng ký) thì
        // `close_action` chọn thoát, đúng với mặc định tắt của
        // `run_in_background`.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let app = window.app_handle();
                let run_in_background = app
                    .try_state::<Arc<Db>>()
                    .and_then(|db| db.get_settings().ok())
                    .map(|settings| settings.run_in_background);
                if tray::close_action(run_in_background) == tray::CloseAction::HideToTray {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
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

            // Sau `app.manage(queue)`: mục "Pause all downloads" của menu khay
            // lấy hàng đợi ra từ state, nên state phải có trước khi khay tồn
            // tại để người dùng bấm được.
            tray::build_tray(app.handle())?;

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
            commands::queue_control::pause_all_jobs,
            commands::queue_control::resume_all_jobs,
            commands::queue_control::cancel_all_jobs,
            commands::queue_control::reorder_queue,
            commands::presets::list_presets,
            commands::presets::create_preset,
            commands::presets::rename_preset,
            commands::presets::update_preset,
            commands::presets::delete_preset,
            commands::presets::set_default_preset,
            commands::history::list_queue,
            commands::history::list_history,
            commands::history::count_history,
            commands::history::clear_history,
            commands::history::open_containing_folder,
            commands::library::list_library,
            commands::library::library_stats,
            commands::library::library_items_for_job,
            commands::library::reconcile_library,
            commands::library::remove_library_items,
            commands::library::relink_library_item,
            commands::library::redownload_library_item,
            commands::library::rename_library_item,
            commands::library::move_library_items,
            commands::library::delete_library_items,
            commands::library::reveal_library_item,
            commands::library::export_library_playlist,
            commands::url_list::read_url_list_file,
            logging::get_logs,
            logging::clear_logs,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
