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
    /// `None` means the percentage is genuinely **unknown**, not zero: yt-dlp
    /// reported no usable total size for this stream, which is the normal
    /// case for audio-only formats and HLS (confirmed live — a chunked
    /// response with no `Content-Length` makes yt-dlp emit
    /// `"total_bytes": null` on every single tick).
    ///
    /// This used to be a plain `f64` that fell back to `0.0`, which pinned
    /// the progress bar at 0% for the whole download and left it there after
    /// the job finished. "Unknown" and "0% done" are different states and the
    /// UI has to be able to tell them apart, so the distinction lives in the
    /// type rather than in a sentinel value.
    pub percent: Option<f64>,
    /// Always reported by yt-dlp even when the total isn't, so the UI has
    /// something true to show ("12.3 MB · 1.2 MB/s") in place of a
    /// percentage it doesn't have.
    pub downloaded_bytes: Option<i64>,
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
    let mut cmd = Command::new(&ytdlp_path);
    cmd.args(args);
    crate::downloader::hide_cmd_window(&mut cmd);
    cmd.stdin(Stdio::null())
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
    let mut cmd = Command::new(&ffmpeg_path);
    cmd.args(["-v", "error", "-i", file_path, "-map", "0:a", "-c", "copy", "-f", "null", "-"]);
    super::hide_cmd_window(&mut cmd);
    cmd.stdin(Stdio::null())
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

/// Reads a numeric progress field as a whole number.
///
/// `as_i64()` alone is not enough: yt-dlp writes these fields as JSON
/// *numbers*, but not all of them as integers. `speed` is a float
/// (`"speed": 248724.07319898077`, verified against the bundled binary), and
/// `serde_json::Value::as_i64` returns `None` for any number with a
/// fractional part — so the old `.as_i64()` silently dropped the download
/// speed on every tick and the queue showed "--" for it forever. Missing
/// fields and JSON `null` (yt-dlp writes `"eta": null` when there's no total
/// to compute one from) still come back as `None`, which is the truth.
fn number_as_i64(value: Option<&serde_json::Value>) -> Option<i64> {
    value
        .and_then(|v| v.as_f64())
        .filter(|number| number.is_finite())
        .map(|number| number.round() as i64)
}

fn parse_progress(value: &serde_json::Value) -> ProgressUpdate {
    let downloaded = value.get("downloaded_bytes").and_then(|v| v.as_f64());
    let total = value
        .get("total_bytes")
        .and_then(|v| v.as_f64())
        .or_else(|| value.get("total_bytes_estimate").and_then(|v| v.as_f64()));
    // No total (or a nonsensical one) means there is no percentage to
    // compute — say so, rather than claiming 0%.
    let percent = match (downloaded, total) {
        (Some(d), Some(t)) if t > 0.0 && d.is_finite() => Some((d / t) * 100.0),
        _ => None,
    };
    ProgressUpdate {
        percent,
        downloaded_bytes: number_as_i64(value.get("downloaded_bytes")),
        speed_bytes_per_sec: number_as_i64(value.get("speed")),
        eta_seconds: number_as_i64(value.get("eta")),
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
    } else if crate::downloader::retry::has_network_marker(stderr) {
        // Kiểm tra lỗi mạng SAU các lỗi nội dung: một thông báo "private video"
        // đôi khi cũng chứa từ "connection reset", và lỗi nội dung phải thắng
        // để không bị thử lại vô ích.
        AppError::network_error(stderr.lines().last().unwrap_or(stderr).to_string())
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
        assert_eq!(update.percent, Some(25.0));
        assert_eq!(update.downloaded_bytes, Some(2_500_000));
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
        assert_eq!(update.percent, Some(25.0));
    }

    #[test]
    fn a_missing_zero_or_non_numeric_total_means_unknown_not_zero_percent() {
        // THE bug this whole change exists for. yt-dlp reports no total size
        // for audio-only formats and HLS — the payload below is a verbatim
        // (trimmed) tick from the bundled yt-dlp binary against a chunked
        // response with no Content-Length. Reporting `0.0` here pinned the
        // progress bar at 0% for the entire download and left it at 0% after
        // the job completed; 30 of the user's 42 completed audio jobs are
        // stored that way.
        let no_total = json!({
            "status": "downloading",
            "downloaded_bytes": 523_264,
            "total_bytes": serde_json::Value::Null,
            "eta": serde_json::Value::Null,
            "speed": 367853.0524615014_f64,
        });
        assert_eq!(
            parse_progress(&no_total).percent,
            None,
            "no total size means the percentage is unknown, not 0%"
        );

        // A zero total would make the division meaningless (0/0, or +inf).
        let zero_total = json!({"downloaded_bytes": 1_000, "total_bytes": 0});
        assert_eq!(parse_progress(&zero_total).percent, None);

        // Not a number at all — yt-dlp has changed field shapes before.
        let junk_total = json!({"downloaded_bytes": 1_000, "total_bytes": "N/A"});
        assert_eq!(parse_progress(&junk_total).percent, None);

        // ...and the field simply absent.
        assert_eq!(parse_progress(&json!({"status": "starting"})).percent, None);
    }

    #[test]
    fn the_numbers_that_are_known_survive_a_missing_total() {
        // The point of distinguishing "unknown" from 0%: there is still
        // something true to show. Both fields come from the same payload as
        // the missing total, so the UI can render "511.0 KB · 359.2 KB/s"
        // instead of a percentage that would simply be false.
        let update = parse_progress(&json!({
            "status": "downloading",
            "downloaded_bytes": 523_264,
            "total_bytes": serde_json::Value::Null,
            "speed": 367853.0524615014_f64,
        }));
        assert_eq!(update.percent, None);
        assert_eq!(update.downloaded_bytes, Some(523_264));
        assert_eq!(update.speed_bytes_per_sec, Some(367_853));
    }

    #[test]
    fn a_fractional_speed_is_kept_rather_than_dropped() {
        // Regression test: yt-dlp emits `speed` as a float on every tick
        // (verified against the bundled binary), and `Value::as_i64` returns
        // None for any number with a fractional part — so the previous
        // `.as_i64()` parse dropped the speed on *every* download and the
        // queue rendered "--" for it, including in the very case where the
        // percentage is unknown and the speed is the only honest number left.
        let update = parse_progress(&json!({
            "downloaded_bytes": 1_024,
            "speed": 248724.07319898077_f64,
            "eta": 12,
        }));
        assert_eq!(update.speed_bytes_per_sec, Some(248_724));
        assert_eq!(update.eta_seconds, Some(12));
    }

    #[test]
    fn missing_fields_are_reported_as_missing_instead_of_panicking() {
        let update = parse_progress(&json!({"status": "starting"}));
        assert_eq!(update.percent, None);
        assert_eq!(update.downloaded_bytes, None);
        assert_eq!(update.speed_bytes_per_sec, None);
        assert_eq!(update.eta_seconds, None);
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
        // Danh sách dấu hiệu đầy đủ được ghim từng cái một trong
        // `downloader::retry`; ở đây chỉ cần chứng minh nhánh mạng của bộ phân
        // loại có nối dây tới đó.
        for stderr in [
            "ERROR: [Errno 110] Connection timed out",
            "ERROR: Unable to download webpage: HTTP Error 503: Service Unavailable",
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
    fn content_failures_win_over_network_markers_in_the_same_message() {
        // Đây là test duy nhất ghim THỨ TỰ các nhánh: cả hai chuỗi dưới đây
        // mang đồng thời dấu hiệu nội dung lẫn dấu hiệu mạng. Nếu ai đó đẩy
        // nhánh mạng lên trước, chúng sẽ ra NETWORK_ERROR và một video riêng tư
        // vĩnh viễn sẽ bị thử lại hết vòng này tới vòng khác.
        assert_eq!(
            classify_ytdlp_error("ERROR: Private video. Sign in — connection reset while checking").code,
            "ACCESS_DENIED",
            "content failure must win over a network marker in the same message"
        );
        assert_eq!(
            classify_ytdlp_error("ERROR: Unsupported URL: https://example.com (connection timed out)").code,
            "UNSUPPORTED_PLATFORM",
            "content failure must win over a network marker in the same message"
        );
    }

    #[test]
    fn classifies_unknown_errors_as_generic_download_failed() {
        assert_eq!(
            classify_ytdlp_error("ERROR: something unusual happened").code,
            "DOWNLOAD_FAILED"
        );
        // "Network Ten" là tên một đài truyền hình, không phải sự cố đường
        // truyền. Thông báo này là chặn theo bản quyền — vĩnh viễn — nên nó rơi
        // vào nhóm gom và thất bại ngay, thay vì bắt người dùng chờ hết chuỗi
        // backoff cho đúng cái ca mà SC-106 đòi phải hỏng nhanh.
        assert_eq!(
            classify_ytdlp_error(
                "ERROR: Video unavailable. This video contains content from Network Ten, who has blocked it"
            )
            .code,
            "DOWNLOAD_FAILED",
            "tên đài có chữ Network không được biến lỗi bản quyền thành lỗi mạng"
        );
    }

    #[test]
    fn only_the_last_stderr_line_reaches_the_message() {
        // yt-dlp in cả loạt cảnh báo trước dòng ERROR thật. Nếu nhét nguyên
        // khối stderr vào `message` thì banner lỗi trên giao diện ngập rác, nên
        // mỗi nhánh chỉ lấy dòng cuối — kiểm cả bốn vì mỗi nhánh tự lấy một lần.
        let err = classify_ytdlp_error("WARNING: noise\nERROR: Connection reset by peer");
        assert_eq!(
            err.message, "ERROR: Connection reset by peer",
            "only the last line reaches the UI"
        );

        let err = classify_ytdlp_error("WARNING: noise\nERROR: Private video. Sign in to view");
        assert_eq!(err.message, "ERROR: Private video. Sign in to view");

        let err = classify_ytdlp_error("WARNING: noise\nERROR: something unusual happened");
        assert_eq!(err.message, "ERROR: something unusual happened");

        let err = classify_ytdlp_error("WARNING: noise\nERROR: Unsupported URL: https://example.com");
        assert!(
            err.message.ends_with("ERROR: Unsupported URL: https://example.com"),
            "nhánh này bọc thêm tiền tố nhưng vẫn chỉ lấy dòng cuối: {}",
            err.message
        );
    }
}
