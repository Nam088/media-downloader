use std::sync::Arc;

use tauri::State;

use crate::db::Db;
use crate::error::AppError;
use crate::models::AppSettings;

#[tauri::command]
pub fn get_settings(db: State<Arc<Db>>) -> Result<AppSettings, AppError> {
    db.get_settings()
}

#[derive(Debug, serde::Deserialize)]
pub struct UpdateSettingsInput {
    pub theme: Option<String>,
    pub language: Option<String>,
    pub default_output_directory: Option<String>,
}

#[tauri::command]
pub fn update_settings(
    db: State<Arc<Db>>,
    patch: UpdateSettingsInput,
) -> Result<AppSettings, AppError> {
    let mut current = db.get_settings()?;
    if let Some(theme) = patch.theme {
        current.theme = theme;
    }
    if let Some(language) = patch.language {
        current.language = language;
    }
    if let Some(dir) = patch.default_output_directory {
        current.default_output_directory = dir;
    }
    db.update_settings(&current)?;
    Ok(current)
}
