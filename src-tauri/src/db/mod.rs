use std::path::Path;
use std::sync::Mutex;

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use rusqlite_migration::{Migrations, M};
use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::models::{
    AppSettings, DownloadJob, GalleryMode, HistoryQuery, JobStatus, LibraryBreakdownEntry,
    LibraryItem, LibraryQuery, LibraryStats, MediaType, NewLibraryFile, OutputOptions,
};

fn migrations() -> Migrations<'static> {
    Migrations::new(vec![
        M::up(include_str!("migrations/0001_init.sql")),
        M::up(include_str!("migrations/0002_gallery_support.sql")),
        M::up(include_str!("migrations/0003_gallery_images_only_mode.sql")),
        M::up(include_str!("migrations/0004_settings_key_value.sql")),
        M::up(include_str!("migrations/0005_fix_stale_app_settings_schema.sql")),
        M::up(include_str!("migrations/0006_gallery_selected_urls.sql")),
        M::up(include_str!("migrations/0007_job_titles.sql")),
        M::up(include_str!("migrations/0008_queue_scheduling.sql")),
        M::up(include_str!("migrations/0009_backfill_completed_progress.sql")),
        M::up(include_str!("migrations/0010_job_output_options.sql")),
        M::up(include_str!("migrations/0011_output_presets.sql")),
        M::up(include_str!("migrations/0012_library_index.sql")),
    ])
}

/// SQL fragment shared by every statement in this module that writes
/// `download_jobs.status`, binding the new status as `?1`.
///
/// A row whose status is `completed` must also read 100%: the job is finished
/// by definition, so a lower stored percentage contradicts its own status.
/// Keeping this next to the status write — rather than in a separate helper
/// call sites have to remember — is what makes that invariant structural.
/// Every other status is left untouched, including `failed`/`canceled`, which
/// legitimately stop partway.
const COMPLETION_FORCES_FULL_PROGRESS: &str =
    "progress_percent = CASE WHEN ?1 = 'completed' THEN 100.0 ELSE progress_percent END";

/// FR-322: một thao tác ghi lên hệ thống tệp gặp file đã tồn tại phải **thất
/// bại rõ ràng**, không bao giờ tự đổi tên hay ghi đè. Mã lỗi khai báo một
/// lần ở đây vì nó đi qua hai tầng: `db::library_write_error` sinh ra nó từ
/// vi phạm chỉ mục duy nhất trên `file_path`, còn `commands::library` sinh ra
/// nó từ phép kiểm tra trên đĩa trước khi động vào file.
pub const FILE_EXISTS_ERROR_CODE: &str = "FILE_EXISTS";

pub struct Db(Mutex<Connection>);

impl Db {
    pub fn open(db_path: &Path) -> Result<Self, AppError> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut conn = Connection::open(db_path)?;

        // migrations/0002 rebuilds `download_jobs` (SQLite can't ALTER a
        // CHECK constraint in place) via DROP + RENAME, while
        // `downloaded_files.job_id` holds a FOREIGN KEY into it — dropping a
        // table another table's FK still points at fails with "FOREIGN KEY
        // constraint failed" if enforcement is on. `rusqlite_migration` runs
        // each migration inside its own transaction, where toggling this
        // pragma is a documented no-op, so it must be turned off here,
        // before `to_latest()`, and back on after — exactly as
        // `rusqlite_migration`'s own docs prescribe for this situation.
        conn.pragma_update(None, "foreign_keys", "OFF")
            .map_err(AppError::internal)?;
        migrations()
            .to_latest(&mut conn)
            .map_err(AppError::internal)?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(AppError::internal)?;

        let db = Db(Mutex::new(conn));
        // FR-303. Ngay sau migration và trước khi bất kỳ ai đọc thư viện, để
        // lần mở tab Thư viện đầu tiên đã thấy đủ lịch sử cũ thay vì một
        // trạng thái rỗng gây hiểu nhầm. Xem `backfill_library_index`: hoàn
        // toàn không chạm đĩa, nên nó không kéo dài thời gian khởi động theo
        // số lượng file.
        db.backfill_library_index()?;
        Ok(db)
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.0.lock().expect("db mutex poisoned")
    }

    // ---- download_jobs ------------------------------------------------

    pub fn insert_job(&self, job: &DownloadJob) -> Result<(), AppError> {
        let conn = self.conn();
        // Column is still named `selected_gallery_urls` (from when this held
        // URL strings) but now holds a JSON array of indices — renaming the
        // column isn't worth another migration for a dev-only field.
        let selected_gallery_indices_json = job
            .selected_gallery_indices
            .as_ref()
            .map(|indices| serde_json::to_string(indices).expect("Vec<u32> always serializes"));
        // Luôn ghi một chuỗi JSON, kể cả khi là bộ mặc định: từ đây trở đi mọi
        // tác vụ đều nêu rõ nó đã chạy với lựa chọn nào (FR-235). NULL được để
        // dành riêng cho những dòng có TRƯỚC migration 0010, nơi "không nêu"
        // mới là sự thật.
        let output_options_json = serde_json::to_string(&job.output_options)
            .expect("OutputOptions always serializes");
        conn.execute(
            "INSERT INTO download_jobs (
                id, source_url, platform, media_type, audio_quality, video_quality,
                gallery_mode, selected_gallery_urls, status, progress_percent,
                speed_bytes_per_sec, eta_seconds, error_message, output_directory,
                output_file_path, is_playlist_item, parent_playlist_id,
                retried_from_job_id, created_at, updated_at, title, playlist_title,
                queue_position, retry_count, next_retry_at, output_options
            ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26)",
            params![
                job.id,
                job.source_url,
                job.platform,
                media_type_str(&job.media_type),
                job.audio_quality,
                job.video_quality,
                job.gallery_mode.as_ref().map(gallery_mode_str),
                selected_gallery_indices_json,
                job.status.as_str(),
                job.progress_percent,
                job.speed_bytes_per_sec,
                job.eta_seconds,
                job.error_message,
                job.output_directory,
                job.output_file_path,
                job.is_playlist_item as i64,
                job.parent_playlist_id,
                job.retried_from_job_id,
                job.created_at,
                job.updated_at,
                job.title,
                job.playlist_title,
                job.queue_position,
                job.retry_count,
                job.next_retry_at,
                output_options_json,
            ],
        )?;
        Ok(())
    }

    pub fn get_job(&self, job_id: &str) -> Result<Option<DownloadJob>, AppError> {
        let conn = self.conn();
        conn.query_row(
            "SELECT * FROM download_jobs WHERE id = ?1",
            params![job_id],
            row_to_job,
        )
        .optional()
        .map_err(AppError::from)
    }

    /// `progress_percent = None` means the source reported no total size, so
    /// there is no percentage to record for this tick (see
    /// `ytdlp::ProgressUpdate::percent`).
    ///
    /// The column stays `REAL NOT NULL` and keeps its **last known** value in
    /// that case — `COALESCE(?1, progress_percent)` — rather than becoming
    /// nullable. Making it nullable would mean rebuilding the whole table
    /// (SQLite can't drop NOT NULL via ALTER) to store a distinction that
    /// only ever matters while a run is in flight: "unknown" is a property of
    /// the live tick, and it travels on the `job:progress` event, which is
    /// what the UI actually renders from. What a *stored* row needs to answer
    /// is "how far did this get", and the last known value answers that
    /// better than NULL does — including after a restart, where the event is
    /// long gone and only the row remains.
    pub fn update_job_progress(
        &self,
        job_id: &str,
        progress_percent: Option<f64>,
        speed_bytes_per_sec: Option<i64>,
        eta_seconds: Option<i64>,
    ) -> Result<(), AppError> {
        let conn = self.conn();
        conn.execute(
            "UPDATE download_jobs SET progress_percent = COALESCE(?1, progress_percent),
             speed_bytes_per_sec = ?2, eta_seconds = ?3, updated_at = ?4 WHERE id = ?5",
            params![
                progress_percent,
                speed_bytes_per_sec,
                eta_seconds,
                Utc::now().to_rfc3339(),
                job_id
            ],
        )?;
        Ok(())
    }

    /// Moving a job to `Completed` also forces `progress_percent` to 100.
    ///
    /// A completed job is by definition finished, so a stored percentage that
    /// says otherwise is wrong no matter what the external tool last
    /// reported — and yt-dlp routinely reports nothing usable at all for
    /// audio-only streams and HLS, which is how 37 completed jobs in the
    /// user's own database ended up frozen at 0%.
    ///
    /// This lives here, in the statement that writes the status, and not in a
    /// sibling `complete_job` helper, precisely so status and progress can
    /// never disagree: there are at least two completion paths (`run_job` for
    /// yt-dlp, `run_gallery_job` for gallery-dl) and a separate helper would
    /// be one more thing every future one has to remember. See
    /// `COMPLETION_FORCES_FULL_PROGRESS`.
    pub fn update_job_status(
        &self,
        job_id: &str,
        status: JobStatus,
        error_message: Option<&str>,
    ) -> Result<(), AppError> {
        let conn = self.conn();
        conn.execute(
            &format!(
                "UPDATE download_jobs SET status = ?1, error_message = ?2, updated_at = ?3,
                 {COMPLETION_FORCES_FULL_PROGRESS} WHERE id = ?4"
            ),
            params![status.as_str(), error_message, Utc::now().to_rfc3339(), job_id],
        )?;
        Ok(())
    }

    pub fn set_job_output_file(&self, job_id: &str, output_file_path: &str) -> Result<(), AppError> {
        let conn = self.conn();
        conn.execute(
            "UPDATE download_jobs SET output_file_path = ?1, updated_at = ?2 WHERE id = ?3",
            params![output_file_path, Utc::now().to_rfc3339(), job_id],
        )?;
        Ok(())
    }

    pub fn list_jobs_by_statuses(&self, statuses: &[JobStatus]) -> Result<Vec<DownloadJob>, AppError> {
        let conn = self.conn();
        let placeholders = statuses.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT * FROM download_jobs WHERE status IN ({placeholders}) ORDER BY created_at ASC"
        );
        let mut stmt = conn.prepare(&sql)?;
        let status_strs: Vec<&str> = statuses.iter().map(|s| s.as_str()).collect();
        let rows = stmt.query_map(rusqlite::params_from_iter(status_strs), row_to_job)?;
        let mut jobs = Vec::new();
        for row in rows {
            jobs.push(row?);
        }
        Ok(jobs)
    }

    /// Một trang Lịch sử: mới nhất trước (`updated_at DESC`), lọc ở backend
    /// (trạng thái tab + tìm kiếm) nên số trang từ `count_history` luôn khớp
    /// với đúng tập đang hiển thị (SC tương tự FR-328 của Library) — thay cho
    /// cách cũ nạp cả bảng rồi lọc trong bộ nhớ giao diện.
    pub fn list_history_page(&self, query: &HistoryQuery) -> Result<Vec<DownloadJob>, AppError> {
        let conn = self.conn();
        let filter = HistoryFilterSql::build(query);
        let sql = format!(
            "SELECT * FROM download_jobs {} ORDER BY updated_at DESC LIMIT ? OFFSET ?",
            filter.where_clause
        );
        let mut params = filter.params;
        params.push(Box::new(query.limit));
        params.push(Box::new(query.offset));
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(params.iter()), row_to_job)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(AppError::from)
    }

    /// Tổng số dòng khớp CÙNG bộ lọc mà `list_history_page` vừa dùng — cố ý
    /// KHÔNG áp `limit`/`offset`, vì số trang mô tả cả tập kết quả chứ không
    /// phải một trang riêng lẻ.
    pub fn count_history(&self, query: &HistoryQuery) -> Result<i64, AppError> {
        let conn = self.conn();
        let filter = HistoryFilterSql::build(query);
        conn.query_row(
            &format!("SELECT COUNT(*) FROM download_jobs {}", filter.where_clause),
            rusqlite::params_from_iter(filter.params.iter()),
            |row| row.get(0),
        )
        .map_err(AppError::from)
    }

    /// Xoá mọi tác vụ đang ở một trong các trạng thái đã cho, trả về số dòng
    /// bị xoá. Dùng cho "Xoá lịch sử" — chỉ động tới bản ghi tác vụ, KHÔNG
    /// đụng tới file đã tải hay bảng `downloaded_files` (đó là việc của
    /// Library, một khái niệm khác).
    pub fn delete_jobs_by_statuses(&self, statuses: &[JobStatus]) -> Result<usize, AppError> {
        let conn = self.conn();
        let placeholders = statuses.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!("DELETE FROM download_jobs WHERE status IN ({placeholders})");
        let status_strs: Vec<&str> = statuses.iter().map(|s| s.as_str()).collect();
        let count = conn.execute(&sql, rusqlite::params_from_iter(status_strs))?;
        Ok(count)
    }

    // ---- truy vấn điều phối hàng đợi ----------------------------------

    /// Job kế tiếp mà bộ điều phối được phép khởi chạy: đang `queued`, và
    /// không nằm trong khoảng chờ thử lại. `now_rfc3339` được truyền vào thay
    /// vì đọc đồng hồ ở đây để test kiểm soát được thời gian.
    ///
    /// Thứ tự: `queue_position` trước, `created_at` sau. Vế thứ hai giữ cho
    /// các job cũ (đều mang `queue_position = 0` từ migration 0008) vẫn chạy
    /// đúng thứ tự chúng được tạo.
    pub fn next_dispatchable_job(
        &self,
        now_rfc3339: &str,
    ) -> Result<Option<DownloadJob>, AppError> {
        let conn = self.conn();
        conn.query_row(
            "SELECT * FROM download_jobs
             WHERE status = 'queued'
               AND (next_retry_at IS NULL OR next_retry_at <= ?1)
             ORDER BY queue_position ASC, created_at ASC
             LIMIT 1",
            params![now_rfc3339],
            row_to_job,
        )
        .optional()
        .map_err(AppError::from)
    }

    /// Vị trí cho job mới thêm vào cuối hàng đợi.
    pub fn next_queue_position(&self) -> Result<f64, AppError> {
        let conn = self.conn();
        let max: Option<f64> = conn.query_row(
            "SELECT MAX(queue_position) FROM download_jobs
             WHERE status IN ('queued','paused','downloading','fetching_metadata')",
            [],
            |row| row.get(0),
        )?;
        Ok(position_between(max, None))
    }

    // ---- sắp xếp lại thứ tự và thao tác hàng loạt ---------------------
    //
    // Người gọi nhóm này là `DownloadQueue::move_job` (lệnh `reorder_queue`) và
    // `DownloadQueue::apply_bulk` (các lệnh `pause_all_jobs` / `resume_all_jobs`
    // / `cancel_all_jobs`).

    /// Đặt một job vào giữa hai hàng xóm (`None` nghĩa là đầu hoặc cuối danh
    /// sách) — thao tác đằng sau một lần kéo-thả (FR-117).
    ///
    /// Chỉ ghi đúng một dòng. Đó không chỉ là chuyện nhanh: nếu phải đánh số
    /// lại cả danh sách thì một job được thêm vào trong lúc người dùng đang kéo
    /// sẽ bị ghi đè vị trí, vì danh sách giao diện gửi lên đã cũ.
    ///
    /// Giao diện gửi id của hai hàng xóm chứ không gửi số: giao diện không nên
    /// phải biết gì về cách đánh số nội bộ.
    ///
    /// Toàn bộ thao tác nằm trong đúng một transaction, giữ đúng một lần khoá
    /// mutex: đọc hai hàng xóm, chuẩn hoá nếu cần, đọc lại, rồi ghi. Nếu đọc và
    /// ghi tách rời nhau thì một lần enqueue hay một lần kéo khác chen vào giữa
    /// sẽ khiến điểm giữa vừa tính trở thành số cũ — đúng loại tranh chấp mà
    /// hàm này tự nhận là tránh được.
    pub fn move_job_between(
        &self,
        job_id: &str,
        before_job_id: Option<&str>,
        after_job_id: Option<&str>,
    ) -> Result<(), AppError> {
        // Một job không thể là hàng xóm của chính nó: điểm giữa sẽ tính từ
        // chính vị trí sắp bị ghi đè, cho ra kết quả vô nghĩa. Đây là lỗi của
        // phía gọi, nên báo lỗi thay vì đoán ý.
        if Some(job_id) == before_job_id || Some(job_id) == after_job_id {
            return Err(AppError::new(
                "INVALID_ARGUMENT",
                "A job cannot be moved relative to itself",
            ));
        }

        let mut conn = self.conn();
        let tx = conn.transaction()?;

        let mut before = position_of(&tx, before_job_id)?;
        let mut after = position_of(&tx, after_job_id)?;

        // Chèn liên tiếp vào cùng một chỗ chia đôi khe hở mỗi lần. Khi nó hẹp
        // tới mức f64 sắp hết chỗ, đánh số lại rồi đọc lại hàng xóm — giá trị
        // đọc trước khi chuẩn hoá đã không còn đúng nữa.
        if needs_renormalize(before, after) {
            renormalize_positions_within(&tx)?;
            before = position_of(&tx, before_job_id)?;
            after = position_of(&tx, after_job_id)?;
        }

        let changed = tx.execute(
            "UPDATE download_jobs SET queue_position = ?1, updated_at = ?2 WHERE id = ?3",
            params![
                position_between(before, after),
                Utc::now().to_rfc3339(),
                job_id
            ],
        )?;
        // Không có dòng nào khớp nghĩa là job đã bị xoá: trả lỗi thay vì `Ok`
        // im lặng, và bỏ luôn transaction để lần chuẩn hoá ở trên (nếu có)
        // không bị commit cho một thao tác rốt cuộc không xảy ra.
        if changed == 0 {
            return Err(AppError::not_found("Job"));
        }
        tx.commit()?;
        Ok(())
    }

    /// Gọi một lần lúc khởi động: job còn ghi `downloading`/`fetching_metadata`
    /// là tàn dư của một phiên bị đóng đột ngột — tiến trình tải của chúng đã
    /// chết cùng ứng dụng. Chuyển về `paused` để người dùng tiếp tục hoặc huỷ
    /// (FR-115). Trả về số dòng đã đổi.
    pub fn reset_interrupted_jobs(&self) -> Result<usize, AppError> {
        let conn = self.conn();
        let changed = conn.execute(
            "UPDATE download_jobs SET status = 'paused', updated_at = ?1
             WHERE status IN ('downloading','fetching_metadata')",
            params![Utc::now().to_rfc3339()],
        )?;
        Ok(changed)
    }

    /// Đưa job về hàng chờ kèm mốc thời gian được phép thử lại (FR-121).
    /// `error_message` được giữ lại để giao diện hiển thị lý do đang chờ.
    pub fn mark_job_for_retry(
        &self,
        job_id: &str,
        next_retry_at_rfc3339: &str,
        error_message: &str,
    ) -> Result<(), AppError> {
        let conn = self.conn();
        conn.execute(
            "UPDATE download_jobs
             SET status = 'queued',
                 retry_count = retry_count + 1,
                 next_retry_at = ?1,
                 error_message = ?2,
                 updated_at = ?3
             WHERE id = ?4",
            params![
                next_retry_at_rfc3339,
                error_message,
                Utc::now().to_rfc3339(),
                job_id
            ],
        )?;
        Ok(())
    }

    /// Xoá mốc chờ thử lại và đưa bộ đếm về 0 — dùng khi người dùng can thiệp
    /// thủ công (tạm dừng, huỷ).
    ///
    /// Đưa `retry_count` về 0 chứ không chỉ xoá `next_retry_at`: một job từng
    /// thất bại 3 lần vì mạng, được người dùng tạm dừng rồi tiếp tục, phải
    /// được nhận lại đủ số lượt thử — nếu không, nó sẽ thất bại vĩnh viễn ngay
    /// ở lần chạy đầu tiên sau khi tiếp tục (FR-123).
    pub fn clear_retry_deadline(&self, job_id: &str) -> Result<(), AppError> {
        let conn = self.conn();
        conn.execute(
            "UPDATE download_jobs SET next_retry_at = NULL, retry_count = 0, updated_at = ?1
             WHERE id = ?2",
            params![Utc::now().to_rfc3339(), job_id],
        )?;
        Ok(())
    }

    /// Đổi trạng thái hàng loạt, trả về id của những job **khớp**
    /// `from_statuses` để tầng gọi biết cần phát sự kiện cho những job nào
    /// (FR-118).
    ///
    /// "Khớp" chứ không phải "đã đổi": nếu `to_status` cũng nằm trong
    /// `from_statuses` thì các dòng vốn đã ở trạng thái đích vẫn được liệt kê,
    /// và phía gọi sẽ phát sự kiện thừa cho chúng. Phía gọi chịu trách nhiệm
    /// không truyền vào tổ hợp đó.
    pub fn bulk_update_status(
        &self,
        from_statuses: &[JobStatus],
        to_status: JobStatus,
    ) -> Result<Vec<String>, AppError> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let placeholders = from_statuses
            .iter()
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(",");
        let status_strs: Vec<&str> = from_statuses.iter().map(|s| s.as_str()).collect();

        let ids: Vec<String> = {
            let sql = format!("SELECT id FROM download_jobs WHERE status IN ({placeholders})");
            let mut stmt = tx.prepare(&sql)?;
            let rows = stmt.query_map(rusqlite::params_from_iter(status_strs.iter()), |row| {
                row.get::<_, String>(0)
            })?;
            rows.collect::<rusqlite::Result<Vec<String>>>()?
        };

        for id in &ids {
            // `bulk_plan` only ever targets paused/queued/canceled, so the
            // completion clause is inert here today. It is included anyway so
            // the invariant belongs to *writing the status* rather than to
            // one particular function that happens to remember it.
            tx.execute(
                &format!(
                    "UPDATE download_jobs SET status = ?1, updated_at = ?2,
                     {COMPLETION_FORCES_FULL_PROGRESS} WHERE id = ?3"
                ),
                params![to_status.as_str(), Utc::now().to_rfc3339(), id],
            )?;
        }
        tx.commit()?;
        Ok(ids)
    }

    // ---- downloaded_files / chỉ mục Thư viện ---------------------------

    /// Ghi (hoặc cập nhật) một file kết quả vào chỉ mục Thư viện.
    ///
    /// FR-302: một tác vụ có thể gọi hàm này nhiều lần — tách chương ghi thêm
    /// một dòng cho mỗi chương — và mọi dòng đều mang cùng `job_id`. Không có
    /// gì ở đây giới hạn "một tác vụ một file".
    ///
    /// UPSERT theo `file_path` chứ không phải INSERT thuần: tải lại cùng một
    /// link ghi đè đúng file cũ trên đĩa, nên hai dòng cùng đường dẫn sẽ là
    /// hai ô giống hệt nhau trong lưới trỏ vào một file duy nhất (chính là
    /// thứ migration 0012 vừa dọn: 105 dòng thật của người dùng chỉ ứng với
    /// 66 file). `duration_seconds`/`thumbnail_path` giữ giá trị cũ khi lần
    /// ghi mới không có gì tốt hơn — mất một thứ đã đo được vì lần sau đo
    /// hụt là một bước lùi.
    pub fn insert_downloaded_file(&self, file: &NewLibraryFile) -> Result<(), AppError> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO downloaded_files (
                id, job_id, file_path, file_format, file_size_bytes, completed_at,
                title, media_type, platform, source_url, duration_seconds,
                thumbnail_path, search_text, is_missing
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,0)
             ON CONFLICT(file_path) DO UPDATE SET
                job_id = excluded.job_id,
                file_format = excluded.file_format,
                file_size_bytes = excluded.file_size_bytes,
                completed_at = excluded.completed_at,
                title = excluded.title,
                media_type = excluded.media_type,
                platform = excluded.platform,
                source_url = excluded.source_url,
                duration_seconds = COALESCE(excluded.duration_seconds, duration_seconds),
                thumbnail_path = COALESCE(excluded.thumbnail_path, thumbnail_path),
                search_text = excluded.search_text,
                is_missing = 0",
            params![
                uuid::Uuid::new_v4().to_string(),
                file.job_id,
                file.file_path,
                file.file_format,
                file.file_size_bytes,
                Utc::now().to_rfc3339(),
                file.title,
                media_type_str(&file.media_type),
                file.platform,
                file.source_url,
                file.duration_seconds,
                file.thumbnail_path,
                search_text_for(&file.title, &file.file_path),
            ],
        )?;
        Ok(())
    }

    /// FR-303: nạp vào Thư viện những file đã tải từ trước khi Thư viện tồn
    /// tại, dựa trên `download_jobs` — thứ dữ liệu lịch sử duy nhất có sẵn.
    ///
    /// Chạy trong `Db::open`, sau migration, ở MỌI lần khởi động chứ không
    /// một lần rồi thôi: cả hai câu lệnh đều tự giới hạn vào phần còn thiếu
    /// (`media_type = ''` là dấu vết của dòng có trước 0012; `NOT EXISTS` cho
    /// phần chưa có dòng nào), nên lần thứ hai trở đi là hai lần quét không
    /// đổi gì. Đổi lại, một cài đặt nào đó ghi thêm dòng qua đường cũ vẫn
    /// được vá, thay vì hỏng vĩnh viễn vì đã lỡ chuyến migration.
    ///
    /// KHÔNG chạm đĩa. Không `stat`, không đọc kích thước thật, không đánh
    /// dấu thiếu — đó là việc của `reconcile_library` (FR-327): một vòng lặp
    /// `stat` trên 10.000 file nằm giữa `Db::open` và cửa sổ đầu tiên là đúng
    /// nghĩa "chặn giao diện". Vì thế `output_file_path` trỏ vào một file
    /// không còn tồn tại vẫn được nạp bình thường, y như mọi mục khác, và chỉ
    /// đổi màu sau vòng đối soát kế tiếp.
    pub fn backfill_library_index(&self) -> Result<LibraryBackfillReport, AppError> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;

        // Bước 1 — bổ sung cột cho các dòng đã có (dòng ghi trước 0012).
        let stale: Vec<StaleIndexRow> = {
            let mut stmt = tx.prepare(
                "SELECT f.id, f.file_path, j.title, j.media_type, j.platform, j.source_url
                 FROM downloaded_files f
                 LEFT JOIN download_jobs j ON j.id = f.job_id
                 WHERE f.media_type = ''",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok(StaleIndexRow {
                    id: row.get(0)?,
                    file_path: row.get(1)?,
                    job_title: row.get(2)?,
                    // `LEFT JOIN` cho ra `NULL` ở một dòng mồ côi (tác vụ đã
                    // bị xoá khỏi lịch sử) — không phải lỗi, chỉ là không còn
                    // gì để chép sang.
                    media_type: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    platform: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                    source_url: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                })
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        let enriched = stale.len();
        for row in stale {
            let title = display_title(row.job_title.as_deref(), &row.file_path);
            tx.execute(
                "UPDATE downloaded_files
                 SET title = ?1, media_type = ?2, platform = ?3, source_url = ?4,
                     search_text = ?5, file_format = ?6
                 WHERE id = ?7",
                params![
                    title,
                    // Dòng mồ côi rơi về `video` — cùng luật mà `row_to_job`
                    // vẫn áp cho một giá trị lạ. Vẫn tốt hơn là bỏ hẳn dòng đó
                    // khỏi thư viện, vì file trên đĩa thì vẫn còn thật.
                    if row.media_type.is_empty() { "video" } else { row.media_type.as_str() },
                    row.platform,
                    row.source_url,
                    search_text_for(&title, &row.file_path),
                    // Chuẩn hoá lại luôn: các dòng cũ mang định dạng do bản
                    // `insert_downloaded_file` trước 0012 rút ra bằng
                    // `Path::extension` thô, nên một thư mục kết quả của tác
                    // vụ gallery đã kịp để lại chuỗi
                    // `co k1lltheguard46 (28f98824)` trong bộ lọc định dạng.
                    media_file_extension(&row.file_path),
                    row.id,
                ],
            )?;
        }

        // Bước 2 — những tác vụ hoàn tất chưa hề có dòng nào trong chỉ mục.
        // Trên CSDL thật thì mọi tác vụ hoàn tất đều đã có dòng, nhưng bản
        // ghi `downloaded_files` chỉ tồn tại từ khi tính năng ghi nó tồn tại;
        // `output_file_path` mới là thứ đi cùng tác vụ từ đầu, nên nó là
        // nguồn dự phòng đúng đắn cho FR-303.
        let unindexed_jobs: Vec<UnindexedJobRow> = {
            let mut stmt = tx.prepare(
                "SELECT j.id, j.output_file_path, j.title, j.media_type, j.platform,
                        j.source_url, j.updated_at
                 FROM download_jobs j
                 WHERE j.status = 'completed'
                   AND j.output_file_path IS NOT NULL
                   AND j.output_file_path <> ''
                   AND NOT EXISTS (
                       SELECT 1 FROM downloaded_files f WHERE f.file_path = j.output_file_path
                   )",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok(UnindexedJobRow {
                    job_id: row.get(0)?,
                    file_path: row.get(1)?,
                    job_title: row.get(2)?,
                    media_type: row.get(3)?,
                    platform: row.get(4)?,
                    source_url: row.get(5)?,
                    updated_at: row.get(6)?,
                })
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        let mut inserted = 0usize;
        for row in unindexed_jobs {
            let title = display_title(row.job_title.as_deref(), &row.file_path);
            // `INSERT OR IGNORE`: hai tác vụ hoàn tất có thể cùng trỏ vào một
            // đường dẫn (tải lại đè lên file cũ). Dòng đầu thắng, dòng sau bị
            // bỏ qua — cùng luật gộp mà 0012 vừa áp cho dữ liệu cũ.
            //
            // `file_size_bytes = 0`: kích thước thật chỉ đọc được bằng cách
            // chạm đĩa, việc mà vòng đối soát (`reconcile_library`) làm ở nền
            // và cập nhật lại — xem chú thích của hàm này.
            let changed = tx.execute(
                "INSERT OR IGNORE INTO downloaded_files (
                    id, job_id, file_path, file_format, file_size_bytes, completed_at,
                    title, media_type, platform, source_url, search_text, is_missing
                 ) VALUES (?1,?2,?3,?4,0,?5,?6,?7,?8,?9,?10,0)",
                params![
                    uuid::Uuid::new_v4().to_string(),
                    row.job_id,
                    row.file_path,
                    media_file_extension(&row.file_path),
                    row.updated_at,
                    title,
                    row.media_type,
                    row.platform,
                    row.source_url,
                    search_text_for(&title, &row.file_path),
                ],
            )?;
            inserted += changed;
        }

        tx.commit()?;
        Ok(LibraryBackfillReport { enriched, inserted })
    }

    /// FR-307 → FR-310. Toàn bộ việc lọc/sắp/phân trang nằm trong SQL: trả cả
    /// 10.000 dòng qua cầu IPC rồi để giao diện lọc là cách chắc chắn nhất để
    /// phá SC-302.
    pub fn list_library(&self, query: &LibraryQuery) -> Result<Vec<LibraryItem>, AppError> {
        let conn = self.conn();
        let filter = LibraryFilterSql::build(query);
        let sql = format!(
            "SELECT * FROM downloaded_files {} ORDER BY {} {}",
            filter.where_clause,
            query.sort.order_by(query.direction),
            limit_clause(query),
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(
            rusqlite::params_from_iter(filter.params.iter()),
            row_to_library_item,
        )?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(AppError::from)
    }

    /// FR-328. Tính trên CÙNG bộ lọc mà `list_library` vừa dùng, nên tổng số
    /// luôn khớp với thứ đang hiện trên màn hình (SC-307). Phân trang cố ý
    /// KHÔNG được áp: thống kê nói về cả tập kết quả, không về trang hiện tại.
    pub fn library_stats(&self, query: &LibraryQuery) -> Result<LibraryStats, AppError> {
        let conn = self.conn();
        let filter = LibraryFilterSql::build(query);

        let (total_items, total_size_bytes, missing_items) = conn.query_row(
            &format!(
                "SELECT COUNT(*), COALESCE(SUM(file_size_bytes), 0),
                        COALESCE(SUM(is_missing), 0)
                 FROM downloaded_files {}",
                filter.where_clause
            ),
            rusqlite::params_from_iter(filter.params.iter()),
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;

        let breakdown = |column: &str| -> Result<Vec<LibraryBreakdownEntry>, AppError> {
            let sql = format!(
                "SELECT {column}, COUNT(*), COALESCE(SUM(file_size_bytes), 0)
                 FROM downloaded_files {}
                 GROUP BY {column} ORDER BY COUNT(*) DESC, {column} ASC",
                filter.where_clause
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(rusqlite::params_from_iter(filter.params.iter()), |row| {
                Ok(LibraryBreakdownEntry {
                    key: row.get(0)?,
                    item_count: row.get(1)?,
                    total_size_bytes: row.get(2)?,
                })
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(AppError::from)
        };

        let formats = {
            // `file_format <> ''` chứ không phải mọi giá trị: một thư mục kết
            // quả của tác vụ gallery không có định dạng nào, và một ô lọc rỗng
            // không phải là một lựa chọn người dùng bấm được. Mục đó vẫn nằm
            // trong thư viện và vẫn được đếm vào tổng — nó chỉ không góp mặt
            // trong danh sách định dạng.
            let separator = if filter.where_clause.is_empty() { "WHERE" } else { "AND" };
            let sql = format!(
                "SELECT DISTINCT file_format FROM downloaded_files {} {separator} file_format <> ''
                 ORDER BY file_format ASC",
                filter.where_clause
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(rusqlite::params_from_iter(filter.params.iter()), |row| {
                row.get::<_, String>(0)
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };

        Ok(LibraryStats {
            total_items,
            total_size_bytes,
            missing_items,
            by_platform: breakdown("platform")?,
            by_media_type: breakdown("media_type")?,
            formats,
        })
    }

    pub fn library_item(&self, item_id: &str) -> Result<Option<LibraryItem>, AppError> {
        let conn = self.conn();
        conn.query_row(
            "SELECT * FROM downloaded_files WHERE id = ?1",
            params![item_id],
            row_to_library_item,
        )
        .optional()
        .map_err(AppError::from)
    }

    /// Lấy nhiều mục **theo đúng thứ tự người gọi đưa vào** — FR-330 nói rõ
    /// danh sách phát phải giữ thứ tự đang hiển thị, mà thứ tự ấy chỉ tồn tại
    /// ở phía người gọi (nó là thứ tự sắp xếp + lựa chọn hiện hành). Một câu
    /// `WHERE id IN (...)` trả về theo thứ tự của CSDL, nên việc sắp lại phải
    /// làm ở đây chứ không phải hy vọng chúng trùng nhau.
    ///
    /// Id không còn tồn tại thì bị bỏ qua, không phải lỗi: mục có thể vừa bị
    /// một thao tác khác dọn đi giữa lúc người dùng đang chọn.
    pub fn library_items(&self, item_ids: &[String]) -> Result<Vec<LibraryItem>, AppError> {
        if item_ids.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.conn();
        let placeholders = item_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let mut stmt = conn.prepare(&format!(
            "SELECT * FROM downloaded_files WHERE id IN ({placeholders})"
        ))?;
        let rows = stmt.query_map(
            rusqlite::params_from_iter(item_ids.iter()),
            row_to_library_item,
        )?;
        let mut by_id: std::collections::HashMap<String, LibraryItem> = rows
            .collect::<rusqlite::Result<Vec<_>>>()?
            .into_iter()
            .map(|item| (item.id.clone(), item))
            .collect();
        Ok(item_ids
            .iter()
            .filter_map(|id| by_id.remove(id))
            .collect())
    }

    /// FR-302: mọi file mà một tác vụ tạo ra, kể cả khi tách chương sinh ra
    /// hàng chục file cho đúng một dòng hàng đợi.
    pub fn library_items_for_job(&self, job_id: &str) -> Result<Vec<LibraryItem>, AppError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT * FROM downloaded_files WHERE job_id = ?1 ORDER BY completed_at ASC, rowid ASC",
        )?;
        let rows = stmt.query_map(params![job_id], row_to_library_item)?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(AppError::from)
    }

    /// Một trang của vòng đối soát (FR-323/FR-327). Chỉ trả về đúng những cột
    /// mà phép `stat` cần so sánh, để một vòng quét 10.000 mục không kéo theo
    /// 10.000 bản ghi đầy đủ vào bộ nhớ.
    pub fn library_reconcile_page(
        &self,
        offset: i64,
        limit: i64,
    ) -> Result<Vec<LibraryFileState>, AppError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, file_path, is_missing, file_size_bytes FROM downloaded_files
             ORDER BY rowid ASC LIMIT ?1 OFFSET ?2",
        )?;
        let rows = stmt.query_map(params![limit, offset], |row| {
            Ok(LibraryFileState {
                id: row.get(0)?,
                file_path: row.get(1)?,
                is_missing: row.get::<_, i64>(2)? != 0,
                file_size_bytes: row.get(3)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(AppError::from)
    }

    /// Ghi lại kết quả `stat` của một lô và trả về **những id thật sự đổi
    /// trạng thái thiếu/còn**. Giao diện chỉ cần vẽ lại chừng ấy ô, nên một
    /// vòng đối soát trên thư viện không đổi gì sẽ không phát sinh lần render
    /// nào (FR-327).
    pub fn apply_library_file_states(
        &self,
        states: &[LibraryFileState],
    ) -> Result<Vec<String>, AppError> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let mut changed = Vec::new();
        for state in states {
            let updated = tx.execute(
                "UPDATE downloaded_files SET is_missing = ?1, file_size_bytes = ?2
                 WHERE id = ?3 AND (is_missing <> ?1 OR file_size_bytes <> ?2)",
                params![state.is_missing as i64, state.file_size_bytes, state.id],
            )?;
            if updated > 0 {
                changed.push(state.id.clone());
            }
        }
        tx.commit()?;
        Ok(changed)
    }

    /// Trỏ một mục sang đường dẫn khác: đổi tên (FR-317), di chuyển (FR-319)
    /// và tìm lại file (FR-325) đều là đúng phép này ở tầng dữ liệu.
    ///
    /// `is_missing` về 0 vì người gọi vừa xác nhận file có mặt ở đó — đó là
    /// điều kiện tiên quyết của cả ba thao tác.
    pub fn set_library_item_path(
        &self,
        item_id: &str,
        new_path: &str,
        file_size_bytes: i64,
    ) -> Result<LibraryItem, AppError> {
        {
            let conn = self.conn();
            let title: String = conn
                .query_row(
                    "SELECT title FROM downloaded_files WHERE id = ?1",
                    params![item_id],
                    |row| row.get(0),
                )
                .optional()?
                .ok_or_else(|| AppError::not_found("Library item"))?;
            conn.execute(
                "UPDATE downloaded_files
                 SET file_path = ?1, file_format = ?2, file_size_bytes = ?3,
                     search_text = ?4, is_missing = 0
                 WHERE id = ?5",
                params![
                    new_path,
                    media_file_extension(new_path),
                    file_size_bytes,
                    search_text_for(&title, new_path),
                    item_id,
                ],
            )
            .map_err(library_write_error)?;
        }
        self.library_item(item_id)?
            .ok_or_else(|| AppError::not_found("Library item"))
    }

    /// FR-324: gỡ mục khỏi chỉ mục mà KHÔNG đụng tới đĩa. Dùng cho hai việc
    /// khác nhau về ý định nhưng giống hệt nhau ở tầng này: dọn các mục thiếu,
    /// và bước ghi sổ sau khi file đã được đưa vào thùng rác (FR-318).
    pub fn remove_library_items(&self, item_ids: &[String]) -> Result<usize, AppError> {
        if item_ids.is_empty() {
            return Ok(0);
        }
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let mut removed = 0usize;
        for id in item_ids {
            removed += tx.execute("DELETE FROM downloaded_files WHERE id = ?1", params![id])?;
        }
        tx.commit()?;
        Ok(removed)
    }

    // ---- app_settings (generic key-value — new settings need no migration) --

    /// Reads `key`, lazily creating it with `default` if it's never been set
    /// (a fresh install, or a setting introduced after the user's db was
    /// created) — self-healing instead of requiring every setting to have
    /// been seeded by a migration up front.
    fn get_setting_or_default(conn: &Connection, key: &str, default: &str) -> Result<String, AppError> {
        conn.execute(
            "INSERT INTO app_settings (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO NOTHING",
            params![key, default],
        )?;
        conn.query_row("SELECT value FROM app_settings WHERE key = ?1", params![key], |row| row.get(0))
            .map_err(AppError::from)
    }

    fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<(), AppError> {
        conn.execute(
            "INSERT INTO app_settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn get_settings(&self) -> Result<AppSettings, AppError> {
        let conn = self.conn();
        Ok(AppSettings {
            theme: Self::get_setting_or_default(&conn, "theme", "system")?,
            language: Self::get_setting_or_default(&conn, "language", "system")?,
            default_output_directory: Self::get_setting_or_default(&conn, "default_output_directory", "")?,
            show_logs_tab: Self::get_setting_or_default(&conn, "show_logs_tab", "0")? != "0",
            // `unwrap_or` ở đây là chủ ý: một giá trị rác trong DB (do người
            // dùng sửa tay hoặc một lần ghi hỏng) phải rơi về mặc định chứ
            // không được làm hỏng cả màn hình cài đặt.
            max_concurrent_downloads: Self::get_setting_or_default(
                &conn,
                "max_concurrent_downloads",
                "3",
            )?
            .parse()
            .unwrap_or(3),
            rate_limit_kbps: Self::get_setting_or_default(&conn, "rate_limit_kbps", "0")?
                .parse()
                .unwrap_or(0),
            max_retry_attempts: Self::get_setting_or_default(&conn, "max_retry_attempts", "3")?
                .parse()
                .unwrap_or(3),
            run_in_background: Self::get_setting_or_default(&conn, "run_in_background", "0")? == "1",
        })
    }

    pub fn update_settings(&self, settings: &AppSettings) -> Result<(), AppError> {
        let conn = self.conn();
        Self::set_setting(&conn, "theme", &settings.theme)?;
        Self::set_setting(&conn, "language", &settings.language)?;
        Self::set_setting(&conn, "default_output_directory", &settings.default_output_directory)?;
        Self::set_setting(&conn, "show_logs_tab", if settings.show_logs_tab { "1" } else { "0" })?;
        Self::set_setting(
            &conn,
            "max_concurrent_downloads",
            &settings.max_concurrent_downloads.to_string(),
        )?;
        Self::set_setting(&conn, "rate_limit_kbps", &settings.rate_limit_kbps.to_string())?;
        Self::set_setting(
            &conn,
            "max_retry_attempts",
            &settings.max_retry_attempts.to_string(),
        )?;
        Self::set_setting(
            &conn,
            "run_in_background",
            if settings.run_in_background { "1" } else { "0" },
        )?;
        Ok(())
    }

    // ---- presets (FR-228 → FR-233) -------------------------------------

    /// Sắp theo tên vì đây là danh sách người dùng đọc và chọn từ đó; thứ tự
    /// theo thời điểm tạo sẽ khiến vị trí của một preset đổi chỗ chỉ vì nó
    /// vừa được sửa.
    pub fn list_presets(&self) -> Result<Vec<Preset>, AppError> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, name, output_options, is_default, created_at, updated_at
             FROM presets ORDER BY name",
        )?;
        let presets = stmt
            .query_map([], row_to_preset)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(presets)
    }

    /// Preset mới KHÔNG bao giờ tự nhận cờ mặc định, kể cả khi nó là preset
    /// đầu tiên: "lưu cấu hình này lại" (FR-228) và "từ nay áp nó cho mọi liên
    /// kết mới" (FR-230) là hai ý định khác nhau, và gộp chúng lại sẽ âm thầm
    /// đổi hành vi của mọi lần xem trước sau đó mà người dùng không hề yêu cầu.
    pub fn create_preset(&self, name: &str, options: &OutputOptions) -> Result<Preset, AppError> {
        let preset = Preset {
            id: uuid::Uuid::new_v4().to_string(),
            name: normalize_preset_name(name)?,
            output_options: options.clone(),
            is_default: false,
            created_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
        };
        let conn = self.conn();
        conn.execute(
            "INSERT INTO presets (id, name, output_options, is_default, created_at, updated_at)
             VALUES (?1, ?2, ?3, 0, ?4, ?5)",
            params![
                preset.id,
                preset.name,
                serialize_options(&preset.output_options)?,
                preset.created_at,
                preset.updated_at,
            ],
        )
        .map_err(preset_write_error)?;
        Ok(preset)
    }

    pub fn rename_preset(&self, preset_id: &str, name: &str) -> Result<Preset, AppError> {
        let name = normalize_preset_name(name)?;
        let conn = self.conn();
        let changed = conn
            .execute(
                "UPDATE presets SET name = ?2, updated_at = ?3 WHERE id = ?1",
                params![preset_id, name, Utc::now().to_rfc3339()],
            )
            .map_err(preset_write_error)?;
        if changed == 0 {
            return Err(AppError::not_found("Preset"));
        }
        preset_at(&conn, preset_id)?.ok_or_else(|| AppError::not_found("Preset"))
    }

    /// Ghi đè cả blob tuỳ chọn, không vá từng trường: bên gọi gửi lên nguyên
    /// một `OutputOptions`, nên "trường này không xuất hiện" phải có nghĩa là
    /// giá trị mặc định của nó, chứ không phải "giữ nguyên giá trị cũ".
    pub fn update_preset_options(
        &self,
        preset_id: &str,
        options: &OutputOptions,
    ) -> Result<Preset, AppError> {
        let conn = self.conn();
        let changed = conn.execute(
            "UPDATE presets SET output_options = ?2, updated_at = ?3 WHERE id = ?1",
            params![
                preset_id,
                serialize_options(options)?,
                Utc::now().to_rfc3339()
            ],
        )?;
        if changed == 0 {
            return Err(AppError::not_found("Preset"));
        }
        preset_at(&conn, preset_id)?.ok_or_else(|| AppError::not_found("Preset"))
    }

    /// Xoá preset mặc định KHÔNG đôn preset khác lên thay. Trạng thái sau đó là
    /// "không có preset mặc định" — đúng bằng trạng thái của một cài đặt mới,
    /// nên mọi nơi đọc đã phải xử lý được nó rồi. Tự chọn đại một preset khác
    /// sẽ là chuyện ngược lại: từ lần xem trước kế tiếp, một bộ tuỳ chọn người
    /// dùng chưa bao giờ chọn làm mặc định bỗng được áp cho mọi liên kết.
    pub fn delete_preset(&self, preset_id: &str) -> Result<(), AppError> {
        let conn = self.conn();
        let changed = conn.execute("DELETE FROM presets WHERE id = ?1", params![preset_id])?;
        if changed == 0 {
            return Err(AppError::not_found("Preset"));
        }
        Ok(())
    }

    /// Đặt `preset_id` làm mặc định, xoá cờ của preset mặc định cũ trong CÙNG
    /// một giao dịch.
    ///
    /// Thứ tự hai câu lệnh không đổi chỗ được: chỉ mục một phần
    /// `presets_single_default` được SQLite kiểm tra ngay tại từng dòng bị
    /// ghi, nên bật cờ mới trước sẽ đụng phải cờ cũ còn nguyên và thất bại.
    /// Giao dịch lo nốt nửa còn lại: một lỗi ở bước sau (kể cả trường hợp
    /// `preset_id` không tồn tại) cuộn ngược cả việc xoá cờ cũ, nên không có
    /// đường nào dẫn tới "không còn preset mặc định nào" ngoài việc người dùng
    /// tự xoá nó.
    pub fn set_default_preset(&self, preset_id: &str) -> Result<Preset, AppError> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let now = Utc::now().to_rfc3339();
        tx.execute(
            "UPDATE presets SET is_default = 0, updated_at = ?2
             WHERE is_default = 1 AND id <> ?1",
            params![preset_id, now],
        )?;
        let changed = tx.execute(
            "UPDATE presets SET is_default = 1, updated_at = ?2 WHERE id = ?1",
            params![preset_id, now],
        )?;
        if changed == 0 {
            return Err(AppError::not_found("Preset"));
        }
        let preset = preset_at(&tx, preset_id)?.ok_or_else(|| AppError::not_found("Preset"))?;
        tx.commit()?;
        Ok(preset)
    }
}

/// Một bộ `Tuỳ chọn đầu ra` có tên, lưu lâu dài, có cờ đánh dấu mặc định
/// (Key Entity "Preset", FR-228 → FR-233).
///
/// `output_options` mang NGUYÊN kiểu mà một tác vụ mang, không phải một bản
/// sao rút gọn: một preset đúng là tuỳ chọn đầu ra của một tác vụ cộng thêm
/// cái tên. Nhờ vậy giao diện áp preset bằng một phép gán, việc thêm tuỳ chọn
/// mới không phải sửa chỗ nào ở đây, và FR-233 được `#[serde(default)]` của
/// `OutputOptions` lo sẵn.
///
/// Giao diện cần đủ dữ liệu để thực hiện FR-231 (mức chất lượng trong preset
/// không có ở nguồn hiện tại thì chọn mức gần nhất và nói rõ đã đổi gì) — nên
/// bản ghi trả về nguyên vẹn thứ đã lưu, không hề bị lọc theo nguồn nào cả;
/// việc đối chiếu với danh sách format thật là việc của phía áp preset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub output_options: OutputOptions,
    pub is_default: bool,
    pub created_at: String,
    pub updated_at: String,
}

fn serialize_options(options: &OutputOptions) -> Result<String, AppError> {
    serde_json::to_string(options).map_err(AppError::internal)
}

/// Cắt khoảng trắng thừa rồi chặn tên rỗng.
///
/// Việc cắt không phải để cho đẹp: `" Nhạc "` và `"Nhạc"` mà cùng lưu được thì
/// chỉ mục UNIQUE trên tên chẳng ngăn được gì, và danh sách preset sẽ có hai
/// mục trông hệt nhau.
fn normalize_preset_name(name: &str) -> Result<String, AppError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(AppError::new(
            "PRESET_NAME_REQUIRED",
            "A preset needs a name",
        ));
    }
    Ok(trimmed.to_string())
}

/// Dịch vi phạm ràng buộc thành mã lỗi mà giao diện dịch được.
///
/// Chỉ dùng cho hai câu lệnh ghi tên (thêm mới, đổi tên). Cả hai đều không hề
/// chạm vào `is_default`, nên chỉ mục UNIQUE duy nhất chúng có thể đụng phải
/// là `presets_name_unique` — `presets_single_default` nằm ngoài tầm với của
/// chúng. Nhờ giới hạn ấy mà việc quy mọi `ConstraintViolation` về "trùng tên"
/// ở đây là đúng chứ không phải đoán.
fn preset_write_error(err: rusqlite::Error) -> AppError {
    match &err {
        rusqlite::Error::SqliteFailure(inner, _)
            if inner.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            AppError::new(
                "PRESET_NAME_TAKEN",
                "A preset with this name already exists",
            )
        }
        _ => AppError::from(err),
    }
}

fn preset_at(conn: &Connection, preset_id: &str) -> Result<Option<Preset>, AppError> {
    conn.query_row(
        "SELECT id, name, output_options, is_default, created_at, updated_at
         FROM presets WHERE id = ?1",
        params![preset_id],
        row_to_preset,
    )
    .optional()
    .map_err(AppError::from)
}

fn row_to_preset(row: &rusqlite::Row) -> rusqlite::Result<Preset> {
    let output_options_raw: String = row.get("output_options")?;
    Ok(Preset {
        id: row.get("id")?,
        name: row.get("name")?,
        // FR-233 nằm ở chính phép đọc này: blob thiếu trường (preset lưu từ
        // phiên bản trước khi tuỳ chọn ấy tồn tại) vẫn đọc *thành công* nhờ
        // `#[serde(default)]` trên `OutputOptions`, và trường vắng mặt nhận
        // giá trị mặc định.
        //
        // `unwrap_or_default` chỉ đỡ trường hợp còn lại: blob hỏng hẳn (sửa
        // tay CSDL). Chỗ rơi là bộ mặc định chứ không phải một lỗi làm biến
        // mất TOÀN BỘ danh sách preset khỏi giao diện.
        output_options: serde_json::from_str(&output_options_raw).unwrap_or_default(),
        is_default: row.get::<_, i64>("is_default")? != 0,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

/// Khe hở hẹp nhất còn chấp nhận được giữa hai vị trí liền kề.
///
/// `f64` có 52 bit phần định trị, nên trên lý thuyết còn chia đôi được sâu hơn
/// ngưỡng này rất nhiều. Đặt ngưỡng cao hơn giới hạn thật nhiều bậc để không
/// bao giờ chạm tới vùng mà phép lấy điểm giữa trả về đúng bằng một trong hai
/// đầu mút — lúc đó thứ tự sẽ hỏng một cách âm thầm.
const MIN_POSITION_GAP: f64 = 1e-6;

/// Vị trí nằm giữa hai hàng xóm. `None` nghĩa là không có hàng xóm ở phía đó,
/// tức là đang thả vào đầu hoặc cuối danh sách.
pub fn position_between(before: Option<f64>, after: Option<f64>) -> f64 {
    match (before, after) {
        (None, None) => 1.0,
        (None, Some(after)) => after - 1.0,
        (Some(before), None) => before + 1.0,
        (Some(before), Some(after)) => (before + after) / 2.0,
    }
}

/// Khe hở giữa hai hàng xóm đã hẹp tới mức phải đánh số lại chưa.
///
/// Chỉ đúng khi có cả hai hàng xóm: ở đầu hoặc cuối danh sách thì luôn còn chỗ
/// vì ta cộng/trừ hẳn 1.0 chứ không chia đôi.
pub fn needs_renormalize(before: Option<f64>, after: Option<f64>) -> bool {
    match (before, after) {
        (Some(before), Some(after)) => (after - before).abs() < MIN_POSITION_GAP,
        _ => false,
    }
}

/// Vị trí hiện tại của một hàng xóm, đọc trong transaction đang mở.
///
/// `Ok(None)` chỉ có đúng một nghĩa: phía đó không có hàng xóm nào (đang thả
/// vào đầu hoặc cuối danh sách). Nếu phía gọi *có* đưa id mà không tìm thấy
/// dòng nào thì đó là lỗi, không phải "đầu danh sách" — hàng xóm có thể vừa
/// hoàn tất và biến khỏi danh sách trong lúc người dùng đang kéo, và đặt nhầm
/// job vào một chỗ tuỳ tiện tệ hơn nhiều so với báo lỗi.
fn position_of(conn: &Connection, job_id: Option<&str>) -> Result<Option<f64>, AppError> {
    let Some(job_id) = job_id else {
        return Ok(None);
    };
    let found: Option<f64> = conn
        .query_row(
            "SELECT queue_position FROM download_jobs WHERE id = ?1",
            params![job_id],
            |row| row.get(0),
        )
        .optional()?;
    match found {
        Some(position) => Ok(Some(position)),
        None => Err(AppError::not_found("Job")),
    }
}

/// Phần ruột của việc đánh số lại, chạy trên transaction do phía gọi mở.
///
/// Tách ra để `move_job_between` chuẩn hoá được *bên trong* transaction của
/// chính nó thay vì mở một transaction thứ hai — nếu không, khoảng giữa hai
/// transaction là chỗ một thao tác khác chen vào và làm hỏng giá trị vừa đọc.
fn renormalize_positions_within(conn: &Connection) -> Result<(), AppError> {
    let ids: Vec<String> = {
        let mut stmt = conn.prepare(
            "SELECT id FROM download_jobs
             WHERE status IN ('queued','paused','downloading','fetching_metadata')
             ORDER BY queue_position ASC, created_at ASC",
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<Vec<String>>>()?
    };

    for (index, id) in ids.iter().enumerate() {
        conn.execute(
            "UPDATE download_jobs SET queue_position = ?1 WHERE id = ?2",
            params![(index + 1) as f64, id],
        )?;
    }
    Ok(())
}

fn media_type_str(media_type: &MediaType) -> &'static str {
    match media_type {
        MediaType::Audio => "audio",
        MediaType::Video => "video",
        MediaType::Gallery => "gallery",
    }
}

fn gallery_mode_str(gallery_mode: &GalleryMode) -> &'static str {
    match gallery_mode {
        GalleryMode::Files => "files",
        GalleryMode::AudioOnly => "audio_only",
        GalleryMode::ImagesOnly => "images_only",
        GalleryMode::Slideshow => "slideshow",
    }
}

fn row_to_job(row: &rusqlite::Row) -> rusqlite::Result<DownloadJob> {
    let media_type_raw: String = row.get("media_type")?;
    let gallery_mode_raw: Option<String> = row.get("gallery_mode")?;
    let selected_gallery_indices_raw: Option<String> = row.get("selected_gallery_urls")?;
    let output_options_raw: Option<String> = row.get("output_options")?;
    let status_raw: String = row.get("status")?;
    Ok(DownloadJob {
        id: row.get("id")?,
        source_url: row.get("source_url")?,
        platform: row.get("platform")?,
        media_type: match media_type_raw.as_str() {
            "audio" => MediaType::Audio,
            "gallery" => MediaType::Gallery,
            _ => MediaType::Video,
        },
        audio_quality: row.get("audio_quality")?,
        video_quality: row.get("video_quality")?,
        gallery_mode: gallery_mode_raw.and_then(|v| match v.as_str() {
            "files" => Some(GalleryMode::Files),
            "audio_only" => Some(GalleryMode::AudioOnly),
            "images_only" => Some(GalleryMode::ImagesOnly),
            "slideshow" => Some(GalleryMode::Slideshow),
            _ => None,
        }),
        selected_gallery_indices: selected_gallery_indices_raw
            .and_then(|raw| serde_json::from_str::<Vec<u32>>(&raw).ok()),
        status: JobStatus::from_str(&status_raw).unwrap_or(JobStatus::Failed),
        progress_percent: row.get("progress_percent")?,
        speed_bytes_per_sec: row.get("speed_bytes_per_sec")?,
        eta_seconds: row.get("eta_seconds")?,
        error_message: row.get("error_message")?,
        output_directory: row.get("output_directory")?,
        output_file_path: row.get("output_file_path")?,
        is_playlist_item: row.get::<_, i64>("is_playlist_item")? != 0,
        parent_playlist_id: row.get("parent_playlist_id")?,
        retried_from_job_id: row.get("retried_from_job_id")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        title: row.get("title")?,
        playlist_title: row.get("playlist_title")?,
        queue_position: row.get("queue_position")?,
        retry_count: row.get("retry_count")?,
        next_retry_at: row.get("next_retry_at")?,
        // NULL = dòng có trước migration 0010, tức tác vụ chạy vào lúc chưa hề
        // có lựa chọn nào để nêu; JSON hỏng = ai đó sửa tay CSDL. Cả hai đều
        // rơi về bộ mặc định, vốn đúng bằng hành vi đang chạy hôm nay — nên
        // một dòng cũ đọc ra vẫn mô tả đúng thứ nó đã thực sự làm.
        output_options: output_options_raw
            .and_then(|raw| serde_json::from_str::<OutputOptions>(&raw).ok())
            .unwrap_or_default(),
    })
}

fn row_to_library_item(row: &rusqlite::Row) -> rusqlite::Result<LibraryItem> {
    let media_type_raw: String = row.get("media_type")?;
    Ok(LibraryItem {
        id: row.get("id")?,
        file_path: row.get("file_path")?,
        title: row.get("title")?,
        // Cùng luật rơi về `Video` như `row_to_job`: một giá trị lạ (hoặc `''`
        // của một dòng mồ côi chưa nạp được) không được phép làm hỏng cả danh
        // sách, và "video" là mặc định vô hại nhất cho phần hiển thị.
        media_type: match media_type_raw.as_str() {
            "audio" => MediaType::Audio,
            "gallery" => MediaType::Gallery,
            _ => MediaType::Video,
        },
        file_format: row.get("file_format")?,
        file_size_bytes: row.get("file_size_bytes")?,
        duration_seconds: row.get("duration_seconds")?,
        platform: row.get("platform")?,
        source_url: row.get("source_url")?,
        thumbnail_path: row.get("thumbnail_path")?,
        downloaded_at: row.get("completed_at")?,
        is_missing: row.get::<_, i64>("is_missing")? != 0,
        job_id: row.get("job_id")?,
    })
}

/// Một dòng `downloaded_files` ghi TRƯỚC migration 0012, cùng dữ liệu của tác
/// vụ sinh ra nó — đầu vào của bước làm giàu trong `backfill_library_index`.
struct StaleIndexRow {
    id: String,
    file_path: String,
    job_title: Option<String>,
    media_type: String,
    platform: String,
    source_url: String,
}

/// Một tác vụ đã hoàn tất mà chỉ mục chưa có dòng nào cho file của nó.
struct UnindexedJobRow {
    job_id: String,
    file_path: String,
    job_title: Option<String>,
    media_type: String,
    platform: String,
    source_url: String,
    updated_at: String,
}

/// Kết quả một lần nạp lại lịch sử (FR-303) — hai con số cho hai đường vào
/// khác nhau, để nhật ký khởi động nói được đã vá cái gì thay vì chỉ "xong".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LibraryBackfillReport {
    /// Dòng đã có sẵn trong `downloaded_files` nhưng thiếu các cột của 0012.
    pub enriched: usize,
    /// Tác vụ hoàn tất chưa hề có dòng nào, dựng lại từ `output_file_path`.
    pub inserted: usize,
}

/// Trạng thái trên đĩa của một mục, đi qua lại giữa vòng `stat` chạy nền và
/// CSDL (FR-323).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryFileState {
    pub id: String,
    pub file_path: String,
    pub is_missing: bool,
    pub file_size_bytes: i64,
}

/// Mệnh đề `WHERE` cùng tham số của nó, dựng từ [`HistoryQuery`]. Luôn ép
/// thêm điều kiện thuộc ba trạng thái kết thúc — Lịch sử không bao giờ trả về
/// một tác vụ đang chạy hay đang chờ, bất kể `query.status` là gì.
struct HistoryFilterSql {
    where_clause: String,
    params: Vec<Box<dyn rusqlite::ToSql>>,
}

impl HistoryFilterSql {
    fn build(query: &HistoryQuery) -> Self {
        let mut clauses = vec!["status IN (?, ?, ?)".to_string()];
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![
            Box::new(JobStatus::Completed.as_str().to_string()),
            Box::new(JobStatus::Failed.as_str().to_string()),
            Box::new(JobStatus::Canceled.as_str().to_string()),
        ];

        if let Some(status) = &query.status {
            clauses.push("status = ?".to_string());
            params.push(Box::new(status.as_str().to_string()));
        }

        if let Some(search) = query.search.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            let needle = format!("%{}%", escape_like(&search.to_lowercase()));
            clauses.push(
                "(LOWER(source_url) LIKE ? ESCAPE '\\'
                  OR LOWER(COALESCE(output_file_path, '')) LIKE ? ESCAPE '\\'
                  OR LOWER(platform) LIKE ? ESCAPE '\\')"
                    .to_string(),
            );
            params.push(Box::new(needle.clone()));
            params.push(Box::new(needle.clone()));
            params.push(Box::new(needle));
        }

        HistoryFilterSql {
            where_clause: format!("WHERE {}", clauses.join(" AND ")),
            params,
        }
    }
}

/// Mệnh đề `WHERE` cùng tham số của nó, dựng từ [`LibraryQuery`].
///
/// Mọi giá trị đều đi qua tham số bind (`?`), kể cả những chỗ trông như hằng —
/// chuỗi duy nhất được ghép vào câu lệnh là số lượng dấu `?` và tên cột, cả
/// hai đều do mã này chọn chứ không do người dùng. Từ khoá tìm kiếm chứa `%`
/// hay `_` vì thế cũng không phá được câu lệnh, chỉ là một `LIKE` rộng hơn
/// mong đợi (xem `escape_like`).
struct LibraryFilterSql {
    where_clause: String,
    params: Vec<Box<dyn rusqlite::ToSql>>,
}

impl LibraryFilterSql {
    fn build(query: &LibraryQuery) -> Self {
        let mut clauses: Vec<String> = Vec::new();
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

        // FR-307: một từ khoá khớp tiêu đề HOẶC tên file. `search_text` đã
        // gộp sẵn cả hai và đã hạ hoa theo Unicode lúc ghi, nên ở đây chỉ cần
        // hạ hoa từ khoá cho cân xứng.
        if let Some(search) = query.search.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            clauses.push("search_text LIKE ? ESCAPE '\\'".to_string());
            params.push(Box::new(format!("%{}%", escape_like(&search.to_lowercase()))));
        }

        // Mỗi bộ lọc là một mệnh đề AND riêng (FR-308); nhiều giá trị trong
        // cùng một bộ lọc là OR, thể hiện bằng `IN (...)`.
        if !query.media_types.is_empty() {
            let placeholders = vec!["?"; query.media_types.len()].join(",");
            clauses.push(format!("media_type IN ({placeholders})"));
            for media_type in &query.media_types {
                params.push(Box::new(media_type_str(media_type).to_string()));
            }
        }
        if !query.platforms.is_empty() {
            let placeholders = vec!["?"; query.platforms.len()].join(",");
            clauses.push(format!("platform IN ({placeholders})"));
            for platform in &query.platforms {
                params.push(Box::new(platform.clone()));
            }
        }
        if !query.formats.is_empty() {
            let placeholders = vec!["?"; query.formats.len()].join(",");
            clauses.push(format!("file_format IN ({placeholders})"));
            for format in &query.formats {
                params.push(Box::new(format.clone()));
            }
        }
        if let Some(from) = &query.downloaded_from {
            clauses.push("completed_at >= ?".to_string());
            params.push(Box::new(from.clone()));
        }
        if let Some(to) = &query.downloaded_to {
            clauses.push("completed_at <= ?".to_string());
            params.push(Box::new(to.clone()));
        }
        if let Some(is_missing) = query.is_missing {
            clauses.push("is_missing = ?".to_string());
            params.push(Box::new(is_missing as i64));
        }

        let where_clause = if clauses.is_empty() {
            String::new()
        } else {
            format!("WHERE {}", clauses.join(" AND "))
        };
        LibraryFilterSql {
            where_clause,
            params,
        }
    }
}

/// `%` và `_` trong từ khoá người dùng gõ phải là ký tự thường, không phải ký
/// tự đại diện — một tiêu đề có thật như `"100% Music"` mà biến thành "khớp
/// mọi thứ" là một kết quả tìm kiếm sai, chỉ có điều sai theo hướng im lặng.
fn escape_like(raw: &str) -> String {
    raw.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// `LIMIT`/`OFFSET` ghép thẳng vào câu lệnh vì cả hai đã là `i64` do serde
/// phân giải — không có đường nào để một chuỗi tuỳ ý đi tới đây. `OFFSET`
/// không có nghĩa nếu thiếu `LIMIT`, nên SQLite đòi một `LIMIT` đứng trước và
/// `-1` là cách chính tắc để nói "không giới hạn".
fn limit_clause(query: &LibraryQuery) -> String {
    match (query.limit, query.offset) {
        (None, None) => String::new(),
        (limit, offset) => format!(
            "LIMIT {} OFFSET {}",
            limit.unwrap_or(-1),
            offset.unwrap_or(0)
        ),
    }
}

/// Nội dung cột `search_text`: tiêu đề và đường dẫn, hạ hoa bằng bảng Unicode
/// của Rust. Xem chú thích trong migration 0012 — `LIKE` và `lower()` của
/// SQLite chỉ biết A-Z, nên nếu không có bước này thì tìm `"đừng"` sẽ không ra
/// `"ĐỪNG VỀ TRỄ NHA"`, vốn là một tiêu đề có thật trong thư viện người dùng.
fn search_text_for(title: &str, file_path: &str) -> String {
    format!("{title}\n{file_path}").to_lowercase()
}

/// Tiêu đề để hiện cho một mục nạp lại từ lịch sử (FR-303).
///
/// 81 trong 105 tác vụ hoàn tất của người dùng KHÔNG có `title` — cột ấy chỉ
/// tồn tại từ migration 0007. Tên file là câu trả lời đúng cho chúng chứ
/// không phải chuỗi rỗng hay `"untitled"`: chính yt-dlp đã đặt tên file ấy từ
/// tiêu đề thật của nguồn, nên nó vẫn là tiêu đề, chỉ đi đường vòng.
fn display_title(job_title: Option<&str>, file_path: &str) -> String {
    if let Some(title) = job_title.map(str::trim).filter(|t| !t.is_empty()) {
        return title.to_string();
    }
    let stem = Path::new(file_path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("")
        .trim();
    if stem.is_empty() {
        file_path.to_string()
    } else {
        stem.to_string()
    }
}

/// Định dạng file của một đường dẫn, theo nghĩa mà bộ lọc FR-308 dùng được.
///
/// Không phải mọi thứ đứng sau dấu chấm cuối cùng đều là một phần mở rộng.
/// Một tác vụ gallery nhiều ảnh lưu cả **thư mục** làm đường dẫn kết quả, và
/// tên thư mục ấy là caption của bài đăng — trong CSDL thật của người dùng có
/// một thư mục tên `https   vgen.co k1lltheguard46 (28f98824)`, mà
/// `Path::extension` đọc thành định dạng `co k1lltheguard46 (28f98824)`. Thả
/// nguyên chuỗi đó vào danh sách định dạng sẽ cho người dùng một ô lọc vô
/// nghĩa mà chỉ đúng một mục khớp.
///
/// Luật: tối đa 5 ký tự và toàn chữ/số — đủ cho mọi định dạng media thật
/// (`mp3`, `webm`, `flac`, `jpeg`, `opus`), và loại thẳng những chuỗi có
/// khoảng trắng hay dấu ngoặc. Không nhận ra được thì trả `""`, nghĩa là
/// "không có định dạng", một câu trả lời đúng cho một thư mục.
pub(crate) fn media_file_extension(file_path: &str) -> String {
    Path::new(file_path)
        .extension()
        .and_then(|ext| ext.to_str())
        .filter(|ext| {
            !ext.is_empty()
                && ext.chars().count() <= 5
                && ext.chars().all(|c| c.is_ascii_alphanumeric())
        })
        .unwrap_or("")
        .to_ascii_lowercase()
}

/// Vi phạm chỉ mục duy nhất trên `file_path` nghĩa là đúng một chuyện: đã có
/// một mục khác trỏ vào đường dẫn đó. FR-322 đòi thất bại rõ ràng thay vì ghi
/// đè, nên nó được dịch thành mã lỗi riêng chứ không lẫn vào `INTERNAL`.
fn library_write_error(err: rusqlite::Error) -> AppError {
    if let rusqlite::Error::SqliteFailure(inner, _) = &err {
        if inner.code == rusqlite::ErrorCode::ConstraintViolation {
            return AppError::new(
                FILE_EXISTS_ERROR_CODE,
                "Another library item already points at that path",
            );
        }
    }
    AppError::from(err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        AudioOutput, CodecPreference, JobStatus, LibrarySort, MediaType, SortDirection,
        VideoContainer,
    };

    /// Mỗi test dùng một file DB riêng trong thư mục tạm của hệ điều hành để
    /// migration chạy thật (in-memory không kiểm chứng được `to_latest`).
    fn temp_db() -> Db {
        let path = std::env::temp_dir()
            .join(format!("media-downloader-test-{}.db", uuid::Uuid::new_v4()));
        Db::open(&path).expect("db opens")
    }

    fn sample_job(id: &str) -> DownloadJob {
        DownloadJob {
            id: id.to_string(),
            source_url: "https://example.com/v".to_string(),
            platform: "youtube".to_string(),
            media_type: MediaType::Audio,
            audio_quality: Some("128kbps".to_string()),
            video_quality: None,
            gallery_mode: None,
            selected_gallery_indices: None,
            status: JobStatus::Queued,
            progress_percent: 0.0,
            speed_bytes_per_sec: None,
            eta_seconds: None,
            error_message: None,
            output_directory: "/tmp".to_string(),
            output_file_path: None,
            is_playlist_item: false,
            parent_playlist_id: None,
            retried_from_job_id: None,
            created_at: "2026-07-26T00:00:00Z".to_string(),
            updated_at: "2026-07-26T00:00:00Z".to_string(),
            title: None,
            playlist_title: None,
            queue_position: 0.0,
            retry_count: 0,
            next_retry_at: None,
            output_options: OutputOptions::default(),
        }
    }

    #[test]
    fn round_trips_scheduling_fields() {
        let db = temp_db();
        let mut job = sample_job("job-1");
        job.queue_position = 7.5;
        job.retry_count = 2;
        job.next_retry_at = Some("2026-07-26T00:00:30Z".to_string());
        db.insert_job(&job).expect("insert works");

        let loaded = db.get_job("job-1").expect("query works").expect("job exists");
        assert_eq!(loaded.queue_position, 7.5);
        assert_eq!(loaded.retry_count, 2);
        assert_eq!(
            loaded.next_retry_at.as_deref(),
            Some("2026-07-26T00:00:30Z")
        );
        // `insert_job` gán tham số theo vị trí `?1..?25`: chỉ cần lệch thứ tự
        // một chỗ là ghi nhầm cột mà vẫn chạy trót lọt. So sánh nguyên cả bản
        // ghi canh giữ đủ 25 cột, chứ không riêng ba cột vừa thêm.
        assert_eq!(loaded, job);
    }

    fn job_with_status(id: &str, status: JobStatus, updated_at: &str) -> DownloadJob {
        let mut job = sample_job(id);
        job.status = status;
        job.updated_at = updated_at.to_string();
        job
    }

    fn history_query(limit: i64, offset: i64) -> HistoryQuery {
        HistoryQuery {
            limit,
            offset,
            ..Default::default()
        }
    }

    #[test]
    fn paginates_history_most_recent_first() {
        let db = temp_db();
        db.insert_job(&job_with_status(
            "a",
            JobStatus::Completed,
            "2026-07-26T00:00:01Z",
        ))
        .unwrap();
        db.insert_job(&job_with_status(
            "b",
            JobStatus::Completed,
            "2026-07-26T00:00:03Z",
        ))
        .unwrap();
        db.insert_job(&job_with_status(
            "c",
            JobStatus::Failed,
            "2026-07-26T00:00:02Z",
        ))
        .unwrap();
        // A still-active job must never leak into a history page.
        db.insert_job(&job_with_status(
            "d",
            JobStatus::Downloading,
            "2026-07-26T00:00:04Z",
        ))
        .unwrap();

        let page1 = db.list_history_page(&history_query(2, 0)).unwrap();
        assert_eq!(
            page1.iter().map(|j| j.id.as_str()).collect::<Vec<_>>(),
            vec!["b", "c"]
        );

        let page2 = db.list_history_page(&history_query(2, 2)).unwrap();
        assert_eq!(
            page2.iter().map(|j| j.id.as_str()).collect::<Vec<_>>(),
            vec!["a"]
        );
    }

    #[test]
    fn counts_the_same_filter_the_page_uses_ignoring_limit_and_offset() {
        let db = temp_db();
        for i in 0..5 {
            db.insert_job(&job_with_status(
                &format!("job-{i}"),
                JobStatus::Completed,
                &format!("2026-07-26T00:00:0{i}Z"),
            ))
            .unwrap();
        }
        db.insert_job(&job_with_status(
            "still-running",
            JobStatus::Downloading,
            "2026-07-26T00:00:09Z",
        ))
        .unwrap();

        assert_eq!(db.count_history(&history_query(2, 0)).unwrap(), 5);
    }

    #[test]
    fn filters_history_to_a_single_tab_status() {
        let db = temp_db();
        db.insert_job(&job_with_status(
            "done",
            JobStatus::Completed,
            "2026-07-26T00:00:01Z",
        ))
        .unwrap();
        db.insert_job(&job_with_status(
            "broke",
            JobStatus::Failed,
            "2026-07-26T00:00:02Z",
        ))
        .unwrap();

        let query = HistoryQuery {
            status: Some(JobStatus::Failed),
            ..history_query(10, 0)
        };
        let page = db.list_history_page(&query).unwrap();
        assert_eq!(
            page.iter().map(|j| j.id.as_str()).collect::<Vec<_>>(),
            vec!["broke"]
        );
        assert_eq!(db.count_history(&query).unwrap(), 1);
    }

    #[test]
    fn filters_history_by_search_across_url_file_and_platform() {
        let db = temp_db();
        let mut matches_platform =
            job_with_status("a", JobStatus::Completed, "2026-07-26T00:00:01Z");
        matches_platform.platform = "tiktok".to_string();
        db.insert_job(&matches_platform).unwrap();

        let mut matches_file = job_with_status("b", JobStatus::Completed, "2026-07-26T00:00:02Z");
        matches_file.output_file_path = Some("/out/My TikTok Clip.mp4".to_string());
        db.insert_job(&matches_file).unwrap();

        let mut no_match = job_with_status("c", JobStatus::Completed, "2026-07-26T00:00:03Z");
        no_match.platform = "youtube".to_string();
        db.insert_job(&no_match).unwrap();

        let query = HistoryQuery {
            search: Some("tiktok".to_string()),
            ..history_query(10, 0)
        };
        let ids: Vec<String> = db
            .list_history_page(&query)
            .unwrap()
            .into_iter()
            .map(|j| j.id)
            .collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"a".to_string()));
        assert!(ids.contains(&"b".to_string()));
        assert_eq!(db.count_history(&query).unwrap(), 2);
    }

    #[test]
    fn deletes_only_the_requested_statuses() {
        let db = temp_db();
        db.insert_job(&job_with_status(
            "a",
            JobStatus::Completed,
            "2026-07-26T00:00:01Z",
        ))
        .unwrap();
        db.insert_job(&job_with_status(
            "b",
            JobStatus::Failed,
            "2026-07-26T00:00:02Z",
        ))
        .unwrap();
        db.insert_job(&job_with_status(
            "c",
            JobStatus::Downloading,
            "2026-07-26T00:00:03Z",
        ))
        .unwrap();

        let deleted = db
            .delete_jobs_by_statuses(&[
                JobStatus::Completed,
                JobStatus::Failed,
                JobStatus::Canceled,
            ])
            .unwrap();

        assert_eq!(deleted, 2);
        assert!(db.get_job("a").unwrap().is_none());
        assert!(db.get_job("b").unwrap().is_none());
        assert!(db.get_job("c").unwrap().is_some());
    }

    #[test]
    fn queue_position_column_is_declared_real_not_integer() {
        // Kiểu cột ở đây là thứ chịu tải: fractional indexing chèn điểm giữa
        // hai hàng xóm (1.0 và 2.0 -> 1.5), nên cột mang affinity INTEGER sẽ
        // làm tròn mất giá trị đó và phá luôn cách sắp thứ tự. Một phép
        // `assert_eq!(pos, 7.5)` bình thường KHÔNG bắt được lỗi này: SQLite
        // vẫn lưu 7.5 nguyên vẹn vào cột INTEGER (affinity chỉ là gợi ý, và
        // 7.5 không chuyển vô tổn hao sang integer nên nó giữ dạng real). Vì
        // vậy phải kiểm tra thẳng kiểu đã khai báo trong lược đồ.
        let db = temp_db();
        let conn = db.conn();
        let mut stmt = conn.prepare("PRAGMA table_info(download_jobs)").unwrap();
        let declared_type = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>("name")?, row.get::<_, String>("type")?))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
            .into_iter()
            .find(|(name, _)| name == "queue_position")
            .map(|(_, column_type)| column_type)
            .expect("cột queue_position phải tồn tại");

        assert_eq!(declared_type, "REAL");
    }

    /// Mở một DB tạm rồi chỉ chạy migration *tới* `version`, dựng lại đúng
    /// lược đồ ở thời điểm trước khi 0008 tồn tại — `Db::open` luôn chạy
    /// `to_latest` nên không dùng được cho việc này. Việc tắt/bật
    /// `PRAGMA foreign_keys` lặp lại y hệt `Db::open` vì migration 0002/0003
    /// rebuild `download_jobs` bằng DROP + RENAME trong khi
    /// `downloaded_files.job_id` vẫn trỏ vào nó (xem chú thích ở `Db::open`).
    fn raw_conn_at_version(version: usize) -> Connection {
        raw_conn_at_version_with_path(version).0
    }

    /// Như trên nhưng trả kèm đường dẫn file, để một test có thể đóng kết nối
    /// thô rồi mở lại CHÍNH file đó bằng `Db::open` — cách duy nhất kiểm
    /// chứng được `backfill_library_index`, vốn chỉ chạy trên đường
    /// `Db::open` và chỉ có việc làm khi trong CSDL đã có sẵn dữ liệu của một
    /// phiên bản cũ.
    fn raw_conn_at_version_with_path(version: usize) -> (Connection, std::path::PathBuf) {
        let path =
            std::env::temp_dir().join(format!("media-downloader-test-{}.db", uuid::Uuid::new_v4()));
        let mut conn = Connection::open(&path).expect("db opens");
        conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
        migrations()
            .to_version(&mut conn, version)
            .expect("migrates to the requested version");
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        (conn, path)
    }

    #[test]
    fn migration_backfills_positions_for_pre_existing_rows() {
        // Job tạo trước 0008 không hề có `queue_position`, nên ngay sau khi
        // thêm cột chúng đều mang mặc định 0 — hoà nhau hết, không còn thứ tự
        // tương đối nào để sắp. Dòng `UPDATE ... SET queue_position = rowid`
        // trong 0008 tồn tại chính là để phá thế hoà đó, nên test phải dựng
        // được một dòng *có trước* migration mới kiểm chứng được nó.
        //
        // Phải chèn bằng SQL thô: `insert_job` ghi cả ba cột mới, vốn chưa tồn
        // tại ở version 7.
        let mut conn = raw_conn_at_version(7);
        for (index, id) in ["job-a", "job-b", "job-c"].into_iter().enumerate() {
            conn.execute(
                "INSERT INTO download_jobs (
                    id, source_url, platform, media_type, status,
                    output_directory, created_at, updated_at
                ) VALUES (?1,?2,'youtube','audio','queued','/tmp',?3,?3)",
                params![
                    id,
                    format!("https://example.com/{id}"),
                    format!("2026-07-26T00:00:0{index}Z"),
                ],
            )
            .expect("raw insert works against the version-7 schema");
        }

        migrations().to_latest(&mut conn).expect("0008 applies");

        let mut stmt = conn
            .prepare("SELECT queue_position FROM download_jobs ORDER BY rowid")
            .unwrap();
        let positions: Vec<f64> = stmt
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();

        assert_eq!(positions.len(), 3);
        // Con số cụ thể không phải hợp đồng — điều backfill phải bảo đảm là
        // các dòng cũ sắp được thứ tự *so với nhau*, tức phân biệt và tăng dần
        // theo rowid, thay vì cùng nằm ở 0.
        assert!(
            positions.windows(2).all(|pair| pair[0] < pair[1]),
            "vị trí sau backfill phải tăng dần theo rowid, nhận được {positions:?}"
        );
        assert!(
            positions.iter().all(|position| *position > 0.0),
            "không dòng cũ nào được phép giữ mặc định 0, nhận được {positions:?}"
        );
    }

    /// Chèn một dòng bằng SQL thô ở lược đồ *trước* 0009, với đúng cặp
    /// (status, progress_percent) mà lỗi cũ để lại. Không dùng `insert_job`:
    /// nó đi qua `Db`, mà `Db::open` luôn chạy `to_latest`, nên sẽ không còn
    /// dòng nào "có trước migration" để backfill kiểm chứng.
    fn insert_raw_job(conn: &Connection, id: &str, status: &str, progress_percent: f64) {
        conn.execute(
            "INSERT INTO download_jobs (
                id, source_url, platform, media_type, status, progress_percent,
                output_directory, created_at, updated_at
            ) VALUES (?1,?2,'youtube','audio',?3,?4,'/tmp','2026-07-20T00:00:00Z','2026-07-20T00:00:00Z')",
            params![id, format!("https://example.com/{id}"), status, progress_percent],
        )
        .expect("raw insert works against the pre-0009 schema");
    }

    fn raw_progress_of(conn: &Connection, id: &str) -> f64 {
        conn.query_row(
            "SELECT progress_percent FROM download_jobs WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .expect("job row exists")
    }

    #[test]
    fn migration_0009_backfills_completed_jobs_stuck_at_a_wrong_percentage() {
        // Những dòng này là hình ảnh thu nhỏ của CSDL thật: 37 job `completed`
        // nằm ở 0% vì `parse_progress` từng quy "không biết tổng dung lượng"
        // thành 0.0 và ghi đè ở mọi nhịp. Phải dựng chúng ở lược đồ TRƯỚC
        // 0009 thì mới có thứ cho backfill sửa — nếu tạo qua `Db` thì
        // `update_job_status` mới đã ép sẵn 100 và test sẽ chỉ kiểm chứng
        // đúng cái nó vừa tự tay ghi.
        let mut conn = raw_conn_at_version(8);
        insert_raw_job(&conn, "audio-stuck-at-zero", "completed", 0.0);
        insert_raw_job(&conn, "video-stuck-partway", "completed", 43.5);
        insert_raw_job(&conn, "already-full", "completed", 100.0);

        migrations().to_latest(&mut conn).expect("0009 applies");

        assert_eq!(raw_progress_of(&conn, "audio-stuck-at-zero"), 100.0);
        assert_eq!(
            raw_progress_of(&conn, "video-stuck-partway"),
            100.0,
            "đã completed thì 43.5% là con số mâu thuẫn với chính trạng thái của nó"
        );
        assert_eq!(raw_progress_of(&conn, "already-full"), 100.0);
    }

    #[test]
    fn migration_0009_leaves_every_unfinished_or_stopped_job_alone() {
        // Nửa còn lại của hợp đồng, và là nửa dễ hỏng hơn: một câu UPDATE
        // thiếu mệnh đề WHERE cũng làm test trên xanh. 3 job `paused` trong
        // CSDL thật đang ở 0% và phải giữ nguyên 0% — chúng chưa tải xong.
        // `failed`/`canceled` thì đã dừng thật ở đâu đó, nên phần trăm dở
        // dang là thông tin đúng chứ không phải rác cần dọn.
        let mut conn = raw_conn_at_version(8);
        insert_raw_job(&conn, "paused", "paused", 0.0);
        insert_raw_job(&conn, "queued", "queued", 0.0);
        insert_raw_job(&conn, "downloading", "downloading", 12.5);
        insert_raw_job(&conn, "failed-halfway", "failed", 50.0);
        insert_raw_job(&conn, "failed-at-zero", "failed", 0.0);
        insert_raw_job(&conn, "canceled", "canceled", 0.0);

        migrations().to_latest(&mut conn).expect("0009 applies");

        assert_eq!(raw_progress_of(&conn, "paused"), 0.0);
        assert_eq!(raw_progress_of(&conn, "queued"), 0.0);
        assert_eq!(raw_progress_of(&conn, "downloading"), 12.5);
        assert_eq!(raw_progress_of(&conn, "failed-halfway"), 50.0);
        assert_eq!(raw_progress_of(&conn, "failed-at-zero"), 0.0);
        assert_eq!(raw_progress_of(&conn, "canceled"), 0.0);
    }

    // ---- specs/003-media-output: lựa chọn đầu ra (migration 0010) ---------

    #[test]
    fn migration_0010_leaves_pre_existing_jobs_meaning_exactly_what_they_meant() {
        // Dòng này được tạo ở lược đồ version 9, khi cột `output_options` còn
        // chưa tồn tại — nên phải chèn bằng SQL thô. Đi qua `insert_job` thì
        // `Db::open` đã chạy `to_latest` và dòng sinh ra sẽ mang sẵn chuỗi JSON
        // do chính lượt ghi ấy đặt vào; test khi đó chỉ kiểm chứng đúng thứ nó
        // vừa tự tay ghi, chứ không nói gì về hành vi của migration.
        let mut conn = raw_conn_at_version(9);
        conn.execute(
            "INSERT INTO download_jobs (
                id, source_url, platform, media_type, audio_quality, status,
                output_directory, created_at, updated_at
            ) VALUES ('legacy','https://example.com/v','youtube','audio','128kbps',
                      'completed','/tmp','2026-07-20T00:00:00Z','2026-07-20T00:00:00Z')",
            [],
        )
        .expect("raw insert works against the version-9 schema");

        migrations().to_latest(&mut conn).expect("0010 applies");

        // Nửa thứ nhất: migration KHÔNG bịa giá trị cho dòng cũ. "Tác vụ này
        // không nêu lựa chọn nào" và "tác vụ này đã chọn đúng bộ mặc định" là
        // hai chuyện khác nhau, và chỉ cái đầu đúng với dữ liệu có trước.
        let stored: Option<String> = conn
            .query_row(
                "SELECT output_options FROM download_jobs WHERE id = 'legacy'",
                [],
                |row| row.get(0),
            )
            .expect("cột output_options phải tồn tại sau migration");
        assert_eq!(stored, None, "migration không được backfill dòng cũ");

        // Nửa thứ hai, và là nửa người dùng thật sự cảm nhận được: đọc dòng ấy
        // qua đúng bộ đọc thật phải cho ra bộ mặc định — tức đúng hành vi mà
        // tác vụ đó đã chạy khi được tạo.
        let job = conn
            .query_row(
                "SELECT * FROM download_jobs WHERE id = 'legacy'",
                [],
                row_to_job,
            )
            .expect("dòng cũ vẫn đọc được nguyên vẹn");
        assert_eq!(job.output_options, OutputOptions::default());
        assert_eq!(
            job.audio_quality.as_deref(),
            Some("128kbps"),
            "migration không được đụng tới dữ liệu sẵn có của dòng"
        );
    }

    #[test]
    fn output_options_survive_a_round_trip_through_the_database() {
        // FR-235: thử lại phải tái tạo đúng cấu hình ban đầu, nên mọi lựa chọn
        // phải quay về nguyên vẹn — kể cả bitrate nằm bên trong biến thể enum,
        // vốn là chỗ một lược đồ tuần tự hoá sai sẽ đánh rơi âm thầm.
        let db = temp_db();
        let mut job = sample_job("job-1");
        job.output_options = OutputOptions {
            audio: AudioOutput::Opus {
                bitrate_kbps: Some(192),
            },
            video_container: VideoContainer::Mkv,
            codec_preference: CodecPreference::Quality,
            embed_metadata: true,
            embed_thumbnail: true,
            ..OutputOptions::default()
        };
        db.insert_job(&job).unwrap();

        let loaded = db.get_job("job-1").unwrap().unwrap();
        assert_eq!(loaded.output_options, job.output_options);
        // So sánh nguyên cả bản ghi: `insert_job` gán tham số theo vị trí
        // `?1..?26`, nên lệch một chỗ là ghi nhầm cột mà vẫn chạy trót lọt.
        assert_eq!(loaded, job);
    }

    #[test]
    fn a_lossless_choice_has_nowhere_to_put_a_bitrate_even_after_a_round_trip() {
        // FR-203 ở tầng lưu trữ. Kiểu dữ liệu đã khiến `Flac { bitrate }`
        // không viết ra được, nhưng cột là JSON tự do — nên điều đáng kiểm
        // chứng là chuỗi thật sự nằm trong CSDL cũng không mang bitrate nào,
        // chứ không phải chỉ struct trong bộ nhớ.
        let db = temp_db();
        let mut job = sample_job("job-1");
        job.output_options = OutputOptions {
            audio: AudioOutput::Flac,
            ..OutputOptions::default()
        };
        db.insert_job(&job).unwrap();

        let stored: String = db
            .conn()
            .query_row(
                "SELECT output_options FROM download_jobs WHERE id = 'job-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();

        // Kiểm đúng điều đang được khẳng định — nhánh `audio` trong chuỗi đã
        // lưu không mang bitrate — chứ không so nguyên cả chuỗi: một khẳng
        // định trên toàn chuỗi sẽ đỏ mỗi lần phase này thêm một tuỳ chọn
        // không liên quan, và khi đó chẳng nói được gì về FR-203 cả.
        let stored_value: serde_json::Value =
            serde_json::from_str(&stored).expect("cột chứa JSON hợp lệ");
        let audio = stored_value
            .get("audio")
            .expect("lựa chọn audio phải nằm trong chuỗi đã lưu");
        assert_eq!(audio.get("format").and_then(|v| v.as_str()), Some("flac"));
        assert!(
            !stored.contains("bitrate"),
            "chuỗi thật sự nằm trong CSDL không được mang bitrate nào: {stored}"
        );
    }

    #[test]
    fn an_options_blob_from_an_older_version_keeps_working() {
        // FR-233: bản ghi lưu trước khi một tuỳ chọn tồn tại phải vẫn đọc
        // được, và tuỳ chọn mới nhận giá trị mặc định — chứ không làm hỏng cả
        // bản ghi. Đây là hợp đồng mà các lát cắt sau của phase này (phụ đề,
        // cắt đoạn, chapter) sẽ dựa vào khi thêm trường mới.
        let stored = r#"{"audio":{"format":"m4a","bitrate_kbps":256}}"#;
        let parsed: OutputOptions = serde_json::from_str(stored).expect("vẫn phải đọc được");

        assert_eq!(
            parsed.audio,
            AudioOutput::M4a {
                bitrate_kbps: Some(256)
            }
        );
        assert_eq!(parsed.video_container, VideoContainer::Mp4);
        assert_eq!(parsed.codec_preference, CodecPreference::Compatibility);
        assert!(!parsed.embed_metadata);
    }

    #[test]
    fn a_corrupted_options_blob_falls_back_to_todays_behaviour() {
        // Cột JSON không có kiểm tra ở tầng SQL — đó là cái giá đã biết trước
        // của quyết định gom một cột. Chỗ rơi phải là bộ mặc định (đúng hành
        // vi hiện hành), chứ không phải một dòng không đọc nổi khiến cả hàng
        // đợi biến mất khỏi giao diện.
        let db = temp_db();
        db.insert_job(&sample_job("job-1")).unwrap();
        db.conn()
            .execute(
                "UPDATE download_jobs SET output_options = 'not json at all' WHERE id = 'job-1'",
                [],
            )
            .unwrap();

        let loaded = db.get_job("job-1").unwrap().expect("dòng vẫn phải đọc được");
        assert_eq!(loaded.output_options, OutputOptions::default());
    }

    #[test]
    fn completing_a_job_forces_full_progress_even_from_a_lower_value() {
        // Bất biến cấu trúc: một dòng nói `completed` mà lại nói 12% thì tự
        // mâu thuẫn. Đặt sẵn 12% qua đường tiến độ bình thường rồi mới hoàn
        // tất — đúng trình tự mà một job audio thật đi qua khi yt-dlp chỉ báo
        // được vài nhịp có tổng dung lượng rồi thôi.
        let db = temp_db();
        db.insert_job(&sample_job("job-1")).unwrap();
        db.update_job_progress("job-1", Some(12.0), Some(1_000), Some(5))
            .unwrap();

        db.update_job_status("job-1", JobStatus::Completed, None)
            .unwrap();

        assert_eq!(db.get_job("job-1").unwrap().unwrap().progress_percent, 100.0);
    }

    #[test]
    fn completing_a_job_that_never_reported_any_progress_still_reads_full() {
        // Ca thật sự gây ra 37 dòng hỏng: yt-dlp không báo tổng dung lượng lần
        // nào, nên không có nhịp tiến độ nào ghi được số, và job xong ở 0%.
        let db = temp_db();
        db.insert_job(&sample_job("job-1")).unwrap();
        db.update_job_progress("job-1", None, Some(2_000), None).unwrap();

        db.update_job_status("job-1", JobStatus::Completed, None)
            .unwrap();

        assert_eq!(db.get_job("job-1").unwrap().unwrap().progress_percent, 100.0);
    }

    #[test]
    fn failing_or_canceling_a_job_keeps_the_progress_it_really_reached() {
        // Mặt còn lại: chỉ `completed` mới được ép 100. Một job hỏng ở 43% đã
        // thật sự tải được 43%, và ghi đè con số đó là làm mất thông tin —
        // nhất là khi người dùng đang cân nhắc thử lại.
        let db = temp_db();
        for id in ["failed", "canceled", "paused"] {
            db.insert_job(&sample_job(id)).unwrap();
            db.update_job_progress(id, Some(43.0), None, None).unwrap();
        }

        db.update_job_status("failed", JobStatus::Failed, Some("network timeout"))
            .unwrap();
        db.update_job_status("canceled", JobStatus::Canceled, None)
            .unwrap();
        db.update_job_status("paused", JobStatus::Paused, None).unwrap();

        for id in ["failed", "canceled", "paused"] {
            assert_eq!(
                db.get_job(id).unwrap().unwrap().progress_percent,
                43.0,
                "{id} phải giữ nguyên phần trăm dở dang có thật của nó"
            );
        }
    }

    #[test]
    fn an_unknown_percentage_keeps_the_last_known_one_instead_of_resetting_to_zero() {
        // `None` nghĩa là "nhịp này không biết phần trăm", không phải "0%".
        // Cột vẫn là REAL NOT NULL nên nó giữ giá trị biết được gần nhất; ghi
        // 0.0 vào đây sẽ tái tạo đúng lỗi cũ ở tầng dưới. Tốc độ thì ngược
        // lại: nó là số đo của chính nhịp này nên vẫn được cập nhật.
        let db = temp_db();
        db.insert_job(&sample_job("job-1")).unwrap();
        db.update_job_progress("job-1", Some(37.5), Some(1_000), Some(9))
            .unwrap();

        db.update_job_progress("job-1", None, Some(2_500), None).unwrap();

        let loaded = db.get_job("job-1").unwrap().unwrap();
        assert_eq!(loaded.progress_percent, 37.5);
        assert_eq!(loaded.speed_bytes_per_sec, Some(2_500));
        assert_eq!(loaded.eta_seconds, None);
    }

    #[test]
    fn next_dispatchable_job_respects_position_then_created_at() {
        let db = temp_db();
        let mut later = sample_job("later");
        later.queue_position = 5.0;
        later.created_at = "2026-07-26T00:00:00Z".to_string();
        let mut earlier = sample_job("earlier");
        earlier.queue_position = 1.0;
        earlier.created_at = "2026-07-26T23:00:00Z".to_string();
        db.insert_job(&later).unwrap();
        db.insert_job(&earlier).unwrap();

        let picked = db
            .next_dispatchable_job("2026-07-27T00:00:00Z")
            .unwrap()
            .expect("a job is dispatchable");
        assert_eq!(picked.id, "earlier", "queue_position thắng created_at");
    }

    #[test]
    fn next_dispatchable_job_skips_jobs_waiting_to_retry() {
        let db = temp_db();
        let mut waiting = sample_job("waiting");
        waiting.next_retry_at = Some("2026-07-26T00:10:00Z".to_string());
        db.insert_job(&waiting).unwrap();

        let too_early = db.next_dispatchable_job("2026-07-26T00:05:00Z").unwrap();
        assert!(
            too_early.is_none(),
            "chưa tới giờ thử lại thì không được chọn"
        );

        let due = db.next_dispatchable_job("2026-07-26T00:10:01Z").unwrap();
        assert_eq!(due.expect("tới giờ rồi").id, "waiting");
    }

    #[test]
    fn next_dispatchable_job_ignores_non_queued_statuses() {
        let db = temp_db();
        let mut paused = sample_job("paused");
        paused.status = JobStatus::Paused;
        db.insert_job(&paused).unwrap();

        assert!(db
            .next_dispatchable_job("2026-07-27T00:00:00Z")
            .unwrap()
            .is_none());
    }

    #[test]
    fn next_queue_position_appends_past_the_maximum() {
        let db = temp_db();
        assert_eq!(
            db.next_queue_position().unwrap(),
            1.0,
            "hàng đợi rỗng bắt đầu từ 1.0"
        );

        let mut job = sample_job("job-1");
        job.queue_position = 4.0;
        db.insert_job(&job).unwrap();
        assert_eq!(db.next_queue_position().unwrap(), 5.0);

        // Job đã kết thúc không còn nằm trong hàng đợi, nên vị trí của nó
        // không được kéo theo vị trí cấp cho job mới — nếu không, mỗi lần tải
        // xong lại đẩy số thứ tự phình lên vô hạn.
        let mut finished = sample_job("finished");
        finished.status = JobStatus::Completed;
        finished.queue_position = 900.0;
        db.insert_job(&finished).unwrap();
        assert_eq!(
            db.next_queue_position().unwrap(),
            5.0,
            "job đã hoàn tất không được tính vào cuối hàng đợi"
        );
    }

    #[test]
    fn position_between_takes_the_midpoint_of_two_neighbours() {
        assert_eq!(position_between(Some(1.0), Some(2.0)), 1.5);
        assert_eq!(position_between(Some(1.5), Some(2.0)), 1.75);
    }

    #[test]
    fn position_between_handles_the_ends_of_the_list() {
        assert_eq!(position_between(None, None), 1.0, "hàng đợi rỗng");
        assert_eq!(position_between(None, Some(3.0)), 2.0, "thả lên đầu");
        assert_eq!(position_between(Some(3.0), None), 4.0, "thả xuống cuối");
    }

    #[test]
    fn needs_renormalize_only_when_the_gap_has_collapsed() {
        assert!(!needs_renormalize(Some(1.0), Some(2.0)));
        assert!(needs_renormalize(Some(1.0), Some(1.0 + 1e-9)));
        assert!(
            !needs_renormalize(None, Some(1.0)),
            "ở đầu hoặc cuối danh sách thì luôn còn chỗ"
        );
    }

    #[test]
    fn move_job_between_only_rewrites_the_moved_row() {
        let db = temp_db();
        // Cố ý KHÔNG dùng 1.0/2.0/3.0: đó đúng là kết quả của một lần chuẩn
        // hoá, nên với bộ số ấy "chỉ ghi một dòng" và "ghi lại cả danh sách"
        // cho ra y hệt nhau và test không phân biệt được. 10/20/30 thì một lần
        // chuẩn hoá thừa sẽ lộ ra ngay ở vị trí của "a" và "b".
        for (id, position) in [("a", 10.0), ("b", 20.0), ("c", 30.0)] {
            let mut job = sample_job(id);
            job.queue_position = position;
            db.insert_job(&job).unwrap();
        }

        // Kéo "c" vào giữa "a" và "b".
        db.move_job_between("c", Some("a"), Some("b")).unwrap();

        assert_eq!(db.get_job("c").unwrap().unwrap().queue_position, 15.0);
        assert_eq!(
            db.get_job("a").unwrap().unwrap().queue_position,
            10.0,
            "hàng xóm không được đụng tới"
        );
        assert_eq!(
            db.get_job("b").unwrap().unwrap().queue_position,
            20.0,
            "hàng xóm không được đụng tới"
        );
    }

    #[test]
    fn move_job_between_renormalizes_when_the_gap_collapses() {
        let db = temp_db();
        // Hai hàng xóm sát nhau tới mức không còn chỗ chèn vào giữa.
        for (id, position) in [("a", 1.0), ("b", 1.0 + 1e-12), ("c", 9.0)] {
            let mut job = sample_job(id);
            job.queue_position = position;
            db.insert_job(&job).unwrap();
        }

        db.move_job_between("c", Some("a"), Some("b")).unwrap();

        let a = db.get_job("a").unwrap().unwrap().queue_position;
        let b = db.get_job("b").unwrap().unwrap().queue_position;
        let c = db.get_job("c").unwrap().unwrap().queue_position;
        // Hai khẳng định dưới bắt hai lỗi khác nhau và cần giữ cả hai: bỏ hẳn
        // nhánh chuẩn hoá thì khe hở vẫn hẹp (assert thứ hai bắt), còn đảo
        // `ORDER BY` của lần chuẩn hoá thành DESC thì khe hở rộng nhưng thứ tự
        // sai (assert thứ nhất bắt).
        assert!(a < c && c < b, "thứ tự a < c < b phải đúng sau khi chuẩn hoá");
        assert!(b - a > 0.1, "sau chuẩn hoá khe hở phải rộng trở lại");
        // Giá trị cụ thể: chuẩn hoá đưa a,b,c về 1.0/2.0/3.0 rồi "c" nhận điểm
        // giữa của a và b. Kiểm tra thẳng con số để một lần chuẩn hoá giữ đúng
        // thứ tự nhưng để lại khe hở nham nhở không lọt qua được.
        assert_eq!((a, b, c), (1.0, 2.0, 1.5));
    }

    #[test]
    fn move_job_between_rejects_a_neighbour_that_no_longer_exists() {
        // Hàng xóm có thể vừa tải xong và rơi khỏi danh sách trong lúc người
        // dùng đang kéo. Coi id không tìm thấy là "đầu danh sách" sẽ đặt job
        // vào một chỗ hoàn toàn khác chỗ người dùng thả — im lặng và sai.
        let db = temp_db();
        for (id, position) in [("a", 10.0), ("b", 20.0), ("c", 30.0)] {
            let mut job = sample_job(id);
            job.queue_position = position;
            db.insert_job(&job).unwrap();
        }

        let result = db.move_job_between("c", Some("ghost"), Some("b"));

        assert!(result.is_err(), "hàng xóm không tồn tại phải là lỗi");
        assert_eq!(
            db.get_job("c").unwrap().unwrap().queue_position,
            30.0,
            "thất bại thì không được đổi vị trí của job"
        );
    }

    #[test]
    fn move_job_between_rejects_a_job_that_no_longer_exists() {
        let db = temp_db();
        let mut anchor = sample_job("a");
        anchor.queue_position = 10.0;
        db.insert_job(&anchor).unwrap();

        let result = db.move_job_between("ghost", Some("a"), None);

        assert!(result.is_err(), "job không tồn tại phải là lỗi, không phải Ok");
        assert_eq!(db.get_job("a").unwrap().unwrap().queue_position, 10.0);
    }

    #[test]
    fn move_job_between_rejects_using_the_moved_job_as_its_own_neighbour() {
        let db = temp_db();
        for (id, position) in [("a", 10.0), ("b", 20.0)] {
            let mut job = sample_job(id);
            job.queue_position = position;
            db.insert_job(&job).unwrap();
        }

        assert!(db.move_job_between("a", Some("a"), Some("b")).is_err());
        assert!(db.move_job_between("a", Some("b"), Some("a")).is_err());
        assert_eq!(
            db.get_job("a").unwrap().unwrap().queue_position,
            10.0,
            "thất bại thì không được đổi vị trí của job"
        );
    }

    #[test]
    fn reset_interrupted_jobs_pauses_downloading_and_fetching() {
        let db = temp_db();
        let mut downloading = sample_job("downloading");
        downloading.status = JobStatus::Downloading;
        let mut fetching = sample_job("fetching");
        fetching.status = JobStatus::FetchingMetadata;
        let mut completed = sample_job("completed");
        completed.status = JobStatus::Completed;
        db.insert_job(&downloading).unwrap();
        db.insert_job(&fetching).unwrap();
        db.insert_job(&completed).unwrap();

        let count = db.reset_interrupted_jobs().unwrap();

        assert_eq!(count, 2);
        assert_eq!(
            db.get_job("downloading").unwrap().unwrap().status,
            JobStatus::Paused
        );
        assert_eq!(
            db.get_job("fetching").unwrap().unwrap().status,
            JobStatus::Paused
        );
        assert_eq!(
            db.get_job("completed").unwrap().unwrap().status,
            JobStatus::Completed,
            "job đã xong không được đụng tới"
        );
    }

    #[test]
    fn mark_job_for_retry_requeues_with_a_future_deadline() {
        let db = temp_db();
        let mut running = sample_job("job-1");
        running.status = JobStatus::Downloading;
        db.insert_job(&running).unwrap();

        db.mark_job_for_retry("job-1", "2026-07-26T00:00:30Z", "network timeout")
            .unwrap();

        let loaded = db.get_job("job-1").unwrap().unwrap();
        assert_eq!(loaded.status, JobStatus::Queued);
        assert_eq!(loaded.retry_count, 1);
        assert_eq!(loaded.next_retry_at.as_deref(), Some("2026-07-26T00:00:30Z"));
        assert_eq!(loaded.error_message.as_deref(), Some("network timeout"));
    }

    #[test]
    fn clear_retry_deadline_gives_back_the_full_retry_budget() {
        // Người dùng tạm dừng một job đang đếm ngược để thử lại rồi tiếp tục
        // nó. Nếu chỉ xoá `next_retry_at` mà giữ nguyên `retry_count = 3`, lần
        // thất bại mạng kế tiếp sẽ bị coi là "đã hết lượt" và job chết ngay,
        // dù người dùng chưa từng thấy nó thử lại lần nào.
        let db = temp_db();
        let mut waiting = sample_job("job-1");
        waiting.retry_count = 3;
        waiting.next_retry_at = Some("2026-07-26T00:10:00Z".to_string());
        db.insert_job(&waiting).unwrap();

        db.clear_retry_deadline("job-1").unwrap();

        let loaded = db.get_job("job-1").unwrap().unwrap();
        assert_eq!(loaded.next_retry_at, None);
        assert_eq!(loaded.retry_count, 0);
        assert_eq!(
            loaded.status,
            JobStatus::Queued,
            "xoá mốc thử lại không được tự đổi trạng thái — phía gọi mới quyết định trạng thái cuối"
        );
    }

    #[test]
    fn bulk_update_status_returns_the_ids_it_changed() {
        let db = temp_db();
        let mut queued = sample_job("queued");
        queued.status = JobStatus::Queued;
        let mut done = sample_job("done");
        done.status = JobStatus::Completed;
        db.insert_job(&queued).unwrap();
        db.insert_job(&done).unwrap();

        let changed = db
            .bulk_update_status(&[JobStatus::Queued], JobStatus::Paused)
            .unwrap();

        assert_eq!(changed, vec!["queued".to_string()]);
        assert_eq!(
            db.get_job("queued").unwrap().unwrap().status,
            JobStatus::Paused
        );
    }

    #[test]
    fn new_settings_have_sensible_defaults() {
        let db = temp_db();
        let settings = db.get_settings().unwrap();

        assert_eq!(settings.max_concurrent_downloads, 3, "giữ nguyên hành vi cũ");
        assert_eq!(settings.rate_limit_kbps, 0, "0 nghĩa là không giới hạn");
        assert_eq!(settings.max_retry_attempts, 3);
        assert!(!settings.run_in_background, "chạy nền phải mặc định tắt");
    }

    #[test]
    fn settings_round_trip_through_the_database() {
        let db = temp_db();
        let mut settings = db.get_settings().unwrap();
        settings.max_concurrent_downloads = 6;
        settings.rate_limit_kbps = 2048;
        settings.max_retry_attempts = 0;
        settings.run_in_background = true;
        db.update_settings(&settings).unwrap();

        let reloaded = db.get_settings().unwrap();
        assert_eq!(reloaded.max_concurrent_downloads, 6);
        assert_eq!(reloaded.rate_limit_kbps, 2048);
        assert_eq!(reloaded.max_retry_attempts, 0);
        assert!(reloaded.run_in_background);
    }

    /// Giá trị rác trong DB (sửa tay, hoặc một lần ghi hỏng) phải rơi về mặc
    /// định chứ không được làm hỏng cả màn hình cài đặt.
    #[test]
    fn garbage_numeric_settings_fall_back_to_defaults() {
        let db = temp_db();
        {
            let conn = db.conn();
            Db::set_setting(&conn, "max_concurrent_downloads", "not-a-number").unwrap();
            Db::set_setting(&conn, "rate_limit_kbps", "-1").unwrap();
            Db::set_setting(&conn, "max_retry_attempts", "").unwrap();
        }

        let settings = db.get_settings().unwrap();

        assert_eq!(settings.max_concurrent_downloads, 3);
        assert_eq!(settings.rate_limit_kbps, 0);
        assert_eq!(settings.max_retry_attempts, 3);
    }

    // ---- presets (FR-228 → FR-233) -------------------------------------

    /// Một bộ tuỳ chọn khác mặc định ở ít nhất hai chỗ, dựng bằng struct-update
    /// từ `default()` để không vỡ khi phase này thêm tuỳ chọn mới vào
    /// `OutputOptions`.
    fn sample_options() -> OutputOptions {
        OutputOptions {
            audio: AudioOutput::Opus {
                bitrate_kbps: Some(192),
            },
            video_container: VideoContainer::Mkv,
            codec_preference: CodecPreference::Quality,
            embed_metadata: !OutputOptions::default().embed_metadata,
            ..OutputOptions::default()
        }
    }

    #[test]
    fn migration_0011_is_registered_and_is_what_creates_the_presets_table() {
        // Đăng ký migration là bước âm thầm nhất trong cả tính năng: quên dòng
        // `M::up` thì file .sql vẫn nằm đó, `cargo build` vẫn xanh, và lỗi chỉ
        // lộ ra ở lần chạy thật đầu tiên dưới dạng "no such table: presets".
        //
        // Phải kiểm bằng `raw_conn_at_version(10)` chứ không phải `temp_db()`:
        // chỉ ở phiên bản 10 mới khẳng định được rằng bảng CHƯA có, tức chính
        // 0011 là thứ tạo ra nó chứ không phải một migration nào khác.
        let mut conn = raw_conn_at_version(10);
        assert_eq!(
            table_count(&conn, "presets"),
            0,
            "presets không được tồn tại trước 0011 — nếu có, test này không kiểm chứng gì cả"
        );

        migrations().to_latest(&mut conn).expect("0011 applies");

        assert_eq!(
            table_count(&conn, "presets"),
            1,
            "0011 phải được đăng ký trong `migrations()`, không chỉ nằm trong thư mục"
        );
    }

    fn table_count(conn: &Connection, name: &str) -> i64 {
        conn.query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
            params![name],
            |row| row.get(0),
        )
        .unwrap()
    }

    #[test]
    fn a_preset_survives_a_round_trip_through_the_database() {
        // FR-228: cấu hình đầu ra hiện tại lưu thành preset có tên và tồn tại
        // qua các lần khởi động. Mọi lựa chọn phải quay về nguyên vẹn — kể cả
        // bitrate nằm bên trong biến thể enum, chỗ mà một lược đồ tuần tự hoá
        // sai sẽ đánh rơi âm thầm.
        let db = temp_db();
        let created = db.create_preset("  Nhạc chất lượng cao  ", &sample_options()).unwrap();

        let listed = db.list_presets().unwrap();

        assert_eq!(listed, vec![created.clone()]);
        assert_eq!(listed[0].output_options, sample_options());
        assert_eq!(
            listed[0].name, "Nhạc chất lượng cao",
            "khoảng trắng thừa bị cắt trước khi lưu, nếu không chỉ mục UNIQUE trên tên chẳng ngăn được gì"
        );
        assert!(
            !listed[0].is_default,
            "lưu một preset không phải là yêu cầu áp nó cho mọi liên kết mới"
        );
    }

    #[test]
    fn only_one_preset_stays_the_default_after_a_second_one_is_set() {
        // FR-230. Chế độ hỏng cần chặn là một lần đặt mặc định mới mà cờ cũ
        // còn nguyên: từ đó "preset mặc định" trở thành thứ phụ thuộc vào thứ
        // tự dòng trả về.
        let db = temp_db();
        let first = db.create_preset("Đầu tiên", &OutputOptions::default()).unwrap();
        let second = db.create_preset("Thứ hai", &sample_options()).unwrap();

        db.set_default_preset(&first.id).unwrap();
        let promoted = db.set_default_preset(&second.id).unwrap();

        assert!(promoted.is_default, "bản ghi trả về phải phản ánh trạng thái vừa ghi");
        let defaults: Vec<String> = db
            .list_presets()
            .unwrap()
            .into_iter()
            .filter(|preset| preset.is_default)
            .map(|preset| preset.id)
            .collect();
        assert_eq!(
            defaults,
            vec![second.id],
            "chỉ preset được đặt sau cùng còn giữ cờ mặc định"
        );
    }

    #[test]
    fn the_database_itself_refuses_two_defaults() {
        // Cùng bất biến với test trên, nhưng ở tầng khác: kiểm rằng nó được
        // CSDL bảo đảm chứ không phải do `set_default_preset` cư xử tử tế. Câu
        // UPDATE thô dưới đây là đúng cái mà một chỗ gọi cẩu thả (hoặc một
        // migration tương lai) sẽ viết.
        let db = temp_db();
        db.create_preset("Đầu tiên", &OutputOptions::default()).unwrap();
        db.create_preset("Thứ hai", &OutputOptions::default()).unwrap();

        let forced = db
            .conn()
            .execute("UPDATE presets SET is_default = 1", []);

        assert!(
            forced.is_err(),
            "chỉ mục một phần phải làm 'hai preset cùng mặc định' trở thành trạng thái không ghi được"
        );
    }

    #[test]
    fn setting_a_preset_that_no_longer_exists_keeps_the_current_default() {
        // Nửa còn lại của bất biến "đúng một mặc định": hỏng giữa chừng không
        // được để lại KHÔNG cái nào. Bước xoá cờ cũ chạy trước bước bật cờ
        // mới, nên nếu không có giao dịch cuộn ngược, một id sai sẽ xoá sạch
        // mặc định rồi báo lỗi.
        let db = temp_db();
        let kept = db.create_preset("Đang là mặc định", &OutputOptions::default()).unwrap();
        db.set_default_preset(&kept.id).unwrap();

        let failed = db.set_default_preset("không-tồn-tại");

        assert!(failed.is_err());
        let defaults: Vec<String> = db
            .list_presets()
            .unwrap()
            .into_iter()
            .filter(|preset| preset.is_default)
            .map(|preset| preset.id)
            .collect();
        assert_eq!(defaults, vec![kept.id], "mặc định cũ phải còn nguyên");
    }

    #[test]
    fn deleting_the_default_preset_leaves_no_default_and_touches_nothing_else() {
        // Trạng thái sau khi xoá preset mặc định phải xác định. Lựa chọn ở đây
        // là "không còn preset mặc định" — đúng bằng trạng thái của một cài
        // đặt mới — chứ KHÔNG đôn preset khác lên thay, vì làm vậy sẽ áp một
        // bộ tuỳ chọn người dùng chưa bao giờ chọn cho mọi liên kết kế tiếp.
        let db = temp_db();
        let doomed = db.create_preset("Sắp bị xoá", &OutputOptions::default()).unwrap();
        let survivor = db.create_preset("Còn lại", &sample_options()).unwrap();
        db.set_default_preset(&doomed.id).unwrap();

        db.delete_preset(&doomed.id).unwrap();

        let remaining = db.list_presets().unwrap();
        assert_eq!(remaining, vec![survivor], "preset còn lại không bị đụng tới");
        assert!(
            remaining.iter().all(|preset| !preset.is_default),
            "không preset nào được tự nhận cờ mặc định thay cho cái vừa bị xoá"
        );
    }

    #[test]
    fn deleting_a_preset_that_no_longer_exists_is_an_error_not_a_silent_success() {
        let db = temp_db();
        let error = db.delete_preset("không-tồn-tại").unwrap_err();
        assert_eq!(error.code, "NOT_FOUND");
    }

    #[test]
    fn two_presets_cannot_share_a_name() {
        // Quyết định: tên là duy nhất. Hai mục trùng tên trong danh sách chọn
        // thì người dùng không còn cách nào nhắm đúng mục để sửa hay xoá.
        let db = temp_db();
        db.create_preset("Podcast", &OutputOptions::default()).unwrap();

        let clash = db.create_preset(" Podcast ", &sample_options()).unwrap_err();

        assert_eq!(
            clash.code, "PRESET_NAME_TAKEN",
            "phải là mã lỗi giao diện dịch được, không phải INTERNAL"
        );
        assert_eq!(db.list_presets().unwrap().len(), 1);
    }

    #[test]
    fn renaming_onto_an_existing_name_is_rejected_and_changes_nothing() {
        let db = temp_db();
        db.create_preset("Podcast", &OutputOptions::default()).unwrap();
        let other = db.create_preset("Nhạc", &sample_options()).unwrap();

        let clash = db.rename_preset(&other.id, "Podcast").unwrap_err();

        assert_eq!(clash.code, "PRESET_NAME_TAKEN");
        assert_eq!(
            db.list_presets()
                .unwrap()
                .into_iter()
                .map(|preset| preset.name)
                .collect::<Vec<_>>(),
            vec!["Nhạc".to_string(), "Podcast".to_string()]
        );
    }

    #[test]
    fn an_empty_name_is_rejected_at_creation_and_at_rename() {
        let db = temp_db();
        let created = db.create_preset("Có tên", &OutputOptions::default()).unwrap();

        assert_eq!(
            db.create_preset("   ", &OutputOptions::default()).unwrap_err().code,
            "PRESET_NAME_REQUIRED"
        );
        assert_eq!(
            db.rename_preset(&created.id, "\t\n").unwrap_err().code,
            "PRESET_NAME_REQUIRED"
        );
    }

    #[test]
    fn renaming_and_updating_keep_the_other_half_of_the_preset_intact() {
        // FR-229: sửa và đổi tên là hai thao tác riêng. Đổi tên không được đụng
        // tới tuỳ chọn, và ghi đè tuỳ chọn không được đụng tới tên hay cờ mặc
        // định.
        let db = temp_db();
        let created = db.create_preset("Tên cũ", &sample_options()).unwrap();
        db.set_default_preset(&created.id).unwrap();

        let renamed = db.rename_preset(&created.id, "Tên mới").unwrap();
        assert_eq!(renamed.name, "Tên mới");
        assert_eq!(renamed.output_options, sample_options());
        assert!(renamed.is_default);

        let updated = db
            .update_preset_options(&created.id, &OutputOptions::default())
            .unwrap();
        assert_eq!(updated.output_options, OutputOptions::default());
        assert_eq!(updated.name, "Tên mới");
        assert!(updated.is_default);
    }

    #[test]
    fn a_preset_stored_by_an_older_version_loads_with_the_missing_option_defaulted() {
        // FR-233. Blob được dựng bằng cách BỚT một khoá khỏi dạng tuần tự hoá
        // hôm nay, chứ không gõ tay một chuỗi JSON: nhờ vậy test vẫn đúng khi
        // phase này thêm tiếp tuỳ chọn, và không khoá tên trường nào vào đây.
        let db = temp_db();
        let defaults = serde_json::to_value(OutputOptions::default()).unwrap();
        let mut older = serde_json::to_value(sample_options()).unwrap();

        // Bỏ hẳn một khoá mà giá trị đã lưu KHÁC mặc định — đóng vai "tuỳ chọn
        // được thêm vào sau khi preset này được lưu".
        let dropped = older
            .as_object()
            .unwrap()
            .iter()
            .find(|(key, value)| defaults.get(key.as_str()) != Some(*value))
            .map(|(key, _)| key.clone())
            .expect("bộ tuỳ chọn mẫu phải khác mặc định ở ít nhất hai chỗ");
        older.as_object_mut().unwrap().remove(&dropped);

        db.conn()
            .execute(
                "INSERT INTO presets (id, name, output_options, is_default, created_at, updated_at)
                 VALUES ('p-old', 'Từ bản cũ', ?1, 0, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                params![serde_json::to_string(&older).unwrap()],
            )
            .unwrap();

        let loaded = db.list_presets().unwrap().remove(0);
        let loaded_value = serde_json::to_value(&loaded.output_options).unwrap();

        assert_eq!(
            loaded_value.get(&dropped),
            defaults.get(&dropped),
            "tuỳ chọn vắng mặt phải nhận giá trị mặc định, không làm hỏng cả bản ghi"
        );
        for (key, value) in older.as_object().unwrap() {
            assert_eq!(
                loaded_value.get(key),
                Some(value),
                "tuỳ chọn mà bản cũ CÓ ghi phải về nguyên vẹn: {key}"
            );
        }
        // Chốt chặn để test không rỗng nghĩa: một phép đọc kiểu "cứ trả
        // `OutputOptions::default()`" vẫn qua được hai khẳng định trên nếu blob
        // chỉ khác mặc định ở đúng khoá vừa bị bỏ.
        assert_ne!(loaded.output_options, OutputOptions::default());
    }

    #[test]
    fn a_corrupted_preset_blob_does_not_take_the_whole_list_down() {
        // Cột JSON không có kiểm tra ở tầng SQL. Một blob hỏng (sửa tay CSDL)
        // phải rơi về bộ mặc định, chứ không làm `list_presets` trả lỗi và
        // khiến MỌI preset biến mất khỏi giao diện.
        let db = temp_db();
        db.create_preset("Lành lặn", &sample_options()).unwrap();
        let broken = db.create_preset("Hỏng", &sample_options()).unwrap();
        db.conn()
            .execute(
                "UPDATE presets SET output_options = 'not json at all' WHERE id = ?1",
                params![broken.id],
            )
            .unwrap();

        let listed = db.list_presets().unwrap();

        assert_eq!(listed.len(), 2);
        let broken_row = listed.iter().find(|preset| preset.id == broken.id).unwrap();
        assert_eq!(broken_row.output_options, OutputOptions::default());
    }

    // ---- Thư viện (specs/004-library) ---------------------------------

    /// Chèn một dòng vào `downloaded_files` bằng SQL THÔ ở lược đồ *trước*
    /// 0012, tức đúng sáu cột mà bảng có từ 0001. Không dùng
    /// `insert_downloaded_file`: nó ghi cả tám cột mới, nên sẽ chẳng còn dòng
    /// nào "có trước migration" để backfill phải xử lý.
    fn insert_legacy_file_row(conn: &Connection, id: &str, job_id: &str, file_path: &str) {
        conn.execute(
            "INSERT INTO downloaded_files (id, job_id, file_path, file_format, file_size_bytes, completed_at)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                id,
                job_id,
                file_path,
                Path::new(file_path)
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .unwrap_or(""),
                4_221_741i64,
                "2026-07-25T13:15:29Z",
            ],
        )
        .expect("raw insert works against the pre-0012 schema");
    }

    /// Chèn một tác vụ hoàn tất bằng SQL thô ở lược đồ trước 0012.
    fn insert_legacy_completed_job(
        conn: &Connection,
        id: &str,
        title: Option<&str>,
        media_type: &str,
        platform: &str,
        output_file_path: Option<&str>,
    ) {
        conn.execute(
            "INSERT INTO download_jobs (
                id, source_url, platform, media_type, status, progress_percent,
                output_directory, output_file_path, title, created_at, updated_at
             ) VALUES (?1,?2,?3,?4,'completed',100.0,'/tmp',?5,?6,
                       '2026-07-25T13:00:00Z','2026-07-25T13:15:29Z')",
            params![
                id,
                format!("https://example.com/{id}"),
                platform,
                media_type,
                output_file_path,
                title,
            ],
        )
        .expect("raw insert works against the pre-0012 schema");
    }

    fn column_names(conn: &Connection, table: &str) -> Vec<String> {
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info({table})"))
            .unwrap();
        stmt.query_map([], |row| row.get::<_, String>("name"))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    }

    #[test]
    fn migration_0012_is_registered_and_is_what_adds_the_library_columns() {
        // Đăng ký migration là bước âm thầm nhất trong cả tính năng: quên dòng
        // `M::up` thì file .sql vẫn nằm nguyên trong thư mục, `cargo build`
        // vẫn xanh, và lỗi chỉ lộ ra ở lần chạy thật đầu tiên dưới dạng
        // "no such column: title".
        //
        // Phải kiểm bằng `raw_conn_at_version(11)` chứ không phải `temp_db()`:
        // chỉ ở phiên bản 11 mới khẳng định được các cột CHƯA có, tức chính
        // 0012 là thứ tạo ra chúng chứ không phải một migration nào khác.
        let (mut conn, _path) = raw_conn_at_version_with_path(11);
        let before = column_names(&conn, "downloaded_files");
        for column in ["title", "media_type", "platform", "source_url"] {
            assert!(
                !before.contains(&column.to_string()),
                "cột {column} không được tồn tại trước 0012 — nếu có, test này không kiểm chứng gì cả"
            );
        }

        migrations().to_latest(&mut conn).expect("0012 applies");

        let after = column_names(&conn, "downloaded_files");
        for column in [
            "title",
            "media_type",
            "platform",
            "source_url",
            "duration_seconds",
            "thumbnail_path",
            "is_missing",
            "search_text",
        ] {
            assert!(
                after.contains(&column.to_string()),
                "0012 phải được đăng ký trong `migrations()`, không chỉ nằm trong thư mục — thiếu cột {column}"
            );
        }
    }

    #[test]
    fn backfill_indexes_a_download_made_before_the_library_existed() {
        // FR-303 + SC-303. Dòng được dựng ở lược đồ v11 bằng SQL thô, nên mọi
        // giá trị mà test khẳng định bên dưới (tiêu đề, nền tảng, URL, loại
        // nội dung) KHÔNG hề được ghi vào `downloaded_files` — chúng chỉ tồn
        // tại trên `download_jobs`, và việc chúng xuất hiện trong thư viện là
        // toàn bộ công việc của backfill.
        let (conn, path) = raw_conn_at_version_with_path(11);
        insert_legacy_completed_job(
            &conn,
            "job-legacy",
            Some("Đừng Về Trễ Nha"),
            "audio",
            "soundcloud",
            Some("/tmp/dung-ve-tre-nha.mp3"),
        );
        insert_legacy_file_row(&conn, "file-legacy", "job-legacy", "/tmp/dung-ve-tre-nha.mp3");
        drop(conn);

        let db = Db::open(&path).expect("migration + backfill run on open");
        let items = db.list_library(&LibraryQuery::default()).unwrap();

        assert_eq!(items.len(), 1);
        let item = &items[0];
        assert_eq!(item.title, "Đừng Về Trễ Nha");
        assert_eq!(item.media_type, MediaType::Audio);
        assert_eq!(item.platform, "soundcloud");
        assert_eq!(item.source_url, "https://example.com/job-legacy");
        assert_eq!(item.job_id, "job-legacy");
        assert_eq!(item.file_path, "/tmp/dung-ve-tre-nha.mp3");
        assert_eq!(item.file_format, "mp3");
        assert_eq!(item.file_size_bytes, 4_221_741);
        // Thời lượng và ảnh đại diện KHÔNG bịa ra được từ lịch sử: cả hai đòi
        // mở chính file đó, việc mà FR-327 cấm làm lúc khởi động.
        assert_eq!(item.duration_seconds, None);
        assert_eq!(item.thumbnail_path, None);
    }

    #[test]
    fn backfill_falls_back_to_the_filename_when_the_job_never_had_a_title() {
        // 81 trong 105 tác vụ hoàn tất của người dùng không có `title` (cột ấy
        // chỉ tồn tại từ 0007). Bỏ trống ở đó sẽ cho ra một lưới đầy ô không
        // tên — mà chính yt-dlp đã đặt tên file từ tiêu đề thật của nguồn.
        let (conn, path) = raw_conn_at_version_with_path(11);
        insert_legacy_completed_job(&conn, "job-untitled", None, "video", "tiktok", Some("/tmp/a.mp4"));
        insert_legacy_file_row(
            &conn,
            "file-untitled",
            "job-untitled",
            "/tmp/BƯỚC QUA MÙA CÔ ĐƠN ⧸ Vũ. (Official MV).mp4",
        );
        drop(conn);

        let db = Db::open(&path).expect("migration + backfill run on open");
        let items = db.list_library(&LibraryQuery::default()).unwrap();

        let backfilled = items
            .iter()
            .find(|item| item.job_id == "job-untitled" && item.file_path.contains("BƯỚC QUA"))
            .expect("dòng cũ vẫn phải có mặt trong thư viện");
        assert_eq!(backfilled.title, "BƯỚC QUA MÙA CÔ ĐƠN ⧸ Vũ. (Official MV)");
    }

    #[test]
    fn backfill_keeps_going_past_a_file_that_no_longer_exists() {
        // `output_file_path` trỏ vào một file đã bị xoá từ lâu là chuyện bình
        // thường sau ba tháng. Nó KHÔNG được làm hỏng cả lượt nạp — và mục
        // vẫn phải vào thư viện (đánh dấu thiếu là việc của vòng đối soát),
        // vì gỡ nó đi sẽ tước mất của người dùng chính nút "tải lại" của
        // FR-326.
        let (conn, path) = raw_conn_at_version_with_path(11);
        let gone = "/tmp/khong-bao-gio-ton-tai-c0ffee/gone.mp3";
        assert!(!Path::new(gone).exists(), "test phải thật sự trỏ vào một file không có");
        insert_legacy_completed_job(&conn, "job-gone", Some("Đã mất"), "audio", "youtube", Some(gone));
        insert_legacy_file_row(&conn, "file-gone", "job-gone", gone);
        insert_legacy_completed_job(
            &conn,
            "job-here",
            Some("Còn đây"),
            "audio",
            "youtube",
            Some("/tmp/here.mp3"),
        );
        insert_legacy_file_row(&conn, "file-here", "job-here", "/tmp/here.mp3");
        drop(conn);

        let db = Db::open(&path).expect("một file đã biến mất không được làm hỏng backfill");
        let items = db.list_library(&LibraryQuery::default()).unwrap();

        assert_eq!(items.len(), 2, "cả hai mục phải được nạp, kể cả mục trỏ vào file đã mất");
        let missing_one = items.iter().find(|item| item.file_path == gone).unwrap();
        assert_eq!(missing_one.title, "Đã mất");
    }

    #[test]
    fn backfill_rebuilds_an_index_row_for_a_completed_job_that_never_had_one() {
        // `downloaded_files` chỉ được ghi từ khi tính năng ghi nó tồn tại;
        // `output_file_path` thì đi cùng tác vụ từ migration đầu tiên. Một
        // tác vụ hoàn tất mà không có dòng nào trong chỉ mục vẫn phải xuất
        // hiện trong thư viện (SC-303: 100%).
        let (conn, path) = raw_conn_at_version_with_path(11);
        insert_legacy_completed_job(
            &conn,
            "job-no-row",
            Some("Chỉ có trong lịch sử"),
            "video",
            "bilibili",
            Some("/tmp/only-in-history.mp4"),
        );
        drop(conn);

        let db = Db::open(&path).expect("migration + backfill run on open");
        let items = db.list_library(&LibraryQuery::default()).unwrap();

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Chỉ có trong lịch sử");
        assert_eq!(items[0].job_id, "job-no-row");
        assert_eq!(items[0].file_format, "mp4");
        assert_eq!(items[0].media_type, MediaType::Video);
    }

    #[test]
    fn backfill_is_idempotent_across_restarts() {
        // Backfill chạy ở MỌI lần `Db::open`. Lần thứ hai không được nhân đôi
        // gì cả, nếu không thì mỗi lần khởi động lại sẽ làm thư viện phình ra.
        let (conn, path) = raw_conn_at_version_with_path(11);
        insert_legacy_completed_job(&conn, "job-x", Some("X"), "audio", "youtube", Some("/tmp/x.mp3"));
        insert_legacy_file_row(&conn, "file-x", "job-x", "/tmp/x.mp3");
        drop(conn);

        let first = Db::open(&path).unwrap();
        let after_first = first.list_library(&LibraryQuery::default()).unwrap();
        drop(first);
        let second = Db::open(&path).unwrap();
        let after_second = second.list_library(&LibraryQuery::default()).unwrap();

        assert_eq!(after_first.len(), 1);
        assert_eq!(after_second, after_first);
    }

    #[test]
    fn migration_0012_merges_rows_that_point_at_the_same_file() {
        // Edge case của spec: tải lại cùng một link ghi đè đúng file cũ trên
        // đĩa nhưng lại thêm một dòng mới mỗi lần. CSDL thật của người dùng có
        // 105 dòng cho 66 file. Ba ô giống hệt nhau trong lưới, cùng trỏ vào
        // một file duy nhất, là một lỗi hiển thị mà không bộ lọc nào chữa
        // được.
        let (mut conn, _path) = raw_conn_at_version_with_path(11);
        insert_legacy_completed_job(&conn, "job-1", Some("Bản cũ"), "audio", "youtube", Some("/tmp/same.mp3"));
        insert_legacy_completed_job(&conn, "job-2", Some("Bản mới"), "audio", "youtube", Some("/tmp/same.mp3"));
        conn.execute(
            "INSERT INTO downloaded_files (id, job_id, file_path, file_format, file_size_bytes, completed_at)
             VALUES ('old','job-1','/tmp/same.mp3','mp3',100,'2026-07-01T00:00:00Z')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO downloaded_files (id, job_id, file_path, file_format, file_size_bytes, completed_at)
             VALUES ('new','job-2','/tmp/same.mp3','mp3',200,'2026-07-20T00:00:00Z')",
            [],
        )
        .unwrap();

        migrations().to_latest(&mut conn).expect("0012 applies");

        let survivors: Vec<String> = conn
            .prepare("SELECT id FROM downloaded_files")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(
            survivors,
            vec!["new".to_string()],
            "dòng còn lại phải là dòng mới nhất — nó mô tả nội dung đang thật sự nằm trên đĩa"
        );
    }

    /// Chèn một mục thư viện qua đúng đường mà ứng dụng dùng, rồi ép
    /// `completed_at` về một mốc xác định (hàm ghi luôn dùng `now()`, vốn
    /// không kiểm soát được trong test lọc theo khoảng thời gian).
    #[allow(clippy::too_many_arguments)]
    fn add_item(
        db: &Db,
        job_id: &str,
        file_path: &str,
        title: &str,
        media_type: MediaType,
        platform: &str,
        file_size_bytes: i64,
        duration_seconds: Option<i64>,
        completed_at: &str,
    ) -> String {
        let job = DownloadJob {
            media_type: media_type.clone(),
            platform: platform.to_string(),
            ..sample_job(job_id)
        };
        if db.get_job(job_id).unwrap().is_none() {
            db.insert_job(&job).unwrap();
        }
        db.insert_downloaded_file(&NewLibraryFile {
            job_id: job_id.to_string(),
            file_path: file_path.to_string(),
            file_format: media_file_extension(file_path),
            file_size_bytes,
            title: title.to_string(),
            media_type,
            platform: platform.to_string(),
            source_url: format!("https://example.com/{job_id}"),
            duration_seconds,
            thumbnail_path: None,
        })
        .unwrap();
        db.conn()
            .execute(
                "UPDATE downloaded_files SET completed_at = ?1 WHERE file_path = ?2",
                params![completed_at, file_path],
            )
            .unwrap();
        db.conn()
            .query_row(
                "SELECT id FROM downloaded_files WHERE file_path = ?1",
                params![file_path],
                |row| row.get(0),
            )
            .unwrap()
    }

    /// Một thư viện nhỏ nhưng đủ đa dạng để mọi bộ lọc và mọi tiêu chí sắp
    /// xếp đều có ít nhất một dòng bị loại và một dòng được giữ.
    fn library_fixture(db: &Db) {
        add_item(db, "j1", "/tmp/alpha.mp3", "Alpha Song", MediaType::Audio, "youtube", 300, Some(180), "2026-01-10T00:00:00Z");
        add_item(db, "j2", "/tmp/beta.mp4", "Beta Video", MediaType::Video, "youtube", 100, Some(60), "2026-03-10T00:00:00Z");
        add_item(db, "j3", "/tmp/gamma.mp3", "Gamma Track", MediaType::Audio, "soundcloud", 200, None, "2026-05-10T00:00:00Z");
        add_item(db, "j4", "/tmp/delta.jpg", "Delta Post", MediaType::Gallery, "tiktok", 50, None, "2026-07-10T00:00:00Z");
    }

    fn titles(items: &[LibraryItem]) -> Vec<&str> {
        items.iter().map(|item| item.title.as_str()).collect()
    }

    #[test]
    fn search_matches_the_title() {
        let db = temp_db();
        library_fixture(&db);
        let found = db
            .list_library(&LibraryQuery {
                search: Some("gamma tr".to_string()),
                ..LibraryQuery::default()
            })
            .unwrap();
        assert_eq!(titles(&found), vec!["Gamma Track"]);
    }

    #[test]
    fn search_matches_the_filename_too() {
        // FR-307 nói rõ "tiêu đề VÀ tên file" — người dùng nhớ tên file mà
        // không nhớ tiêu đề là một nửa số lần tìm.
        let db = temp_db();
        library_fixture(&db);
        let found = db
            .list_library(&LibraryQuery {
                search: Some("delta.jpg".to_string()),
                ..LibraryQuery::default()
            })
            .unwrap();
        assert_eq!(titles(&found), vec!["Delta Post"]);
    }

    #[test]
    fn search_ignores_case_even_for_vietnamese_titles() {
        // `LIKE` của SQLite chỉ không phân biệt hoa thường với A-Z, và
        // `lower()` dựng sẵn của nó cũng vậy: `'ĐỪNG' LIKE '%đừng%'` là FALSE.
        // Với một thư viện mà phần lớn tiêu đề là tiếng Việt, đó là khác biệt
        // giữa "tìm được" và "không bao giờ tìm được". Đây là lý do tồn tại
        // của cột `search_text` (hạ hoa bằng Rust) — bỏ nó đi và test này đỏ.
        let db = temp_db();
        add_item(db_ref(&db), "j-vi", "/tmp/vi.mp3", "ĐỪNG VỀ TRỄ NHA", MediaType::Audio, "soundcloud", 10, None, "2026-07-01T00:00:00Z");

        let found = db
            .list_library(&LibraryQuery {
                search: Some("đừng về".to_string()),
                ..LibraryQuery::default()
            })
            .unwrap();
        assert_eq!(titles(&found), vec!["ĐỪNG VỀ TRỄ NHA"]);

        let found_upper = db
            .list_library(&LibraryQuery {
                search: Some("TRỄ".to_string()),
                ..LibraryQuery::default()
            })
            .unwrap();
        assert_eq!(titles(&found_upper), vec!["ĐỪNG VỀ TRỄ NHA"]);
    }

    fn db_ref(db: &Db) -> &Db {
        db
    }

    #[test]
    fn a_percent_sign_in_the_search_term_is_a_literal_not_a_wildcard() {
        let db = temp_db();
        library_fixture(&db);
        add_item(db_ref(&db), "j5", "/tmp/hundred.mp3", "100% Music", MediaType::Audio, "youtube", 10, None, "2026-06-01T00:00:00Z");

        let found = db
            .list_library(&LibraryQuery {
                search: Some("100%".to_string()),
                ..LibraryQuery::default()
            })
            .unwrap();

        assert_eq!(
            titles(&found),
            vec!["100% Music"],
            "`%` phải là ký tự thường; nếu là ký tự đại diện thì mọi mục đều khớp"
        );
    }

    #[test]
    fn filters_by_media_type() {
        let db = temp_db();
        library_fixture(&db);
        let found = db
            .list_library(&LibraryQuery {
                media_types: vec![MediaType::Audio],
                sort: LibrarySort::Title,
                direction: SortDirection::Asc,
                ..LibraryQuery::default()
            })
            .unwrap();
        assert_eq!(titles(&found), vec!["Alpha Song", "Gamma Track"]);
    }

    #[test]
    fn filters_by_platform() {
        let db = temp_db();
        library_fixture(&db);
        let found = db
            .list_library(&LibraryQuery {
                platforms: vec!["soundcloud".to_string()],
                ..LibraryQuery::default()
            })
            .unwrap();
        assert_eq!(titles(&found), vec!["Gamma Track"]);
    }

    #[test]
    fn filters_by_format() {
        let db = temp_db();
        library_fixture(&db);
        let found = db
            .list_library(&LibraryQuery {
                formats: vec!["mp4".to_string()],
                ..LibraryQuery::default()
            })
            .unwrap();
        assert_eq!(titles(&found), vec!["Beta Video"]);
    }

    #[test]
    fn filters_by_date_range_inclusive_at_both_ends() {
        let db = temp_db();
        library_fixture(&db);
        let found = db
            .list_library(&LibraryQuery {
                downloaded_from: Some("2026-03-10T00:00:00Z".to_string()),
                downloaded_to: Some("2026-05-10T00:00:00Z".to_string()),
                sort: LibrarySort::DownloadedAt,
                direction: SortDirection::Asc,
                ..LibraryQuery::default()
            })
            .unwrap();
        assert_eq!(titles(&found), vec!["Beta Video", "Gamma Track"]);
    }

    #[test]
    fn several_filters_combine_with_and_not_or() {
        // FR-308. Nếu các bộ lọc gộp bằng HOẶC thì câu truy vấn này sẽ trả về
        // cả bốn mục — nên con số 1 ở đây mới là thứ phân biệt hai hành vi.
        let db = temp_db();
        library_fixture(&db);
        let found = db
            .list_library(&LibraryQuery {
                search: Some("a".to_string()),
                media_types: vec![MediaType::Audio],
                platforms: vec!["youtube".to_string()],
                formats: vec!["mp3".to_string()],
                downloaded_from: Some("2026-01-01T00:00:00Z".to_string()),
                downloaded_to: Some("2026-02-01T00:00:00Z".to_string()),
                ..LibraryQuery::default()
            })
            .unwrap();
        assert_eq!(titles(&found), vec!["Alpha Song"]);
    }

    #[test]
    fn several_values_inside_one_filter_combine_with_or() {
        let db = temp_db();
        library_fixture(&db);
        let found = db
            .list_library(&LibraryQuery {
                platforms: vec!["soundcloud".to_string(), "tiktok".to_string()],
                sort: LibrarySort::Title,
                direction: SortDirection::Asc,
                ..LibraryQuery::default()
            })
            .unwrap();
        assert_eq!(titles(&found), vec!["Delta Post", "Gamma Track"]);
    }

    #[test]
    fn sorts_by_every_criterion_in_both_directions() {
        let db = temp_db();
        library_fixture(&db);
        let sorted = |sort, direction| {
            let items = db
                .list_library(&LibraryQuery {
                    sort,
                    direction,
                    ..LibraryQuery::default()
                })
                .unwrap();
            items
                .iter()
                .map(|item| item.title.clone())
                .collect::<Vec<_>>()
        };

        assert_eq!(
            sorted(LibrarySort::DownloadedAt, SortDirection::Desc),
            vec!["Delta Post", "Gamma Track", "Beta Video", "Alpha Song"]
        );
        assert_eq!(
            sorted(LibrarySort::DownloadedAt, SortDirection::Asc),
            vec!["Alpha Song", "Beta Video", "Gamma Track", "Delta Post"]
        );
        assert_eq!(
            sorted(LibrarySort::Title, SortDirection::Asc),
            vec!["Alpha Song", "Beta Video", "Delta Post", "Gamma Track"]
        );
        assert_eq!(
            sorted(LibrarySort::Title, SortDirection::Desc),
            vec!["Gamma Track", "Delta Post", "Beta Video", "Alpha Song"]
        );
        assert_eq!(
            sorted(LibrarySort::FileSize, SortDirection::Desc),
            vec!["Alpha Song", "Gamma Track", "Beta Video", "Delta Post"]
        );
        assert_eq!(
            sorted(LibrarySort::FileSize, SortDirection::Asc),
            vec!["Delta Post", "Beta Video", "Gamma Track", "Alpha Song"]
        );
    }

    #[test]
    fn items_with_no_known_duration_sort_last_in_both_directions() {
        // "Không biết" không phải là "ngắn nhất". Mọi mục nạp lại từ lịch sử
        // cũ đều mang `NULL`, nên nếu chúng dồn lên đầu ở chiều tăng dần thì
        // toàn bộ 105 mục ngày đầu của người dùng sẽ chặn mất phần thư viện
        // có thời lượng thật.
        let db = temp_db();
        library_fixture(&db);
        let asc = db
            .list_library(&LibraryQuery {
                sort: LibrarySort::Duration,
                direction: SortDirection::Asc,
                ..LibraryQuery::default()
            })
            .unwrap();
        assert_eq!(titles(&asc)[..2], ["Beta Video", "Alpha Song"]);
        assert!(asc[2].duration_seconds.is_none() && asc[3].duration_seconds.is_none());

        let desc = db
            .list_library(&LibraryQuery {
                sort: LibrarySort::Duration,
                direction: SortDirection::Desc,
                ..LibraryQuery::default()
            })
            .unwrap();
        assert_eq!(titles(&desc)[..2], ["Alpha Song", "Beta Video"]);
        assert!(desc[2].duration_seconds.is_none() && desc[3].duration_seconds.is_none());
    }

    #[test]
    fn paging_returns_disjoint_windows() {
        let db = temp_db();
        library_fixture(&db);
        let page = |offset| {
            db.list_library(&LibraryQuery {
                sort: LibrarySort::Title,
                direction: SortDirection::Asc,
                limit: Some(2),
                offset: Some(offset),
                ..LibraryQuery::default()
            })
            .unwrap()
        };
        assert_eq!(titles(&page(0)), vec!["Alpha Song", "Beta Video"]);
        assert_eq!(titles(&page(2)), vec!["Delta Post", "Gamma Track"]);
    }

    #[test]
    fn every_sort_option_is_served_by_an_index() {
        // FR-310 ở quy mô 10.000 mục là một câu hỏi về chỉ mục, không về mã
        // Rust: thiếu chỉ mục thì mỗi lần mở trang SQLite phải đọc toàn bộ
        // bảng vào một b-tree tạm rồi mới trả được 60 dòng đầu. `EXPLAIN QUERY
        // PLAN` là cách duy nhất khẳng định điều đó mà không phải đo thời gian
        // (một phép đo sẽ chập chờn theo máy chạy test).
        let db = temp_db();
        library_fixture(&db);
        let conn = db.conn();
        for sort in [
            LibrarySort::DownloadedAt,
            LibrarySort::Title,
            LibrarySort::FileSize,
            LibrarySort::Duration,
        ] {
            for direction in [SortDirection::Asc, SortDirection::Desc] {
                let sql = format!(
                    "EXPLAIN QUERY PLAN SELECT * FROM downloaded_files ORDER BY {} LIMIT 60",
                    sort.order_by(direction)
                );
                let plan: Vec<String> = conn
                    .prepare(&sql)
                    .unwrap()
                    .query_map([], |row| row.get::<_, String>("detail"))
                    .unwrap()
                    .collect::<rusqlite::Result<_>>()
                    .unwrap();
                let plan = plan.join(" | ");
                assert!(
                    plan.contains("USING INDEX"),
                    "{sort:?}/{direction:?} phải đọc theo chỉ mục, nhận được: {plan}"
                );
                assert!(
                    !plan.contains("TEMP B-TREE"),
                    "{sort:?}/{direction:?} không được sắp lại cả bảng trong bộ nhớ, nhận được: {plan}"
                );
            }
        }
    }

    #[test]
    fn filtering_by_media_type_still_reads_through_an_index() {
        let db = temp_db();
        library_fixture(&db);
        let conn = db.conn();
        let plan: Vec<String> = conn
            .prepare(
                "EXPLAIN QUERY PLAN SELECT * FROM downloaded_files
                 WHERE media_type IN ('audio') ORDER BY completed_at DESC, rowid ASC LIMIT 60",
            )
            .unwrap()
            .query_map([], |row| row.get::<_, String>("detail"))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        let plan = plan.join(" | ");
        assert!(plan.contains("USING INDEX"), "nhận được: {plan}");
        assert!(!plan.contains("TEMP B-TREE"), "nhận được: {plan}");
    }

    #[test]
    fn one_job_can_own_several_library_items() {
        // FR-302: tách chương sinh ra nhiều file cho ĐÚNG MỘT dòng hàng đợi,
        // và cả ba phải nằm trong thư viện, cùng trỏ về tác vụ gốc.
        let db = temp_db();
        add_item(db_ref(&db), "j-ch", "/tmp/full.m4a", "Toàn bộ", MediaType::Audio, "youtube", 900, Some(900), "2026-07-01T00:00:00Z");
        add_item(db_ref(&db), "j-ch", "/tmp/full - 001 Mở đầu.m4a", "Toàn bộ - 001 Mở đầu", MediaType::Audio, "youtube", 300, Some(300), "2026-07-01T00:00:01Z");
        add_item(db_ref(&db), "j-ch", "/tmp/full - 002 Kết.m4a", "Toàn bộ - 002 Kết", MediaType::Audio, "youtube", 600, Some(600), "2026-07-01T00:00:02Z");

        let items = db.library_items_for_job("j-ch").unwrap();

        assert_eq!(items.len(), 3);
        assert!(items.iter().all(|item| item.job_id == "j-ch"));
    }

    #[test]
    fn a_repeat_download_updates_the_row_instead_of_adding_a_twin() {
        let db = temp_db();
        add_item(db_ref(&db), "j-a", "/tmp/same.mp3", "Lần đầu", MediaType::Audio, "youtube", 100, Some(10), "2026-07-01T00:00:00Z");
        add_item(db_ref(&db), "j-b", "/tmp/same.mp3", "Lần sau", MediaType::Audio, "youtube", 250, None, "2026-07-02T00:00:00Z");

        let items = db.list_library(&LibraryQuery::default()).unwrap();

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Lần sau");
        assert_eq!(items[0].file_size_bytes, 250);
        assert_eq!(items[0].job_id, "j-b");
        // Thời lượng đã đo được một lần thì không bị lần ghi sau (đo hụt) xoá đi.
        assert_eq!(items[0].duration_seconds, Some(10));
    }

    #[test]
    fn a_file_that_disappeared_is_marked_missing_and_can_come_back() {
        // FR-323/SC-304, và cả trường hợp "ổ đĩa ngoài đã tháo": mục bị đánh
        // dấu thiếu, KHÔNG bị xoá, và tự sáng lại ở vòng đối soát sau.
        let db = temp_db();
        let id = add_item(db_ref(&db), "j1", "/tmp/vanished.mp3", "Biến mất", MediaType::Audio, "youtube", 100, None, "2026-07-01T00:00:00Z");

        let page = db.library_reconcile_page(0, 10).unwrap();
        assert_eq!(page.len(), 1);
        assert!(!page[0].is_missing);

        let changed = db
            .apply_library_file_states(&[LibraryFileState {
                is_missing: true,
                ..page[0].clone()
            }])
            .unwrap();
        assert_eq!(changed, vec![id.clone()]);
        assert!(db.library_item(&id).unwrap().unwrap().is_missing);

        // Không đổi gì thì không báo đổi gì — giao diện không phải vẽ lại.
        let unchanged = db
            .apply_library_file_states(&[LibraryFileState {
                is_missing: true,
                ..page[0].clone()
            }])
            .unwrap();
        assert!(unchanged.is_empty());

        let back = db
            .apply_library_file_states(&[LibraryFileState {
                is_missing: false,
                ..page[0].clone()
            }])
            .unwrap();
        assert_eq!(back, vec![id.clone()]);
        assert!(!db.library_item(&id).unwrap().unwrap().is_missing);
    }

    #[test]
    fn only_missing_items_come_back_when_asked_for_them() {
        let db = temp_db();
        library_fixture(&db);
        let page = db.library_reconcile_page(0, 10).unwrap();
        let target = page.iter().find(|state| state.file_path == "/tmp/beta.mp4").unwrap();
        db.apply_library_file_states(&[LibraryFileState {
            is_missing: true,
            ..target.clone()
        }])
        .unwrap();

        let missing = db
            .list_library(&LibraryQuery {
                is_missing: Some(true),
                ..LibraryQuery::default()
            })
            .unwrap();

        assert_eq!(titles(&missing), vec!["Beta Video"]);
    }

    #[test]
    fn relinking_points_the_item_at_the_new_path_and_clears_missing() {
        // FR-325: trỏ lại thay vì phải tải lại. Đường dẫn, định dạng và cả
        // `search_text` phải theo kịp — nếu không, tìm theo tên file mới sẽ
        // không ra gì.
        let db = temp_db();
        let id = add_item(db_ref(&db), "j1", "/tmp/old.mp3", "Bài hát", MediaType::Audio, "youtube", 100, None, "2026-07-01T00:00:00Z");
        let state = db.library_reconcile_page(0, 10).unwrap().remove(0);
        db.apply_library_file_states(&[LibraryFileState { is_missing: true, ..state }])
            .unwrap();

        let relinked = db
            .set_library_item_path(&id, "/tmp/moved/new-name.flac", 4242)
            .unwrap();

        assert_eq!(relinked.file_path, "/tmp/moved/new-name.flac");
        assert_eq!(relinked.file_format, "flac");
        assert_eq!(relinked.file_size_bytes, 4242);
        assert!(!relinked.is_missing);
        let by_new_name = db
            .list_library(&LibraryQuery {
                search: Some("new-name".to_string()),
                ..LibraryQuery::default()
            })
            .unwrap();
        assert_eq!(by_new_name.len(), 1);
    }

    #[test]
    fn the_index_refuses_to_point_two_items_at_one_file() {
        // FR-322 ở tầng CSDL: kể cả khi một lỗi ở tầng trên bỏ sót phép kiểm
        // tra trên đĩa, chỉ mục vẫn không thể ghi được trạng thái "hai mục
        // cùng một file".
        let db = temp_db();
        library_fixture(&db);
        let id = db
            .list_library(&LibraryQuery {
                search: Some("alpha".to_string()),
                ..LibraryQuery::default()
            })
            .unwrap()[0]
            .id
            .clone();

        let err = db
            .set_library_item_path(&id, "/tmp/beta.mp4", 10)
            .unwrap_err();

        assert_eq!(err.code, FILE_EXISTS_ERROR_CODE);
        let untouched = db.library_item(&id).unwrap().unwrap();
        assert_eq!(untouched.file_path, "/tmp/alpha.mp3");
    }

    #[test]
    fn library_items_come_back_in_the_order_the_caller_asked_for() {
        // FR-330: thứ tự trong danh sách phát phải khớp thứ tự đang hiển thị,
        // mà thứ tự ấy chỉ tồn tại ở phía người gọi.
        let db = temp_db();
        library_fixture(&db);
        let by_title = db
            .list_library(&LibraryQuery {
                sort: LibrarySort::Title,
                direction: SortDirection::Asc,
                ..LibraryQuery::default()
            })
            .unwrap();
        let reversed: Vec<String> = by_title.iter().rev().map(|item| item.id.clone()).collect();

        let fetched = db.library_items(&reversed).unwrap();

        assert_eq!(
            titles(&fetched),
            vec!["Gamma Track", "Delta Post", "Beta Video", "Alpha Song"]
        );
    }

    #[test]
    fn library_items_skips_ids_that_no_longer_exist() {
        let db = temp_db();
        library_fixture(&db);
        let existing = db.list_library(&LibraryQuery::default()).unwrap()[0].id.clone();

        let fetched = db
            .library_items(&[existing.clone(), "khong-ton-tai".to_string()])
            .unwrap();

        assert_eq!(fetched.len(), 1);
        assert_eq!(fetched[0].id, existing);
    }

    #[test]
    fn stats_match_the_filter_they_were_given() {
        // SC-307: sai số bằng không so với thứ đang hiển thị.
        let db = temp_db();
        library_fixture(&db);

        let all = db.library_stats(&LibraryQuery::default()).unwrap();
        assert_eq!(all.total_items, 4);
        assert_eq!(all.total_size_bytes, 650);
        assert_eq!(all.missing_items, 0);
        assert_eq!(
            all.by_platform,
            vec![
                LibraryBreakdownEntry { key: "youtube".to_string(), item_count: 2, total_size_bytes: 400 },
                LibraryBreakdownEntry { key: "soundcloud".to_string(), item_count: 1, total_size_bytes: 200 },
                LibraryBreakdownEntry { key: "tiktok".to_string(), item_count: 1, total_size_bytes: 50 },
            ]
        );
        assert_eq!(all.formats, vec!["jpg", "mp3", "mp4"]);

        let audio_only = db
            .library_stats(&LibraryQuery {
                media_types: vec![MediaType::Audio],
                ..LibraryQuery::default()
            })
            .unwrap();
        assert_eq!(audio_only.total_items, 2);
        assert_eq!(audio_only.total_size_bytes, 500);
        assert_eq!(audio_only.by_media_type.len(), 1);
    }

    #[test]
    fn removing_items_touches_only_the_index() {
        let db = temp_db();
        library_fixture(&db);
        let ids: Vec<String> = db
            .list_library(&LibraryQuery::default())
            .unwrap()
            .into_iter()
            .take(2)
            .map(|item| item.id)
            .collect();

        let removed = db.remove_library_items(&ids).unwrap();

        assert_eq!(removed, 2);
        assert_eq!(db.list_library(&LibraryQuery::default()).unwrap().len(), 2);
        // Tác vụ gốc vẫn còn: dọn một mục khỏi thư viện không phải là xoá
        // lịch sử tải.
        assert!(db.get_job("j1").unwrap().is_some());
    }

    #[test]
    fn a_caption_shaped_folder_name_is_not_mistaken_for_a_file_format() {
        // Đúng một đường dẫn trong CSDL thật của người dùng: tác vụ gallery
        // nhiều ảnh lưu cả thư mục làm kết quả, và caption của bài đăng có
        // dấu chấm trong đó. `Path::extension` đọc nó thành một "định dạng"
        // dài 27 ký tự, và bộ lọc định dạng của FR-308 sẽ chào nó như một
        // lựa chọn.
        assert_eq!(
            media_file_extension("/Users/x/Downloads/https   vgen.co k1lltheguard46 (28f98824)"),
            ""
        );
        assert_eq!(media_file_extension("/tmp/#xuhuong #story #fyp (e21678a4)"), "");
        // Định dạng thật vẫn phải qua, kể cả khi nguồn viết hoa.
        assert_eq!(media_file_extension("/tmp/a.MP3"), "mp3");
        assert_eq!(media_file_extension("/tmp/a.webm"), "webm");
        assert_eq!(media_file_extension("/tmp/a.jpeg"), "jpeg");
    }

    #[test]
    fn the_format_filter_is_not_offered_an_empty_option() {
        // Mục không có định dạng (thư mục kết quả của gallery) vẫn nằm trong
        // thư viện và vẫn được đếm vào tổng — nó chỉ không được xuất hiện
        // như một ô lọc rỗng mà người dùng không hiểu để làm gì.
        let db = temp_db();
        library_fixture(&db);
        add_item(db_ref(&db), "j-dir", "/tmp/#caption (abc123)", "Bài đăng nhiều ảnh", MediaType::Gallery, "tiktok", 128, None, "2026-07-20T00:00:00Z");

        let stats = db.library_stats(&LibraryQuery::default()).unwrap();

        assert_eq!(stats.total_items, 5);
        assert_eq!(stats.formats, vec!["jpg", "mp3", "mp4"]);
    }

}
