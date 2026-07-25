use std::path::PathBuf;

use tauri::AppHandle;
use tokio::sync::OnceCell;

use crate::error::AppError;

use super::bundled_tool::ensure_cached_onedir;

/// Official yt-dlp release ships two kinds of standalone builds: a single
/// self-extracting "onefile" executable (what `research.md` §2 originally
/// specified) and a "onedir" build (an executable next to an `_internal/`
/// folder with the Python runtime already unpacked on disk).
///
/// Onefile re-extracts its bundled runtime into a fresh temp directory on
/// *every* launch — measured at ~14s just for `--version` on this machine,
/// vs ~0.3s for the onedir build once it's sitting on a real filesystem.
/// Since `preview_media`/downloads spawn yt-dlp on every single call, that
/// difference is the entire "why is preview so slow" complaint. Bundling the
/// onedir folder as a Tauri *resource* and copying it once into the app's
/// data directory (rather than trying to make it a `externalBin` sidecar,
/// which only supports a single file) avoids paying that tax more than once
/// per install.
#[cfg(target_os = "macos")]
const YTDLP_EXE_NAME: &str = "yt-dlp_macos";
#[cfg(target_os = "windows")]
const YTDLP_EXE_NAME: &str = "yt-dlp.exe";
#[cfg(all(unix, not(target_os = "macos")))]
const YTDLP_EXE_NAME: &str = "yt-dlp_linux";

const BUNDLED_RESOURCE_DIR: &str = "yt-dlp-onedir";
const CACHE_DIR_NAME: &str = "yt-dlp-onedir";

static YTDLP_PATH: OnceCell<PathBuf> = OnceCell::const_new();

/// Returns the path to the (already-extracted, ready-to-run) yt-dlp
/// executable, copying the bundled resource into the app's data directory
/// the first time this is called in a given app run. Safe to call
/// concurrently — later callers just await the same in-flight copy.
pub async fn resolve_ytdlp_executable(app: &AppHandle) -> Result<PathBuf, AppError> {
    YTDLP_PATH
        .get_or_try_init(|| ensure_cached_onedir(app, BUNDLED_RESOURCE_DIR, CACHE_DIR_NAME, YTDLP_EXE_NAME))
        .await
        .cloned()
}

/// ffmpeg stays a plain single-file `externalBin` sidecar (it's a compiled C
/// binary, not a frozen Python app, so it has none of the onefile
/// re-extraction cost above). We never spawn it ourselves though — yt-dlp
/// invokes it internally as its own postprocessor via `--ffmpeg-location`.
/// Tauri's own sidecar resolution (`tauri_plugin_shell`) places `externalBin`
/// files in the same directory as the running app's own executable in both
/// dev and production (its build script copies them there); this replicates
/// that same lookup without depending on the shell plugin just for a path.
pub fn resolve_ffmpeg_path() -> Result<PathBuf, AppError> {
    let exe_path = std::env::current_exe().map_err(AppError::internal)?;
    let exe_dir = exe_path
        .parent()
        .ok_or_else(|| AppError::internal("current executable has no parent directory"))?;
    let name = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
    Ok(exe_dir.join(name))
}
