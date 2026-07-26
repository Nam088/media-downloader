use std::path::PathBuf;

use tauri::AppHandle;
use tokio::sync::OnceCell;

use crate::error::AppError;

use super::bundled_tool::ensure_cached_onedir;

/// spotiflac-worker is this project's own thin protocol wrapper
/// (`scripts/spotiflac_worker.py`) around the SpotiFLAC pip module — the
/// official standalone executables have no structured progress output and no
/// stdin channel for Cloudflare grant injection, so we always self-build,
/// with PyInstaller `--onedir` for the same startup-speed reason as yt-dlp
/// and gallery-dl (see `scripts/build-spotiflac-onedir.sh`), and reuse the
/// exact "bundle as Tauri resource, copy into app-data on first run"
/// mechanism proven out by both.
#[cfg(windows)]
const SPOTIFLAC_EXE_NAME: &str = "spotiflac-worker.exe";
#[cfg(not(windows))]
const SPOTIFLAC_EXE_NAME: &str = "spotiflac-worker";

const BUNDLED_RESOURCE_DIR: &str = "spotiflac-onedir";
const CACHE_DIR_NAME: &str = "spotiflac-onedir";

static SPOTIFLAC_PATH: OnceCell<PathBuf> = OnceCell::const_new();

/// Returns the path to the (already-extracted, ready-to-run) spotiflac-worker
/// executable, copying the bundled resource into the app's data directory the
/// first time this is called in a given app run. Safe to call concurrently —
/// later callers just await the same in-flight copy.
pub async fn resolve_spotiflac_executable(app: &AppHandle) -> Result<PathBuf, AppError> {
    SPOTIFLAC_PATH
        .get_or_try_init(|| {
            ensure_cached_onedir(app, BUNDLED_RESOURCE_DIR, CACHE_DIR_NAME, SPOTIFLAC_EXE_NAME)
        })
        .await
        .cloned()
}
