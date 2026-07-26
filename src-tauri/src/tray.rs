//! Biểu tượng khay hệ thống và hành vi nút đóng cửa sổ (FR-127, FR-129).
//!
//! Quyết định duy nhất đáng kiểm thử ở đây là `close_action`; phần còn lại là
//! vào-ra với khay hệ thống và cửa sổ.

use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};

use crate::downloader::queue::DownloadQueue;

/// Việc cần làm khi người dùng bấm nút đóng cửa sổ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseAction {
    /// Giấu cửa sổ xuống khay, tiến trình sống tiếp và hàng đợi chạy tiếp.
    HideToTray,
    /// Thoát hẳn ứng dụng, đúng như hành vi trước khi có chế độ chạy nền.
    Quit,
}

/// `run_in_background` là giá trị **đọc tại thời điểm đóng**, không phải giá
/// trị cache lúc khởi động: người dùng bật/tắt trong Cài đặt phải có hiệu lực
/// ngay, chứ không phải từ lần chạy sau.
///
/// `None` = không đọc được cài đặt. Khi đó thoát hẳn, vì mặc định của
/// `run_in_background` là tắt: đoán sang "giấu xuống khay" sẽ để lại một tiến
/// trình vô hình mà người dùng chưa bao giờ đồng ý cho chạy nền.
pub fn close_action(run_in_background: Option<bool>) -> CloseAction {
    match run_in_background {
        Some(true) => CloseAction::HideToTray,
        Some(false) | None => CloseAction::Quit,
    }
}

/// Đưa cửa sổ chính trở lại trước mặt người dùng, dù trước đó nó bị giấu hay
/// bị thu nhỏ. Cả ba bước đều cần: `show` không tự bỏ thu nhỏ, và không có
/// `set_focus` thì cửa sổ hiện ra sau các cửa sổ khác.
fn reveal_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Dựng biểu tượng khay kèm menu: mở lại cửa sổ, tạm dừng tất cả, thoát.
pub fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Open Media Downloader", true, None::<&str>)?;
    let pause_all = MenuItem::with_id(app, "pause_all", "Pause all downloads", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &pause_all, &quit])?;

    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("Media Downloader")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => reveal_main_window(app),
            "pause_all" => {
                // `tauri::async_runtime::spawn`, KHÔNG phải `tokio::spawn`:
                // callback của menu khay chạy trên luồng sự kiện của hệ điều
                // hành, nơi không có runtime tokio nào đang trong ngữ cảnh.
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Some(queue) = app.try_state::<DownloadQueue>() {
                        let _ = queue.pause_all().await;
                    }
                });
            }
            "quit" => app.exit(0),
            _ => {}
        });

    // Không có icon thì Tauri vẫn dựng khay nhưng ô biểu tượng trống trơn;
    // dùng luôn icon của cửa sổ để khay trông giống ứng dụng.
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    builder.build(app)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_mode_on_hides_the_window_instead_of_quitting() {
        assert_eq!(close_action(Some(true)), CloseAction::HideToTray);
    }

    #[test]
    fn background_mode_off_quits_the_process() {
        // Mặc định là tắt, và tắt phải nghĩa là tắt: đóng cửa sổ thì ứng dụng
        // biến mất hoàn toàn, không để lại biểu tượng khay nào.
        assert_eq!(close_action(Some(false)), CloseAction::Quit);
    }

    #[test]
    fn unreadable_settings_fall_back_to_quitting() {
        // Không đọc được cài đặt thì phải theo mặc định (tắt). Đoán sang chạy
        // nền sẽ để lại một tiến trình vô hình mà người dùng không hề bật.
        assert_eq!(close_action(None), CloseAction::Quit);
    }
}
