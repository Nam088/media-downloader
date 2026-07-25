use std::process::Stdio;
use std::sync::Arc;

use tauri::AppHandle;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

use crate::error::AppError;

use super::gallery_dl_binary::resolve_gallery_dl_executable;

/// Shared handle to a spawned gallery-dl child process — mirrors
/// `downloader::ytdlp::YtDlpChild` (a plain `tokio::process::Child` can't be
/// shared between the command that spawned it and a later cancel command).
pub type GalleryDlChild = Arc<Mutex<Child>>;

/// Same file-extension classification the reference implementation
/// (`ytb-download-ui`'s `post_process_gallery`) used to tell a slideshow
/// post's background-music file apart from its images — used both to label
/// preview items (`commands::media::build_gallery_media_source`) and to sort
/// downloaded files by kind before post-processing
/// (`downloader::queue::run_gallery_job`).
const AUDIO_EXTENSIONS: &[&str] = &["mp3", "m4a", "wav", "aac", "ogg", "opus", "flac"];

pub fn is_audio_extension(extension: &str) -> bool {
    AUDIO_EXTENSIONS.contains(&extension.to_lowercase().as_str())
}

/// Same classification as `is_audio_extension`, applied to a file's own path
/// (via its extension) rather than metadata gallery-dl reported ahead of
/// time — used once files actually exist on disk.
pub fn is_audio_file_path(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(is_audio_extension)
        .unwrap_or(false)
}

/// One downloadable item from a gallery-dl `Message.Url` entry
/// (`gallery_dl.extractor.message.Message.Url = 3` — see gallery-dl's own
/// `message.py`). `filename`/`extension` come straight from the extractor's
/// own per-item metadata dict, the same fields gallery-dl itself uses to
/// build the final on-disk filename.
#[derive(Debug, Clone, serde::Serialize)]
pub struct GalleryEntry {
    pub url: String,
    pub filename: Option<String>,
    pub extension: Option<String>,
}

/// Result of `dump_gallery_json`. `title`/`category` are best-effort: unlike
/// yt-dlp (one consistent `title` field across every extractor), gallery-dl
/// extractors each expose their own metadata shape (Twitter's post text is
/// `content`, Pixiv's is `title`, TikTok's is `desc`, ...), so there is no
/// single universal field — `title` tries a handful of common candidate keys
/// across every message's metadata and falls back to `"{category} post"`.
#[derive(Debug, Clone)]
pub struct GalleryDump {
    pub entries: Vec<GalleryEntry>,
    pub title: Option<String>,
    pub category: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct GalleryProgressUpdate {
    pub completed_files: u32,
    pub total_files: u32,
}

/// Printed after each file gallery-dl actually finishes downloading (the
/// `after` event — see gallery-dl's `job.py`). Literal marker text must be
/// paired with a real `{field}` reference in the same `--Print` argument: a
/// bare word with no braces is itself treated as an implicit field lookup by
/// gallery-dl's template engine (confirmed empirically — `--Print
/// "after:MARKER"` printed the string `None`, the value of a nonexistent
/// field named literally `MARKER`, not the literal text `MARKER`), so this
/// marker rides along with the always-present `{filename}` field instead of
/// standing alone.
const FILE_DONE_MARKER: &str = "MEDIA_DOWNLOADER_GALLERY_FILE_DONE::";

async fn spawn_gallery_dl(app: &AppHandle, args: Vec<String>) -> Result<Child, AppError> {
    let exe_path = resolve_gallery_dl_executable(app).await?;
    Command::new(&exe_path)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(AppError::internal)
}

/// Runs `gallery-dl --dump-json --no-download <url>` and returns every
/// downloadable item it found, without downloading anything — the gallery-dl
/// equivalent of `ytdlp::dump_metadata_json`, used for `preview_media`'s
/// fallback path when yt-dlp itself has no extractor for a URL (or the URL
/// resolves to an image/gallery post yt-dlp can't represent, e.g. a TikTok
/// slideshow — see `research.md` §2's gallery-dl amendment).
pub async fn dump_gallery_json(
    app: &AppHandle,
    url: &str,
    on_spawn: impl FnOnce(GalleryDlChild),
) -> Result<GalleryDump, AppError> {
    let args = vec!["--dump-json".into(), "--no-download".into(), url.to_string()];

    let mut child = spawn_gallery_dl(app, args).await?;
    let stdout = child.stdout.take().expect("stdout was piped");
    let stderr = child.stderr.take().expect("stderr was piped");
    let child = Arc::new(Mutex::new(child));
    on_spawn(Arc::clone(&child));

    let stdout_task = tokio::spawn(read_all(stdout));
    let stderr_task = tokio::spawn(read_all(stderr));

    let status = child.lock().await.wait().await.map_err(AppError::internal)?;
    let stdout_buf = stdout_task.await.map_err(AppError::internal)?;
    let stderr_buf = stderr_task.await.map_err(AppError::internal)?;

    if !status.success() {
        return Err(classify_gallery_dl_error(&stderr_buf));
    }

    parse_dump_json(&stdout_buf)
}

/// Downloads every item gallery-dl finds for `url` into `output_dir`
/// (expected to already be a job-exclusive directory — gallery-dl's `-D`
/// flag downloads directly into it with no `category/title/` nesting, so a
/// shared folder like the user's chosen Downloads directory would otherwise
/// mix unrelated files together with no reliable way to tell which ones this
/// job created). Returns the final list of downloaded file paths.
pub async fn run_gallery_download(
    app: &AppHandle,
    url: &str,
    output_dir: &str,
    total_files: u32,
    mut on_progress: impl FnMut(GalleryProgressUpdate) + Send + 'static,
) -> Result<Vec<String>, AppError> {
    let args = vec![
        "-D".into(),
        output_dir.to_string(),
        "--Print".into(),
        format!("after:{FILE_DONE_MARKER}{{filename}}"),
        url.to_string(),
    ];

    let mut child = spawn_gallery_dl(app, args).await?;
    let stdout = child.stdout.take().expect("stdout was piped");
    let stderr = child.stderr.take().expect("stderr was piped");
    let stderr_task = tokio::spawn(read_all(stderr));

    let mut completed_files = 0u32;
    let mut lines = BufReader::new(stdout).lines();
    while let Some(line) = lines.next_line().await.map_err(AppError::internal)? {
        if line.starts_with(FILE_DONE_MARKER) {
            completed_files += 1;
            on_progress(GalleryProgressUpdate {
                completed_files,
                total_files,
            });
        }
    }

    let status = child.wait().await.map_err(AppError::internal)?;
    let stderr_buf = stderr_task.await.map_err(AppError::internal)?;

    if !status.success() {
        return Err(classify_gallery_dl_error(&stderr_buf));
    }

    list_downloaded_files(output_dir).await
}

async fn list_downloaded_files(dir: &str) -> Result<Vec<String>, AppError> {
    let mut entries = tokio::fs::read_dir(dir).await.map_err(AppError::internal)?;
    let mut paths = Vec::new();
    while let Some(entry) = entries.next_entry().await.map_err(AppError::internal)? {
        if entry.file_type().await.map_err(AppError::internal)?.is_file() {
            paths.push(entry.path().to_string_lossy().into_owned());
        }
    }
    paths.sort();
    Ok(paths)
}

/// Candidate metadata keys tried, in order, for a human-readable title —
/// see `GalleryDump::title`'s doc comment for why there's no single field.
const TITLE_KEY_CANDIDATES: &[&str] = &["title", "content", "desc", "description", "display_name"];

fn parse_dump_json(raw: &str) -> Result<GalleryDump, AppError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(GalleryDump {
            entries: Vec::new(),
            title: None,
            category: None,
        });
    }
    let messages: Vec<serde_json::Value> = serde_json::from_str(trimmed).map_err(AppError::internal)?;

    const MESSAGE_URL: i64 = 3; // gallery_dl.extractor.message.Message.Url

    let mut entries = Vec::new();
    let mut title: Option<String> = None;
    let mut category: Option<String> = None;

    for message in &messages {
        let Some(tuple) = message.as_array() else { continue };
        let Some(msg_type) = tuple.first().and_then(|v| v.as_i64()) else { continue };
        let metadata = tuple.get(if msg_type == MESSAGE_URL { 2 } else { 1 });

        if category.is_none() {
            category = metadata
                .and_then(|m| m.get("category"))
                .and_then(|v| v.as_str())
                .map(String::from);
        }
        if title.is_none() {
            title = metadata.and_then(|m| {
                TITLE_KEY_CANDIDATES.iter().find_map(|key| {
                    m.get(key)
                        .and_then(|v| v.as_str())
                        .map(|s| s.chars().take(120).collect::<String>())
                        .filter(|s| !s.trim().is_empty())
                })
            });
        }

        if msg_type != MESSAGE_URL {
            continue;
        }
        let Some(url) = tuple.get(1).and_then(|v| v.as_str()) else { continue };
        entries.push(GalleryEntry {
            url: url.to_string(),
            filename: metadata
                .and_then(|m| m.get("filename"))
                .and_then(|v| v.as_str())
                .map(String::from),
            extension: metadata
                .and_then(|m| m.get("extension"))
                .and_then(|v| v.as_str())
                .map(String::from),
        });
    }
    Ok(GalleryDump { entries, title, category })
}

async fn read_all(mut reader: impl tokio::io::AsyncRead + Unpin) -> String {
    use tokio::io::AsyncReadExt;
    let mut buf = String::new();
    let _ = reader.read_to_string(&mut buf).await;
    buf
}

/// Maps gallery-dl stderr output to the same error taxonomy
/// `ytdlp::classify_ytdlp_error` uses, so the frontend's `ErrorBanner`
/// doesn't need a second code path for gallery-dl-originated failures.
fn classify_gallery_dl_error(stderr: &str) -> AppError {
    let lower = stderr.to_lowercase();
    if lower.contains("unsupported url") || lower.contains("no extractor") {
        AppError::unsupported_platform(stderr.lines().last().unwrap_or(stderr))
    } else if lower.contains("401") || lower.contains("403") || lower.contains("private") || lower.contains("login") {
        AppError::access_denied(stderr.lines().last().unwrap_or(stderr).to_string())
    } else {
        AppError::new(
            "DOWNLOAD_FAILED",
            stderr.lines().last().unwrap_or(stderr).to_string(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_url_messages_and_extracts_category_from_directory_message() {
        let raw = json!([
            [2, {"category": "wikipedia", "page": "Cat"}],
            [3, "https://example.com/a.jpg", {"filename": "a", "extension": "jpg"}],
            [3, "https://example.com/b.png", {"filename": "b", "extension": "png"}],
        ])
        .to_string();

        let dump = parse_dump_json(&raw).unwrap();
        assert_eq!(dump.entries.len(), 2);
        assert_eq!(dump.entries[0].url, "https://example.com/a.jpg");
        assert_eq!(dump.entries[0].extension.as_deref(), Some("jpg"));
        assert_eq!(dump.category.as_deref(), Some("wikipedia"));
    }

    #[test]
    fn falls_back_through_title_key_candidates_in_priority_order() {
        let raw = json!([
            [3, "https://example.com/a.jpg", {"desc": "a real post caption", "extension": "jpg"}],
        ])
        .to_string();

        let dump = parse_dump_json(&raw).unwrap();
        assert_eq!(dump.title.as_deref(), Some("a real post caption"));
    }

    #[test]
    fn empty_output_yields_an_empty_list_instead_of_an_error() {
        assert!(parse_dump_json("").unwrap().entries.is_empty());
    }

    #[test]
    fn classifies_unsupported_url_errors_correctly() {
        let err = classify_gallery_dl_error("[gallery-dl][error] Unsupported URL 'https://example.com/x'");
        assert_eq!(err.code, "UNSUPPORTED_PLATFORM");
    }

    #[test]
    fn classifies_unknown_errors_as_generic_download_failed() {
        let err = classify_gallery_dl_error("[gallery-dl][error] network timeout");
        assert_eq!(err.code, "DOWNLOAD_FAILED");
    }
}
