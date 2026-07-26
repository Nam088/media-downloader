//! Lệnh riêng của engine SpotiFLAC: vòng round-trip Cloudflare grant code
//! (FR-007, contracts/tauri-interface.md §2).

use tauri::State;

use crate::downloader::queue::DownloadQueue;
use crate::error::AppError;

/// Bơm grant code người dùng vừa nhập xuống worker đang giữ challenge.
///
/// Không trả về trạng thái mới: grant đúng hay sai chỉ lộ ra khi worker chạy
/// tiếp (job về `downloading` qua `job:status_changed`) hay phát lại challenge
/// (`job:cloudflare_challenge`) — giao diện chờ sự kiện, không chờ giá trị trả
/// về của lệnh này.
#[tauri::command]
pub async fn submit_cloudflare_grant(
    queue: State<'_, DownloadQueue>,
    job_id: String,
    grant: String,
) -> Result<(), AppError> {
    queue.submit_cloudflare_grant(&job_id, &grant).await
}

/// Challenge đang chờ của một job, để hộp thoại dựng lại được sau khi giao
/// diện tải lại. `None` nghĩa là không còn gì để nhập (grant đã được nhận,
/// job đã bị huỷ, hoặc worker đã chết).
#[tauri::command]
pub async fn get_pending_challenge(
    queue: State<'_, DownloadQueue>,
    job_id: String,
) -> Result<Option<PendingChallengeView>, AppError> {
    Ok(queue
        .pending_challenge(&job_id)
        .await
        .map(|challenge_url| PendingChallengeView { challenge_url }))
}

#[derive(Debug, serde::Serialize)]
pub struct PendingChallengeView {
    pub challenge_url: String,
}
