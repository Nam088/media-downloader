use std::process::Stdio;
use std::sync::Arc;

use tauri::AppHandle;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

use crate::error::AppError;

use super::ytdlp_binary::{resolve_ffmpeg_path, resolve_ytdlp_executable};

/// Marker prefix used to disambiguate the final output path (printed once via
/// yt-dlp's `--print after_move:...`) from the per-line JSON progress events
/// emitted by `--progress-template "%(progress)j"`. Without a marker, a plain
/// path line and a JSON line look different enough in practice, but a marker
/// makes the intent explicit and immune to yt-dlp changing quoting behavior.
const FILEPATH_MARKER: &str = "MEDIA_DOWNLOADER_FILEPATH::";

/// Shared handle to a spawned yt-dlp child process. `Arc<Mutex<_>>` because
/// the process needs to be killable from a *different* Tauri command
/// (`cancel_preview_media`) than the one that spawned and is still reading
/// its output — a plain `tokio::process::Child` can't be shared like that.
pub type YtDlpChild = Arc<Mutex<Child>>;

#[derive(Debug, Clone, Default)]
pub struct ProgressUpdate {
    pub percent: f64,
    pub speed_bytes_per_sec: Option<i64>,
    pub eta_seconds: Option<i64>,
}

fn base_process_args() -> Vec<String> {
    vec![
        "--no-warnings".into(),
        // Prefer IPv4: on networks with broken/slow IPv6 routing, yt-dlp
        // otherwise tries IPv6 first and waits for it to time out before
        // falling back — often the biggest single contributor to a slow
        // preview. `--socket-timeout` bounds the worst case instead of
        // hanging indefinitely on one unresponsive endpoint.
        "-4".into(),
        "--socket-timeout".into(),
        "20".into(),
    ]
}

async fn spawn_ytdlp(app: &AppHandle, args: Vec<String>) -> Result<Child, AppError> {
    let ytdlp_path = resolve_ytdlp_executable(app).await?;
    Command::new(&ytdlp_path)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(AppError::internal)
}

/// Runs `yt-dlp --dump-single-json --flat-playlist <url>` and returns the raw
/// parsed JSON. `--flat-playlist` keeps playlist previews fast (no per-video
/// format fetch); for a single (non-playlist) URL it has no effect, so the
/// full `formats` array (needed for FR-004/FR-019 dynamic quality options) is
/// still present.
///
/// `on_spawn` receives a shared, killable handle to the child so a "Stop"
/// button (`cancel_preview_media`) can actually terminate a slow preview
/// instead of just hiding the spinner while yt-dlp keeps running unattended.
pub async fn dump_metadata_json(
    app: &AppHandle,
    url: &str,
    on_spawn: impl FnOnce(YtDlpChild),
) -> Result<serde_json::Value, AppError> {
    let mut args = base_process_args();
    args.push("--dump-single-json".into());
    args.push("--flat-playlist".into());
    args.push(url.to_string());

    let mut child = spawn_ytdlp(app, args).await?;
    let stdout = child.stdout.take().expect("stdout was piped");
    let stderr = child.stderr.take().expect("stderr was piped");
    let child = Arc::new(Mutex::new(child));
    on_spawn(Arc::clone(&child));

    let stdout_task = tokio::spawn(read_all(stdout));
    let stderr_task = tokio::spawn(read_all(stderr));

    let status = child.lock().await.wait().await.map_err(AppError::internal)?;
    let stdout_buf = stdout_task.await.map_err(AppError::internal)?;
    let stderr_buf = stderr_task.await.map_err(AppError::internal)?;

    if was_killed(&status) {
        // Killed by `cancel_preview_media` rather than exiting on its own —
        // surface this distinctly so the frontend can treat it as "the user
        // stopped this", not a real failure to show in red.
        return Err(AppError::new("CANCELED", "Preview was canceled"));
    }
    if !status.success() {
        return Err(classify_ytdlp_error(&stderr_buf));
    }

    serde_json::from_str(stdout_buf.trim()).map_err(AppError::internal)
}

/// Spawns `yt-dlp` with the given extra args (format/audio-extraction flags
/// are supplied by the caller — see `downloader::queue`), streams progress
/// via `--progress-template "%(progress)j"`, and returns the final output
/// file path once yt-dlp exits successfully.
pub async fn run_download(
    app: &AppHandle,
    url: &str,
    output_template: &str,
    extra_args: Vec<String>,
    mut on_progress: impl FnMut(ProgressUpdate) + Send + 'static,
) -> Result<String, AppError> {
    let ffmpeg_path = resolve_ffmpeg_path()?;

    let mut args = base_process_args();
    args.push("--newline".into());
    args.push("--ffmpeg-location".into());
    args.push(ffmpeg_path.to_string_lossy().into_owned());
    // yt-dlp silently sets `quiet`/`noprogress` to true whenever `--print`
    // is present (its own CLI does this so `--print` output isn't mixed
    // with a progress bar meant for a terminal). Without `--progress` here
    // to force it back on, `--progress-template` below would never emit
    // anything — confirmed by testing: `--print` alone suppresses every
    // progress line, and only `--progress` restores them.
    args.push("--progress".into());
    args.push("--progress-template".into());
    args.push("%(progress)j".into());
    args.push("--print".into());
    args.push(format!("after_move:{FILEPATH_MARKER}%(filepath)s"));
    args.push("-o".into());
    args.push(output_template.to_string());
    args.extend(extra_args);
    args.push(url.to_string());

    let mut child = spawn_ytdlp(app, args).await?;
    let stdout = child.stdout.take().expect("stdout was piped");
    let stderr = child.stderr.take().expect("stderr was piped");

    let stderr_task = tokio::spawn(read_all(stderr));

    let mut output_path: Option<String> = None;
    let mut lines = BufReader::new(stdout).lines();
    while let Some(line) = lines.next_line().await.map_err(AppError::internal)? {
        let line = line.trim();
        if let Some(path) = line.strip_prefix(FILEPATH_MARKER) {
            output_path = Some(path.to_string());
        } else if let Ok(progress) = serde_json::from_str::<serde_json::Value>(line) {
            on_progress(parse_progress(&progress));
        }
    }

    let status = child.wait().await.map_err(AppError::internal)?;
    let stderr_buf = stderr_task.await.map_err(AppError::internal)?;

    if !status.success() {
        return Err(classify_ytdlp_error(&stderr_buf));
    }

    output_path.ok_or_else(|| {
        AppError::internal("yt-dlp exited successfully but did not report an output file path")
    })
}

/// Probes a downloaded file for an audio stream by asking ffmpeg to remux
/// just the audio track to a null output — no need for a separately bundled
/// `ffprobe`, since ffmpeg alone already ships as our sidecar.
///
/// This exists because of a documented TikTok/yt-dlp interaction
/// (yt-dlp issue #15891): TikTok's CDN can intermittently serve a video-only
/// file for a format id whose metadata still claims `acodec=aac`, so yt-dlp
/// has no way to detect the mismatch itself and reports success. yt-dlp
/// maintainers confirmed this is TikTok serving inconsistent media, not a
/// parsing bug, and that re-downloading the same URL commonly gets a
/// different (correct) file. `run_job` in `downloader::queue` uses this to
/// retry rather than deliver a silently audio-less "successful" download.
pub async fn output_has_audio_stream(file_path: &str) -> bool {
    let Ok(ffmpeg_path) = resolve_ffmpeg_path() else {
        return true; // fail open: don't block a completed download over a probe we can't even run
    };
    Command::new(&ffmpeg_path)
        .args(["-v", "error", "-i", file_path, "-map", "0:a", "-c", "copy", "-f", "null", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .map(|status| status.success())
        .unwrap_or(true)
}

async fn read_all(mut reader: impl tokio::io::AsyncRead + Unpin) -> String {
    use tokio::io::AsyncReadExt;
    let mut buf = String::new();
    let _ = reader.read_to_string(&mut buf).await;
    buf
}

#[cfg(unix)]
fn was_killed(status: &std::process::ExitStatus) -> bool {
    use std::os::unix::process::ExitStatusExt;
    status.signal().is_some()
}

#[cfg(not(unix))]
fn was_killed(status: &std::process::ExitStatus) -> bool {
    // Windows reports a killed process as a non-zero exit code, not a
    // distinct "signal" — callers already treat any failure without this
    // flag as a normal (non-canceled) error, which is the closest available
    // approximation here.
    !status.success() && status.code() == Some(1)
}

fn parse_progress(value: &serde_json::Value) -> ProgressUpdate {
    let downloaded = value.get("downloaded_bytes").and_then(|v| v.as_f64());
    let total = value
        .get("total_bytes")
        .and_then(|v| v.as_f64())
        .or_else(|| value.get("total_bytes_estimate").and_then(|v| v.as_f64()));
    let percent = match (downloaded, total) {
        (Some(d), Some(t)) if t > 0.0 => (d / t) * 100.0,
        _ => 0.0,
    };
    ProgressUpdate {
        percent,
        speed_bytes_per_sec: value.get("speed").and_then(|v| v.as_i64()),
        eta_seconds: value.get("eta").and_then(|v| v.as_i64()),
    }
}

/// Maps yt-dlp stderr output to the FR-009/FR-012 error taxonomy the frontend
/// understands (ErrorBanner localizes by `code`, not by parsing stderr text).
fn classify_ytdlp_error(stderr: &str) -> AppError {
    let lower = stderr.to_lowercase();
    if lower.contains("private video")
        || lower.contains("sign in")
        || lower.contains("login")
        || lower.contains("drm")
        || lower.contains("premium")
    {
        AppError::access_denied(stderr.lines().last().unwrap_or(stderr).to_string())
    } else if lower.contains("unsupported url") || lower.contains("no extractor") {
        AppError::unsupported_platform(stderr.lines().last().unwrap_or(stderr))
    } else if crate::downloader::retry::has_network_marker(&lower) {
        // Kiểm tra lỗi mạng SAU các lỗi nội dung: một thông báo "private video"
        // đôi khi cũng chứa từ "connection", và lỗi nội dung phải thắng để không
        // bị thử lại vô ích.
        AppError::new(
            "NETWORK_ERROR",
            stderr.lines().last().unwrap_or(stderr).to_string(),
        )
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
    fn base_args_force_ipv4_and_bound_the_socket_timeout() {
        let args = base_process_args();
        assert!(args.iter().any(|a| a == "-4"));
        assert!(args.iter().any(|a| a == "--socket-timeout"));
    }

    #[test]
    fn parses_percent_speed_and_eta_from_a_progress_dict() {
        let progress = json!({
            "status": "downloading",
            "downloaded_bytes": 2_500_000,
            "total_bytes": 10_000_000,
            "speed": 512_000,
            "eta": 15,
        });
        let update = parse_progress(&progress);
        assert!((update.percent - 25.0).abs() < f64::EPSILON);
        assert_eq!(update.speed_bytes_per_sec, Some(512_000));
        assert_eq!(update.eta_seconds, Some(15));
    }

    #[test]
    fn falls_back_to_total_bytes_estimate_when_exact_total_is_unknown() {
        let progress = json!({
            "downloaded_bytes": 1_000_000,
            "total_bytes_estimate": 4_000_000,
        });
        let update = parse_progress(&progress);
        assert!((update.percent - 25.0).abs() < f64::EPSILON);
    }

    #[test]
    fn missing_fields_default_to_zero_percent_instead_of_panicking() {
        let update = parse_progress(&json!({"status": "starting"}));
        assert_eq!(update.percent, 0.0);
        assert_eq!(update.speed_bytes_per_sec, None);
    }

    #[test]
    fn classifies_private_or_login_required_errors_as_access_denied() {
        let err = classify_ytdlp_error("ERROR: Private video. Sign in if you've been granted access");
        assert_eq!(err.code, "ACCESS_DENIED");
    }

    #[test]
    fn classifies_unsupported_url_errors_correctly() {
        let err = classify_ytdlp_error("ERROR: Unsupported URL: https://example.com/x");
        assert_eq!(err.code, "UNSUPPORTED_PLATFORM");
    }

    #[test]
    fn classifies_network_failures_separately() {
        for stderr in [
            "ERROR: network timeout",
            "ERROR: [Errno 110] Connection timed out",
            "ERROR: unable to download video data: <urlopen error [Errno -3] Temporary failure in name resolution>",
            "ERROR: Unable to download webpage: HTTP Error 503: Service Unavailable",
            "ERROR: HTTP Error 429: Too Many Requests",
            "ERROR: Connection reset by peer",
        ] {
            assert_eq!(
                classify_ytdlp_error(stderr).code,
                "NETWORK_ERROR",
                "phải nhận ra là lỗi mạng: {stderr}"
            );
        }
    }

    #[test]
    fn content_failures_do_not_become_network_errors() {
        assert_eq!(
            classify_ytdlp_error("ERROR: Private video. Sign in if you've been granted access").code,
            "ACCESS_DENIED"
        );
        assert_eq!(
            classify_ytdlp_error("ERROR: Unsupported URL: https://example.com").code,
            "UNSUPPORTED_PLATFORM"
        );
        assert_eq!(
            classify_ytdlp_error("ERROR: something unusual happened").code,
            "DOWNLOAD_FAILED"
        );
    }
}
