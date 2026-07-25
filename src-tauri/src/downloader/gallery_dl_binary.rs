use std::path::PathBuf;

use tauri::AppHandle;
use tokio::sync::OnceCell;

use crate::error::AppError;

use super::bundled_tool::ensure_cached_onedir;

/// gallery-dl publishes official standalone builds for Windows/Linux only
/// (built with PyInstaller `--onefile` — the same "re-extracts its runtime
/// on every single launch" problem yt-dlp's official onefile build has, see
/// `ytdlp_binary`'s doc comment) and no official build at all for macOS.
///
/// `scripts/build-gallery-dl-onedir.sh` builds our own `--onedir` binary for
/// every platform instead (in CI, per `.github/workflows/release.yml`, and
/// on demand for local dev via `scripts/fetch-dev-binaries.sh`), using
/// gallery-dl's own official PyInstaller hook
/// (`scripts/pyinstaller-hooks/hook-gallery_dl.py`) so its ~282 dynamically
/// imported extractor modules are all actually included in the build. This
/// sidesteps both the missing-macOS-build and the onefile-slowness problems
/// at once, and reuses the exact same "bundle as Tauri resource, copy into
/// app-data on first run" mechanism already proven out for yt-dlp.
#[cfg(windows)]
const GALLERY_DL_EXE_NAME: &str = "gallery-dl.exe";
#[cfg(not(windows))]
const GALLERY_DL_EXE_NAME: &str = "gallery-dl";

const BUNDLED_RESOURCE_DIR: &str = "gallery-dl-onedir";
const CACHE_DIR_NAME: &str = "gallery-dl-onedir";

static GALLERY_DL_PATH: OnceCell<PathBuf> = OnceCell::const_new();

/// Returns the path to the (already-extracted, ready-to-run) gallery-dl
/// executable, copying the bundled resource into the app's data directory
/// the first time this is called in a given app run. Safe to call
/// concurrently — later callers just await the same in-flight copy.
pub async fn resolve_gallery_dl_executable(app: &AppHandle) -> Result<PathBuf, AppError> {
    GALLERY_DL_PATH
        .get_or_try_init(|| {
            ensure_cached_onedir(app, BUNDLED_RESOURCE_DIR, CACHE_DIR_NAME, GALLERY_DL_EXE_NAME)
        })
        .await
        .cloned()
}
