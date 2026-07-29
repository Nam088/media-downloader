use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use chrono::Utc;
use tauri::{AppHandle, Emitter};
use tokio::sync::{watch, Mutex as AsyncMutex, Notify};

use crate::db::Db;
use crate::error::{AppError, CANCELED_ERROR_CODE};
use crate::logging::log_event;
use crate::models::{
    AudioOutput, CodecPreference, DownloadJob, GalleryMode, JobStatus, MediaType, NewLibraryFile,
    SegmentMode, SubtitleDelivery, TrimRange, VideoContainer,
};

use super::filename;
use super::gallery_dl;
use super::retry::{decide_outcome, Outcome};
use super::scheduler::{available_slots, TICK_INTERVAL_MS};
use super::ytdlp;
use super::ytdlp_binary;

/// Upper bound on attempts for the audio-only recovery fetch itself (see
/// `recover_missing_audio`). It used to be a single, unretried shot: once
/// `MAX_NO_AUDIO_ATTEMPTS` was exhausted on the main video download, the
/// "last resort" audio-only request got exactly one try at the very same
/// intermittent source that had just failed 3 times in a row, so it carried
/// no better odds than one more plain retry would have. A separate request
/// for just the audio track is cheap (seconds, not minutes), so retrying it
/// too costs little and gives the intermittency a few more independent
/// chances to land on a working response.
const MAX_AUDIO_RECOVERY_ATTEMPTS: u32 = 3;

/// Delay between "no audio" retries of the main download (see
/// `MAX_NO_AUDIO_ATTEMPTS`). The loop used to retry with zero delay,
/// back-to-back against the same source in well under a second — plausible
/// for a purely random per-request inconsistency, but if the CDN/extraction
/// glitch is instead tied to a short-lived edge node or cached response, a
/// few seconds of real gap gives the next attempt a better chance of
/// actually hitting different upstream state instead of the same one.
const NO_AUDIO_RETRY_DELAY_SECS: u64 = 3;

/// Upper bound on redownload attempts when the output has no audio track
/// (see `ytdlp::output_has_audio_stream`). Originally documented against
/// yt-dlp issue #15891 (TikTok's CDN can intermittently serve a video-only
/// file under a format id whose metadata still claims `acodec=aac`), but the
/// same symptom is now also confirmed on YouTube (yt-dlp issues #16128,
/// #12482): YouTube's rollout of SABR streaming means which formats a
/// request actually gets back — DASH audio-only tracks included or not — can
/// vary between otherwise-identical requests for the same video. In both
/// cases maintainers confirmed re-requesting commonly gets a different,
/// correct result, so a couple of retries is a real fix here, not just a
/// delay.
///
/// Đây là cơ chế riêng, KHÔNG phải retry vì lỗi tạm thời: lỗi ở đây là một
/// lần tải "thành công" nhưng cho ra file thiếu tiếng, nên không có mã lỗi nào
/// để bộ điều phối nhìn thấy. Retry vì lỗi mạng thuộc về `finish_job` +
/// `downloader::retry`.
const MAX_NO_AUDIO_ATTEMPTS: u32 = 3;

/// Lỗi báo hiệu người dùng đã chủ động dừng. `finish_job` nhận ra mã này và
/// không đụng gì tới job cả — `pause`/`cancel` đã đặt trạng thái cuối cùng.
fn canceled(during: &str) -> AppError {
    AppError::new(CANCELED_ERROR_CODE, format!("Job canceled during {during}"))
}

#[derive(Clone, serde::Serialize)]
struct JobProgressEvent {
    job_id: String,
    /// `null` when the source reported no total size (audio-only formats,
    /// HLS), i.e. the percentage is genuinely unknown — see
    /// `ytdlp::ProgressUpdate::percent`. The frontend renders an
    /// indeterminate bar plus `downloaded_bytes`/`speed_bytes_per_sec` in
    /// that case instead of a "0%" that would be a lie.
    progress_percent: Option<f64>,
    /// Carried so the UI has a true number to show when there is no
    /// percentage. Never persisted — the queue table has no column for it,
    /// and it only means anything for a run that is currently in flight.
    downloaded_bytes: Option<i64>,
    speed_bytes_per_sec: Option<i64>,
    eta_seconds: Option<i64>,
}

#[derive(Clone, serde::Serialize)]
struct JobStatusChangedEvent {
    job_id: String,
    status: String,
    error_message: Option<String>,
    /// Only set when `status = completed` — lets the frontend show the
    /// output path immediately without a separate `list_history` round-trip
    /// (that command is added later, in User Story 3).
    output_file_path: Option<String>,
    /// FR-227: số file mà lần chạy này tạo ra — file gốc CỘNG một file cho mỗi
    /// chương khi tác vụ bật tách chương. Chỉ có giá trị cho đúng những tác vụ
    /// ấy; `None` nghĩa là "một file như mọi khi", không phải "không có file
    /// nào".
    ///
    /// Là một con số trên MỘT sự kiện của MỘT tác vụ, chứ không phải N tác vụ
    /// mới: FR-227 nói rõ một lần tách chương vẫn phải hiện thành đúng một mục
    /// trong hàng đợi và lịch sử.
    produced_file_count: Option<u32>,
}

/// Một lần chạy cụ thể của một job, kèm tín hiệu huỷ để `pause_job`/
/// `cancel_job` (T034) dừng được tiến trình yt-dlp bên dưới. yt-dlp không tạm
/// dừng giữa chừng được theo cách chạy trên mọi hệ điều hành (Windows không có
/// SIGSTOP), nên "tạm dừng" được hiện thực là: giết tiến trình, giữ
/// `status = paused`, và `resume_job` gọi lại yt-dlp với `--continue` trên
/// đúng file `.part` dang dở.
///
/// `run_id` tồn tại để sửa một lỗi tranh chấp thật: khi người dùng tạm dừng
/// rồi tiếp tục rất nhanh, task của lần chạy cũ có thể kết thúc *sau* khi lần
/// chạy mới đã đăng ký. Nếu nó dọn dẹp chỉ theo `job_id` thì sẽ xoá nhầm handle
/// huỷ của lần chạy mới, và job đó không còn tạm dừng hay huỷ được nữa; tệ hơn,
/// một lỗi của lần chạy cũ sẽ ghi đè trạng thái của lần chạy mới. Kết quả chỉ
/// được thi hành khi `run_id` khớp với lần chạy đang đăng ký (FR-125).
struct RunningJob {
    cancel_tx: watch::Sender<bool>,
    run_id: u64,
}


pub struct DownloadQueue {
    db: Arc<Db>,
    app: AppHandle,
    running: Arc<AsyncMutex<HashMap<String, RunningJob>>>,
    /// Đọc lại mỗi vòng dispatch nên người dùng đổi số luồng là có hiệu lực
    /// ngay, không cần dựng lại hàng đợi (FR-113).
    max_concurrent: Arc<AtomicUsize>,
    /// Đánh thức dispatcher khi có việc mới, để không phải đợi hết nhịp tick.
    wake: Arc<Notify>,
    /// Tên gốc (thư mục + stem, chưa phần mở rộng) đang được MỘT job còn
    /// đang chạy giữ chỗ — xem chú thích ở `claim_output_stem`.
    claimed_stems: Arc<StdMutex<HashSet<String>>>,
}

/// Bản sao các handle dùng chung, để task nền không phải giữ `&DownloadQueue`.
#[derive(Clone)]
struct QueueHandles {
    db: Arc<Db>,
    app: AppHandle,
    running: Arc<AsyncMutex<HashMap<String, RunningJob>>>,
    max_concurrent: Arc<AtomicUsize>,
    wake: Arc<Notify>,
    claimed_stems: Arc<StdMutex<HashSet<String>>>,
}

impl DownloadQueue {
    pub fn new(db: Arc<Db>, app: AppHandle, max_concurrent: usize) -> Self {
        let queue = Self {
            db,
            app,
            running: Arc::new(AsyncMutex::new(HashMap::new())),
            max_concurrent: Arc::new(AtomicUsize::new(max_concurrent.clamp(1, 8))),
            wake: Arc::new(Notify::new()),
            claimed_stems: Arc::new(StdMutex::new(HashSet::new())),
        };
        queue.spawn_dispatcher();
        queue
    }


    /// Người dùng đổi số luồng trong Cài đặt. Đánh thức dispatcher ngay để
    /// việc tăng số luồng có hiệu lực tức thì thay vì đợi hết nhịp tick.
    pub fn set_max_concurrent(&self, value: usize) {
        self.max_concurrent
            .store(value.clamp(1, 8), Ordering::Relaxed);
        self.wake.notify_one();
    }

    fn handles(&self) -> QueueHandles {
        QueueHandles {
            db: Arc::clone(&self.db),
            app: self.app.clone(),
            running: Arc::clone(&self.running),
            max_concurrent: Arc::clone(&self.max_concurrent),
            wake: Arc::clone(&self.wake),
            claimed_stems: Arc::clone(&self.claimed_stems),
        }
    }

    /// Task duy nhất quyết định job nào được chạy. Thức dậy theo nhịp tick
    /// hoặc khi được đánh thức, rồi khởi chạy tối đa số job mà slot cho phép.
    ///
    /// Nhịp tick là bắt buộc chứ không thừa: job đang chờ thử lại đến hạn theo
    /// đồng hồ và không có ai gọi `wake` hộ nó.
    ///
    /// Dùng `tauri::async_runtime::spawn` chứ không phải `tokio::spawn`: hàm
    /// này được gọi từ `setup()` của Tauri, nơi không có runtime tokio nào
    /// đang "vào ngữ cảnh" nên `tokio::spawn` sẽ panic.
    fn spawn_dispatcher(&self) {
        let handles = self.handles();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::select! {
                    _ = handles.wake.notified() => {}
                    _ = tokio::time::sleep(std::time::Duration::from_millis(TICK_INTERVAL_MS)) => {}
                }
                if let Err(err) = dispatch_ready(&handles).await {
                    log_event(
                        &handles.app,
                        "WARN",
                        format!("Dispatcher tick failed: {err}"),
                    );
                }
            }
        });
    }

    /// Ghi job vào cuối hàng đợi rồi đánh thức dispatcher. Không tự chạy gì
    /// cả — việc quyết định khi nào chạy hoàn toàn thuộc về dispatcher.
    ///
    /// Nhận `&mut` để trả lại cho phía gọi đúng bản ghi đã được lưu: phía gọi
    /// gửi thẳng job này về giao diện, nên `queue_position` mà nó thấy phải là
    /// vị trí thật trong DB chứ không phải giá trị 0 lúc dựng job.
    ///
    /// Việc gán `next_queue_position()` ở đây là bất biến then chốt của cả cơ
    /// chế sắp thứ tự: nếu job mới cứ nằm ở 0.0 thì mọi job mới đều hoà nhau và
    /// đứng trước toàn bộ hàng đợi hiện có.
    pub async fn enqueue(&self, job: &mut DownloadJob) -> Result<(), AppError> {
        job.queue_position = self.db.next_queue_position()?;
        job.status = JobStatus::Queued;
        self.db.insert_job(job)?;
        emit_status_changed(&self.app, &job.id, JobStatus::Queued, None, None);
        self.wake.notify_one();
        Ok(())
    }

    /// Dừng một job đang chạy hoặc đang chờ. `to_status` là trạng thái cuối
    /// cùng (`Paused` hay `Canceled`).
    ///
    /// Gửi tín hiệu huỷ khiến `tokio::select!` trong `run_job` thắng, tiến
    /// trình con bị drop, và `kill_on_drop(true)` giết nó. Với job còn đang chờ
    /// trong DB thì không có gì để giết — chỉ cần đổi trạng thái là dispatcher
    /// sẽ không chọn nó nữa.
    ///
    /// Cố ý KHÔNG gỡ handle khỏi bảng `running`: chủ sở hữu duy nhất của một
    /// entry là chính task của lần chạy đó (`finish_job`). Gỡ ở đây sẽ drop
    /// `cancel_tx` và làm `run_job` mất luôn khả năng quan sát tín hiệu.
    async fn stop_job(&self, job_id: &str, to_status: JobStatus) -> Result<(), AppError> {
        if let Some(entry) = self.running.lock().await.get(job_id) {
            let _ = entry.cancel_tx.send(true);
        }
        // Xoá mốc chờ thử lại: người dùng đã can thiệp thủ công nên vòng thử
        // lại tự động phải dừng hẳn (FR-123).
        self.db.clear_retry_deadline(job_id)?;
        self.db.update_job_status(job_id, to_status.clone(), None)?;
        emit_status_changed(&self.app, job_id, to_status, None, None);
        self.wake.notify_one();
        Ok(())
    }

    pub async fn cancel(&self, job_id: &str) -> Result<(), AppError> {
        self.stop_job(job_id, JobStatus::Canceled).await
    }

    pub async fn pause(&self, job_id: &str) -> Result<(), AppError> {
        self.stop_job(job_id, JobStatus::Paused).await
    }

    /// Đưa job đã tạm dừng về lại hàng chờ. Không tự chạy — dispatcher lo.
    /// Giữ nguyên `queue_position` nên job quay lại đúng chỗ cũ trong hàng đợi.
    pub async fn resume(&self, job_id: &str) -> Result<(), AppError> {
        let job = self
            .db
            .get_job(job_id)?
            .ok_or_else(|| AppError::not_found("Job"))?;
        if job.status != JobStatus::Paused {
            return Err(AppError::new(
                "INVALID_JOB_STATE",
                "Only a paused job can be resumed",
            ));
        }
        self.db.update_job_status(job_id, JobStatus::Queued, None)?;
        emit_status_changed(&self.app, job_id, JobStatus::Queued, None, None);
        self.wake.notify_one();
        Ok(())
    }

    /// Creates a brand-new job that repeats a failed/canceled one, keeping
    /// `retried_from_job_id` pointing at the original (data-model.md §1) —
    /// the old row is left untouched in history, matching FR-006.
    pub async fn retry(&self, job_id: &str) -> Result<DownloadJob, AppError> {
        let original = self
            .db
            .get_job(job_id)?
            .ok_or_else(|| AppError::not_found("Job"))?;

        let now = Utc::now().to_rfc3339();
        let mut retried = DownloadJob {
            id: uuid::Uuid::new_v4().to_string(),
            source_url: original.source_url,
            platform: original.platform,
            media_type: original.media_type,
            audio_quality: original.audio_quality,
            video_quality: original.video_quality,
            gallery_mode: original.gallery_mode,
            selected_gallery_indices: original.selected_gallery_indices,
            status: JobStatus::Queued,
            progress_percent: 0.0,
            speed_bytes_per_sec: None,
            eta_seconds: None,
            error_message: None,
            output_directory: original.output_directory,
            output_file_path: None,
            is_playlist_item: original.is_playlist_item,
            parent_playlist_id: original.parent_playlist_id,
            retried_from_job_id: Some(job_id.to_string()),
            created_at: now.clone(),
            updated_at: now,
            title: original.title,
            playlist_title: original.playlist_title,
            // `enqueue` ghi đè bằng vị trí cuối hàng đợi thật sự.
            queue_position: 0.0,
            // Thử lại thủ công tạo ra một job MỚI chạy lại từ đầu, nên bộ đếm
            // tự-thử-lại bắt đầu lại từ 0 thay vì kế thừa của job cũ.
            retry_count: 0,
            next_retry_at: None,
            // FR-235: thử lại phải tái tạo đúng cấu hình đầu ra của bản gốc,
            // không phải cấu hình đang hiển thị trên màn hình lúc bấm.
            output_options: original.output_options,
        };

        self.enqueue(&mut retried).await?;
        Ok(retried)
    }

    /// Tạm dừng mọi tác vụ chưa kết thúc (FR-118).
    pub async fn pause_all(&self) -> Result<Vec<String>, AppError> {
        self.apply_bulk(bulk_plan(BulkAction::Pause)).await
    }

    /// Đưa mọi tác vụ đang tạm dừng về hàng chờ, giữ nguyên thứ tự cũ —
    /// `queue_position` không bị đụng tới nên hàng đợi chạy tiếp đúng chỗ cũ.
    pub async fn resume_all(&self) -> Result<Vec<String>, AppError> {
        self.apply_bulk(bulk_plan(BulkAction::Resume)).await
    }

    /// Huỷ mọi tác vụ chưa kết thúc, kể cả những tác vụ đang tạm dừng.
    pub async fn cancel_all(&self) -> Result<Vec<String>, AppError> {
        self.apply_bulk(bulk_plan(BulkAction::Cancel)).await
    }

    /// Phần thi hành dùng chung của ba lệnh hàng loạt. Mọi khác biệt giữa
    /// chúng nằm trong `BulkPlan` (hàm thuần `bulk_plan`), nên hàm này chỉ còn
    /// vào-ra: bảng `running`, DB, sự kiện.
    ///
    /// Trả về id của những job bị tác động để phía gọi phát `job:status_changed`
    /// cho đúng chừng đó job — giao diện không phải nạp lại cả hàng đợi.
    async fn apply_bulk(&self, plan: BulkPlan) -> Result<Vec<String>, AppError> {
        // Thứ tự này là bắt buộc: gửi tín hiệu huỷ cho mọi job đang chạy TRƯỚC
        // khi ghi trạng thái mới. Làm ngược lại thì giữa hai bước, dispatcher
        // kịp thấy một slot vừa trống và khởi chạy một job vốn đang chờ — job
        // đó vừa bị đánh dấu `paused` xong sẽ chạy bất chấp.
        if plan.stops_jobs {
            for entry in self.running.lock().await.values() {
                let _ = entry.cancel_tx.send(true);
            }
        }

        let changed = self
            .db
            .bulk_update_status(&plan.from_statuses, plan.to_status.clone())?;

        for job_id in &changed {
            // Một lần dừng hàng loạt phải giống hệt N lần dừng đơn lẻ
            // (`stop_job`): người dùng đã can thiệp thủ công nên vòng thử lại
            // tự động phải dừng hẳn và job được nhận lại đủ ngân sách thử lại
            // (FR-123). Thiếu bước này, một job đang trong khoảng chờ thử lại
            // vẫn còn `next_retry_at` và sẽ tự chạy lại dù người dùng vừa tạm
            // dừng tất cả.
            if plan.stops_jobs {
                self.db.clear_retry_deadline(job_id)?;
            }
            emit_status_changed(&self.app, job_id, plan.to_status.clone(), None, None);
        }

        self.wake.notify_one();
        Ok(changed)
    }

    /// Đặt một job vào giữa hai hàng xóm mới của nó sau một lần kéo-thả
    /// (FR-117). Không đụng tới job đang chạy — chúng cứ chạy nốt, thứ tự chỉ
    /// quyết định ai được khởi chạy tiếp theo.
    pub fn move_job(
        &self,
        job_id: &str,
        before_job_id: Option<&str>,
        after_job_id: Option<&str>,
    ) -> Result<(), AppError> {
        self.db
            .move_job_between(job_id, before_job_id, after_job_id)?;
        // Thứ tự mới có thể đưa một job khác lên đầu hàng chờ; đánh thức
        // dispatcher để nó chọn lại ngay thay vì đợi hết nhịp tick.
        self.wake.notify_one();
        Ok(())
    }
}

/// Ba lệnh tác động lên cả hàng đợi. Tách khỏi phần thi hành để mọi quyết
/// định của chúng nằm trong một hàm thuần, kiểm thử được mà không cần
/// `AppHandle` hay database.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BulkAction {
    Pause,
    Resume,
    Cancel,
}

/// Toàn bộ khác biệt giữa ba lệnh hàng loạt.
#[derive(Debug, PartialEq, Eq)]
struct BulkPlan {
    /// Chỉ những job đang ở một trong các trạng thái này mới bị tác động.
    from_statuses: Vec<JobStatus>,
    to_status: JobStatus,
    /// Đây có phải một thao tác *dừng* (tạm dừng/huỷ) không. Thao tác dừng
    /// phải giết tiến trình của job đang chạy và trả lại ngân sách thử lại,
    /// đúng như `stop_job` làm cho một job đơn lẻ.
    stops_jobs: bool,
}

/// Mọi trạng thái "chưa kết thúc" — tập hợp mà các lệnh hàng loạt được phép
/// đụng tới. `completed`/`failed`/`canceled` là trạng thái cuối: một lệnh hàng
/// loạt không được lôi chúng trở lại hàng đợi.
const UNFINISHED_STATUSES: [JobStatus; 4] = [
    JobStatus::Queued,
    JobStatus::FetchingMetadata,
    JobStatus::Downloading,
    JobStatus::Paused,
];

/// Mọi trạng thái chưa kết thúc TRỪ chính trạng thái đích.
///
/// Loại trạng thái đích ra không phải chuyện làm đẹp: `Db::bulk_update_status`
/// trả về id của những job **khớp** điều kiện chứ không phải những job thực sự
/// đổi trạng thái. Để trạng thái đích lọt vào danh sách nguồn thì mỗi job vốn
/// đã ở đúng trạng thái đó cũng bị coi là "vừa đổi": giao diện nhận sự kiện
/// thừa, và với một thao tác dừng thì bộ đếm thử lại của nó bị xoá oan.
fn unfinished_statuses_except(to_status: &JobStatus) -> Vec<JobStatus> {
    UNFINISHED_STATUSES
        .iter()
        .filter(|status| *status != to_status)
        .cloned()
        .collect()
}

fn bulk_plan(action: BulkAction) -> BulkPlan {
    match action {
        BulkAction::Pause => BulkPlan {
            from_statuses: unfinished_statuses_except(&JobStatus::Paused),
            to_status: JobStatus::Paused,
            stops_jobs: true,
        },
        // Cố tình KHÔNG dùng `unfinished_statuses_except`: chỉ job đang tạm
        // dừng mới được tiếp tục. Nếu `downloading` lọt vào đây thì một job
        // đang chạy sẽ bị đánh dấu `queued` và dispatcher khởi chạy nó lần thứ
        // hai song song với chính nó.
        BulkAction::Resume => BulkPlan {
            from_statuses: vec![JobStatus::Paused],
            to_status: JobStatus::Queued,
            stops_jobs: false,
        },
        BulkAction::Cancel => BulkPlan {
            from_statuses: unfinished_statuses_except(&JobStatus::Canceled),
            to_status: JobStatus::Canceled,
            stops_jobs: true,
        },
    }
}

/// Khởi chạy job cho tới khi hết slot hoặc hết job đủ điều kiện.
///
/// Chạy tuần tự trong đúng một task nên không cần khoá gì thêm: giữa lúc chọn
/// job và lúc đánh dấu nó `downloading` không có ai khác xen vào chọn trùng.
async fn dispatch_ready(handles: &QueueHandles) -> Result<(), AppError> {
    loop {
        let running_count = handles.running.lock().await.len();
        if available_slots(running_count, &handles.max_concurrent) == 0 {
            return Ok(());
        }

        let now = Utc::now().to_rfc3339();
        let Some(job) = handles.db.next_dispatchable_job(&now)? else {
            return Ok(());
        };

        start_job(handles, job).await?;
    }
}

/// Chuyển một job từ hàng chờ sang đang chạy: đánh dấu trạng thái, đăng ký
/// handle huỷ, rồi spawn task thực thi.
async fn start_job(handles: &QueueHandles, job: DownloadJob) -> Result<(), AppError> {
    let job_id = job.id.clone();
    // Giữ lại nhãn để dòng log lúc thất bại vẫn nói được job đó là link nào —
    // `job` bị chuyển quyền sở hữu vào task ngay bên dưới.
    let log_label = format!("{} — {}", job.platform, job.source_url);
    let (cancel_tx, cancel_rx) = watch::channel(false);
    let run_id = next_run_id();

    // Đánh dấu `downloading` TRƯỚC khi spawn, vì `next_dispatchable_job` chỉ
    // lọc theo `status = 'queued'` — nếu để task tự đánh dấu, vòng dispatch kế
    // tiếp (cách đây tối đa một nhịp tick) sẽ chọn lại đúng job này và chạy nó
    // lần thứ hai song song với chính nó.
    //
    // Truyền `None` cho `error_message` cũng là chủ ý: nó xoá thông báo lỗi mà
    // `mark_job_for_retry` để lại, nên lần chạy mới không hiển thị lý do thất
    // bại của lần trước.
    handles
        .db
        .update_job_status(&job_id, JobStatus::Downloading, None)?;
    emit_status_changed(&handles.app, &job_id, JobStatus::Downloading, None, None);

    handles
        .running
        .lock()
        .await
        .insert(
            job_id.clone(),
            RunningJob {
                cancel_tx,
                run_id,
            },
        );

    let task_handles = handles.clone();
    let task_job_id = job_id.clone();
    tokio::spawn(async move {
        let outcome = run_job(&task_handles, job, cancel_rx).await;
        finish_job(&task_handles, &task_job_id, run_id, &log_label, outcome).await;
        // Slot vừa trống — báo dispatcher biết ngay thay vì đợi hết nhịp tick.
        task_handles.wake.notify_one();
    });

    Ok(())
}

/// Bộ đếm lần chạy, chỉ dùng để phân biệt các lần chạy của cùng một job.
fn next_run_id() -> u64 {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

/// Kết quả của lần chạy `run_id` có còn được phép thi hành không, dựa trên
/// `run_id` đang được đăng ký trong bảng `running` cho job đó.
///
/// `None` (không còn entry nào) nghĩa là chưa có lần chạy nào khác chiếm chỗ,
/// nên kết quả vẫn còn giá trị. `Some(khác)` nghĩa là một lần chạy mới đã đăng
/// ký trong lúc lần chạy này đang kết thúc: kết quả đã lỗi thời và thi hành nó
/// sẽ ghi đè trạng thái của lần chạy đang thực sự diễn ra.
fn is_current_run(registered_run_id: Option<u64>, run_id: u64) -> bool {
    match registered_run_id {
        Some(registered) => registered == run_id,
        None => true,
    }
}

/// Xử lý kết quả một lần chạy: hoàn tất, thất bại vĩnh viễn, hay xếp lại hàng
/// để thử lại. Đây là nơi DUY NHẤT quyết định có thử lại hay không — `run_job`
/// chỉ chạy đúng một lần rồi trả lỗi ra ngoài.
async fn finish_job(
    handles: &QueueHandles,
    job_id: &str,
    run_id: u64,
    log_label: &str,
    outcome: Result<(), AppError>,
) {
    let is_current = {
        let mut running = handles.running.lock().await;
        let registered = running.get(job_id).map(|entry| entry.run_id);
        let is_current = is_current_run(registered, run_id);
        if is_current {
            running.remove(job_id);
        }
        is_current
    };
    if !is_current {
        return;
    }

    let Err(err) = outcome else {
        return; // `run_job` đã tự đánh dấu hoàn tất và phát sự kiện.
    };

    let max_retries = handles
        .db
        .get_settings()
        .map(|settings| settings.max_retry_attempts as i64)
        .unwrap_or(3);
    let retry_count = handles
        .db
        .get_job(job_id)
        .ok()
        .flatten()
        .map(|job| job.retry_count)
        .unwrap_or(0);

    match decide_outcome(&err.code, retry_count, max_retries) {
        Outcome::Ignore => return,
        Outcome::Retry { delay_seconds } => {
            let next_retry_at =
                (Utc::now() + chrono::Duration::seconds(delay_seconds as i64)).to_rfc3339();
            log_event(
                &handles.app,
                "WARN",
                format!(
                    "Job {job_id} ({log_label}) failed with {}, retrying in {delay_seconds}s: {}",
                    err.code, err.message
                ),
            );
            if handles
                .db
                .mark_job_for_retry(job_id, &next_retry_at, &err.message)
                .is_ok()
            {
                // Cố ý đi qua đúng cái chốt mà nhánh thất bại vĩnh viễn dùng,
                // với trạng thái thật của job (`Queued`): `notification_for`
                // trả `None` cho nó, nên một lần thử lại KHÔNG bắn thông báo.
                // Gọi ở đây thay vì im lặng bỏ qua để quy tắc đó nằm trong một
                // hàm thuần kiểm thử được, chứ không nằm ở chỗ thiếu một dòng.
                crate::notify::notify_job_finished(
                    &handles.app,
                    &JobStatus::Queued,
                    log_label,
                    Some(&err.message),
                );
                emit_status_changed(
                    &handles.app,
                    job_id,
                    JobStatus::Queued,
                    Some(err.message),
                    None,
                );
                return;
            }
            // Không ghi được lịch thử lại thì rơi xuống nhánh thất bại bên
            // dưới: bỏ qua sẽ để job kẹt ở `downloading` vĩnh viễn.
        }
        Outcome::Fail => {}
    }

    log_event(
        &handles.app,
        "ERROR",
        format!(
            "Job {job_id} failed ({log_label}) [{}]: {}",
            err.code, err.message
        ),
    );
    let _ = handles
        .db
        .update_job_status(job_id, JobStatus::Failed, Some(&err.message));
    // Thất bại vĩnh viễn: mọi lần thử lại đã dùng hết hoặc lỗi không đáng thử
    // lại. Đây là lúc người dùng cần biết, nhất là khi họ đã đóng cửa sổ đi
    // làm việc khác (FR-128).
    crate::notify::notify_job_finished(
        &handles.app,
        &JobStatus::Failed,
        log_label,
        Some(&err.message),
    );
    emit_status_changed(&handles.app, job_id, JobStatus::Failed, Some(err.message), None);
}

/// Chạy một job đúng MỘT lần. Thất bại được trả ra ngoài cho `finish_job`
/// quyết định — không còn vòng `for attempt` nào ở đây, vì một job đang chờ
/// thử lại phải là một dòng dữ liệu (huỷ được, hiển thị được, sống qua khởi
/// động lại) chứ không phải một task đang ngủ.
///
/// Trạng thái `downloading` đã do `start_job` đặt trước khi spawn, nên hàm này
/// không đặt lại.
async fn run_job(
    handles: &QueueHandles,
    job: DownloadJob,
    mut cancel_rx: watch::Receiver<bool>,
) -> Result<(), AppError> {
    if job.media_type == MediaType::Gallery {
        return run_gallery_job(handles, job, cancel_rx).await;
    }

    // `_stem_claim` isn't read anywhere — it exists purely to stay alive
    // until `run_job` returns (any path: success, `?` error, cancel), so its
    // `Drop` releases the claimed name for the next job. See
    // `claim_output_stem`.
    let (naming, _stem_claim) = resolve_output_naming(handles, &job).await;
    let output_template = naming.main.clone();
    // Đọc lại mỗi lần chạy chứ không cache lúc dựng hàng đợi: người dùng đổi
    // giới hạn tốc độ thì job được khởi chạy sau đó phải dùng giá trị mới.
    let rate_limit_kbps = handles
        .db
        .get_settings()
        .map(|settings| settings.rate_limit_kbps)
        .unwrap_or(0);
    let plan = build_ytdlp_args(&job, rate_limit_kbps)?;
    // FR-210: bước hậu xử lý bị bỏ qua phải nói ra được lý do. Ghi TRƯỚC khi
    // tải chứ không phải lúc kết thúc, để người dùng mở nhật ký giữa chừng vẫn
    // thấy — và để lý do còn đó kể cả khi tác vụ sau đó thất bại vì chuyện khác.
    for note in &plan.skipped {
        log_event(
            &handles.app,
            "INFO",
            format!("Job {} ({}): {note}", job.id, job.source_url),
        );
    }
    let mut extra_args = plan.args;
    // Mẫu tên riêng cho file chương: nếu không truyền, yt-dlp dùng mẫu mặc
    // định của nó (`%(title)s - %(section_number)03d %(section_title)s.%(ext)s`)
    // và mọi công sức làm sạch/chống ghi đè ở trên không áp cho các file kết
    // quả thật sự của một tác vụ tách chương.
    if let Some(chapter_template) = &naming.chapter {
        extra_args.push("-o".into());
        extra_args.push(chapter_template.clone());
    }

    // FR-304: ảnh đại diện phải nằm trên máy để lưới Thư viện hiện được khi
    // không có mạng. Lấy nó ngay tại lần tải này — yt-dlp vừa mới lấy metadata
    // của nguồn nên URL ảnh đang nằm sẵn trong tay nó; hỏi lại sau này nghĩa
    // là một vòng mạng thứ hai cho một link có thể đã hết hạn (CDN của TikTok
    // ký URL ngắn hạn) hoặc đã bị gỡ.
    //
    // Ảnh đi vào thư mục dữ liệu của ứng dụng, KHÔNG vào thư mục tải của người
    // dùng: đây là dữ liệu nội bộ, và rải file `.webp` cạnh mỗi bài nhạc là
    // thứ người dùng không hề yêu cầu. Tách thư mục còn giữ cho phép đếm file
    // chương (`new_chapter_file_names`) không đếm nhầm ảnh vừa ghi ra.
    let thumbnail_dir = thumbnail_dir(&handles.app);
    if let Some(dir) = &thumbnail_dir {
        // `job.id` là UUID nên an toàn tuyệt đối trong một mẫu `-o`; chỉ phần
        // thư mục mới cần escape (yt-dlp đọc `%` ở bất kỳ đâu là mở đầu một
        // trường mẫu).
        extra_args.push("--write-thumbnail".into());
        extra_args.push("-o".into());
        extra_args.push(format!(
            "thumbnail:{}/{}.%(ext)s",
            filename::escape_for_ytdlp_template(&dir.to_string_lossy()),
            job.id
        ));
    }

    let mut output_path: Option<String> = None;
    for attempt in 1..=MAX_NO_AUDIO_ATTEMPTS {
        let job_id_for_progress = job.id.clone();
        let app_for_progress = handles.app.clone();
        let db_for_progress = Arc::clone(&handles.db);

        let download_fut = ytdlp::run_download(
            &handles.app,
            &job.source_url,
            &output_template,
            extra_args.clone(),
            move |update| {
                let _ = db_for_progress.update_job_progress(
                    &job_id_for_progress,
                    update.percent,
                    update.speed_bytes_per_sec,
                    update.eta_seconds,
                );
                emit_progress(&app_for_progress, &job_id_for_progress, &update);
            },
        );

        let download_result = tokio::select! {
            result = download_fut => result,
            _ = cancel_rx.changed() => return Err(canceled("download")),
        };
        let reported_path = download_result?;
        let path = match recover_actual_output_path(&job.output_directory, &reported_path, &naming).await {
            Some(real_path) => {
                if real_path != reported_path {
                    log_event(
                        &handles.app,
                        "WARN",
                        format!(
                            "Job {}: yt-dlp's reported output path didn't exist on disk (likely a Windows console-encoding issue with non-ASCII characters in the title); recovered the real file from disk instead",
                            job.id
                        ),
                    );
                }
                real_path
            }
            None => {
                return Err(AppError::internal(
                    "yt-dlp reported an output path that doesn't exist, and the real file couldn't be found on disk either",
                ));
            }
        };

        // Only muxed video jobs are at risk of a *silent* audio loss: an
        // audio-only extraction (`-x`) fails loudly instead (ffmpeg can't
        // extract a stream that isn't there), so it surfaces as a real error
        // that `finish_job` gets to classify.
        if job.media_type == MediaType::Video && !ytdlp::output_has_audio_stream(&path).await {
            if attempt < MAX_NO_AUDIO_ATTEMPTS {
                log_event(
                    &handles.app,
                    "WARN",
                    format!("Job {} attempt {attempt}/{MAX_NO_AUDIO_ATTEMPTS}: downloaded video had no audio track, retrying in {NO_AUDIO_RETRY_DELAY_SECS}s", job.id),
                );
                let _ = tokio::fs::remove_file(&path).await;
                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_secs(NO_AUDIO_RETRY_DELAY_SECS)) => {}
                    _ = cancel_rx.changed() => return Err(canceled("download")),
                }
                continue;
            }
            // Last attempt still missing audio. Per the community workaround
            // documented on the same yt-dlp issues (TikTok can consistently
            // serve a video-only file for a given format on videos where
            // download is disabled, so retrying the identical request isn't
            // guaranteed to help): separately fetch just the best audio
            // track and mux it onto the otherwise-good video, rather than
            // discarding a mostly-fine download over its audio track alone.
            let mut recovery_result = recover_missing_audio(&handles.app, &job.source_url, &path).await;
            for recovery_attempt in 2..=MAX_AUDIO_RECOVERY_ATTEMPTS {
                let Err(recovery_err) = &recovery_result else {
                    break;
                };
                log_event(
                    &handles.app,
                    "WARN",
                    format!(
                        "Job {} audio recovery attempt {}/{MAX_AUDIO_RECOVERY_ATTEMPTS} failed, retrying: [{}] {}",
                        job.id, recovery_attempt - 1, recovery_err.code, recovery_err.message
                    ),
                );
                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_secs(NO_AUDIO_RETRY_DELAY_SECS)) => {}
                    _ = cancel_rx.changed() => return Err(canceled("download")),
                }
                recovery_result = recover_missing_audio(&handles.app, &job.source_url, &path).await;
            }

            match recovery_result {
                Ok(fixed_path) => {
                    output_path = Some(fixed_path);
                    break;
                }
                Err(recovery_err) => {
                    // No audio could be recovered after every attempt — used
                    // to fail the whole job here. Per user preference, a
                    // video-only file is still a usable result (silent
                    // video beats no video), so this now delivers it as a
                    // completed job instead of an error. The reason is still
                    // logged so it's visible *why* the file has no sound,
                    // rather than silently shipping a mystery-silent file.
                    log_event(
                        &handles.app,
                        "WARN",
                        format!(
                            "Job {} audio recovery failed after {MAX_AUDIO_RECOVERY_ATTEMPTS} attempts ([{}] {}); delivering the video without audio instead of failing the job",
                            job.id, recovery_err.code, recovery_err.message
                        ),
                    );
                    output_path = Some(path);
                    break;
                }
            }
        }

        output_path = Some(path);
        break;
    }
    let output_path = output_path.expect("loop always returns via `?`/cancel or sets output_path");

    let thumbnail_path = thumbnail_dir
        .as_deref()
        .and_then(|dir| written_thumbnail_path(dir, &job.id));
    index_library_file(
        handles,
        &job,
        &output_path,
        library_title(&job, &output_path),
        thumbnail_path,
    )
    .await?;

    // FR-227. Mỗi file chương được ghi thêm vào `downloaded_files` (bảng đã có
    // sẵn, không cần đổi lược đồ) nên số file kết quả còn đó sau khi khởi động
    // lại, trong khi hàng đợi vẫn chỉ có ĐÚNG MỘT dòng `download_jobs` cho cả
    // tác vụ — không fan-out thành N mục rời rạc.
    let produced_file_count = record_chapter_files(handles, &job, &naming).await?;

    handles.db.set_job_output_file(&job.id, &output_path)?;
    handles
        .db
        .update_job_status(&job.id, JobStatus::Completed, None)?;
    // Link nguồn là nhãn dự phòng khi job chưa bao giờ có tiêu đề (ví dụ một
    // mục fan-out từ playlist phẳng): một thông báo "Download complete" trống
    // không nói được vừa xong cái gì khi có nhiều tác vụ cùng chạy.
    crate::notify::notify_job_finished(
        &handles.app,
        &JobStatus::Completed,
        job.title.as_deref().unwrap_or(&job.source_url),
        None,
    );
    emit_completed(&handles.app, &job.id, output_path, produced_file_count);

    Ok(())
}

/// Ghi từng file chương vào `downloaded_files` và trả về tổng số file mà tác
/// vụ này tạo ra (file gốc + mỗi chương một file), hoặc `None` khi tác vụ
/// không tách chương — hoặc khi tên file do chính yt-dlp đặt nên ta không nhận
/// ra file nào là của mình. Một con số đoán mò còn tệ hơn không có số nào.
async fn record_chapter_files(
    handles: &QueueHandles,
    job: &DownloadJob,
    naming: &OutputNaming,
) -> Result<Option<u32>, AppError> {
    let Some(prefix) = naming.chapter_prefix.as_deref() else {
        return Ok(None);
    };

    let after = read_file_names(&job.output_directory).await;
    let mut chapter_names = new_chapter_file_names(&naming.files_before, &after, prefix);
    chapter_names.sort();

    for name in &chapter_names {
        let path = Path::new(&job.output_directory).join(name);
        // FR-302: mỗi chương là một dòng riêng trong Thư viện, tất cả cùng
        // `job_id`. Tiêu đề lấy từ tên file chứ KHÔNG dùng `job.title`: mẫu
        // tên chương đã nhét số thứ tự và tên chương vào đấy, nên tên file là
        // thứ duy nhất phân biệt được chương 3 với chương 4 — dùng tiêu đề
        // của tác vụ sẽ cho ra hai mươi ô trùng tên nhau.
        index_library_file(handles, job, &path.to_string_lossy(), file_stem_title(&path), None)
            .await?;
    }

    log_event(
        &handles.app,
        "INFO",
        format!(
            "Job {}: chapter split produced {} file(s) beside the original",
            job.id,
            chapter_names.len()
        ),
    );
    Ok(Some(chapter_names.len() as u32 + 1))
}

/// Ghi một file kết quả vào chỉ mục Thư viện (FR-301).
///
/// Mọi thứ Thư viện cần được chốt **tại đây, ngay lúc tác vụ hoàn tất**, chứ
/// không hỏi lại sau: đây là thời điểm duy nhất mà file vừa nằm trên đĩa, tác
/// vụ còn nguyên nền tảng/URL/loại nội dung, và ffmpeg còn đáng bỏ ra một lần
/// gọi cho một file. Hỏi lại ở thời điểm hiển thị nghĩa là 10.000 lần gọi
/// ffmpeg khi người dùng mở tab.
async fn index_library_file(
    handles: &QueueHandles,
    job: &DownloadJob,
    file_path: &str,
    title: String,
    thumbnail_path: Option<String>,
) -> Result<(), AppError> {
    let file_size_bytes = tokio::fs::metadata(file_path)
        .await
        .map(|meta| meta.len() as i64)
        .unwrap_or(0);
    let duration_seconds = probe_media_duration_secs(file_path)
        .await
        .map(|secs| secs as i64);

    handles.db.insert_downloaded_file(&NewLibraryFile {
        job_id: job.id.clone(),
        file_path: file_path.to_string(),
        file_format: crate::db::media_file_extension(file_path),
        file_size_bytes,
        title,
        media_type: job.media_type.clone(),
        platform: job.platform.clone(),
        source_url: job.source_url.clone(),
        duration_seconds,
        thumbnail_path,
    })
}

/// Tiêu đề hiển thị của một file kết quả: tiêu đề của tác vụ nếu có, còn
/// không thì tên file.
///
/// Vế thứ hai không phải cho có: một mục fan-out từ playlist phẳng không hề
/// mang tiêu đề (backend chỉ liệt kê được URL), nhưng chính yt-dlp đã đặt tên
/// file từ tiêu đề thật nó lấy được — nên tên file vẫn là tiêu đề, chỉ đi
/// đường vòng. Bỏ trống ở đó sẽ cho ra một lưới đầy ô không tên.
fn library_title(job: &DownloadJob, file_path: &str) -> String {
    job.title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| file_stem_title(Path::new(file_path)))
}

fn file_stem_title(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("")
        .to_string()
}

/// Thư mục chứa ảnh đại diện cục bộ (FR-304), nằm trong thư mục dữ liệu ứng
/// dụng cạnh chính file CSDL.
///
/// `None` khi không xác định được thư mục dữ liệu — hiếm, nhưng khi đó tác vụ
/// vẫn phải tải xong: thiếu ảnh đại diện là một khiếm khuyết hiển thị, không
/// phải lý do để một lần tải thất bại.
fn thumbnail_dir(app: &AppHandle) -> Option<std::path::PathBuf> {
    use tauri::Manager;
    let dir = app.path().app_data_dir().ok()?.join("thumbnails");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

/// Ảnh mà yt-dlp vừa ghi cho tác vụ này, nếu có.
///
/// Phần mở rộng do nguồn quyết định (`.jpg`, `.webp`, `.png`…), nên phải dò
/// theo phần gốc `<job_id>.` thay vì đoán một đuôi cụ thể. `None` là kết quả
/// hoàn toàn bình thường: nguồn không có ảnh, hoặc lần tải ảnh thất bại —
/// yt-dlp chỉ cảnh báo chứ không làm hỏng tác vụ, và Thư viện dùng ảnh thay
/// thế theo loại nội dung.
fn written_thumbnail_path(dir: &Path, job_id: &str) -> Option<String> {
    let prefix = format!("{job_id}.");
    let entries = std::fs::read_dir(dir).ok()?;
    entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .find(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(&prefix))
        })
        .map(|path| path.to_string_lossy().into_owned())
}

/// Phần mở rộng ảnh mà webview hiển thị được trực tiếp — dùng để quyết định
/// một file kết quả của gallery-dl có tự làm ảnh đại diện cho mình được không.
const IMAGE_EXTENSIONS: [&str; 6] = ["jpg", "jpeg", "png", "webp", "gif", "avif"];

fn is_image_file(file_path: &str) -> bool {
    Path::new(file_path)
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .is_some_and(|ext| IMAGE_EXTENSIONS.contains(&ext.as_str()))
}

/// Fallback per-image duration in `GalleryMode::Slideshow`, used only when
/// the audio track's actual length can't be probed (see
/// `probe_media_duration_secs`) — normally each image's display time is the
/// audio's total length divided evenly across the image count, clamped to
/// `[MIN_SLIDESHOW_IMAGE_DURATION_SECS, MAX_SLIDESHOW_IMAGE_DURATION_SECS]`,
/// so the slideshow's pacing actually matches the music instead of a fixed
/// 3s/image cadence that runs short or leaves dead air.
const DEFAULT_SLIDESHOW_IMAGE_DURATION_SECS: f64 = 3.0;
const MIN_SLIDESHOW_IMAGE_DURATION_SECS: f64 = 1.5;
const MAX_SLIDESHOW_IMAGE_DURATION_SECS: f64 = 8.0;

/// gallery-dl-backed equivalent of the yt-dlp path above (`MediaType::Gallery`
/// jobs only) — see `research.md` §2's gallery-dl amendment. Downloads every
/// file gallery-dl finds into a job-exclusive subfolder (gallery-dl's `-D`
/// flag has no per-job namespacing of its own, so a shared folder like the
/// user's chosen Downloads directory would otherwise mix unrelated files
/// together), then applies `job.gallery_mode`.
async fn run_gallery_job(
    handles: &QueueHandles,
    job: DownloadJob,
    mut cancel_rx: watch::Receiver<bool>,
) -> Result<(), AppError> {
    let gallery_mode = job.gallery_mode.clone().ok_or_else(|| {
        AppError::new("MISSING_QUALITY", "gallery_mode is required for gallery downloads")
    })?;

    // Re-dump (cheap: `--no-download`) rather than trusting the cached
    // preview — gives an accurate `total_files` for progress percent and a
    // human-readable folder name, using the exact same data the download
    // itself is about to act on.
    //
    // Giai đoạn này có thể mất vài giây với post nhiều ảnh. Không quan sát tín
    // hiệu huỷ ở đây đồng nghĩa nút Huỷ không có tác dụng gì suốt quãng đó
    // (FR-124) — bỏ future đi cũng drop luôn tiến trình con, và
    // `kill_on_drop(true)` giết nó.
    let dump_result = tokio::select! {
        result = gallery_dl::dump_gallery_json(&handles.app, &job.source_url, |_child| {}) => result,
        _ = cancel_rx.changed() => return Err(canceled("gallery listing")),
    };
    let dump = dump_result?;

    // TikTok's bot-detection can 403 a gallery-dl request outright (confirmed
    // live — same platform-side flakiness already documented for yt-dlp,
    // issues #15891/#15642). Oddly, gallery-dl's `--dump-json` mode treats
    // that as *non-fatal*: it logs the error but still exits 0 with an empty
    // `[]`, while an actual download of the same blocked URL exits with a
    // real error. Nên một dump rỗng phải được coi là lỗi đường truyền, không
    // phải "post này không có gì": nó là đúng cái mà một lần thử lại có cơ hội
    // vượt qua, và giờ lần thử lại đó do bộ điều phối lo — có khoảng chờ tăng
    // dần, huỷ được, và hiện ra trong hàng đợi.
    if dump.entries.is_empty() {
        return Err(AppError::network_error(
            "gallery-dl found nothing for this link — the source may be blocking automated requests",
        ));
    }
    // Narrow to the user's selection (checkbox grid in the gallery preview),
    // if one was made, via gallery-dl's own `--range` (item numbers in its
    // own 1-based crawl order). Matched by *ordinal position*, not URL — see
    // `models::DownloadJob.selected_gallery_indices`'s doc comment for why:
    // a site's own item order for a given, unchanged post is stable across
    // separate crawls even when its per-item URLs aren't (TikTok serves
    // fresh, short-lived, signed CDN URLs every crawl, but the same items in
    // the same order). The audio track's own index is always included
    // regardless of what was selected — this only ever restricts which
    // *images* get fetched; whether audio ends up kept is entirely
    // `gallery_mode`'s call (`AudioOnly`/`Slideshow` need it,
    // `Files`/`ImagesOnly` keep or drop it after the fact).
    let resolved_indices: Option<Vec<usize>> = job.selected_gallery_indices.as_ref().map(|selected| {
        dump.entries
            .iter()
            .enumerate()
            .filter(|(i, entry)| {
                let is_audio = entry.extension.as_deref().map(gallery_dl::is_audio_extension).unwrap_or(false);
                is_audio || selected.contains(&(*i as u32))
            })
            .map(|(i, _)| i)
            .collect()
    });
    // Nothing usable to narrow to (an empty selection would otherwise
    // silently download zero images), or the selection already covers
    // everything — either way, no `--range` restriction at all.
    let resolved_indices =
        resolved_indices.filter(|indices| !indices.is_empty() && indices.len() < dump.entries.len());
    let range: Option<String> = resolved_indices.as_ref().map(|indices| {
        indices
            .iter()
            .map(|i| (i + 1).to_string()) // gallery-dl's --range is 1-based
            .collect::<Vec<_>>()
            .join(",")
    });
    let total_files = resolved_indices.map(|indices| indices.len()).unwrap_or(dump.entries.len()).max(1) as u32;

    let folder_label = dump
        .title
        .clone()
        .unwrap_or_else(|| format!("{} gallery", job.platform));
    let job_dir = format!(
        "{}/{} ({})",
        job.output_directory,
        sanitize_path_component(&folder_label),
        &job.id[..8],
    );
    tokio::fs::create_dir_all(&job_dir).await.map_err(AppError::internal)?;

    let job_id_for_progress = job.id.clone();
    let app_for_progress = handles.app.clone();
    let db_for_progress = Arc::clone(&handles.db);

    let download_fut = gallery_dl::run_gallery_download(
        &handles.app,
        &job.source_url,
        range.as_deref(),
        &job_dir,
        total_files,
        move |update| {
            // Gallery progress is counted by the app itself (files done /
            // files total), so unlike the yt-dlp path it is always known —
            // hence `Some`, never `None`. This is why gallery jobs were the
            // only media type whose stored progress was already always
            // correct.
            let percent = Some((update.completed_files as f64 / update.total_files as f64) * 100.0);
            let _ = db_for_progress.update_job_progress(&job_id_for_progress, percent, None, None);
            emit_progress(
                &app_for_progress,
                &job_id_for_progress,
                &ytdlp::ProgressUpdate {
                    percent,
                    downloaded_bytes: None,
                    speed_bytes_per_sec: None,
                    eta_seconds: None,
                },
            );
        },
    );

    let download_result = tokio::select! {
        result = download_fut => result,
        _ = cancel_rx.changed() => return Err(canceled("gallery download")),
    };
    let downloaded_files = download_result?;

    let (audio_paths, image_paths): (Vec<String>, Vec<String>) =
        downloaded_files.into_iter().partition(|path| gallery_dl::is_audio_file_path(path));

    let output_path = match gallery_mode {
        GalleryMode::Files => job_dir.clone(),
        GalleryMode::AudioOnly => {
            for image_path in &image_paths {
                let _ = tokio::fs::remove_file(image_path).await;
            }
            match audio_paths.as_slice() {
                [] => {
                    return Err(AppError::new(
                        "DOWNLOAD_FAILED",
                        "No audio track was found for this gallery post",
                    ))
                }
                [single] => single.clone(),
                _ => job_dir.clone(),
            }
        }
        GalleryMode::ImagesOnly => {
            for audio_path in &audio_paths {
                let _ = tokio::fs::remove_file(audio_path).await;
            }
            match image_paths.as_slice() {
                [] => {
                    return Err(AppError::new(
                        "DOWNLOAD_FAILED",
                        "No images were found for this gallery post",
                    ))
                }
                [single] => single.clone(),
                _ => job_dir.clone(),
            }
        }
        GalleryMode::Slideshow => {
            if image_paths.is_empty() || audio_paths.is_empty() {
                return Err(AppError::new(
                    "DOWNLOAD_FAILED",
                    "Slideshow mode needs at least one image and one audio track",
                ));
            }
            let merged_path = merge_gallery_slideshow(&job_dir, &image_paths, &audio_paths[0]).await?;
            for path in image_paths.iter().chain(audio_paths.iter()) {
                let _ = tokio::fs::remove_file(path).await;
            }
            merged_path
        }
    };

    // FR-304 cho nhánh gallery-dl: không có bước `--write-thumbnail` nào ở
    // đây, nhưng cũng không cần — với `Files`/`ImagesOnly` thì file kết quả
    // CHÍNH LÀ một tấm ảnh, nên nó tự làm ảnh đại diện cho mình, hoàn toàn
    // ngoại tuyến và không tốn thêm byte nào. Với `AudioOnly`/`Slideshow`
    // (kết quả là mp3/mp4) thì không có ảnh nào còn lại để trỏ vào — ảnh
    // nguồn đã bị xoá sau khi ghép — nên `None`, và lưới dùng ảnh thay thế
    // theo loại nội dung.
    let thumbnail_path = is_image_file(&output_path).then(|| output_path.clone());
    index_library_file(
        handles,
        &job,
        &output_path,
        library_title(&job, &output_path),
        thumbnail_path,
    )
    .await?;
    handles.db.set_job_output_file(&job.id, &output_path)?;
    handles
        .db
        .update_job_status(&job.id, JobStatus::Completed, None)?;
    // Link nguồn là nhãn dự phòng khi job chưa bao giờ có tiêu đề (ví dụ một
    // mục fan-out từ playlist phẳng): một thông báo "Download complete" trống
    // không nói được vừa xong cái gì khi có nhiều tác vụ cùng chạy.
    crate::notify::notify_job_finished(
        &handles.app,
        &JobStatus::Completed,
        job.title.as_deref().unwrap_or(&job.source_url),
        None,
    );
    emit_status_changed(
        &handles.app,
        &job.id,
        JobStatus::Completed,
        None,
        Some(output_path),
    );

    Ok(())
}

/// Strips characters invalid in a filename on at least one of
/// Windows/macOS/Linux, so a post title/caption (which may contain anything)
/// is always safe to use as a folder name.
fn sanitize_path_component(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| if r#"<>:"/\|?*"#.contains(c) || c.is_control() { ' ' } else { c })
        .collect();
    let trimmed = cleaned.trim();
    let truncated: String = trimmed.chars().take(80).collect();
    if truncated.is_empty() {
        "gallery".to_string()
    } else {
        truncated
    }
}

/// Crossfade duration between consecutive images in `GalleryMode::Slideshow`
/// — TikTok's own slideshow posts use a horizontal slide, not a hard cut.
const SLIDESHOW_TRANSITION_SECS: f64 = 0.5;

/// Canvas every image is scaled/padded onto — skips the reference
/// implementation's dynamic first-image-dimension detection (which needs
/// `ffprobe`, a binary this project doesn't bundle) in favor of a fixed
/// default that already matches the near-universal aspect ratio of the
/// slideshow posts this targets.
const SLIDESHOW_CANVAS: (u32, u32) = (1080, 1920);

/// Reads a media file's real duration via ffmpeg's own stderr banner
/// (`  Duration: 00:00:12.34, start: ...`) — no `ffprobe` needed (this
/// project doesn't bundle it). `ffmpeg -i <file>` always prints this line
/// once it's parsed the input, even with no output specified, so this just
/// discards everything ffmpeg would otherwise fail on past that point.
///
/// Dùng cho hai việc: canh nhịp slideshow (theo độ dài bản nhạc) và ghi
/// `duration_seconds` vào chỉ mục Thư viện (FR-301). Dòng banner không phân
/// biệt audio với video, nên cùng một phép đo đúng cho cả hai.
async fn probe_media_duration_secs_with(
    ffmpeg_path: &std::path::Path,
    media_path: &str,
) -> Option<f64> {
    let mut cmd = tokio::process::Command::new(ffmpeg_path);
    cmd.args(["-i", media_path]);
    crate::downloader::hide_cmd_window(&mut cmd);
    let output = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .await
        .ok()?;
    parse_ffmpeg_duration_line(&String::from_utf8_lossy(&output.stderr))
}

/// Như trên nhưng tự tìm lấy ffmpeg. `None` khi không đo được vì bất kỳ lý do
/// gì (không có ffmpeg, file là một tấm ảnh, container lạ) — và `None` ở chỗ
/// gọi mang đúng nghĩa "không biết thời lượng", một câu trả lời hợp lệ mà
/// [`crate::models::LibraryItem`] có sẵn chỗ để chứa.
async fn probe_media_duration_secs(media_path: &str) -> Option<f64> {
    let ffmpeg_path = ytdlp_binary::resolve_ffmpeg_path().ok()?;
    probe_media_duration_secs_with(&ffmpeg_path, media_path).await
}

/// Pure parsing half of `probe_media_duration_secs_with`, split out so it's
/// testable without actually spawning ffmpeg.
fn parse_ffmpeg_duration_line(stderr: &str) -> Option<f64> {
    let line = stderr.lines().find(|l| l.trim_start().starts_with("Duration:"))?;
    let hms = line.trim_start().strip_prefix("Duration:")?.split(',').next()?.trim();
    let mut parts = hms.split(':');
    let hours: f64 = parts.next()?.parse().ok()?;
    let minutes: f64 = parts.next()?.parse().ok()?;
    let seconds: f64 = parts.next()?.parse().ok()?;
    Some(hours * 3600.0 + minutes * 60.0 + seconds).filter(|d| *d > 0.0)
}

/// Divides the audio's real length evenly across the image count, clamped
/// to a sane per-image range — falls back to
/// `DEFAULT_SLIDESHOW_IMAGE_DURATION_SECS` when the audio's duration
/// couldn't be probed at all. `image_count` is assumed `>= 1` (callers only
/// reach this with at least one image — `GalleryMode::Slideshow` requires
/// it).
fn compute_image_duration_secs(probed_audio_secs: Option<f64>, image_count: usize) -> f64 {
    match probed_audio_secs {
        Some(total) => (total / image_count as f64)
            .clamp(MIN_SLIDESHOW_IMAGE_DURATION_SECS, MAX_SLIDESHOW_IMAGE_DURATION_SECS),
        None => DEFAULT_SLIDESHOW_IMAGE_DURATION_SECS,
    }
}

/// ffmpeg `xfade` transition used between every consecutive pair of images —
/// a single consistent right-to-left slide (not alternated per pair), same
/// direction throughout the whole slideshow.
const SLIDESHOW_TRANSITION: &str = "slideleft";

/// Builds the ffmpeg video-side filter/map arguments for the slideshow
/// encode, split out from `merge_gallery_slideshow` so the N=1 vs N>1
/// branching is unit-testable without spawning ffmpeg. Every branch must end
/// in an explicit `-map` for its own video output: once any `-map` is
/// present on a command (the audio track always gets one, added by the
/// caller), ffmpeg disables automatic stream selection for that output
/// entirely, so an unmapped filtered video stream is silently dropped
/// instead of erroring, leaving an audio-only file with no picture.
fn build_slideshow_video_args(
    image_count: usize,
    scale_pad: &str,
    transition_secs: f64,
    image_duration_secs: f64,
) -> Vec<String> {
    if image_count == 1 {
        vec!["-vf".to_string(), scale_pad.to_string(), "-map".to_string(), "0:v".to_string()]
    } else {
        let mut filter = String::new();
        for i in 0..image_count {
            filter.push_str(&format!("[{i}:v]{scale_pad}[v{i}];"));
        }
        let mut last_label = "v0".to_string();
        let mut offset = image_duration_secs;
        for i in 1..image_count {
            let out_label = if i == image_count - 1 {
                "vout".to_string()
            } else {
                format!("vx{i}")
            };
            filter.push_str(&format!(
                "[{last_label}][v{i}]xfade=transition={SLIDESHOW_TRANSITION}:duration={transition_secs}:offset={offset}[{out_label}];"
            ));
            last_label = out_label;
            offset += image_duration_secs;
        }
        filter.pop(); // trailing ';'
        vec!["-filter_complex".to_string(), filter, "-map".to_string(), "[vout]".to_string()]
    }
}

/// How much extra time to add to only the LAST image's on-screen duration so
/// the slideshow's total length always covers the real audio length, even
/// though `image_duration_secs` (the per-image share) is clamped to a sane
/// range and so can't itself be relied on to sum back up to the real total.
/// Returns `0.0` whenever the naive total already covers the audio (or the
/// audio's length couldn't be probed at all). `-shortest` handles trimming
/// the other direction (naive total longer than audio) on its own.
fn compute_tail_extension_secs(
    probed_audio_secs: Option<f64>,
    image_count: usize,
    image_duration_secs: f64,
    transition_secs: f64,
) -> f64 {
    let nominal_total_secs = image_count as f64 * image_duration_secs + transition_secs;
    probed_audio_secs
        .map(|total| (total - nominal_total_secs).max(0.0))
        .unwrap_or(0.0)
}

/// Merges downloaded gallery images + one audio track into a single
/// slideshow video via ffmpeg's `xfade` filter, crossfading each image into
/// the next with a horizontal slide (`SLIDESHOW_TRANSITION`, the same
/// direction for every pair) rather than a hard cut, matching TikTok's own
/// slideshow transition style (verified manually against real sample images
/// before wiring in — `xfade` produces a genuine sliding transition, not a
/// plain dissolve).
///
/// The audio track is never trimmed: each image's display time is the
/// audio's own real length (probed via `probe_media_duration_secs_with`) divided
/// evenly across the image count, not a fixed per-image duration, which
/// would either run past the music (dead air) or, combined with `-shortest`,
/// silently truncate the audio early to match a shorter slideshow. Because
/// that per-image share is clamped to a sane range (see
/// `compute_image_duration_secs`), a long track shared by few images (or a
/// short track shared by many) can still leave the naive total short of the
/// real audio length; that shortfall is folded entirely into the last
/// image's own on-screen time so the video always covers the full track and
/// `-shortest` stays only a rounding-error safety net.
async fn merge_gallery_slideshow(
    job_dir: &str,
    image_paths: &[String],
    audio_path: &str,
) -> Result<String, AppError> {
    let (canvas_w, canvas_h) = SLIDESHOW_CANVAS;
    let scale_pad = format!(
        "scale={canvas_w}:{canvas_h}:force_original_aspect_ratio=decrease,pad={canvas_w}:{canvas_h}:(ow-iw)/2:(oh-ih)/2,setsar=1,fps=25"
    );

    let audio_file_name = std::path::Path::new(audio_path)
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| AppError::internal("gallery audio track has no filename"))?
        .to_string();
    let output_file_name = "slideshow.mp4";
    let ffmpeg_path = ytdlp_binary::resolve_ffmpeg_path()?;

    let probed_audio_secs = probe_media_duration_secs_with(&ffmpeg_path, audio_path).await;
    let image_duration_secs = compute_image_duration_secs(probed_audio_secs, image_paths.len());
    // The transition borrows time from both the clip it leaves and the one
    // it enters, so it must stay well under a single image's own display
    // time — otherwise a very short per-image duration (many images, short
    // audio) would make consecutive transitions overlap each other.
    let transition_secs = SLIDESHOW_TRANSITION_SECS.min(image_duration_secs * 0.3);

    let mut cmd = tokio::process::Command::new(&ffmpeg_path);
    cmd.current_dir(job_dir).arg("-y");
    crate::downloader::hide_cmd_window(&mut cmd);

    // Each image needs to stay on-screen for its own display time PLUS the
    // transition it crossfades into the next one with (xfade consumes that
    // much of both clips' tails/heads to blend them) — otherwise the
    // transition would eat into black/nothing past the loop's own duration.
    let clip_duration = image_duration_secs + transition_secs;
    let tail_extension_secs = compute_tail_extension_secs(
        probed_audio_secs,
        image_paths.len(),
        image_duration_secs,
        transition_secs,
    );
    let last_index = image_paths.len() - 1;
    for (index, image_path) in image_paths.iter().enumerate() {
        let file_name = std::path::Path::new(image_path)
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| AppError::internal("gallery image has no filename"))?;
        let this_clip_duration = if index == last_index {
            clip_duration + tail_extension_secs
        } else {
            clip_duration
        };
        cmd.args(["-loop", "1", "-t", &this_clip_duration.to_string(), "-i", file_name]);
    }
    cmd.args(["-i", &audio_file_name]);

    cmd.args(build_slideshow_video_args(
        image_paths.len(),
        &scale_pad,
        transition_secs,
        image_duration_secs,
    ));

    let audio_input_index = image_paths.len();
    cmd.args([
        "-map",
        &format!("{audio_input_index}:a"),
        "-c:v",
        "libx264",
        "-r",
        "25",
        "-pix_fmt",
        "yuv420p",
        "-c:a",
        "aac",
        "-shortest",
        output_file_name,
    ]);

    let status = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map_err(AppError::internal)?;

    if !status.success() {
        return Err(AppError::new(
            "DOWNLOAD_FAILED",
            "Failed to merge slideshow images and audio into a video",
        ));
    }

    Ok(std::path::Path::new(job_dir)
        .join(output_file_name)
        .to_string_lossy()
        .into_owned())
}

/// Last-resort recovery for the documented yt-dlp audio-loss bug, seen on
/// both TikTok (issues #15891, #15642) and, via YouTube's SABR streaming
/// rollout, YouTube (issues #16128, #12482): separately fetch just the best
/// audio track for the same URL and mux it onto the already-downloaded (but
/// audio-less) video in place. This mirrors the workaround multiple
/// reporters on those issues converged on independently — after
/// maintainers confirmed the source itself serves inconsistent media (the
/// same format id/request sometimes has audio, sometimes doesn't, despite
/// identical metadata) with no code fix possible in yt-dlp — since
/// re-requesting the *exact same* video format isn't guaranteed to get a
/// different result, but a differently-scoped audio-only request has an
/// independent chance of landing on a working response.
async fn recover_missing_audio(
    app: &AppHandle,
    source_url: &str,
    video_path: &str,
) -> Result<String, AppError> {
    let audio_template = format!("{video_path}.audio-only.%(ext)s");
    let audio_args = vec![
        "--no-playlist".to_string(),
        "-f".to_string(),
        "bestaudio/best".to_string(),
    ];
    let reported_audio_path =
        ytdlp::run_download(app, source_url, &audio_template, audio_args, |_| {}).await?;
    // Same Windows console-encoding issue as the main download's reported
    // path (see `recover_actual_output_path`'s doc comment): the audio-only
    // file is still written to disk with the correct name, only yt-dlp's own
    // printed confirmation of that name can come back mangled. The expected
    // name is known exactly (it's the literal `-o` template above), so a
    // directory scan for that prefix is enough — no need for the fuzzier
    // diff/mtime fallback the main download path uses.
    let audio_path = if tokio::fs::try_exists(&reported_audio_path).await.unwrap_or(false) {
        reported_audio_path
    } else {
        let video_file_name = Path::new(video_path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        let dir = Path::new(video_path)
            .parent()
            .map(|parent| parent.to_string_lossy().into_owned())
            .unwrap_or_default();
        find_file_starting_with(&dir, &format!("{video_file_name}.audio-only."))
            .await
            .ok_or_else(|| {
                AppError::new(
                    "DOWNLOAD_FAILED",
                    "Recovered audio file couldn't be located on disk",
                )
            })?
    };

    if !ytdlp::output_has_audio_stream(&audio_path).await {
        let _ = tokio::fs::remove_file(&audio_path).await;
        return Err(AppError::new(
            "DOWNLOAD_FAILED",
            "Recovered audio track has no audio either",
        ));
    }

    let muxed_path = format!("{video_path}.muxed.mp4");
    let ffmpeg_path = ytdlp_binary::resolve_ffmpeg_path()?;
    let mut cmd = tokio::process::Command::new(&ffmpeg_path);
    cmd.args([
        "-y",
        "-i",
        video_path,
        "-i",
        &audio_path,
        "-map",
        "0:v:0",
        "-map",
        "1:a:0",
        "-c",
        "copy",
        &muxed_path,
    ]);
    crate::downloader::hide_cmd_window(&mut cmd);
    let status = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map_err(AppError::internal)?;

    let _ = tokio::fs::remove_file(&audio_path).await;

    if !status.success() {
        let _ = tokio::fs::remove_file(&muxed_path).await;
        return Err(AppError::new(
            "DOWNLOAD_FAILED",
            "Failed to mux recovered audio into video",
        ));
    }

    tokio::fs::rename(&muxed_path, video_path)
        .await
        .map_err(AppError::internal)?;
    Ok(video_path.to_string())
}

// ---- Đặt tên file đầu ra (FR-212→FR-216) -------------------------------

/// Các mẫu `-o` cho một lần chạy yt-dlp, kèm thứ cần để nhận ra file chương
/// sau khi tải xong.
///
/// Điểm cốt lõi: mẫu chính là một **tên hằng** do chính ta dựng, chứ không
/// phải mẫu `%(title)s` để yt-dlp tự điền. Nếu để yt-dlp điền thì cả FR-214
/// (làm sạch cho ba hệ điều hành) lẫn FR-215 (không ghi đè) đều không có chỗ
/// nào chạy — hai lời hứa ấy nằm trong `downloader::filename`, và nó chỉ áp
/// được lên một cái tên mà ta biết trước.
#[derive(Debug, PartialEq, Eq)]
struct OutputNaming {
    /// Giá trị cho `-o`, luôn kết thúc bằng `.%(ext)s`: phần mở rộng là thứ
    /// duy nhất vẫn để yt-dlp quyết định, vì nó phụ thuộc vào format thật được
    /// chọn lúc tải.
    main: String,
    /// Giá trị cho `-o chapter:` khi tác vụ tách chương.
    chapter: Option<String>,
    /// Tiền tố tên file chương (`"<tên> - "`), dùng để đếm file kết quả.
    chapter_prefix: Option<String>,
    /// Tên file đã có trong thư mục đích TRƯỚC khi tải, để phần đếm chương
    /// không tính nhầm file của lần chạy trước hay của một tác vụ khác.
    files_before: HashSet<String>,
    /// Tên file (KHÔNG mã hoá `%`, KHÔNG phần mở rộng) mà `main` được dựng
    /// từ đó — `None` khi để yt-dlp tự đặt tên. Giữ lại bản thô này để
    /// `recover_actual_output_path` so khớp trực tiếp với tên file thật trên
    /// đĩa, vì bản đã escape trong `main` không còn khớp ký tự với tên file
    /// yt-dlp thực sự ghi ra.
    stem: Option<String>,
}

/// Phần thuần của [`resolve_output_naming`]: dựng mẫu từ một cái tên đã chốt.
/// Tách ra để kiểm thử được toàn bộ chuỗi ký tự đưa cho yt-dlp mà không cần
/// đụng tới đĩa.
///
/// `stem = None` nghĩa là "để yt-dlp tự đặt tên" — xem [`render_output_stem`].
fn compose_output_naming(
    output_directory: &str,
    stem: Option<&str>,
    splits_chapters: bool,
) -> OutputNaming {
    // Thư mục cũng phải escape: yt-dlp đọc `%` ở bất kỳ đâu trong `-o` là mở
    // đầu một trường mẫu, nên một thư mục tên `100% Music` sẽ khiến nó ghi ra
    // chỗ khác hẳn.
    let dir = filename::escape_for_ytdlp_template(output_directory);
    let Some(stem) = stem else {
        return OutputNaming {
            main: format!("{dir}/%(title)s.%(ext)s"),
            chapter: None,
            chapter_prefix: None,
            files_before: HashSet::new(),
            stem: None,
        };
    };

    let escaped = filename::escape_for_ytdlp_template(stem);
    OutputNaming {
        main: format!("{dir}/{escaped}.%(ext)s"),
        // `%(section_number)03d` đệm số chương để thứ tự chữ cái trong trình
        // quản lý file trùng với thứ tự chương. `section_title` do yt-dlp tự
        // làm sạch trước khi ghép vào đường dẫn.
        chapter: splits_chapters.then(|| {
            format!("chapter:{dir}/{escaped} - %(section_number)03d %(section_title)s.%(ext)s")
        }),
        chapter_prefix: splits_chapters.then(|| format!("{stem} - ")),
        files_before: HashSet::new(),
        stem: Some(stem.to_string()),
    }
}

/// Trả kèm `StemClaimGuard`: giữ ở biến cục bộ suốt vòng đời `run_job` thì
/// chỗ vừa giữ tự nhả khi hàm đó kết thúc, dù bằng đường nào (thành công,
/// `?` lỗi sớm, huỷ) — xem chú thích ở `claim_output_stem`.
async fn resolve_output_naming(
    handles: &QueueHandles,
    job: &DownloadJob,
) -> (OutputNaming, Option<StemClaimGuard>) {
    let splits_chapters = job.output_options.segment.splits_chapters();
    let Some(stem) = render_output_stem(job) else {
        return (compose_output_naming(&job.output_directory, None, splits_chapters), None);
    };

    let existing = read_file_names(&job.output_directory).await;
    let (stem, guard) = claim_output_stem(
        &job.output_directory,
        &stem,
        &file_stems(&existing),
        &handles.claimed_stems,
    );
    let mut naming = compose_output_naming(&job.output_directory, Some(&stem), splits_chapters);
    naming.files_before = existing.into_iter().collect();
    (naming, Some(guard))
}

/// Tên file (chưa có phần mở rộng) mà tác vụ này nên ghi ra, hoặc `None` khi
/// phải để yt-dlp tự đặt tên.
///
/// `None` xảy ra ở đúng một tình huống: tác vụ **không mang tiêu đề nào** và
/// người dùng **không đổi mẫu**. Tự đặt tên ở đó sẽ biến mọi mục fan-out của
/// một playlist phẳng (nơi backend chỉ liệt kê được URL, không có tiêu đề)
/// thành `untitled`, `untitled (2)`, `untitled (3)` — tệ hơn hẳn hành vi hôm
/// nay, nơi yt-dlp điền tiêu đề thật mà nó vừa lấy được. Người dùng có đổi mẫu
/// thì ta tôn trọng mẫu ấy, kể cả khi vài trường phải rơi về giá trị dự phòng
/// (FR-216).
fn render_output_stem(job: &DownloadJob) -> Option<String> {
    let template = job.output_options.effective_filename_template();
    if job.title.is_none() && template == filename::DEFAULT_TEMPLATE {
        return None;
    }

    // `channel`, `upload_date`, `playlist_index` chưa có trên `DownloadJob`
    // (không có cột nào mang chúng), nên hiện tại chúng rơi về giá trị dự
    // phòng của FR-216. Đưa được chúng vào cần thêm dữ liệu nguồn đi kèm tác
    // vụ — một thay đổi lược đồ, không thuộc lát cắt này.
    let fields = filename::TemplateFields {
        title: job.title.clone(),
        channel: None,
        playlist_index: None,
        upload_date: None,
        resolution: job.video_quality.clone(),
        ext: expected_extension(job).map(str::to_string),
    };
    Some(filename::render_filename(
        strip_trailing_ext_field(template),
        &fields,
    ))
}

/// Phần mở rộng mà lựa chọn đầu ra đã quyết định, hoặc `None` khi nó chỉ lộ ra
/// lúc tải ("giữ nguyên định dạng gốc").
fn expected_extension(job: &DownloadJob) -> Option<&'static str> {
    match job.media_type {
        MediaType::Audio => job.output_options.audio.ytdlp_audio_format(),
        MediaType::Video => job.output_options.video_container.merge_output_format(),
        MediaType::Gallery => None,
    }
}

/// Cắt `{ext}` ở CUỐI mẫu (kèm dấu chấm ngăn cách nếu có).
///
/// Phần mở rộng thật luôn được nối vào cuối bởi `.%(ext)s`, nên giữ lại
/// `{ext}` ở đó sẽ cho ra `Bài hát.mp3.mp3`. `{ext}` nằm giữa mẫu vẫn được
/// thay bình thường — ở đó người dùng đang cố tình chèn nó vào tên.
fn strip_trailing_ext_field(template: &str) -> &str {
    let template = template.trim_end();
    match template.strip_suffix("{ext}") {
        Some(head) => {
            let head = head.trim_end();
            head.strip_suffix('.').unwrap_or(head)
        }
        None => template,
    }
}

/// Tên chưa bị chiếm trong thư mục đích (FR-215).
///
/// So theo **tên không phần mở rộng**: lúc này ta chưa biết file sẽ là `.mp4`
/// hay `.webm`, nên "đã có `Bài hát.mp3`" cũng phải tính là đã chiếm — nếu
/// không, một lần tải MP4 sẽ đặt tên trùng và lần sau lại ghi đè.
fn unique_stem(output_directory: &str, stem: &str, taken: &HashSet<String>) -> String {
    let desired = Path::new(output_directory).join(stem);
    let unique = filename::deduplicate_path(&desired, |candidate| {
        candidate
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| taken.contains(name))
    });
    unique
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(stem)
        .to_string()
}

/// Nhả chỗ một stem đã giữ khi job kết thúc — thành công, lỗi sớm qua `?`,
/// huỷ, panic, bất kể đường nào — vì đây là logic `Drop` chạy vô điều kiện
/// khi biến giữ nó (một local trong `run_job`) ra khỏi scope, không phải dọn
/// tay ở từng điểm return dễ sót.
struct StemClaimGuard {
    registry: Arc<StdMutex<HashSet<String>>>,
    key: String,
}

impl Drop for StemClaimGuard {
    fn drop(&mut self) {
        if let Ok(mut claimed) = self.registry.lock() {
            claimed.remove(&self.key);
        }
    }
}

fn stem_claim_key(output_directory: &str, stem: &str) -> String {
    format!("{output_directory}\u{0}{stem}")
}

/// Như `unique_stem`, nhưng cũng loại trừ tên đang được MỘT job KHÁC (chạy
/// cùng lúc) giữ chỗ, rồi giữ luôn tên thắng cuộc trước khi trả về.
///
/// `unique_stem` chỉ nhìn vào đĩa — hai job cho CÙNG một URL (người dùng bấm
/// tải/thử lại nhiều lần khi lần trước còn đang chạy) khởi động gần như đồng
/// thời sẽ cùng chụp ảnh thư mục TRƯỚC KHI job nào kịp ghi ra bất cứ gì, cả
/// hai thấy tên gốc đang "trống" như nhau, cùng chọn ĐÚNG MỘT tên, rồi giẫm
/// lên file tạm của nhau giữa chừng (`.fXXX.mp4`, `.temp.mp4`). Xác nhận trực
/// tiếp từ nhật ký người dùng: `WinError 2` (job A xoá file mảnh vừa tải
/// xong để ghép, ngay lúc job B — cùng tên — vẫn đang tải/đọc chính file đó)
/// và `WinError 183` (hai job cùng đổi tên `.temp.mp4` sang cùng một tên
/// đích, job đến sau thấy đích đã bị job kia chiếm).
fn claim_output_stem(
    output_directory: &str,
    stem: &str,
    taken: &HashSet<String>,
    registry: &Arc<StdMutex<HashSet<String>>>,
) -> (String, StemClaimGuard) {
    let desired = Path::new(output_directory).join(stem);
    let mut claimed = registry.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let unique = filename::deduplicate_path(&desired, |candidate| {
        candidate
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                taken.contains(name) || claimed.contains(&stem_claim_key(output_directory, name))
            })
    });
    let winner = unique
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(stem)
        .to_string();
    let key = stem_claim_key(output_directory, &winner);
    claimed.insert(key.clone());
    drop(claimed);
    (winner, StemClaimGuard { registry: Arc::clone(registry), key })
}

/// `reported_path` (`yt-dlp`'s own `--print after_move:...` text) is not
/// trustworthy on Windows: confirmed live against a real Vietnamese-titled
/// video that its plain-text output silently mangles non-ASCII characters
/// (accents dropped or corrupted into different, wrong characters) — byte-
/// identical whether or not `PYTHONUTF8`/`PYTHONIOENCODING`/`--encoding
/// utf-8` are set, so nothing on our side of the pipe can force it straight.
/// The file itself is still written with the correct Unicode name (yt-dlp's
/// own file I/O never goes through that same broken text-output path), so
/// when the reported path doesn't actually exist, the true name is
/// recovered straight from the filesystem instead — first by matching our
/// own known (never round-tripped through yt-dlp's text output) stem, then
/// by falling back to a before/after directory diff, the same technique
/// `record_chapter_files` already relies on for chapter files.
///
/// Silently trusting a wrong path here is what made "Open file"/"Open
/// containing folder" (`commands::history`) and the Library grid
/// (`commands::library`) report "file not found" right after a download
/// that had, in fact, completed onto a perfectly real file on disk.
async fn recover_actual_output_path(
    output_directory: &str,
    reported_path: &str,
    naming: &OutputNaming,
) -> Option<String> {
    if tokio::fs::try_exists(reported_path).await.unwrap_or(false) {
        return Some(reported_path.to_string());
    }

    let after = read_file_names(output_directory).await;

    if let Some(stem) = &naming.stem {
        if let Some(exact) = after.iter().find(|name| {
            Path::new(name).file_stem().and_then(|s| s.to_str()) == Some(stem.as_str())
        }) {
            return Some(Path::new(output_directory).join(exact).to_string_lossy().into_owned());
        }
    }

    let is_chapter_file = |name: &str| {
        naming
            .chapter_prefix
            .as_deref()
            .is_some_and(|prefix| name.starts_with(prefix))
    };
    let mut candidates: Vec<String> = after
        .into_iter()
        .filter(|name| !naming.files_before.contains(name) && !is_chapter_file(name))
        .collect();

    if candidates.len() == 1 {
        return Some(Path::new(output_directory).join(candidates.remove(0)).to_string_lossy().into_owned());
    }
    if candidates.is_empty() {
        return None;
    }

    // More than one candidate left (rare — e.g. a stem match failed AND
    // multiple non-chapter files appeared) — the main output is whichever
    // was written to last, since post-processing (metadata/thumbnail
    // embedding) always touches it after everything else is in place.
    let mut newest: Option<(String, std::time::SystemTime)> = None;
    for name in candidates {
        let full = Path::new(output_directory).join(&name);
        let Ok(modified) = tokio::fs::metadata(&full).await.and_then(|meta| meta.modified()) else {
            continue;
        };
        if newest.as_ref().map(|(_, t)| modified > *t).unwrap_or(true) {
            newest = Some((name, modified));
        }
    }
    newest.map(|(name, _)| Path::new(output_directory).join(name).to_string_lossy().into_owned())
}

/// Tìm file đầu tiên trong `dir` có tên bắt đầu bằng `prefix` — dùng khi biết
/// chính xác tiền tố mong đợi (xem `recover_missing_audio`) nên không cần đến
/// kiểu so khớp mơ hồ hơn (diff/mtime) của `recover_actual_output_path`.
async fn find_file_starting_with(dir: &str, prefix: &str) -> Option<String> {
    read_file_names(dir)
        .await
        .into_iter()
        .find(|name| name.starts_with(prefix))
        .map(|name| Path::new(dir).join(name).to_string_lossy().into_owned())
}

/// Tên file đang có trong thư mục đích. Thư mục chưa tồn tại hoặc không đọc
/// được thì coi như rỗng: chống ghi đè là nỗ lực tốt nhất có thể, không phải
/// lý do để một tác vụ thất bại trước khi bắt đầu.
async fn read_file_names(output_directory: &str) -> Vec<String> {
    let mut names = Vec::new();
    let Ok(mut entries) = tokio::fs::read_dir(output_directory).await else {
        return names;
    };
    while let Ok(Some(entry)) = entries.next_entry().await {
        if let Some(name) = entry.file_name().to_str() {
            names.push(name.to_string());
        }
    }
    names
}

fn file_stems(names: &[String]) -> HashSet<String> {
    names
        .iter()
        .filter_map(|name| {
            Path::new(name)
                .file_stem()
                .and_then(|stem| stem.to_str())
                .map(str::to_string)
        })
        .collect()
}

/// File chương mà chính lần chạy này vừa tạo ra: khớp tiền tố của mẫu chương
/// ta đã truyền, VÀ chưa có mặt trước khi tải. Điều kiện thứ hai là thứ giữ
/// cho một lần chạy lại (hoặc một tác vụ khác cùng thư mục) không bị đếm vào.
fn new_chapter_file_names(before: &HashSet<String>, after: &[String], prefix: &str) -> Vec<String> {
    after
        .iter()
        .filter(|name| name.starts_with(prefix) && !before.contains(*name))
        .cloned()
        .collect()
}

/// Kết quả của việc dựng tham số cho một lần chạy yt-dlp.
///
/// `skipped` tồn tại vì FR-210: khi định dạng đầu ra không chứa được ảnh bìa,
/// bước nhúng phải bị **bỏ qua có ghi lý do**, và tuyệt đối không được làm tác
/// vụ thất bại. Nếu cứ truyền `--embed-thumbnail` cho một container không hỗ
/// trợ thì bộ hậu xử lý của yt-dlp ném lỗi và giết cả tác vụ — tức là đúng
/// điều FR-210 cấm. Nên quyết định bỏ qua được đưa ra ngay ở đây, còn lý do
/// thì đi ngược lên `run_job` để ghi vào nhật ký.
#[derive(Debug, PartialEq, Eq)]
struct YtdlpPlan {
    args: Vec<String>,
    skipped: Vec<String>,
}

/// Container đầu ra có chứa được ảnh bìa hay không (FR-209/FR-210).
#[derive(Debug, PartialEq, Eq)]
enum ThumbnailSupport {
    Supported,
    /// Kèm lý do đọc được cho người dùng, để nhật ký nói rõ vì sao thiếu ảnh
    /// bìa thay vì im lặng.
    Unsupported(String),
}

/// Bộ hậu xử lý `EmbedThumbnail` của yt-dlp chỉ nhận mp3, mkv/mka, ogg/opus,
/// flac, m4a/mp4/mov. Danh sách này được đối chiếu với lựa chọn của người dùng
/// *trước* khi truyền cờ, chứ không phải để yt-dlp tự ném lỗi.
fn thumbnail_support(job: &DownloadJob) -> ThumbnailSupport {
    // "Giữ nguyên định dạng gốc" là trường hợp thật sự không biết trước:
    // container đích do nguồn quyết định tại thời điểm tải, và một nguồn WebM
    // (không nằm trong danh sách trên) sẽ làm bước nhúng ném lỗi. Bỏ qua có
    // ghi lý do là cách duy nhất giữ được lời hứa của FR-210 ở đây.
    const SOURCE_REASON: &str =
        "output keeps the source's own container, which yt-dlp cannot be guaranteed to \
         support for cover art (e.g. WebM) — embedding would fail the job instead of \
         degrading, so it is skipped";

    match job.media_type {
        MediaType::Audio => match job.output_options.audio {
            AudioOutput::Mp3 { .. }
            | AudioOutput::M4a { .. }
            | AudioOutput::Opus { .. }
            | AudioOutput::Flac => ThumbnailSupport::Supported,
            AudioOutput::Wav => ThumbnailSupport::Unsupported(
                "WAV has no tag container that can hold cover art".to_string(),
            ),
            AudioOutput::Source => ThumbnailSupport::Unsupported(SOURCE_REASON.to_string()),
        },
        MediaType::Video => match job.output_options.video_container {
            VideoContainer::Mp4 | VideoContainer::Mkv => ThumbnailSupport::Supported,
            VideoContainer::Source => ThumbnailSupport::Unsupported(SOURCE_REASON.to_string()),
        },
        // `build_ytdlp_args` đã từ chối job gallery trước khi tới đây.
        MediaType::Gallery => ThumbnailSupport::Unsupported(
            "gallery downloads do not go through yt-dlp".to_string(),
        ),
    }
}

/// Định dạng đầu ra có chứa được track phụ đề hay không (FR-220).
#[derive(Debug, PartialEq, Eq)]
enum SubtitleEmbedSupport {
    Supported,
    Unsupported(String),
}

/// Cùng luật với [`thumbnail_support`], khác danh sách: bộ hậu xử lý
/// `FFmpegEmbedSubtitle` chỉ nhúng được vào mp4/mkv/webm. Một file audio thì
/// không có chỗ nào để đặt track phụ đề vào cả.
fn subtitle_embed_support(job: &DownloadJob) -> SubtitleEmbedSupport {
    match job.media_type {
        MediaType::Video => match job.output_options.video_container {
            VideoContainer::Mp4 | VideoContainer::Mkv => SubtitleEmbedSupport::Supported,
            VideoContainer::Source => SubtitleEmbedSupport::Unsupported(
                "output keeps the source's own container, which may be one that cannot hold a \
                 subtitle track — embedding would fail the job instead of degrading, so it is \
                 skipped"
                    .to_string(),
            ),
        },
        MediaType::Audio => SubtitleEmbedSupport::Unsupported(
            "an audio-only output has no subtitle track to embed into".to_string(),
        ),
        MediaType::Gallery => SubtitleEmbedSupport::Unsupported(
            "gallery downloads do not go through yt-dlp".to_string(),
        ),
    }
}

/// FR-217→FR-221. Không có ngôn ngữ nào được chọn thì hàm này không thêm cờ
/// nào — mặc định giữ nguyên hành vi hôm nay.
///
/// Khi người dùng chọn "nhúng" mà định dạng đích không chứa được phụ đề, bước
/// phụ đề bị **bỏ qua có ghi lý do**, đúng luật FR-210 mà ảnh bìa đang theo.
/// Cố tình KHÔNG âm thầm hạ xuống thành file rời: người dùng yêu cầu một file
/// duy nhất có phụ đề bên trong, và đưa họ một thứ khác mà không nói gì là
/// đánh tráo kết quả — giao diện đã vô hiệu hoá lựa chọn này kèm giải thích
/// (FR-220), nên đây chỉ là lưới an toàn cho lời gọi lệnh trực tiếp.
fn apply_subtitle_args(job: &DownloadJob, args: &mut Vec<String>, skipped: &mut Vec<String>) {
    let subtitles = &job.output_options.subtitles;
    let languages = subtitles.normalized_languages();
    if languages.is_empty() {
        return;
    }

    if subtitles.delivery == SubtitleDelivery::Embedded {
        if let SubtitleEmbedSupport::Unsupported(reason) = subtitle_embed_support(job) {
            skipped.push(format!("skipped the subtitles: {reason}"));
            return;
        }
    }

    // Nhiều ngôn ngữ trong MỘT đối số, ngăn bằng dấu phẩy — đó là cú pháp
    // `--sub-langs` (FR-218).
    args.push("--sub-langs".into());
    args.push(languages.join(","));
    // Phụ đề máy sinh nằm ở một kho khác của yt-dlp và cần cờ riêng; thiếu nó
    // thì một video chỉ có phụ đề tự động sẽ về tay không mà không báo gì.
    if subtitles.include_auto_generated {
        args.push("--write-auto-subs".into());
    }
    match subtitles.delivery {
        SubtitleDelivery::SeparateFiles => args.push("--write-subs".into()),
        // `--embed-subs` tự bật phần tải phụ đề rồi xoá file tạm sau khi nhúng,
        // nên KHÔNG kèm `--write-subs`: kèm vào là giữ lại đúng những file rời
        // mà người dùng vừa chọn không muốn có.
        SubtitleDelivery::Embedded => args.push("--embed-subs".into()),
    }
}

/// FR-222→FR-227. Cắt đoạn và tách chương là hai nhánh của cùng một `match`
/// bởi vì [`SegmentMode`] là một enum: không có tổ hợp nào để chúng cùng xuất
/// hiện, nên cũng không có phép kiểm tra nào để quên.
fn apply_segment_args(segment: &SegmentMode, args: &mut Vec<String>) {
    match segment {
        SegmentMode::Whole => {}
        SegmentMode::Trim(range) => {
            args.push("--download-sections".into());
            args.push(download_sections_arg(range));
            // FR-224: cắt đúng điểm yêu cầu bắt buộc mã hoá lại quanh chỗ cắt,
            // nên chậm hơn hẳn — giao diện phải báo trước.
            if range.accurate_cut {
                args.push("--force-keyframes-at-cuts".into());
            }
        }
        SegmentMode::SplitChapters => args.push("--split-chapters".into()),
    }
}

/// Cú pháp khoảng thời gian của `--download-sections`: `*<bắt đầu>-<kết thúc>`.
///
/// Dấu `*` ở đầu là thứ phân biệt "một khoảng thời gian" với "một biểu thức
/// chính quy khớp tên chương" — thiếu nó, yt-dlp sẽ đem chuỗi này đi khớp với
/// tên các chương và không tải gì cả. Mốc thiếu được điền bằng `0` (từ đầu) và
/// `inf` (tới hết), đúng hai giá trị mặc định mà chính yt-dlp dùng.
fn download_sections_arg(range: &TrimRange) -> String {
    let start = range
        .start_seconds
        .map(format_seconds)
        .unwrap_or_else(|| "0".to_string());
    let end = range
        .end_seconds
        .map(format_seconds)
        .unwrap_or_else(|| "inf".to_string());
    format!("*{start}-{end}")
}

/// Số giây dưới dạng yt-dlp đọc được: tối đa 3 chữ số thập phân, không đuôi
/// `.0` thừa, và tuyệt đối không phải ký hiệu khoa học (`1e-7` sẽ bị yt-dlp
/// hiểu sai hoàn toàn).
fn format_seconds(value: f64) -> String {
    let text = format!("{value:.3}");
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    trimmed.to_string()
}

/// Giá trị cho `--audio-quality`, hoặc `None` khi cờ đó không được phép xuất
/// hiện.
///
/// Cửa chặn đầu tiên là FR-203 và nó nằm ở tầng kiểu dữ liệu: `bitrate_kbps()`
/// trả `None` cho WAV/FLAC/Source vì các biến thể ấy **không có** trường
/// bitrate. Nhưng chỉ vậy chưa đủ — nhãn chất lượng đã đối chiếu với nguồn
/// (`DownloadJob.audio_quality`) vẫn tồn tại độc lập, và người dùng hoàn toàn
/// có thể đã chọn "320kbps" ở bước xem trước rồi mới đổi định dạng sang FLAC.
/// Nên hàm này thoát sớm cho mọi định dạng không mất dữ liệu: nhãn kia thậm chí
/// không được đọc tới.
///
/// Thứ tự ưu tiên khi định dạng *có* mất dữ liệu:
///   1. bitrate người dùng chọn thẳng trong lựa chọn đầu ra;
///   2. nhãn chất lượng đã đối chiếu với danh sách format thật của nguồn
///      (FR-019) — đây là đường đi của toàn bộ tác vụ hiện có, nên mặc định
///      giữ nguyên hành vi cũ;
///   3. `0`, tức để yt-dlp tự chọn mức VBR tốt nhất (mục playlist fan-out
///      không qua bước đối chiếu nên không có nhãn nào).
fn audio_quality_arg(
    audio: &AudioOutput,
    validated_label: Option<&str>,
) -> Result<Option<String>, AppError> {
    if !audio.is_lossy() {
        return Ok(None);
    }
    if let Some(bitrate_kbps) = audio.bitrate_kbps() {
        return Ok(Some(format!("{bitrate_kbps}K")));
    }
    Ok(Some(match validated_label {
        Some(quality) => {
            let bitrate_kbps = parse_leading_number(quality).ok_or_else(|| {
                AppError::new(
                    "INVALID_QUALITY_OPTION",
                    format!("Cannot parse audio quality: {quality}"),
                )
            })?;
            format!("{bitrate_kbps}K")
        }
        None => "0".to_string(),
    }))
}

/// `rate_limit_kbps` bằng 0 nghĩa là không giới hạn. Giới hạn này áp cho từng
/// tiến trình yt-dlp, không phải tổng băng thông của ứng dụng — với N job chạy
/// song song, tổng thực tế có thể tới N lần mức này. Giao diện Cài đặt phải nói
/// rõ điều đó.
///
/// Một job không nêu lựa chọn đầu ra nào (`OutputOptions::default()`, tức mọi
/// dòng có trước Phase 2) phải cho ra danh sách tham số **giống hệt từng chuỗi
/// một** với thứ đang chạy hôm nay — xem test
/// `default_options_reproduce_todays_arguments_byte_for_byte`.
fn build_ytdlp_args(job: &DownloadJob, rate_limit_kbps: u32) -> Result<YtdlpPlan, AppError> {
    // `--no-playlist` on every single-item job (audio or video) is a
    // deliberate safety net for FR-013: a URL copied from inside a playlist
    // often still carries a `&list=...` param, and without this flag yt-dlp
    // would silently download the whole playlist instead of just this item.
    // Jobs created for a confirmed `entire_playlist` fan-out (T033) are each
    // their own per-entry URL, so this flag has no effect on them either way.
    let mut args = vec!["--no-playlist".to_string()];
    let mut skipped: Vec<String> = Vec::new();
    let options = &job.output_options;

    // FR-223 ở tầng lệnh: một khoảng thời gian vô nghĩa phải dừng tác vụ tại
    // đây với lý do đọc được, chứ không được lặng lẽ đi tới yt-dlp — nơi nó
    // hoặc bị bỏ qua (tải nguyên cả video) hoặc làm cả tiến trình chết với một
    // thông báo của yt-dlp mà người dùng không hiểu.
    options
        .validate()
        .map_err(|err| AppError::new(err.code(), err.to_string()))?;

    match job.media_type {
        MediaType::Audio => {
            // Explicit `-f` matters here: without it, yt-dlp's default
            // selector is `bestvideo*+bestaudio/best`, which tries to pick
            // separate "best video" and "best audio" candidates and merge
            // them. On sites like TikTok where every format is already a
            // muxed video+audio stream (no dedicated audio-only track), that
            // default can pick two *different* pre-muxed formats and merge
            // them incorrectly, producing a file with no audio track at all
            // once `-x` extracts from it. `bestaudio/best` tells yt-dlp to
            // just take the single best format that actually has audio
            // (preferring a real audio-only stream when one exists) instead
            // of attempting an unnecessary — and here, broken — merge.
            args.push("-f".into());
            args.push("bestaudio/best".into());

            // `None` ở đây là "giữ nguyên định dạng gốc" (FR-202): không `-x`,
            // không `--audio-format`, không `--audio-quality`. Đây là điều kiện
            // đủ để KHÔNG có bước chuyển mã nào chạy — yt-dlp chỉ tải xuống
            // luồng audio tốt nhất và ghi thẳng ra đĩa, ffmpeg không hề được
            // gọi tới, nên cũng là lý do SC-202 nói cách này nhanh hơn hẳn.
            if let Some(audio_format) = options.audio.ytdlp_audio_format() {
                args.push("-x".into());
                args.push("--audio-format".into());
                args.push(audio_format.into());
                // Bitrate chỉ đi kèm định dạng nén mất dữ liệu (FR-203) — với
                // WAV/FLAC hàm này trả `None` và cờ biến mất hoàn toàn, chứ
                // không phải được truyền một giá trị vô hại nào đó.
                if let Some(quality) =
                    audio_quality_arg(&options.audio, job.audio_quality.as_deref())?
                {
                    args.push("--audio-quality".into());
                    args.push(quality);
                }
            }
        }
        MediaType::Video => {
            let height = job
                .video_quality
                .as_deref()
                .map(|quality| {
                    parse_leading_number(quality).ok_or_else(|| {
                        AppError::new(
                            "INVALID_QUALITY_OPTION",
                            format!("Cannot parse video quality: {quality}"),
                        )
                    })
                })
                .transpose()?;
            args.push("-f".into());
            args.push(video_format_selector(height, options.codec_preference));
            // `None` = giữ nguyên container gốc (FR-204): không ép yt-dlp
            // remux sang thứ gì cả.
            if let Some(container) = options.video_container.merge_output_format() {
                args.push("--merge-output-format".into());
                args.push(container.into());
            }
            match options.codec_preference {
                // TikTok's audio-loss bug (yt-dlp issues #15891/#15642) was
                // reported far more often on `bytevc1`/h265 formats than h264 —
                // this is the community-confirmed mitigation (`-S "vcodec:avc"`)
                // layered on top of `video_format_selector`'s own avc1-first `-f`
                // chain, so a tied fallback still leans h264 instead of h265.
                CodecPreference::Compatibility => {
                    args.push("--format-sort".into());
                    args.push("vcodec:avc".into());
                }
                // Ưu tiên chất lượng thì cố tình BỎ luôn `--format-sort`: giữ
                // nó lại sẽ kéo mọi lựa chọn hoà nhau về h264 và làm rỗng ý
                // nghĩa của chính lựa chọn này (FR-205). Không có sắp xếp nào
                // thay thế — thứ tự mặc định của yt-dlp vốn đã ưu tiên chất
                // lượng, và đó chính xác là thứ người dùng vừa yêu cầu.
                CodecPreference::Quality => {}
            }
        }
        // `run_job` branches to `run_gallery_job` (a completely separate,
        // gallery-dl-backed code path) before this function is ever called
        // for a gallery job — this arm only exists so the match stays
        // exhaustive if that invariant is ever broken.
        MediaType::Gallery => {
            return Err(AppError::internal(
                "build_ytdlp_args called for a MediaType::Gallery job",
            ))
        }
    }

    // FR-208. yt-dlp lấy các trường này thẳng từ metadata nguồn, nên trường nào
    // nguồn không có thì đơn giản là không được ghi — không có giá trị suy đoán
    // nào được điền vào (FR-211).
    if options.embed_metadata {
        args.push("--embed-metadata".into());
    }

    // FR-209/FR-210: chỉ truyền cờ khi container đích thật sự chứa được ảnh
    // bìa. Khi không, ghi lý do lại và đi tiếp — tác vụ vẫn thành công, chỉ là
    // không có ảnh bìa.
    if options.embed_thumbnail {
        match thumbnail_support(job) {
            ThumbnailSupport::Supported => args.push("--embed-thumbnail".into()),
            ThumbnailSupport::Unsupported(reason) => {
                skipped.push(format!("skipped embedding the cover art: {reason}"));
            }
        }
    }

    apply_subtitle_args(job, &mut args, &mut skipped);
    apply_segment_args(&options.segment, &mut args);

    if rate_limit_kbps > 0 {
        args.push("--limit-rate".into());
        args.push(format!("{rate_limit_kbps}K"));
    }

    args.push("--continue".into());
    Ok(YtdlpPlan { args, skipped })
}

/// Builds a `-f` format selector that prioritizes H.264 video (`avc1`) +
/// AAC audio (`mp4a`) — the codec pair virtually every player can decode
/// inside an MP4 container. Left unconstrained, yt-dlp's plain "bestvideo"
/// commonly resolves to VP9/AV1 + Opus on sites like YouTube (better
/// compression, but QuickTime, older Windows Media Player, and many TVs/
/// mobile players can't decode VP9 or Opus muxed into `.mp4`), producing a
/// file that "downloads fine" but won't actually play. Falls back to
/// whatever's best if this exact quality has no H.264 rendition (rare, e.g.
/// some 4K/8K sources are AV1-only) so the download still succeeds instead
/// of failing outright — just not with the compatibility guarantee.
///
/// `CodecPreference::Quality` bỏ toàn bộ ràng buộc codec ở trên và lấy đúng
/// thứ tốt nhất nguồn có, kể cả VP9/AV1 (FR-205). Chuỗi kết quả khi đó chỉ còn
/// vế dự phòng cuối cùng của nhánh tương thích — vốn đã là "tốt nhất, bất kể
/// codec" — nên hai chế độ dùng chung một đáy và chỉ khác nhau ở phần ưu tiên
/// đặt phía trước.
fn video_format_selector(height: Option<u32>, codec_preference: CodecPreference) -> String {
    match (codec_preference, height) {
        (CodecPreference::Compatibility, Some(h)) => format!(
            "bestvideo[vcodec^=avc1][height<={h}]+bestaudio[acodec^=mp4a]/\
             best[vcodec^=avc1][height<={h}]/\
             bestvideo[height<={h}]+bestaudio/best[height<={h}]"
        ),
        (CodecPreference::Compatibility, None) => {
            "bestvideo[vcodec^=avc1]+bestaudio[acodec^=mp4a]/\
             best[vcodec^=avc1]/bestvideo+bestaudio/best"
                .to_string()
        }
        (CodecPreference::Quality, Some(h)) => {
            format!("bestvideo[height<={h}]+bestaudio/best[height<={h}]")
        }
        (CodecPreference::Quality, None) => "bestvideo+bestaudio/best".to_string(),
    }
}

/// Extracts the leading integer from labels like `"128kbps"` or `"1080p"`.
fn parse_leading_number(label: &str) -> Option<u32> {
    let digits: String = label.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

fn emit_progress(app: &AppHandle, job_id: &str, update: &ytdlp::ProgressUpdate) {
    let _ = app.emit(
        "job:progress",
        JobProgressEvent {
            job_id: job_id.to_string(),
            progress_percent: update.percent,
            downloaded_bytes: update.downloaded_bytes,
            speed_bytes_per_sec: update.speed_bytes_per_sec,
            eta_seconds: update.eta_seconds,
        },
    );
}

fn emit_status_changed(
    app: &AppHandle,
    job_id: &str,
    status: JobStatus,
    error_message: Option<String>,
    output_file_path: Option<String>,
) {
    let _ = app.emit(
        "job:status_changed",
        JobStatusChangedEvent {
            job_id: job_id.to_string(),
            status: status.as_str().to_string(),
            error_message,
            output_file_path,
            produced_file_count: None,
        },
    );
}

/// Như [`emit_status_changed`] cho trạng thái `completed`, nhưng kèm được số
/// file kết quả (FR-227). Là hàm riêng chứ không phải một tham số thứ sáu để
/// năm chỗ gọi còn lại — nơi khái niệm "số file kết quả" chưa tồn tại — không
/// phải viết `None` cho một thứ chúng không liên quan.
fn emit_completed(
    app: &AppHandle,
    job_id: &str,
    output_file_path: String,
    produced_file_count: Option<u32>,
) {
    let _ = app.emit(
        "job:status_changed",
        JobStatusChangedEvent {
            job_id: job_id.to_string(),
            status: JobStatus::Completed.as_str().to_string(),
            error_message: None,
            output_file_path: Some(output_file_path),
            produced_file_count,
        },
    );
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::models::{OutputOptions, SubtitleOptions};

    #[test]
    fn parses_a_real_ffmpeg_duration_banner() {
        let stderr = "  Duration: 00:00:17.30, start: 0.025057, bitrate: 64 kb/s\n";
        assert_eq!(parse_ffmpeg_duration_line(stderr), Some(17.3));
    }

    #[test]
    fn parses_hours_and_minutes_correctly() {
        let stderr = "  Duration: 01:02:03.50, start: 0.000000, bitrate: 128 kb/s\n";
        let expected = 1.0 * 3600.0 + 2.0 * 60.0 + 3.5;
        assert_eq!(parse_ffmpeg_duration_line(stderr), Some(expected));
    }

    #[test]
    fn returns_none_when_no_duration_line_is_present() {
        assert_eq!(parse_ffmpeg_duration_line("some unrelated ffmpeg output\n"), None);
    }

    #[test]
    fn returns_none_for_an_unparseable_or_zero_duration() {
        assert_eq!(parse_ffmpeg_duration_line("  Duration: N/A, bitrate: N/A\n"), None);
        assert_eq!(
            parse_ffmpeg_duration_line("  Duration: 00:00:00.00, start: 0, bitrate: 0 kb/s\n"),
            None
        );
    }

    #[test]
    fn image_duration_divides_the_real_audio_length_evenly_instead_of_a_fixed_cadence() {
        // Regression test: the slideshow used to show every image for a
        // fixed 3s regardless of the audio's actual length, which either
        // left dead air past the music or — combined with `-shortest` —
        // silently truncated the audio early to match a shorter video.
        assert_eq!(compute_image_duration_secs(Some(17.3), 4), 17.3 / 4.0);
    }

    #[test]
    fn image_duration_falls_back_to_the_default_when_audio_length_is_unknown() {
        assert_eq!(compute_image_duration_secs(None, 4), DEFAULT_SLIDESHOW_IMAGE_DURATION_SECS);
    }

    #[test]
    fn image_duration_clamps_to_a_sane_range_for_extreme_ratios() {
        // Many images sharing a short track: would otherwise be far too
        // fast to actually look at.
        assert_eq!(compute_image_duration_secs(Some(5.0), 20), MIN_SLIDESHOW_IMAGE_DURATION_SECS);
        // One image against a very long track: would otherwise show a
        // single static image for minutes.
        assert_eq!(compute_image_duration_secs(Some(120.0), 1), MAX_SLIDESHOW_IMAGE_DURATION_SECS);
    }

    #[test]
    fn tail_extension_covers_the_shortfall_from_a_max_duration_clamp() {
        // Regression test: 1 image against a 120s track clamps
        // image_duration_secs to MAX_SLIDESHOW_IMAGE_DURATION_SECS (8.0), so
        // the naive total (8.0 + 0.5 transition = 8.5s) is nowhere near the
        // real 120s track. Without extending the tail, `-shortest` would cut
        // the audio down to 8.5s instead of keeping the full track.
        let tail = compute_tail_extension_secs(Some(120.0), 1, MAX_SLIDESHOW_IMAGE_DURATION_SECS, 0.5);
        assert_eq!(tail, 120.0 - (MAX_SLIDESHOW_IMAGE_DURATION_SECS + 0.5));
    }

    #[test]
    fn tail_extension_is_zero_when_no_clamp_was_applied() {
        // 4 images at 17.3/4 = 4.325s each: no clamp involved, so the naive
        // total (which also includes the trailing transition) already meets
        // or exceeds the real track length, so no extension is needed.
        let image_duration_secs = 17.3 / 4.0;
        let tail = compute_tail_extension_secs(Some(17.3), 4, image_duration_secs, 0.5);
        assert_eq!(tail, 0.0);
    }

    #[test]
    fn tail_extension_is_zero_when_audio_length_is_unknown() {
        assert_eq!(compute_tail_extension_secs(None, 3, DEFAULT_SLIDESHOW_IMAGE_DURATION_SECS, 0.5), 0.0);
    }

    #[test]
    fn single_image_slideshow_explicitly_maps_the_filtered_video_stream() {
        // Regression test: ffmpeg disables automatic stream selection for an
        // output once ANY -map is present on it (the caller always adds one
        // for the audio track), so a bare `-vf` with no matching `-map`
        // silently drops the video entirely. The merged file plays audio
        // but has no video track at all.
        let args = build_slideshow_video_args(1, "scale=1080:1920", 0.5, 3.0);
        assert_eq!(args, vec!["-vf", "scale=1080:1920", "-map", "0:v"]);
    }

    #[test]
    fn multi_image_slideshow_still_maps_the_crossfaded_output() {
        let args = build_slideshow_video_args(3, "scale=1080:1920", 0.5, 3.0);
        assert_eq!(args[0], "-filter_complex");
        assert_eq!(args[2], "-map");
        assert_eq!(args[3], "[vout]");
    }

    fn sample_job(media_type: MediaType, audio_quality: Option<&str>, video_quality: Option<&str>) -> DownloadJob {
        DownloadJob {
            id: "job-1".into(),
            source_url: "https://youtube.com/watch?v=abc".into(),
            platform: "youtube".into(),
            media_type,
            audio_quality: audio_quality.map(String::from),
            video_quality: video_quality.map(String::from),
            gallery_mode: None,
            selected_gallery_indices: None,
            status: JobStatus::Queued,
            progress_percent: 0.0,
            speed_bytes_per_sec: None,
            eta_seconds: None,
            error_message: None,
            output_directory: "/tmp".into(),
            output_file_path: None,
            is_playlist_item: false,
            parent_playlist_id: None,
            retried_from_job_id: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            title: None,
            playlist_title: None,
            queue_position: 0.0,
            retry_count: 0,
            next_retry_at: None,
            output_options: OutputOptions::default(),
        }
    }

    /// Chỉ lấy phần tham số, cho những test không quan tâm tới ghi chú bỏ qua.
    fn args_of(job: &DownloadJob) -> Vec<String> {
        build_ytdlp_args(job, 0).unwrap().args
    }

    #[test]
    fn audio_args_use_selected_bitrate_not_a_hardcoded_constant() {
        let job = sample_job(MediaType::Audio, Some("128kbps"), None);
        assert!(args_of(&job).contains(&"128K".to_string()));

        let job_high = sample_job(MediaType::Audio, Some("320kbps"), None);
        assert!(args_of(&job_high).contains(&"320K".to_string()));
    }

    #[test]
    fn audio_downloads_explicitly_select_bestaudio_instead_of_the_ambiguous_default() {
        // Regression test: without an explicit `-f`, yt-dlp's default
        // `bestvideo*+bestaudio` selector can pick two different pre-muxed
        // formats on sites like TikTok (every format has both video and
        // audio, no dedicated audio-only stream) and merge them incorrectly,
        // producing a file with no audio track once `-x` extracts from it.
        let job = sample_job(MediaType::Audio, Some("128kbps"), None);
        let args = args_of(&job);
        let f_index = args.iter().position(|a| a == "-f").expect("-f flag present");
        assert_eq!(args[f_index + 1], "bestaudio/best");
    }

    #[test]
    fn video_args_select_nearest_available_height_via_format_selector() {
        let job = sample_job(MediaType::Video, None, Some("1080p"));
        let args = args_of(&job);
        let format_selector = args
            .iter()
            .find(|a| a.contains("bestvideo"))
            .expect("format selector arg present");
        // `height<=1080` lets yt-dlp itself fall back to the closest lower
        // resolution when the source doesn't have exactly 1080p (US2
        // Acceptance Scenario 2), instead of us hard-coding a fallback list.
        assert!(format_selector.contains("height<=1080"));
    }

    #[test]
    fn video_args_prefer_h264_aac_so_the_mp4_actually_plays() {
        // Regression test: unconstrained "bestvideo" commonly resolves to
        // VP9/AV1 + Opus on YouTube, which plays fine in VLC/browsers but
        // fails to open in QuickTime, older Windows Media Player, and many
        // TVs when muxed into .mp4 — the file "downloads" but won't play.
        let job = sample_job(MediaType::Video, None, Some("1080p"));
        let args = args_of(&job);
        let format_selector = args
            .iter()
            .find(|a| a.contains("bestvideo"))
            .expect("format selector arg present");
        assert!(
            format_selector.starts_with("bestvideo[vcodec^=avc1]"),
            "must try H.264 (avc1) first for MP4 player compatibility: {format_selector}"
        );
        assert!(format_selector.contains("acodec^=mp4a"));
        // ...but must still fall back to non-H.264 rather than fail the
        // download outright when this quality has no avc1 rendition.
        assert!(format_selector.ends_with("best[height<=1080]"));
    }

    #[test]
    fn video_args_include_format_sort_preferring_avc_for_tied_fallbacks() {
        // Regression test for yt-dlp issues #15891/#15642: TikTok's
        // audio-loss bug was reported far more often on h265 (`bytevc1`)
        // than h264 formats. `video_format_selector`'s `-f` chain already
        // tries avc1 first, but `--format-sort vcodec:avc` additionally
        // biases any tied fallback candidate towards h264 too.
        let job = sample_job(MediaType::Video, None, Some("1080p"));
        let args = args_of(&job);
        let sort_index = args
            .iter()
            .position(|a| a == "--format-sort")
            .expect("--format-sort flag present");
        assert_eq!(args[sort_index + 1], "vcodec:avc");
    }

    #[test]
    fn missing_quality_falls_back_to_best_for_playlist_fanout_items() {
        // Playlist entries (T033) skip per-item quality validation since
        // flat-playlist previews don't fetch per-video formats — `None`
        // means "let yt-dlp pick its best", not an error.
        let audio_job = sample_job(MediaType::Audio, None, None);
        let audio_args = args_of(&audio_job);
        assert!(audio_args.contains(&"0".to_string()));

        let video_job = sample_job(MediaType::Video, None, None);
        let video_args = args_of(&video_job);
        assert!(video_args
            .iter()
            .any(|a| a.ends_with("bestvideo+bestaudio/best")));
    }

    #[test]
    fn every_single_item_job_disables_implicit_playlist_download() {
        let job = sample_job(MediaType::Audio, Some("128kbps"), None);
        assert_eq!(args_of(&job).first(), Some(&"--no-playlist".to_string()));
    }

    #[test]
    fn adds_rate_limit_flag_when_configured() {
        let job = sample_job(MediaType::Audio, Some("128kbps"), None);
        let args = build_ytdlp_args(&job, 512).expect("args build").args;

        let index = args
            .iter()
            .position(|a| a == "--limit-rate")
            .expect("cờ giới hạn tốc độ phải có mặt");
        // Đơn vị phải là K: yt-dlp hiểu `--limit-rate 512` là 512 **byte**/s,
        // tức chậm hơn ý người dùng một nghìn lần.
        assert_eq!(args[index + 1], "512K");
    }

    #[test]
    fn omits_rate_limit_flag_when_unlimited() {
        let job = sample_job(MediaType::Audio, Some("128kbps"), None);
        let args = build_ytdlp_args(&job, 0).expect("args build").args;

        assert!(
            !args.iter().any(|a| a == "--limit-rate"),
            "0 nghĩa là không giới hạn, không được truyền cờ"
        );
    }

    #[test]
    fn a_finished_run_is_only_acted_on_while_it_is_still_the_registered_one() {
        // Người dùng tạm dừng rồi tiếp tục rất nhanh: lần chạy #1 kết thúc SAU
        // khi lần chạy #2 đã đăng ký. Thi hành kết quả của #1 lúc đó sẽ gỡ mất
        // handle huỷ của #2 (job không còn tạm dừng được nữa) và ghi đè trạng
        // thái của #2 bằng lỗi của một lần chạy đã chết.
        assert!(!is_current_run(Some(2), 1), "#2 đã chiếm chỗ, kết quả của #1 đã lỗi thời");
        assert!(is_current_run(Some(1), 1));
        // Không còn entry nào nghĩa là chưa ai chiếm chỗ, nên kết quả vẫn phải
        // được thi hành — nếu không, một job thất bại sẽ kẹt ở `downloading`.
        assert!(is_current_run(None, 1));
    }

    // ---- specs/003-media-output: định dạng đầu ra và metadata -------------

    /// Job audio kèm một bộ lựa chọn đầu ra cụ thể. `audio_quality` luôn được
    /// đặt sẵn 320kbps: nhãn ấy là thứ *phải* bị bỏ qua với định dạng không
    /// mất dữ liệu, nên để nó có mặt trong mọi ca thì test mới nói được điều
    /// gì về FR-203.
    fn audio_job_with(audio: AudioOutput) -> DownloadJob {
        let mut job = sample_job(MediaType::Audio, Some("320kbps"), None);
        job.output_options = OutputOptions {
            audio,
            ..OutputOptions::default()
        };
        job
    }

    fn video_job_with(options: OutputOptions) -> DownloadJob {
        let mut job = sample_job(MediaType::Video, None, Some("1080p"));
        job.output_options = options;
        job
    }

    /// Giá trị đi ngay sau một cờ, hoặc `None` khi cờ đó không có mặt.
    fn value_after(args: &[String], flag: &str) -> Option<String> {
        args.iter()
            .position(|a| a == flag)
            .and_then(|index| args.get(index + 1))
            .cloned()
    }

    #[test]
    fn default_options_reproduce_todays_arguments_byte_for_byte() {
        // Đây là lời hứa tương thích ngược của cả lát cắt này: một tác vụ
        // không nêu lựa chọn đầu ra nào — tức mọi dòng có trước Phase 2, và
        // mọi lời gọi từ giao diện chưa cập nhật — phải sinh ra ĐÚNG danh sách
        // tham số mà bản đang phát hành sinh ra.
        //
        // So sánh nguyên cả vector với một hằng số viết tay, chứ không phải
        // `contains`: chỉ có cách này mới bắt được việc một cờ MỚI lặng lẽ
        // được thêm vào (ví dụ `--embed-metadata` bật sẵn), vốn là kiểu hồi
        // quy mà mọi assert kiểu "có chứa" đều cho đi lọt.
        let audio = sample_job(MediaType::Audio, Some("128kbps"), None);
        assert_eq!(
            args_of(&audio),
            vec![
                "--no-playlist",
                "-f",
                "bestaudio/best",
                "-x",
                "--audio-format",
                "mp3",
                "--audio-quality",
                "128K",
                "--continue",
            ]
        );

        let video = sample_job(MediaType::Video, None, Some("1080p"));
        assert_eq!(
            args_of(&video),
            vec![
                "--no-playlist",
                "-f",
                "bestvideo[vcodec^=avc1][height<=1080]+bestaudio[acodec^=mp4a]/\
                 best[vcodec^=avc1][height<=1080]/\
                 bestvideo[height<=1080]+bestaudio/best[height<=1080]",
                "--merge-output-format",
                "mp4",
                "--format-sort",
                "vcodec:avc",
                "--continue",
            ]
        );

        // Cùng lời hứa ấy cho `-o`, thứ không nằm trong danh sách tham số ở
        // trên nhưng quyết định tên file người dùng nhận được. Một tác vụ
        // không nêu lựa chọn nào và không mang tiêu đề — đúng mọi dòng có
        // trước Phase 2 — vẫn phải để yt-dlp đặt tên y như hôm nay.
        for job in [&audio, &video] {
            let naming = compose_output_naming(
                &job.output_directory,
                render_output_stem(job).as_deref(),
                job.output_options.segment.splits_chapters(),
            );
            assert_eq!(naming.main, "/tmp/%(title)s.%(ext)s");
            assert_eq!(naming.chapter, None);
        }
    }

    #[test]
    fn every_audio_format_reaches_ytdlp_as_its_own_format_name() {
        // FR-201: năm định dạng, năm giá trị `--audio-format` khác nhau. Một
        // bảng ánh xạ viết sai (hai định dạng cùng trỏ về "mp3") là lỗi lặng
        // lẽ nhất có thể có ở đây — người dùng chọn FLAC và nhận MP3.
        for (audio, expected) in [
            (AudioOutput::Mp3 { bitrate_kbps: None }, "mp3"),
            (AudioOutput::M4a { bitrate_kbps: None }, "m4a"),
            (AudioOutput::Opus { bitrate_kbps: None }, "opus"),
            (AudioOutput::Wav, "wav"),
            (AudioOutput::Flac, "flac"),
        ] {
            let args = args_of(&audio_job_with(audio.clone()));
            assert_eq!(
                value_after(&args, "--audio-format").as_deref(),
                Some(expected),
                "{audio:?} phải yêu cầu đúng định dạng của chính nó"
            );
            assert!(args.contains(&"-x".to_string()), "{audio:?} vẫn phải tách audio");
        }
    }

    #[test]
    fn keeping_the_source_format_runs_no_transcoding_step_at_all() {
        // FR-202/SC-202. Cả ba cờ dưới đây đều kéo ffmpeg vào cuộc: `-x` bật
        // bộ tách audio, `--audio-format` ép chuyển mã, `--audio-quality` chỉ
        // có nghĩa khi đang mã hoá lại. Còn bất kỳ cái nào thì lời hứa "không
        // có bước chuyển mã nào chạy" là sai.
        let args = args_of(&audio_job_with(AudioOutput::Source));

        for forbidden in ["-x", "--audio-format", "--audio-quality"] {
            assert!(
                !args.iter().any(|a| a == forbidden),
                "giữ nguyên định dạng gốc không được truyền {forbidden}, nhận được {args:?}"
            );
        }
        // ...nhưng vẫn phải tải đúng luồng audio tốt nhất, chứ không phải rơi
        // về bộ chọn mặc định vốn có thể merge nhầm hai luồng pre-muxed.
        assert_eq!(value_after(&args, "-f").as_deref(), Some("bestaudio/best"));
    }

    #[test]
    fn a_lossless_format_never_carries_a_bitrate_even_when_one_was_picked() {
        // FR-203. `audio_job_with` cố tình đặt sẵn `audio_quality =
        // "320kbps"` — đúng tình huống thật: người dùng chọn 320kbps ở bước
        // xem trước rồi mới đổi định dạng sang FLAC. Nhãn ấy vẫn nằm nguyên
        // trên job, và nếu bộ dựng tham số đọc nó vô điều kiện thì `-q 320K`
        // sẽ đi kèm một lần mã hoá không mất dữ liệu, nơi nó vô nghĩa.
        for lossless in [AudioOutput::Wav, AudioOutput::Flac] {
            let args = args_of(&audio_job_with(lossless.clone()));
            assert!(
                !args.iter().any(|a| a == "--audio-quality"),
                "{lossless:?} không được kèm bitrate, nhận được {args:?}"
            );
            assert!(
                !args.iter().any(|a| a == "320K"),
                "{lossless:?} không được để nhãn chất lượng lọt xuống, nhận được {args:?}"
            );
        }
    }

    #[test]
    fn an_explicitly_chosen_bitrate_wins_over_the_preview_label() {
        // Hai nguồn số liệu cùng tồn tại: nhãn đã đối chiếu với format thật
        // của nguồn (320kbps) và bitrate người dùng chọn thẳng ở bộ chọn định
        // dạng (128). Cái sau là lựa chọn mới hơn và cụ thể hơn nên phải thắng.
        let args = args_of(&audio_job_with(AudioOutput::Mp3 {
            bitrate_kbps: Some(128),
        }));

        assert_eq!(value_after(&args, "--audio-quality").as_deref(), Some("128K"));
    }

    #[test]
    fn quality_preference_drops_the_h264_constraints_and_the_avc_sort() {
        // FR-205. Hai nửa của cùng một lựa chọn, và bỏ sót nửa nào cũng làm
        // lựa chọn ấy vô nghĩa: bộ chọn `-f` phải thôi đòi avc1, VÀ
        // `--format-sort vcodec:avc` phải biến mất — giữ lại nó sẽ kéo mọi
        // ứng viên hoà nhau về h264 dù `-f` đã mở.
        let args = args_of(&video_job_with(OutputOptions {
            codec_preference: CodecPreference::Quality,
            ..OutputOptions::default()
        }));

        let selector = value_after(&args, "-f").expect("format selector present");
        assert!(
            !selector.contains("avc1") && !selector.contains("mp4a"),
            "ưu tiên chất lượng không được ràng buộc codec: {selector}"
        );
        assert_eq!(selector, "bestvideo[height<=1080]+bestaudio/best[height<=1080]");
        assert!(
            !args.iter().any(|a| a == "--format-sort"),
            "ưu tiên chất lượng không được giữ lại sắp xếp thiên vị h264: {args:?}"
        );
    }

    #[test]
    fn compatibility_preference_is_the_default_and_keeps_todays_behaviour() {
        // Mặc định phải là "ưu tiên tương thích" (FR-205), nếu không người
        // dùng sẵn có bỗng nhận VP9/AV1 sau một lần cập nhật.
        assert_eq!(CodecPreference::default(), CodecPreference::Compatibility);
        let args = args_of(&video_job_with(OutputOptions::default()));
        assert_eq!(value_after(&args, "--format-sort").as_deref(), Some("vcodec:avc"));
    }

    #[test]
    fn the_video_container_is_the_one_the_user_asked_for() {
        // FR-204.
        let mkv = args_of(&video_job_with(OutputOptions {
            video_container: VideoContainer::Mkv,
            ..OutputOptions::default()
        }));
        assert_eq!(value_after(&mkv, "--merge-output-format").as_deref(), Some("mkv"));

        // Giữ nguyên gốc thì không được ép remux sang bất cứ thứ gì — cờ phải
        // vắng mặt hẳn, chứ không phải mang một giá trị "trung tính" nào đó.
        let source = args_of(&video_job_with(OutputOptions {
            video_container: VideoContainer::Source,
            ..OutputOptions::default()
        }));
        assert!(
            !source.iter().any(|a| a == "--merge-output-format"),
            "giữ nguyên container gốc không được truyền cờ remux: {source:?}"
        );
    }

    #[test]
    fn metadata_and_cover_art_flags_appear_only_when_asked_for() {
        // FR-208/FR-209.
        let job = video_job_with(OutputOptions {
            embed_metadata: true,
            embed_thumbnail: true,
            ..OutputOptions::default()
        });
        let plan = build_ytdlp_args(&job, 0).unwrap();

        assert!(plan.args.contains(&"--embed-metadata".to_string()));
        assert!(plan.args.contains(&"--embed-thumbnail".to_string()));
        assert!(plan.skipped.is_empty(), "MP4 chứa được ảnh bìa, không có gì để bỏ qua");
    }

    #[test]
    fn cover_art_is_skipped_with_a_reason_instead_of_failing_the_job() {
        // FR-210, và là điểm dễ hỏng nhất của cả tính năng: truyền
        // `--embed-thumbnail` cho một container không chứa được ảnh bìa khiến
        // bộ hậu xử lý của yt-dlp ném lỗi và GIẾT tác vụ. Người dùng chọn WAV
        // rồi mất luôn cả file, chỉ vì một lựa chọn phụ không áp dụng được.
        for (label, job) in [
            ("WAV", {
                let mut job = audio_job_with(AudioOutput::Wav);
                job.output_options.embed_thumbnail = true;
                job
            }),
            ("giữ nguyên định dạng gốc", {
                let mut job = audio_job_with(AudioOutput::Source);
                job.output_options.embed_thumbnail = true;
                job
            }),
        ] {
            let plan = build_ytdlp_args(&job, 0).expect("{label}: vẫn phải dựng được tham số");

            assert!(
                !plan.args.contains(&"--embed-thumbnail".to_string()),
                "{label}: không được truyền cờ nhúng ảnh bìa"
            );
            assert_eq!(plan.skipped.len(), 1, "{label}: phải ghi lại đúng một lý do");
            assert!(
                plan.skipped[0].contains("cover art"),
                "{label}: lý do phải nói rõ chuyện gì bị bỏ qua, nhận được {:?}",
                plan.skipped[0]
            );
        }
    }

    #[test]
    fn nothing_is_reported_as_skipped_when_cover_art_was_never_requested() {
        // Mặt còn lại: nếu người dùng không bật nhúng ảnh bìa thì WAV cũng
        // không có gì để "bỏ qua" — một dòng nhật ký giải thích thứ chưa ai
        // yêu cầu chỉ làm nhiễu.
        let plan = build_ytdlp_args(&audio_job_with(AudioOutput::Wav), 0).unwrap();
        assert!(plan.skipped.is_empty());
    }

    // ---- specs/003-media-output: đặt tên file (FR-212→FR-216) -------------

    /// Job video có tiêu đề thật — điều kiện để phần đặt tên của chúng ta được
    /// chạy thay vì nhường lại cho yt-dlp.
    fn titled_job(title: &str) -> DownloadJob {
        let mut job = sample_job(MediaType::Video, None, Some("1080p"));
        job.title = Some(title.to_string());
        job
    }

    #[test]
    fn a_job_without_a_title_still_lets_ytdlp_name_the_file_exactly_as_today() {
        // Mục fan-out từ playlist phẳng không mang tiêu đề nào. Tự đặt tên ở
        // đó sẽ cho ra `untitled`, `untitled (2)`, `untitled (3)`... trong khi
        // yt-dlp lúc tải đã có tiêu đề thật trong tay.
        let job = sample_job(MediaType::Video, None, Some("1080p"));
        assert!(render_output_stem(&job).is_none());

        let naming = compose_output_naming(&job.output_directory, None, false);
        assert_eq!(naming.main, "/tmp/%(title)s.%(ext)s");
        assert_eq!(naming.chapter, None);
        assert_eq!(naming.chapter_prefix, None);
    }

    #[test]
    fn a_titled_job_is_named_by_us_so_sanitising_and_dedup_actually_run() {
        // FR-214: `/` và `:` trong tiêu đề phải rụng TRƯỚC khi tên tới yt-dlp.
        // Nếu để `%(title)s` thì yt-dlp mới là bên đặt tên, và cả bước làm sạch
        // lẫn bước chống ghi đè của chúng ta không có gì để chạy.
        let job = titled_job("AC/DC: Back in Black?");
        let stem = render_output_stem(&job).expect("job có tiêu đề thì ta tự đặt tên");
        assert_eq!(stem, "AC_DC_ Back in Black_");

        let naming = compose_output_naming(&job.output_directory, Some(&stem), false);
        assert_eq!(naming.main, "/tmp/AC_DC_ Back in Black_.%(ext)s");
    }

    #[test]
    fn a_percent_in_the_name_or_the_folder_is_handed_over_as_a_literal() {
        // yt-dlp đọc `%` là mở đầu một trường mẫu ở BẤT KỲ đâu trong `-o`, nên
        // một tiêu đề `100% Real` (hoặc một thư mục `100% Music`) sẽ khiến nó
        // ghi ra một cái tên khác hẳn, hoặc chết vì mẫu không hợp lệ.
        let job = titled_job("100% Real");
        let stem = render_output_stem(&job).unwrap();
        let naming = compose_output_naming("/tmp/100% Music", Some(&stem), false);

        assert_eq!(naming.main, "/tmp/100%% Music/100%% Real.%(ext)s");
    }

    #[test]
    fn a_template_field_the_job_can_fill_reaches_the_name() {
        let mut job = titled_job("Bài hát");
        job.output_options.filename_template = "{title} [{resolution}]".to_string();

        assert_eq!(render_output_stem(&job).unwrap(), "Bài hát [1080p]");
    }

    #[test]
    fn the_extension_is_never_written_twice() {
        // `.%(ext)s` luôn được nối vào cuối, nên `{ext}` ở cuối mẫu phải biến
        // mất — nếu không, `{title}.{ext}` cho ra `Bài hát.mp4.mp4`.
        for template in ["{title}.{ext}", "{title}{ext}", "{title}.{ext}  "] {
            assert_eq!(strip_trailing_ext_field(template), "{title}", "{template}");
        }
        // Ở giữa mẫu thì người dùng đang cố tình chèn nó vào tên, giữ nguyên.
        assert_eq!(
            strip_trailing_ext_field("{title}.{ext}.backup"),
            "{title}.{ext}.backup"
        );

        let mut job = titled_job("Bài hát");
        job.output_options.filename_template = "{title}.{ext}".to_string();
        let naming = compose_output_naming(
            &job.output_directory,
            Some(&render_output_stem(&job).unwrap()),
            false,
        );
        assert_eq!(naming.main, "/tmp/Bài hát.%(ext)s");
    }

    #[test]
    fn an_existing_file_with_any_extension_pushes_the_new_name_aside() {
        // FR-215. So theo tên KHÔNG phần mở rộng là điểm mấu chốt: lúc đặt tên
        // ta chưa biết file sẽ là `.mp4` hay `.webm`, nên `Bài hát.mp3` đã có
        // sẵn cũng phải tính là đã chiếm chỗ.
        let taken = file_stems(&["Bài hát.mp3".to_string(), "khác.mp4".to_string()]);

        assert_eq!(unique_stem("/tmp", "Bài hát", &taken), "Bài hát (2)");
        assert_eq!(unique_stem("/tmp", "Bài hát khác", &taken), "Bài hát khác");
    }

    #[test]
    fn two_jobs_for_the_same_title_never_claim_the_same_stem() {
        // The bug this exists for: two jobs for the same URL, both started
        // before either had written anything to disk, both saw an "empty"
        // directory and both picked the identical stem — then stomped on
        // each other's yt-dlp temp fragment files mid-download (`WinError 2`
        // deleting a file the other job was still reading, `WinError 183`
        // renaming a `.temp.mp4` onto a name the other job's rename had
        // already claimed). `unique_stem` alone can't catch this — it only
        // looks at what's already on disk, and neither job has written
        // anything yet at the moment both check.
        let registry = Arc::new(StdMutex::new(HashSet::new()));
        let empty = HashSet::new();

        let (first, _first_guard) = claim_output_stem("/tmp", "Bài hát", &empty, &registry);
        let (second, _second_guard) = claim_output_stem("/tmp", "Bài hát", &empty, &registry);
        assert_ne!(first, second, "two concurrent jobs must not land on the same stem");
        assert_eq!(first, "Bài hát");
        assert_eq!(second, "Bài hát (2)");

        // Once the first job's guard is dropped (its `run_job` returned —
        // success, error, or cancel), that name is free again for a later
        // job to pick.
        drop(_first_guard);
        let (third, _third_guard) = claim_output_stem("/tmp", "Bài hát", &empty, &registry);
        assert_eq!(third, "Bài hát", "a released claim must become available again");
    }

    // ---- specs/003-media-output: phụ đề (FR-217→FR-221) -------------------

    fn subtitled_video_job(subtitles: SubtitleOptions) -> DownloadJob {
        video_job_with(OutputOptions {
            subtitles,
            ..OutputOptions::default()
        })
    }

    #[test]
    fn several_languages_travel_as_one_comma_separated_argument() {
        // FR-218. `--sub-langs` nhận MỘT đối số; đẩy mỗi ngôn ngữ thành một cờ
        // riêng thì yt-dlp chỉ thấy cái cuối cùng.
        let args = args_of(&subtitled_video_job(SubtitleOptions {
            languages: vec!["vi".into(), "en".into(), "vi".into()],
            ..SubtitleOptions::default()
        }));

        assert_eq!(value_after(&args, "--sub-langs").as_deref(), Some("vi,en"));
        assert!(args.contains(&"--write-subs".to_string()));
        assert!(!args.contains(&"--embed-subs".to_string()));
    }

    #[test]
    fn embedding_and_separate_files_are_two_different_flags() {
        // FR-219. `--embed-subs` tự lo phần tải rồi xoá file tạm, nên đi kèm
        // `--write-subs` sẽ để lại đúng những file rời người dùng vừa từ chối.
        let args = args_of(&subtitled_video_job(SubtitleOptions {
            languages: vec!["vi".into()],
            delivery: SubtitleDelivery::Embedded,
            ..SubtitleOptions::default()
        }));

        assert!(args.contains(&"--embed-subs".to_string()));
        assert!(
            !args.contains(&"--write-subs".to_string()),
            "nhúng mà vẫn ghi file rời thì người dùng nhận cả hai: {args:?}"
        );
    }

    #[test]
    fn auto_generated_subtitles_need_a_flag_of_their_own() {
        // FR-217 scenario 4: video chỉ có phụ đề máy sinh. Chúng nằm ở một kho
        // khác của yt-dlp (`automatic_captions`), nên thiếu cờ này thì tác vụ
        // về tay không mà không có lỗi nào.
        let without = args_of(&subtitled_video_job(SubtitleOptions {
            languages: vec!["en".into()],
            ..SubtitleOptions::default()
        }));
        assert!(!without.contains(&"--write-auto-subs".to_string()));

        let with = args_of(&subtitled_video_job(SubtitleOptions {
            languages: vec!["en".into()],
            include_auto_generated: true,
            ..SubtitleOptions::default()
        }));
        assert!(with.contains(&"--write-auto-subs".to_string()));
    }

    #[test]
    fn embedding_into_something_that_cannot_hold_subtitles_is_skipped_with_a_reason() {
        // FR-220 + FR-210: cùng luật với ảnh bìa. Truyền `--embed-subs` cho một
        // file MP3 khiến bộ hậu xử lý của yt-dlp ném lỗi và GIẾT tác vụ — người
        // dùng mất luôn cả bản nhạc vì một lựa chọn phụ không áp dụng được.
        for (label, job) in [
            ("audio", {
                let mut job = audio_job_with(AudioOutput::Mp3 { bitrate_kbps: None });
                job.output_options.subtitles = SubtitleOptions {
                    languages: vec!["vi".into()],
                    delivery: SubtitleDelivery::Embedded,
                    include_auto_generated: false,
                };
                job
            }),
            ("giữ nguyên container gốc", {
                video_job_with(OutputOptions {
                    video_container: VideoContainer::Source,
                    subtitles: SubtitleOptions {
                        languages: vec!["vi".into()],
                        delivery: SubtitleDelivery::Embedded,
                        include_auto_generated: false,
                    },
                    ..OutputOptions::default()
                })
            }),
        ] {
            let plan = build_ytdlp_args(&job, 0).expect("{label}: vẫn phải dựng được tham số");

            assert!(
                !plan.args.iter().any(|a| a == "--embed-subs"),
                "{label}: không được truyền cờ nhúng phụ đề"
            );
            assert!(
                !plan.args.iter().any(|a| a == "--sub-langs"),
                "{label}: bỏ qua bước phụ đề thì cũng không tải phụ đề về"
            );
            assert_eq!(plan.skipped.len(), 1, "{label}: phải ghi lại đúng một lý do");
            assert!(
                plan.skipped[0].contains("subtitles"),
                "{label}: lý do phải nói rõ thứ bị bỏ qua, nhận được {:?}",
                plan.skipped[0]
            );
        }
    }

    #[test]
    fn separate_subtitle_files_still_work_for_an_audio_job() {
        // Chỉ có bước NHÚNG là bất khả thi với file audio; file `.srt` nằm
        // cạnh bản nhạc thì hoàn toàn bình thường và không có gì để bỏ qua.
        let mut job = audio_job_with(AudioOutput::Mp3 { bitrate_kbps: None });
        job.output_options.subtitles = SubtitleOptions {
            languages: vec!["vi".into()],
            ..SubtitleOptions::default()
        };
        let plan = build_ytdlp_args(&job, 0).unwrap();

        assert!(plan.args.contains(&"--write-subs".to_string()));
        assert!(plan.skipped.is_empty());
    }

    #[test]
    fn no_language_chosen_means_no_subtitle_flag_at_all() {
        let plan = build_ytdlp_args(&video_job_with(OutputOptions::default()), 0).unwrap();
        assert!(!plan.args.iter().any(|a| a.starts_with("--sub")));
        assert!(!plan.args.iter().any(|a| a.contains("subs")));
    }

    // ---- specs/003-media-output: cắt đoạn & chương (FR-222→FR-227) --------

    fn trimmed_job(range: TrimRange) -> DownloadJob {
        video_job_with(OutputOptions {
            segment: SegmentMode::Trim(range),
            ..OutputOptions::default()
        })
    }

    #[test]
    fn the_download_sections_argument_is_exactly_ytdlps_time_range_syntax() {
        // Cú pháp này không có đường nào kiểm chứng ngoài việc viết ra chuỗi
        // mong đợi: thiếu dấu `*` thì yt-dlp đem chuỗi đi khớp TÊN CHƯƠNG và
        // không tải gì; sai dấu `-` thì nó báo "invalid time range".
        assert_eq!(
            download_sections_arg(&TrimRange {
                start_seconds: Some(750.0),
                end_seconds: Some(900.0),
                accurate_cut: false,
            }),
            "*750-900"
        );
        // Chỉ có mốc bắt đầu: `inf` là chính từ khoá yt-dlp dùng cho "tới hết".
        assert_eq!(
            download_sections_arg(&TrimRange {
                start_seconds: Some(750.5),
                end_seconds: None,
                accurate_cut: false,
            }),
            "*750.5-inf"
        );
        // Chỉ có mốc kết thúc: bắt đầu từ 0 chứ không phải bỏ trống.
        assert_eq!(
            download_sections_arg(&TrimRange {
                start_seconds: None,
                end_seconds: Some(90.25),
                accurate_cut: false,
            }),
            "*0-90.25"
        );
    }

    #[test]
    fn a_time_never_reaches_ytdlp_in_scientific_notation() {
        // `format!("{}", 0.0000001)` cho ra `1e-7`, thứ yt-dlp không đọc được
        // như một số giây.
        assert_eq!(format_seconds(0.0000001), "0");
        assert_eq!(format_seconds(12.0), "12");
        assert_eq!(format_seconds(1000.0), "1000");
        assert_eq!(format_seconds(12.3456), "12.346");
    }

    #[test]
    fn trimming_passes_the_range_and_only_asks_for_slow_cuts_when_told_to() {
        let plain = args_of(&trimmed_job(TrimRange {
            start_seconds: Some(750.0),
            end_seconds: Some(900.0),
            accurate_cut: false,
        }));
        assert_eq!(
            value_after(&plain, "--download-sections").as_deref(),
            Some("*750-900")
        );
        assert!(
            !plain.contains(&"--force-keyframes-at-cuts".to_string()),
            "cắt thường phải nhanh như cũ: {plain:?}"
        );

        // FR-224: cắt chính xác là một lựa chọn riêng, và là lựa chọn đắt.
        let accurate = args_of(&trimmed_job(TrimRange {
            start_seconds: Some(750.0),
            end_seconds: Some(900.0),
            accurate_cut: true,
        }));
        assert!(accurate.contains(&"--force-keyframes-at-cuts".to_string()));
    }

    #[test]
    fn an_impossible_time_range_stops_the_job_here_instead_of_at_ytdlp() {
        // FR-223. Giao diện chặn trước, nhưng `create_download_job` gọi trực
        // tiếp được — nên phép kiểm tra thật phải nằm trên đường đi của mọi
        // lời gọi.
        let job = trimmed_job(TrimRange {
            start_seconds: Some(900.0),
            end_seconds: Some(750.0),
            accurate_cut: false,
        });
        let err = build_ytdlp_args(&job, 0).expect_err("khoảng ngược phải bị từ chối");

        assert_eq!(err.code, "INVALID_TRIM_RANGE");
        assert!(
            err.message.contains("end time"),
            "lỗi phải nói được điều gì sai, nhận được {}",
            err.message
        );
    }

    #[test]
    fn splitting_by_chapter_asks_yt_dlp_for_exactly_that() {
        let args = args_of(&video_job_with(OutputOptions {
            segment: SegmentMode::SplitChapters,
            ..OutputOptions::default()
        }));
        assert!(args.contains(&"--split-chapters".to_string()));
    }

    #[test]
    fn a_job_can_never_ask_for_both_trimming_and_a_chapter_split() {
        // FR-226. Không có phép kiểm tra lúc chạy nào ở đây vì không cần: hai
        // lựa chọn là hai biến thể của cùng một enum, nên tổ hợp cấm không
        // dựng nổi. Test này canh đúng điều đó ở đầu ra — nếu ai đó sau này
        // tách chúng thành hai trường ngang hàng, nó sẽ đỏ.
        let trimmed = args_of(&trimmed_job(TrimRange {
            start_seconds: Some(10.0),
            end_seconds: Some(20.0),
            accurate_cut: false,
        }));
        assert!(!trimmed.contains(&"--split-chapters".to_string()));

        let split = args_of(&video_job_with(OutputOptions {
            segment: SegmentMode::SplitChapters,
            ..OutputOptions::default()
        }));
        assert!(!split.iter().any(|a| a == "--download-sections"));
        assert!(!split.iter().any(|a| a == "--force-keyframes-at-cuts"));
    }

    #[test]
    fn a_chapter_split_names_its_chapter_files_from_the_same_stem() {
        // Không truyền `-o chapter:` thì yt-dlp rơi về mẫu mặc định của nó cho
        // file chương, và toàn bộ phần làm sạch/chống ghi đè ở trên không áp
        // cho chính những file mà tác vụ này sinh ra.
        let naming = compose_output_naming("/tmp", Some("Podcast tập 3"), true);

        assert_eq!(
            naming.chapter.as_deref(),
            Some("chapter:/tmp/Podcast tập 3 - %(section_number)03d %(section_title)s.%(ext)s")
        );
        assert_eq!(naming.chapter_prefix.as_deref(), Some("Podcast tập 3 - "));

        // Tác vụ không tách chương thì không có mẫu chương nào, và cũng không
        // có gì để đếm sau đó.
        let plain = compose_output_naming("/tmp", Some("Podcast tập 3"), false);
        assert_eq!(plain.chapter, None);
        assert_eq!(plain.chapter_prefix, None);
    }

    #[test]
    fn only_the_chapter_files_this_run_created_are_counted() {
        // FR-227: con số phải là số file lần chạy NÀY tạo ra. Một lần chạy
        // trước (hoặc một tác vụ khác cùng thư mục) đã để lại file trùng tiền
        // tố, và tính cả chúng vào là báo cho người dùng một con số sai.
        let before: HashSet<String> = ["Podcast - 001 Cũ.mp4".to_string(), "khác.mp4".to_string()]
            .into_iter()
            .collect();
        let after = vec![
            "Podcast - 001 Cũ.mp4".to_string(),
            "Podcast - 002 Mới.mp4".to_string(),
            "Podcast - 003 Mới nữa.mp4".to_string(),
            "Podcast.mp4".to_string(),
            "khác.mp4".to_string(),
        ];

        let created = new_chapter_file_names(&before, &after, "Podcast - ");

        assert_eq!(created.len(), 2);
        assert!(created.contains(&"Podcast - 002 Mới.mp4".to_string()));
        // File gốc không mang tiền tố chương nên không bị tính là một chương.
        assert!(!created.contains(&"Podcast.mp4".to_string()));
    }

    #[test]
    fn build_ytdlp_args_refuses_a_gallery_job_defensively() {
        // run_job branches to run_gallery_job before this is ever reached in
        // practice — this just guards the invariant.
        let job = sample_job(MediaType::Gallery, None, None);
        assert!(build_ytdlp_args(&job, 0).is_err());
    }

    #[test]
    fn pause_all_leaves_already_paused_jobs_alone() {
        // `Db::bulk_update_status` trả về id của những job KHỚP điều kiện, chứ
        // không phải những job thực sự đổi trạng thái. Nếu `paused` lọt vào
        // danh sách nguồn thì mọi job vốn đã tạm dừng cũng bị `apply_bulk` coi
        // là "vừa đổi": giao diện nhận `job:status_changed` thừa, và tệ hơn,
        // `clear_retry_deadline` xoá luôn bộ đếm thử lại của chúng.
        let plan = bulk_plan(BulkAction::Pause);
        assert!(!plan.from_statuses.contains(&JobStatus::Paused));
        assert_eq!(
            plan.from_statuses,
            vec![
                JobStatus::Queued,
                JobStatus::FetchingMetadata,
                JobStatus::Downloading
            ]
        );
        assert_eq!(plan.to_status, JobStatus::Paused);
    }

    #[test]
    fn cancel_all_also_cancels_jobs_that_are_merely_paused() {
        // "Huỷ tất cả" mà bỏ sót job đang tạm dừng thì chúng vẫn nằm nguyên
        // trong hàng đợi sau khi người dùng vừa bấm huỷ tất cả — và tiếp tục
        // được lúc nào cũng chạy lại.
        let plan = bulk_plan(BulkAction::Cancel);
        assert!(plan.from_statuses.contains(&JobStatus::Paused));
        assert_eq!(plan.from_statuses, UNFINISHED_STATUSES.to_vec());
        assert_eq!(plan.to_status, JobStatus::Canceled);
    }

    #[test]
    fn resume_all_only_ever_touches_paused_jobs() {
        // Một job `downloading` bị đánh dấu `queued` sẽ được dispatcher chọn
        // lại và chạy lần thứ hai song song với chính nó; một job `canceled`
        // hay `failed` bị lôi về hàng chờ là tự ý chạy lại thứ người dùng đã
        // dừng.
        let plan = bulk_plan(BulkAction::Resume);
        assert_eq!(plan.from_statuses, vec![JobStatus::Paused]);
        assert_eq!(plan.to_status, JobStatus::Queued);
    }

    #[test]
    fn only_the_stopping_actions_kill_running_processes_and_refund_retries() {
        // `stops_jobs` điều khiển hai việc trong `apply_bulk`: gửi tín hiệu
        // huỷ cho tiến trình đang chạy TRƯỚC khi ghi trạng thái, và gọi
        // `clear_retry_deadline` để một lần dừng hàng loạt giống hệt N lần
        // dừng đơn lẻ (FR-123).
        assert!(bulk_plan(BulkAction::Pause).stops_jobs);
        assert!(bulk_plan(BulkAction::Cancel).stops_jobs);
        // Tiếp tục thì ngược lại: giết tiến trình ở đây là giết đúng những job
        // vừa được cho chạy tiếp.
        assert!(!bulk_plan(BulkAction::Resume).stops_jobs);
    }

    #[test]
    fn bulk_actions_never_reopen_a_finished_job() {
        // Chỉ trạng thái chưa kết thúc mới được đụng tới: `completed`,
        // `failed` và `canceled` là trạng thái cuối, một lệnh hàng loạt không
        // được lôi chúng trở lại hàng đợi.
        for action in [BulkAction::Pause, BulkAction::Resume, BulkAction::Cancel] {
            let plan = bulk_plan(action);
            for terminal in [JobStatus::Completed, JobStatus::Failed, JobStatus::Canceled] {
                assert!(
                    !plan.from_statuses.contains(&terminal),
                    "{action:?} không được nhận job đang ở {terminal:?} làm nguồn"
                );
            }
        }
    }

    #[test]
    fn sanitize_path_component_strips_characters_invalid_as_a_filename() {
        let cleaned = sanitize_path_component("Cool: Post? / Title \\ <weird>");
        assert!(!cleaned.contains(['<', '>', ':', '"', '/', '\\', '|', '?', '*']));
    }

    #[test]
    fn sanitize_path_component_falls_back_when_everything_gets_stripped() {
        assert_eq!(sanitize_path_component("////"), "gallery");
    }

    #[test]
    fn sanitize_path_component_truncates_very_long_titles() {
        let long_title = "a".repeat(500);
        assert!(sanitize_path_component(&long_title).len() <= 80);
    }
}
