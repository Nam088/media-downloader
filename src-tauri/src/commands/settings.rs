use std::sync::Arc;

use tauri::State;

use crate::db::Db;
use crate::downloader::queue::DownloadQueue;
use crate::error::AppError;
use crate::models::AppSettings;

#[tauri::command]
pub fn get_settings(db: State<Arc<Db>>) -> Result<AppSettings, AppError> {
    db.get_settings()
}

#[derive(Debug, Default, serde::Deserialize)]
pub struct UpdateSettingsInput {
    pub theme: Option<String>,
    pub language: Option<String>,
    pub default_output_directory: Option<String>,
    pub show_logs_tab: Option<bool>,
    pub max_concurrent_downloads: Option<u32>,
    pub rate_limit_kbps: Option<u32>,
    pub max_retry_attempts: Option<u32>,
    pub run_in_background: Option<bool>,
    pub spotiflac_service_order: Option<String>,
    pub spotiflac_quality: Option<String>,
    pub spotiflac_extensions_fallback: Option<bool>,
    pub tg_bot_token: Option<String>,
    pub tg_chat_id: Option<String>,
}

/// Bốn provider engine SpotiFLAC hỗ trợ cấu hình thứ tự (FR-004 của
/// specs/006). Khai báo một lần: `sanitize_service_order` lọc theo danh sách
/// này và giao diện Settings render từ cùng bốn giá trị.
pub const SPOTIFLAC_PROVIDERS: [&str; 4] = ["tidal", "qobuz", "deezer", "amazon"];

const SPOTIFLAC_TIERS: [&str; 3] = ["flac16", "flac24", "mp3_320"];

/// Chuẩn hoá chuỗi CSV thứ tự provider: chỉ giữ provider hợp lệ, bỏ trùng
/// nhưng giữ nguyên thứ tự người dùng đưa. Trả `None` khi không còn gì hợp lệ
/// — người gọi giữ nguyên giá trị hiện tại thay vì ghi một danh sách rỗng
/// (một danh sách rỗng đồng nghĩa job nhạc không bao giờ có nguồn để thử).
fn sanitize_service_order(raw: &str) -> Option<String> {
    let mut seen: Vec<&str> = Vec::new();
    for part in raw.split(',') {
        let name = part.trim().to_lowercase();
        if let Some(known) = SPOTIFLAC_PROVIDERS.iter().find(|p| **p == name) {
            if !seen.contains(known) {
                seen.push(known);
            }
        }
    }
    if seen.is_empty() {
        None
    } else {
        Some(seen.join(","))
    }
}

/// Applies a partial update onto the current settings, clamping the values
/// that have a safe range.
///
/// Kept separate from the command itself so the clamping is testable without
/// having to build a Tauri `State`.
fn apply_patch(current: &mut AppSettings, patch: UpdateSettingsInput) {
    if let Some(theme) = patch.theme {
        current.theme = theme;
    }
    if let Some(language) = patch.language {
        current.language = language;
    }
    if let Some(dir) = patch.default_output_directory {
        current.default_output_directory = dir;
    }
    if let Some(show_logs_tab) = patch.show_logs_tab {
        current.show_logs_tab = show_logs_tab;
    }
    if let Some(value) = patch.max_concurrent_downloads {
        // Chặn trên/dưới ở đây chứ không chỉ ở giao diện: lệnh này gọi được
        // trực tiếp, và giá trị 0 sẽ làm bộ điều phối không bao giờ chạy job.
        current.max_concurrent_downloads = value.clamp(1, 8);
    }
    if let Some(value) = patch.rate_limit_kbps {
        current.rate_limit_kbps = value;
    }
    if let Some(value) = patch.max_retry_attempts {
        current.max_retry_attempts = value.min(10);
    }
    if let Some(value) = patch.run_in_background {
        current.run_in_background = value;
    }
    if let Some(raw) = patch.spotiflac_service_order {
        if let Some(sanitized) = sanitize_service_order(&raw) {
            current.spotiflac_service_order = sanitized;
        }
    }
    if let Some(tier) = patch.spotiflac_quality {
        // Cùng triết lý với các giá trị số ở trên: lệnh gọi được trực tiếp,
        // nên một tier lạ phải bị bỏ qua chứ không được ghi xuống DB rồi làm
        // `run_music_job` spawn worker với tham số vô nghĩa.
        if SPOTIFLAC_TIERS.contains(&tier.as_str()) {
            current.spotiflac_quality = tier;
        }
    }
    if let Some(value) = patch.spotiflac_extensions_fallback {
        current.spotiflac_extensions_fallback = value;
    }
    if let Some(token) = patch.tg_bot_token {
        current.tg_bot_token = token.trim().to_string();
    }
    if let Some(chat_id) = patch.tg_chat_id {
        let trimmed = chat_id.trim().to_string();
        // Chat ID cá nhân là một dãy chữ số; rỗng = tắt thông báo Telegram.
        if trimmed.is_empty() || trimmed.chars().all(|c| c.is_ascii_digit()) {
            current.tg_chat_id = trimmed;
        }
    }
}

#[tauri::command]
pub fn update_settings(
    db: State<Arc<Db>>,
    queue: State<DownloadQueue>,
    patch: UpdateSettingsInput,
) -> Result<AppSettings, AppError> {
    let mut current = db.get_settings()?;
    apply_patch(&mut current, patch);
    db.update_settings(&current)?;
    // Áp ngay lên bộ điều phối thay vì chỉ ghi vào DB: nó đọc số luồng từ một
    // `AtomicUsize` chứ không đọc lại cài đặt mỗi vòng, nên nếu không đẩy giá
    // trị mới vào đây thì thay đổi chỉ có hiệu lực ở lần khởi động sau
    // (FR-113). Giá trị đã được `apply_patch` chặn trong khoảng 1..=8.
    queue.set_max_concurrent(current.max_concurrent_downloads as usize);
    Ok(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defaults() -> AppSettings {
        AppSettings {
            theme: "system".to_string(),
            language: "system".to_string(),
            default_output_directory: String::new(),
            show_logs_tab: false,
            max_concurrent_downloads: 3,
            rate_limit_kbps: 0,
            max_retry_attempts: 3,
            run_in_background: false,
            spotiflac_service_order: "tidal,qobuz,deezer,amazon".to_string(),
            spotiflac_quality: "flac16".to_string(),
            spotiflac_extensions_fallback: true,
            tg_bot_token: String::new(),
            tg_chat_id: String::new(),
        }
    }

    #[test]
    fn concurrency_below_one_is_raised_to_one() {
        let mut settings = defaults();
        apply_patch(
            &mut settings,
            UpdateSettingsInput {
                max_concurrent_downloads: Some(0),
                ..Default::default()
            },
        );

        assert_eq!(
            settings.max_concurrent_downloads, 1,
            "0 luồng sẽ làm bộ điều phối không bao giờ chạy job"
        );
    }

    #[test]
    fn concurrency_above_eight_is_capped_at_eight() {
        let mut settings = defaults();
        apply_patch(
            &mut settings,
            UpdateSettingsInput {
                max_concurrent_downloads: Some(99),
                ..Default::default()
            },
        );

        assert_eq!(settings.max_concurrent_downloads, 8);
    }

    #[test]
    fn retry_attempts_are_capped_at_ten() {
        let mut settings = defaults();
        apply_patch(
            &mut settings,
            UpdateSettingsInput {
                max_retry_attempts: Some(500),
                ..Default::default()
            },
        );

        assert_eq!(settings.max_retry_attempts, 10);
    }

    /// Giá trị hợp lệ phải đi qua nguyên vẹn — nếu không, test chặn ở trên có
    /// thể pass chỉ vì hàm luôn trả về một hằng số.
    #[test]
    fn values_inside_the_allowed_range_pass_through_untouched() {
        let mut settings = defaults();
        apply_patch(
            &mut settings,
            UpdateSettingsInput {
                max_concurrent_downloads: Some(5),
                max_retry_attempts: Some(7),
                rate_limit_kbps: Some(1500),
                run_in_background: Some(true),
                ..Default::default()
            },
        );

        assert_eq!(settings.max_concurrent_downloads, 5);
        assert_eq!(settings.max_retry_attempts, 7);
        assert_eq!(settings.rate_limit_kbps, 1500);
        assert!(settings.run_in_background);
    }

    #[test]
    fn a_reordered_provider_list_passes_through_normalized() {
        let mut settings = defaults();
        apply_patch(
            &mut settings,
            UpdateSettingsInput {
                spotiflac_service_order: Some("Qobuz, tidal ,deezer".to_string()),
                ..Default::default()
            },
        );

        assert_eq!(settings.spotiflac_service_order, "qobuz,tidal,deezer");
    }

    #[test]
    fn an_invalid_provider_list_keeps_the_current_order() {
        let mut settings = defaults();
        for garbage in ["", "spotify,napster", ",,,"] {
            apply_patch(
                &mut settings,
                UpdateSettingsInput {
                    spotiflac_service_order: Some(garbage.to_string()),
                    ..Default::default()
                },
            );
            assert_eq!(
                settings.spotiflac_service_order, "tidal,qobuz,deezer,amazon",
                "danh sách rỗng nghĩa là job nhạc không bao giờ có nguồn để thử"
            );
        }
    }

    #[test]
    fn duplicate_providers_are_collapsed_keeping_first_position() {
        let mut settings = defaults();
        apply_patch(
            &mut settings,
            UpdateSettingsInput {
                spotiflac_service_order: Some("tidal,qobuz,tidal,amazon".to_string()),
                ..Default::default()
            },
        );

        assert_eq!(settings.spotiflac_service_order, "tidal,qobuz,amazon");
    }

    #[test]
    fn an_unknown_quality_tier_is_ignored() {
        let mut settings = defaults();
        apply_patch(
            &mut settings,
            UpdateSettingsInput {
                spotiflac_quality: Some("flac999".to_string()),
                ..Default::default()
            },
        );
        assert_eq!(settings.spotiflac_quality, "flac16");

        apply_patch(
            &mut settings,
            UpdateSettingsInput {
                spotiflac_quality: Some("mp3_320".to_string()),
                ..Default::default()
            },
        );
        assert_eq!(settings.spotiflac_quality, "mp3_320");
    }

    #[test]
    fn a_non_numeric_chat_id_is_rejected_but_empty_clears_it() {
        let mut settings = defaults();
        settings.tg_chat_id = "12345".to_string();

        apply_patch(
            &mut settings,
            UpdateSettingsInput {
                tg_chat_id: Some("abc!".to_string()),
                ..Default::default()
            },
        );
        assert_eq!(settings.tg_chat_id, "12345");

        apply_patch(
            &mut settings,
            UpdateSettingsInput {
                tg_chat_id: Some("  ".to_string()),
                ..Default::default()
            },
        );
        assert_eq!(settings.tg_chat_id, "", "rỗng = tắt thông báo Telegram");
    }

    #[test]
    fn the_bot_token_is_stored_trimmed() {
        let mut settings = defaults();
        apply_patch(
            &mut settings,
            UpdateSettingsInput {
                tg_bot_token: Some("  123:abc  ".to_string()),
                ..Default::default()
            },
        );
        assert_eq!(settings.tg_bot_token, "123:abc");
    }
}
