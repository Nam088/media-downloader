use serde::Serialize;

/// Shared error type returned by every Tauri command (contracts/tauri-commands.md).
///
/// `message` is only an English fallback for unmapped codes. Real
/// localization happens on the frontend: `code` is a stable, machine-readable
/// key that `ErrorBanner` maps to an i18next string per `AppSettings.language`
/// (FR-009). Keeping translation in one place (the JSON locale files) avoids
/// duplicating every error string in both Rust and JSON.
#[derive(Debug, Serialize)]
pub struct AppError {
    pub code: String,
    pub message: String,
}

impl AppError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn unsupported_platform(source_url: &str) -> Self {
        Self::new(
            "UNSUPPORTED_PLATFORM",
            format!("Link is not from a supported platform: {source_url}"),
        )
    }

    pub fn access_denied(reason: impl Into<String>) -> Self {
        Self::new("ACCESS_DENIED", reason.into())
    }

    pub fn invalid_quality_option() -> Self {
        Self::new(
            "INVALID_QUALITY_OPTION",
            "Requested quality does not match any option returned by preview_media",
        )
    }

    pub fn not_found(what: &str) -> Self {
        Self::new("NOT_FOUND", format!("{what} not found"))
    }

    pub fn internal(err: impl std::fmt::Display) -> Self {
        Self::new("INTERNAL", err.to_string())
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::error::Error for AppError {}

impl From<rusqlite::Error> for AppError {
    fn from(err: rusqlite::Error) -> Self {
        AppError::internal(err)
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        AppError::internal(err)
    }
}
