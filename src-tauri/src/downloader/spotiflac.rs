//! Engine SpotiFLAC (specs/006-spotiflac-integration): spawn spotiflac-worker,
//! parse giao thức sentinel JSON-line trên stdout, và phân loại lỗi.
//!
//! Giao thức đầy đủ:
//! `specs/006-spotiflac-integration/contracts/spotiflac-worker-protocol.md`.
//! Mỗi lần spawn xử lý đúng MỘT việc (preview 1 URL, hoặc download 1 track) —
//! mirror hình dạng cặp module của gallery-dl (`gallery_dl.rs` +
//! `gallery_dl_binary.rs`).

use std::process::Stdio;
use std::sync::Arc;

use serde::Deserialize;
use tauri::AppHandle;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::Mutex;

use crate::error::{
    AppError, SPOTIFLAC_CHALLENGE_TIMEOUT_CODE, SPOTIFLAC_NO_SOURCE_CODE,
    SPOTIFLAC_REGION_BLOCKED_CODE,
};

use super::retry::has_network_marker;
use super::spotiflac_binary::resolve_spotiflac_executable;

/// Prefix mọi dòng event có cấu trúc từ worker. Dòng không mang prefix là log
/// thô của module Python — chỉ đáng đưa vào LogBuffer ở mức debug.
const EVENT_SENTINEL: &str = "SPOTIFLAC_EVENT::";

/// Phiên bản giao thức duy nhất bản Rust này hiểu. Worker bundle lệch phiên
/// bản (một lần bump module quên cập nhật hai phía) phải bị từ chối ngay từ
/// event `hello` thay vì hỏng ngầm giữa chừng một job.
pub const SUPPORTED_PROTOCOL: u32 = 1;

/// Số dòng log cuối của worker giữ lại để chẩn đoán khi nó chết bất ngờ.
const CRASH_LOG_TAIL: usize = 10;

/// Một event đã parse từ stdout của worker — mirror 1:1 bảng event trong
/// contract §2. `serde(tag = "type")` khớp trường `"type"` trong JSON.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerEvent {
    Hello {
        protocol: u32,
        #[serde(default)]
        module_version: Option<String>,
    },
    PreviewResult(MusicPreview),
    TrackStart {
        provider: String,
    },
    Progress {
        percent: Option<f64>,
        #[serde(default)]
        downloaded_bytes: Option<i64>,
        #[serde(default)]
        speed_bps: Option<i64>,
    },
    ProviderSwitch {
        from: String,
        to: String,
        #[serde(default)]
        reason: Option<String>,
    },
    CloudflareChallenge {
        challenge_url: String,
    },
    TrackDone {
        file_path: String,
        #[serde(default)]
        provider: Option<String>,
        #[serde(default)]
        bit_depth: Option<u32>,
    },
    Error {
        code: String,
        message: String,
        #[serde(default)]
        provider: Option<String>,
    },
}

/// Kết quả preview — `tracks` luôn có ít nhất một phần tử (worker tự trả lỗi
/// `SPOTIFLAC_NO_SOURCE` khi không resolve được gì).
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct MusicPreview {
    pub kind: String,
    pub title: String,
    pub artist: String,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub thumbnail_url: Option<String>,
    pub tracks: Vec<MusicTrackPreview>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct MusicTrackPreview {
    pub url: String,
    pub title: String,
    pub artist: String,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub duration_seconds: Option<i64>,
    #[serde(default)]
    pub track_number: Option<i64>,
    #[serde(default)]
    pub thumbnail_url: Option<String>,
}

/// Một dòng stdout đã phân loại.
#[derive(Debug)]
pub enum WorkerLine {
    Event(WorkerEvent),
    /// Log thô (không có sentinel) — chuyển tiếp vào LogBuffer mức debug.
    Log(String),
}

/// Parse một dòng stdout của worker. Dòng có sentinel nhưng JSON hỏng trả về
/// `None` — một worker bị lỗi giữa chừng không được phép làm sập cả job chỉ
/// vì in dở một dòng.
pub fn parse_worker_line(line: &str) -> Option<WorkerLine> {
    let trimmed = line.trim_end();
    if trimmed.is_empty() {
        return None;
    }
    match trimmed.strip_prefix(EVENT_SENTINEL) {
        Some(json) => serde_json::from_str::<WorkerEvent>(json)
            .ok()
            .map(WorkerLine::Event),
        None => Some(WorkerLine::Log(trimmed.to_string())),
    }
}

/// Map event `error{code,message}` của worker sang `AppError` — điểm duy nhất
/// quyết định lỗi nào retryable: chỉ `SPOTIFLAC_NETWORK` được dịch thành
/// `NETWORK_ERROR` (mã mà `retry::decide_outcome` coi là tạm thời).
pub fn classify_worker_error(code: &str, message: &str) -> AppError {
    match code {
        "SPOTIFLAC_NETWORK" => AppError::network_error(message),
        "SPOTIFLAC_NO_SOURCE" => AppError::new(SPOTIFLAC_NO_SOURCE_CODE, message),
        "SPOTIFLAC_REGION_BLOCKED" => AppError::new(SPOTIFLAC_REGION_BLOCKED_CODE, message),
        "SPOTIFLAC_CHALLENGE_TIMEOUT" => AppError::new(SPOTIFLAC_CHALLENGE_TIMEOUT_CODE, message),
        _ => AppError::internal(format!("spotiflac worker: [{code}] {message}")),
    }
}

/// Worker chết mà không kịp emit event `error` (segfault, OOM, bundle hỏng):
/// phân loại bằng stderr — dấu hiệu mạng thì cho retry, còn lại là internal.
pub fn classify_worker_crash(stderr: &str, exit_code: Option<i32>) -> AppError {
    if has_network_marker(&stderr.to_lowercase()) {
        return AppError::network_error(stderr.trim().to_string());
    }
    AppError::internal(format!(
        "spotiflac worker exited (code {exit_code:?}) without a protocol error: {}",
        stderr.trim()
    ))
}

/// Handle của một worker đang chạy — đủ cho `queue` huỷ (kill), và cho luồng
/// Cloudflare bơm grant code xuống stdin.
#[derive(Clone)]
pub struct SpotiflacChild {
    pub child: Arc<Mutex<Child>>,
    pub stdin: Arc<Mutex<ChildStdin>>,
}

impl SpotiflacChild {
    /// Gửi một lệnh JSON-line xuống stdin worker (contract §3).
    async fn send(&self, payload: serde_json::Value) -> Result<(), AppError> {
        let mut stdin = self.stdin.lock().await;
        let line = format!("{payload}\n");
        stdin
            .write_all(line.as_bytes())
            .await
            .map_err(AppError::internal)?;
        stdin.flush().await.map_err(AppError::internal)
    }

    /// Bơm grant code người dùng nhập cho một Cloudflare challenge đang chờ.
    pub async fn send_grant(&self, grant: &str) -> Result<(), AppError> {
        self.send(serde_json::json!({"type": "grant", "value": grant}))
            .await
    }

    /// Yêu cầu worker tự dọn dẹp và thoát (exit 130). Người gọi vẫn phải giữ
    /// kill-fallback: một worker treo không đọc stdin nữa thì lệnh này không
    /// tới nơi.
    pub async fn send_cancel(&self) -> Result<(), AppError> {
        self.send(serde_json::json!({"type": "cancel"})).await
    }

    pub async fn kill(&self) {
        let mut child = self.child.lock().await;
        let _ = child.kill().await;
    }
}

/// Tham số spawn một lần download đúng 1 track (contract §1).
#[derive(Debug, Clone)]
pub struct MusicDownloadParams {
    pub track_url: String,
    pub output_dir: String,
    /// CSV đã chuẩn hoá từ `AppSettings.spotiflac_service_order`.
    pub services: String,
    /// `flac16` / `flac24` / `mp3_320`.
    pub tier: String,
    pub extensions_fallback: bool,
    /// `TG_BOT_TOKEN`/`TG_CHAT_ID` — rỗng thì không set env (FR-008).
    pub tg_bot_token: String,
    pub tg_chat_id: String,
}

async fn spawn_worker(
    app: &AppHandle,
    args: Vec<String>,
    envs: Vec<(String, String)>,
) -> Result<(SpotiflacChild, BufReader<tokio::process::ChildStdout>), AppError> {
    let exe_path = resolve_spotiflac_executable(app).await?;
    let mut command = Command::new(&exe_path);
    command
        .args(args)
        // PyInstaller onedir + pipe stdout: không có dòng này Python sẽ buffer
        // theo khối và progress chỉ tới theo cụm 4KB.
        .env("PYTHONUNBUFFERED", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for (key, value) in envs {
        command.env(key, value);
    }
    let mut child = command.spawn().map_err(AppError::internal)?;
    let stdout = child.stdout.take().expect("stdout was piped");
    let stdin = child.stdin.take().expect("stdin was piped");
    Ok((
        SpotiflacChild {
            child: Arc::new(Mutex::new(child)),
            stdin: Arc::new(Mutex::new(stdin)),
        },
        BufReader::new(stdout),
    ))
}

/// Đọc trọn stderr sau khi worker kết thúc (cho phân loại crash).
async fn drain_stderr(handle: &SpotiflacChild) -> String {
    let mut child = handle.child.lock().await;
    match child.stderr.take() {
        Some(mut stderr) => {
            use tokio::io::AsyncReadExt;
            let mut buf = String::new();
            let _ = stderr.read_to_string(&mut buf).await;
            buf
        }
        None => String::new(),
    }
}

fn build_download_args(params: &MusicDownloadParams) -> Vec<String> {
    let mut args = vec![
        "download".to_string(),
        "--url".to_string(),
        params.track_url.clone(),
        "--output-dir".to_string(),
        params.output_dir.clone(),
        "--services".to_string(),
        params.services.clone(),
        "--tier".to_string(),
        params.tier.clone(),
    ];
    if !params.extensions_fallback {
        args.push("--no-extensions-fallback".to_string());
    }
    args
}

fn telegram_envs(params: &MusicDownloadParams) -> Vec<(String, String)> {
    // Chỉ set khi CẢ HAI có giá trị: module cần cặp token+chat_id mới gửi
    // được; set một nửa chỉ tạo ra một bot im lặng khó chẩn đoán.
    if params.tg_bot_token.is_empty() || params.tg_chat_id.is_empty() {
        return Vec::new();
    }
    vec![
        ("TG_BOT_TOKEN".to_string(), params.tg_bot_token.clone()),
        ("TG_CHAT_ID".to_string(), params.tg_chat_id.clone()),
    ]
}

/// Chạy `spotiflac-worker preview --url <url>` và trả về metadata. Không tải
/// gì; dùng bởi `commands::media::preview_media` TRƯỚC nhánh yt-dlp.
///
/// `on_spawn` nhận handle để `cancel_preview_media` huỷ được giữa chừng —
/// cùng hợp đồng với `dump_gallery_json`.
pub async fn run_music_preview(
    app: &AppHandle,
    url: &str,
    on_spawn: impl Fn(SpotiflacChild),
) -> Result<MusicPreview, AppError> {
    let args = vec!["preview".to_string(), "--url".to_string(), url.to_string()];
    let (handle, mut stdout) = spawn_worker(app, args, Vec::new()).await?;
    on_spawn(handle.clone());

    let mut preview: Option<MusicPreview> = None;
    let mut worker_error: Option<AppError> = None;
    let mut line = String::new();
    loop {
        line.clear();
        let read = stdout.read_line(&mut line).await.map_err(AppError::internal)?;
        if read == 0 {
            break;
        }
        match parse_worker_line(&line) {
            Some(WorkerLine::Event(WorkerEvent::Hello { protocol, .. })) => {
                if protocol != SUPPORTED_PROTOCOL {
                    handle.kill().await;
                    return Err(AppError::internal(format!(
                        "spotiflac worker speaks protocol {protocol}, this app expects {SUPPORTED_PROTOCOL} — rebuild binaries/spotiflac-onedir"
                    )));
                }
            }
            Some(WorkerLine::Event(WorkerEvent::PreviewResult(result))) => {
                preview = Some(result);
            }
            Some(WorkerLine::Event(WorkerEvent::Error { code, message, .. })) => {
                worker_error = Some(classify_worker_error(&code, &message));
            }
            _ => {}
        }
    }

    let status = handle.child.lock().await.wait().await.map_err(AppError::internal)?;
    if let Some(preview) = preview {
        return Ok(preview);
    }
    if let Some(err) = worker_error {
        return Err(err);
    }
    let stderr = drain_stderr(&handle).await;
    Err(classify_worker_crash(&stderr, status.code()))
}

/// Kết quả cuối của một lần download thành công.
#[derive(Debug, Clone)]
pub struct MusicTrackDone {
    pub file_path: String,
    pub provider: Option<String>,
}

/// Chạy `spotiflac-worker download` cho đúng một track, stream từng event qua
/// `on_event` (progress, provider switch, Cloudflare challenge — mọi quyết
/// định trạng thái job nằm ở `queue::run_music_job`, hàm này chỉ vận chuyển).
///
/// Trả `Ok` khi worker emit `track_done`; lỗi giao thức/crash được phân loại
/// qua `classify_worker_error`/`classify_worker_crash`. Cancel là việc của
/// người gọi: giữ `SpotiflacChild` từ `on_spawn`, gửi `send_cancel` rồi
/// kill-fallback — khi đó hàm này trả lỗi internal/crash và người gọi bỏ qua
/// kết quả (cùng quy ước `CANCELED` như hai engine kia).
pub async fn run_music_download(
    app: &AppHandle,
    params: &MusicDownloadParams,
    on_spawn: impl Fn(SpotiflacChild),
    mut on_event: impl FnMut(&WorkerEvent) + Send,
) -> Result<MusicTrackDone, AppError> {
    let args = build_download_args(params);
    let envs = telegram_envs(params);
    let (handle, mut stdout) = spawn_worker(app, args, envs).await?;
    on_spawn(handle.clone());

    let mut done: Option<MusicTrackDone> = None;
    let mut worker_error: Option<AppError> = None;
    // Vài dòng log cuối của worker. Khi nó chết mà chưa kịp emit `error`,
    // stderr thường rỗng (module log qua stdout) — đây là thứ duy nhất còn
    // nói được chuyện gì vừa xảy ra.
    let mut recent_logs: std::collections::VecDeque<String> = std::collections::VecDeque::new();
    let mut line = String::new();
    loop {
        line.clear();
        let read = stdout.read_line(&mut line).await.map_err(AppError::internal)?;
        if read == 0 {
            break;
        }
        match parse_worker_line(&line) {
            Some(WorkerLine::Event(event)) => {
                match &event {
                    WorkerEvent::Hello { protocol, .. } => {
                        if *protocol != SUPPORTED_PROTOCOL {
                            handle.kill().await;
                            return Err(AppError::internal(format!(
                                "spotiflac worker speaks protocol {protocol}, this app expects {SUPPORTED_PROTOCOL} — rebuild binaries/spotiflac-onedir"
                            )));
                        }
                    }
                    WorkerEvent::TrackDone {
                        file_path,
                        provider,
                        ..
                    } => {
                        done = Some(MusicTrackDone {
                            file_path: file_path.clone(),
                            provider: provider.clone(),
                        });
                    }
                    WorkerEvent::Error { code, message, .. } => {
                        worker_error = Some(classify_worker_error(code, message));
                    }
                    _ => {}
                }
                on_event(&event);
            }
            Some(WorkerLine::Log(text)) => {
                if recent_logs.len() == CRASH_LOG_TAIL {
                    recent_logs.pop_front();
                }
                recent_logs.push_back(text);
            }
            None => {}
        }
    }

    let status = handle.child.lock().await.wait().await.map_err(AppError::internal)?;
    if let Some(done) = done {
        return Ok(done);
    }
    if let Some(err) = worker_error {
        return Err(err);
    }
    let mut diagnostics = drain_stderr(&handle).await;
    if !recent_logs.is_empty() {
        diagnostics.push_str(&format!("\nlast worker logs: {}", Vec::from(recent_logs).join(" | ")));
    }
    Err(classify_worker_crash(&diagnostics, status.code()))
}

/// Máy có Node.js trong PATH không — quyết định có cho phép JS extensions
/// fallback hay không (research.md R5: KHÔNG bao giờ để module tự cài Node).
pub fn node_available() -> bool {
    which_node().is_some()
}

fn which_node() -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    let exe = if cfg!(windows) { "node.exe" } else { "node" };
    std::env::split_paths(&path)
        .map(|dir| dir.join(exe))
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::NETWORK_ERROR_CODE;

    #[test]
    fn a_sentinel_line_parses_into_its_event() {
        let line = r#"SPOTIFLAC_EVENT::{"type":"progress","percent":42.5,"downloaded_bytes":1048576,"speed_bps":524288}"#;
        match parse_worker_line(line) {
            Some(WorkerLine::Event(WorkerEvent::Progress {
                percent,
                downloaded_bytes,
                speed_bps,
            })) => {
                assert_eq!(percent, Some(42.5));
                assert_eq!(downloaded_bytes, Some(1_048_576));
                assert_eq!(speed_bps, Some(524_288));
            }
            other => panic!("expected progress event, got {other:?}"),
        }
    }

    #[test]
    fn a_null_percent_stays_none_for_the_indeterminate_bar() {
        // Cùng semantics với yt-dlp: `None` = "thật sự không biết", giao diện
        // hiện thanh indeterminate chứ không phải 0%.
        let line = r#"SPOTIFLAC_EVENT::{"type":"progress","percent":null}"#;
        match parse_worker_line(line) {
            Some(WorkerLine::Event(WorkerEvent::Progress { percent, .. })) => {
                assert_eq!(percent, None);
            }
            other => panic!("expected progress event, got {other:?}"),
        }
    }

    #[test]
    fn every_protocol_event_type_round_trips() {
        let lines = [
            r#"SPOTIFLAC_EVENT::{"type":"hello","protocol":1,"module_version":"1.5.5"}"#,
            r#"SPOTIFLAC_EVENT::{"type":"track_start","provider":"tidal"}"#,
            r#"SPOTIFLAC_EVENT::{"type":"provider_switch","from":"tidal","to":"qobuz","reason":"module fallback"}"#,
            r#"SPOTIFLAC_EVENT::{"type":"cloudflare_challenge","challenge_url":"https://example.com/challenge"}"#,
            r#"SPOTIFLAC_EVENT::{"type":"track_done","file_path":"/tmp/a.flac","provider":"qobuz","bit_depth":16}"#,
            r#"SPOTIFLAC_EVENT::{"type":"error","code":"SPOTIFLAC_NO_SOURCE","message":"nope"}"#,
        ];
        for line in lines {
            match parse_worker_line(line) {
                Some(WorkerLine::Event(_)) => {}
                other => panic!("line {line} should parse into an event, got {other:?}"),
            }
        }
    }

    #[test]
    fn a_preview_result_parses_with_its_tracks() {
        let line = r#"SPOTIFLAC_EVENT::{"type":"preview_result","kind":"album","title":"An Album","artist":"Someone","album":"An Album","thumbnail_url":null,"tracks":[{"url":"https://open.spotify.com/track/x","title":"Song","artist":"Someone","album":"An Album","duration_seconds":215,"track_number":1,"thumbnail_url":null}]}"#;
        match parse_worker_line(line) {
            Some(WorkerLine::Event(WorkerEvent::PreviewResult(preview))) => {
                assert_eq!(preview.kind, "album");
                assert_eq!(preview.tracks.len(), 1);
                assert_eq!(preview.tracks[0].duration_seconds, Some(215));
            }
            other => panic!("expected preview_result, got {other:?}"),
        }
    }

    #[test]
    fn a_plain_line_is_log_and_broken_json_is_dropped() {
        assert!(matches!(
            parse_worker_line("[log] [tidal] fetching stream"),
            Some(WorkerLine::Log(_))
        ));
        assert!(parse_worker_line("SPOTIFLAC_EVENT::{not json").is_none());
        assert!(parse_worker_line("   ").is_none());
    }

    #[test]
    fn only_the_network_code_maps_to_the_retryable_error() {
        assert_eq!(
            classify_worker_error("SPOTIFLAC_NETWORK", "connection reset").code,
            NETWORK_ERROR_CODE
        );
        assert_eq!(
            classify_worker_error("SPOTIFLAC_NO_SOURCE", "x").code,
            SPOTIFLAC_NO_SOURCE_CODE
        );
        assert_eq!(
            classify_worker_error("SPOTIFLAC_REGION_BLOCKED", "x").code,
            SPOTIFLAC_REGION_BLOCKED_CODE
        );
        assert_eq!(
            classify_worker_error("SPOTIFLAC_CHALLENGE_TIMEOUT", "x").code,
            SPOTIFLAC_CHALLENGE_TIMEOUT_CODE
        );
        assert_eq!(classify_worker_error("SOMETHING_NEW", "x").code, "INTERNAL");
    }

    #[test]
    fn a_crash_with_network_stderr_is_retryable_everything_else_is_internal() {
        assert_eq!(
            classify_worker_crash("ssl: Connection reset by peer", Some(1)).code,
            NETWORK_ERROR_CODE
        );
        assert_eq!(
            classify_worker_crash("ModuleNotFoundError: SpotiFLAC", Some(1)).code,
            "INTERNAL"
        );
    }

    #[test]
    fn download_args_carry_every_parameter() {
        let params = MusicDownloadParams {
            track_url: "https://open.spotify.com/track/x".to_string(),
            output_dir: "/tmp/out".to_string(),
            services: "qobuz,tidal".to_string(),
            tier: "flac24".to_string(),
            extensions_fallback: false,
            tg_bot_token: String::new(),
            tg_chat_id: String::new(),
        };
        let args = build_download_args(&params);
        assert_eq!(args[0], "download");
        assert!(args.contains(&"--no-extensions-fallback".to_string()));
        let services_pos = args.iter().position(|a| a == "--services").unwrap();
        assert_eq!(args[services_pos + 1], "qobuz,tidal");
        let tier_pos = args.iter().position(|a| a == "--tier").unwrap();
        assert_eq!(args[tier_pos + 1], "flac24");
    }

    #[test]
    fn a_worker_speaking_another_protocol_version_is_refused_by_its_hello() {
        // Bump module mà quên cập nhật một trong hai phía là cách hỏng âm
        // thầm nhất: worker vẫn chạy, vẫn in ra thứ gì đó, và job chỉ chết
        // giữa chừng với một lỗi không liên quan. Event `hello` là chốt chặn.
        let matching = r#"SPOTIFLAC_EVENT::{"type":"hello","protocol":1,"module_version":"1.5.5"}"#;
        let mismatched = r#"SPOTIFLAC_EVENT::{"type":"hello","protocol":2,"module_version":"2.0.0"}"#;

        for (line, expected) in [(matching, SUPPORTED_PROTOCOL), (mismatched, 2)] {
            match parse_worker_line(line) {
                Some(WorkerLine::Event(WorkerEvent::Hello { protocol, .. })) => {
                    assert_eq!(protocol, expected);
                    // Chính điều kiện mà `run_music_preview`/`run_music_download`
                    // dùng để giết worker và báo "rebuild binaries".
                    assert_eq!(
                        protocol != SUPPORTED_PROTOCOL,
                        expected != SUPPORTED_PROTOCOL
                    );
                }
                other => panic!("hello phải parse được, got {other:?}"),
            }
        }
    }

    #[test]
    fn telegram_envs_require_both_halves_of_the_pair() {
        let mut params = MusicDownloadParams {
            track_url: String::new(),
            output_dir: String::new(),
            services: "tidal".to_string(),
            tier: "flac16".to_string(),
            extensions_fallback: true,
            tg_bot_token: "123:abc".to_string(),
            tg_chat_id: String::new(),
        };
        assert!(telegram_envs(&params).is_empty(), "token mà thiếu chat_id = bot câm");

        params.tg_chat_id = "42".to_string();
        let envs = telegram_envs(&params);
        assert_eq!(envs.len(), 2);
        assert!(envs.iter().any(|(k, v)| k == "TG_BOT_TOKEN" && v == "123:abc"));
        assert!(envs.iter().any(|(k, v)| k == "TG_CHAT_ID" && v == "42"));
    }
}
