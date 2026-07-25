use std::path::{Path, PathBuf};

use tauri::path::BaseDirectory;
use tauri::{AppHandle, Manager};

use crate::error::AppError;

/// Shared logic behind `ytdlp_binary::resolve_ytdlp_executable` and
/// `gallery_dl_binary::resolve_gallery_dl_executable`: both bundle a
/// PyInstaller "onedir" build (executable + a pre-unpacked Python runtime
/// folder, since the single-file "onefile" alternative re-extracts its whole
/// runtime into a fresh temp dir on *every* launch — see `ytdlp_binary`'s own
/// doc comment for the measured cost) as a Tauri *resource*, then copy it
/// once into the app's data directory the first time it's needed in a given
/// install. `resource_dir_name`/`cache_dir_name` are the same string for both
/// tools today, but kept as separate parameters in case that ever needs to
/// diverge (e.g. a resource bundled under a different name than its cache
/// folder).
pub async fn ensure_cached_onedir(
    app: &AppHandle,
    resource_dir_name: &str,
    cache_dir_name: &str,
    exe_name: &str,
) -> Result<PathBuf, AppError> {
    let app_data_dir = app.path().app_data_dir().map_err(AppError::internal)?;
    let cache_dir = app_data_dir.join(cache_dir_name);
    let exe_path = cache_dir.join(exe_name);
    let current_version = app.package_info().version.to_string();

    let marker_path = cache_dir.join(".bundled-version");
    let up_to_date = exe_path.exists()
        && tokio::fs::read_to_string(&marker_path)
            .await
            .map(|v| v.trim() == current_version)
            .unwrap_or(false);

    if !up_to_date {
        let resource_dir = app
            .path()
            .resolve(resource_dir_name, BaseDirectory::Resource)
            .map_err(AppError::internal)?;

        let cache_dir_for_copy = cache_dir.clone();
        tokio::task::spawn_blocking(move || copy_dir_recursive(&resource_dir, &cache_dir_for_copy))
            .await
            .map_err(AppError::internal)?
            .map_err(AppError::internal)?;

        tokio::fs::write(&marker_path, &current_version)
            .await
            .map_err(AppError::internal)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = tokio::fs::metadata(&exe_path).await.map_err(AppError::internal)?;
            let mut perms = metadata.permissions();
            perms.set_mode(perms.mode() | 0o111); // ensure the exec bits are set
            tokio::fs::set_permissions(&exe_path, perms)
                .await
                .map_err(AppError::internal)?;
        }
    }

    Ok(exe_path)
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let dest_path = dest.join(entry.file_name());
        if file_type.is_dir() {
            copy_dir_recursive(&entry.path(), &dest_path)?;
        } else {
            std::fs::copy(entry.path(), &dest_path)?;
        }
    }
    Ok(())
}
