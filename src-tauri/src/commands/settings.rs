use std::sync::Arc;

use tauri::State;

use crate::db::Db;
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
}

#[tauri::command]
pub fn update_settings(
    db: State<Arc<Db>>,
    patch: UpdateSettingsInput,
) -> Result<AppSettings, AppError> {
    let mut current = db.get_settings()?;
    apply_patch(&mut current, patch);
    db.update_settings(&current)?;
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
}
