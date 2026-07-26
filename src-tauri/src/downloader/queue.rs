use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use chrono::Utc;
use tauri::{AppHandle, Emitter};
use tokio::sync::{watch, Mutex as AsyncMutex, Notify};

use crate::db::Db;
use crate::error::{AppError, CANCELED_ERROR_CODE};
use crate::logging::log_event;
use crate::models::{DownloadJob, GalleryMode, JobStatus, MediaType};

use super::gallery_dl;
use super::retry::{decide_outcome, Outcome};
use super::scheduler::{available_slots, TICK_INTERVAL_MS};
use super::ytdlp;
use super::ytdlp_binary;

/// Upper bound on redownload attempts when the output has no audio track
/// (see `ytdlp::output_has_audio_stream`). yt-dlp issue #15891: TikTok's CDN
/// can intermittently serve a video-only file under a format id whose
/// metadata still claims `acodec=aac`; maintainers confirmed re-downloading
/// commonly gets a different, correct file, so a couple of retries is a real
/// fix here, not just a delay.
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
    progress_percent: f64,
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
}

/// Bản sao các handle dùng chung, để task nền không phải giữ `&DownloadQueue`.
#[derive(Clone)]
struct QueueHandles {
    db: Arc<Db>,
    app: AppHandle,
    running: Arc<AsyncMutex<HashMap<String, RunningJob>>>,
    max_concurrent: Arc<AtomicUsize>,
    wake: Arc<Notify>,
}

impl DownloadQueue {
    pub fn new(db: Arc<Db>, app: AppHandle, max_concurrent: usize) -> Self {
        let queue = Self {
            db,
            app,
            running: Arc::new(AsyncMutex::new(HashMap::new())),
            max_concurrent: Arc::new(AtomicUsize::new(max_concurrent.clamp(1, 8))),
            wake: Arc::new(Notify::new()),
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
        .insert(job_id.clone(), RunningJob { cancel_tx, run_id });

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

    let output_template = format!("{}/%(title)s.%(ext)s", job.output_directory);
    // Đọc lại mỗi lần chạy chứ không cache lúc dựng hàng đợi: người dùng đổi
    // giới hạn tốc độ thì job được khởi chạy sau đó phải dùng giá trị mới.
    let rate_limit_kbps = handles
        .db
        .get_settings()
        .map(|settings| settings.rate_limit_kbps)
        .unwrap_or(0);
    let extra_args = build_ytdlp_args(&job, rate_limit_kbps)?;

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
        let path = download_result?;

        // Only muxed video jobs are at risk of a *silent* audio loss: an
        // audio-only extraction (`-x`) fails loudly instead (ffmpeg can't
        // extract a stream that isn't there), so it surfaces as a real error
        // that `finish_job` gets to classify.
        if job.media_type == MediaType::Video && !ytdlp::output_has_audio_stream(&path).await {
            if attempt < MAX_NO_AUDIO_ATTEMPTS {
                log_event(
                    &handles.app,
                    "WARN",
                    format!("Job {} attempt {attempt}/{MAX_NO_AUDIO_ATTEMPTS}: downloaded video had no audio track, retrying", job.id),
                );
                let _ = tokio::fs::remove_file(&path).await;
                continue;
            }
            // Last attempt still missing audio. Per the community workaround
            // documented on the same yt-dlp issues (TikTok can consistently
            // serve a video-only file for a given format on videos where
            // download is disabled, so retrying the identical request isn't
            // guaranteed to help): separately fetch just the best audio
            // track and mux it onto the otherwise-good video, rather than
            // discarding a mostly-fine download over its audio track alone.
            match recover_missing_audio(&handles.app, &job.source_url, &path).await {
                Ok(fixed_path) => {
                    output_path = Some(fixed_path);
                    break;
                }
                Err(_) => {
                    let _ = tokio::fs::remove_file(&path).await;
                    return Err(AppError::new(
                        "DOWNLOAD_FAILED",
                        "The source served a video with no audio track after multiple attempts, and no separate audio track could be recovered. Please try again.",
                    ));
                }
            }
        }

        output_path = Some(path);
        break;
    }
    let output_path = output_path.expect("loop always returns via `?`/cancel or sets output_path");

    let metadata = tokio::fs::metadata(&output_path).await.ok();
    let file_size = metadata.map(|m| m.len() as i64).unwrap_or(0);
    let file_format = std::path::Path::new(&output_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_string();

    handles
        .db
        .insert_downloaded_file(&job.id, &output_path, &file_format, file_size)?;
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

/// Fallback per-image duration in `GalleryMode::Slideshow`, used only when
/// the audio track's actual length can't be probed (see
/// `probe_audio_duration_secs`) — normally each image's display time is the
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
            let percent = (update.completed_files as f64 / update.total_files as f64) * 100.0;
            let _ = db_for_progress.update_job_progress(&job_id_for_progress, percent, None, None);
            emit_progress(
                &app_for_progress,
                &job_id_for_progress,
                &ytdlp::ProgressUpdate {
                    percent,
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

    let metadata = tokio::fs::metadata(&output_path).await.ok();
    let file_size = metadata.map(|m| m.len() as i64).unwrap_or(0);
    let file_format = std::path::Path::new(&output_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_string();

    handles
        .db
        .insert_downloaded_file(&job.id, &output_path, &file_format, file_size)?;
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

/// Reads the audio track's real duration via ffmpeg's own stderr banner
/// (`  Duration: 00:00:12.34, start: ...`) — no `ffprobe` needed (this
/// project doesn't bundle it). `ffmpeg -i <file>` always prints this line
/// once it's parsed the input, even with no output specified, so this just
/// discards everything ffmpeg would otherwise fail on past that point.
async fn probe_audio_duration_secs(ffmpeg_path: &std::path::Path, audio_path: &str) -> Option<f64> {
    let output = tokio::process::Command::new(ffmpeg_path)
        .args(["-i", audio_path])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .await
        .ok()?;
    parse_ffmpeg_duration_line(&String::from_utf8_lossy(&output.stderr))
}

/// Pure parsing half of `probe_audio_duration_secs`, split out so it's
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
/// audio's own real length (probed via `probe_audio_duration_secs`) divided
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

    let probed_audio_secs = probe_audio_duration_secs(&ffmpeg_path, audio_path).await;
    let image_duration_secs = compute_image_duration_secs(probed_audio_secs, image_paths.len());
    // The transition borrows time from both the clip it leaves and the one
    // it enters, so it must stay well under a single image's own display
    // time — otherwise a very short per-image duration (many images, short
    // audio) would make consecutive transitions overlap each other.
    let transition_secs = SLIDESHOW_TRANSITION_SECS.min(image_duration_secs * 0.3);

    let mut cmd = tokio::process::Command::new(&ffmpeg_path);
    cmd.current_dir(job_dir).arg("-y");

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

    Ok(format!("{job_dir}/{output_file_name}"))
}

/// Last-resort recovery for the documented yt-dlp/TikTok audio-loss bug
/// (yt-dlp issues #15891, #15642): separately fetch just the best audio
/// track for the same URL and mux it onto the already-downloaded (but
/// audio-less) video in place. This mirrors the workaround multiple
/// reporters on those issues converged on independently — after
/// maintainers confirmed TikTok's CDN itself serves inconsistent media
/// (the same format id sometimes has audio, sometimes doesn't, despite
/// identical metadata) and closed both issues with no code fix in yt-dlp —
/// since re-requesting the *exact same* video format isn't guaranteed to
/// get a different result (notably on videos where TikTok disables
/// downloads, where it was reported as consistently silent), but a
/// differently-scoped audio-only request has an independent chance of
/// landing on a working response.
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
    let audio_path = ytdlp::run_download(app, source_url, &audio_template, audio_args, |_| {}).await?;

    if !ytdlp::output_has_audio_stream(&audio_path).await {
        let _ = tokio::fs::remove_file(&audio_path).await;
        return Err(AppError::new(
            "DOWNLOAD_FAILED",
            "Recovered audio track has no audio either",
        ));
    }

    let muxed_path = format!("{video_path}.muxed.mp4");
    let ffmpeg_path = ytdlp_binary::resolve_ffmpeg_path()?;
    let status = tokio::process::Command::new(&ffmpeg_path)
        .args([
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
        ])
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

/// `rate_limit_kbps` bằng 0 nghĩa là không giới hạn. Giới hạn này áp cho từng
/// tiến trình yt-dlp, không phải tổng băng thông của ứng dụng — với N job chạy
/// song song, tổng thực tế có thể tới N lần mức này. Giao diện Cài đặt phải nói
/// rõ điều đó.
fn build_ytdlp_args(job: &DownloadJob, rate_limit_kbps: u32) -> Result<Vec<String>, AppError> {
    // `--no-playlist` on every single-item job (audio or video) is a
    // deliberate safety net for FR-013: a URL copied from inside a playlist
    // often still carries a `&list=...` param, and without this flag yt-dlp
    // would silently download the whole playlist instead of just this item.
    // Jobs created for a confirmed `entire_playlist` fan-out (T033) are each
    // their own per-entry URL, so this flag has no effect on them either way.
    let mut args = vec!["--no-playlist".to_string()];

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
            args.push("-x".into());
            args.push("--audio-format".into());
            args.push("mp3".into());
            args.push("--audio-quality".into());
            args.push(match job.audio_quality.as_deref() {
                Some(quality) => {
                    let bitrate_kbps = parse_leading_number(quality).ok_or_else(|| {
                        AppError::new(
                            "INVALID_QUALITY_OPTION",
                            format!("Cannot parse audio quality: {quality}"),
                        )
                    })?;
                    format!("{bitrate_kbps}K")
                }
                // No quality was validated against a real format list (playlist
                // fan-out items skip that step — see download.rs), so ask
                // yt-dlp for its own best available VBR encoding instead of
                // guessing a bitrate.
                None => "0".to_string(),
            });
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
            args.push(video_format_selector(height));
            args.push("--merge-output-format".into());
            args.push("mp4".into());
            // TikTok's audio-loss bug (yt-dlp issues #15891/#15642) was
            // reported far more often on `bytevc1`/h265 formats than h264 —
            // this is the community-confirmed mitigation (`-S "vcodec:avc"`)
            // layered on top of `video_format_selector`'s own avc1-first `-f`
            // chain, so a tied fallback still leans h264 instead of h265.
            args.push("--format-sort".into());
            args.push("vcodec:avc".into());
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

    if rate_limit_kbps > 0 {
        args.push("--limit-rate".into());
        args.push(format!("{rate_limit_kbps}K"));
    }

    args.push("--continue".into());
    Ok(args)
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
fn video_format_selector(height: Option<u32>) -> String {
    match height {
        Some(h) => format!(
            "bestvideo[vcodec^=avc1][height<={h}]+bestaudio[acodec^=mp4a]/\
             best[vcodec^=avc1][height<={h}]/\
             bestvideo[height<={h}]+bestaudio/best[height<={h}]"
        ),
        None => "bestvideo[vcodec^=avc1]+bestaudio[acodec^=mp4a]/\
                 best[vcodec^=avc1]/bestvideo+bestaudio/best"
            .to_string(),
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
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

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
        }
    }

    #[test]
    fn audio_args_use_selected_bitrate_not_a_hardcoded_constant() {
        let job = sample_job(MediaType::Audio, Some("128kbps"), None);
        let args = build_ytdlp_args(&job, 0).unwrap();
        assert!(args.contains(&"128K".to_string()));

        let job_high = sample_job(MediaType::Audio, Some("320kbps"), None);
        let args_high = build_ytdlp_args(&job_high, 0).unwrap();
        assert!(args_high.contains(&"320K".to_string()));
    }

    #[test]
    fn audio_downloads_explicitly_select_bestaudio_instead_of_the_ambiguous_default() {
        // Regression test: without an explicit `-f`, yt-dlp's default
        // `bestvideo*+bestaudio` selector can pick two different pre-muxed
        // formats on sites like TikTok (every format has both video and
        // audio, no dedicated audio-only stream) and merge them incorrectly,
        // producing a file with no audio track once `-x` extracts from it.
        let job = sample_job(MediaType::Audio, Some("128kbps"), None);
        let args = build_ytdlp_args(&job, 0).unwrap();
        let f_index = args.iter().position(|a| a == "-f").expect("-f flag present");
        assert_eq!(args[f_index + 1], "bestaudio/best");
    }

    #[test]
    fn video_args_select_nearest_available_height_via_format_selector() {
        let job = sample_job(MediaType::Video, None, Some("1080p"));
        let args = build_ytdlp_args(&job, 0).unwrap();
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
        let args = build_ytdlp_args(&job, 0).unwrap();
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
        let args = build_ytdlp_args(&job, 0).unwrap();
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
        let audio_args = build_ytdlp_args(&audio_job, 0).unwrap();
        assert!(audio_args.contains(&"0".to_string()));

        let video_job = sample_job(MediaType::Video, None, None);
        let video_args = build_ytdlp_args(&video_job, 0).unwrap();
        assert!(video_args
            .iter()
            .any(|a| a.ends_with("bestvideo+bestaudio/best")));
    }

    #[test]
    fn every_single_item_job_disables_implicit_playlist_download() {
        let job = sample_job(MediaType::Audio, Some("128kbps"), None);
        let args = build_ytdlp_args(&job, 0).unwrap();
        assert_eq!(args.first(), Some(&"--no-playlist".to_string()));
    }

    #[test]
    fn adds_rate_limit_flag_when_configured() {
        let job = sample_job(MediaType::Audio, Some("128kbps"), None);
        let args = build_ytdlp_args(&job, 512).expect("args build");

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
        let args = build_ytdlp_args(&job, 0).expect("args build");

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
