use std::path::Path;
use std::sync::Mutex;

use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use rusqlite_migration::{Migrations, M};

use crate::error::AppError;
use crate::models::{AppSettings, DownloadJob, DownloadedFile, GalleryMode, JobStatus, MediaType};

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
    ])
}

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

        Ok(Db(Mutex::new(conn)))
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
        conn.execute(
            "INSERT INTO download_jobs (
                id, source_url, platform, media_type, audio_quality, video_quality,
                gallery_mode, selected_gallery_urls, status, progress_percent,
                speed_bytes_per_sec, eta_seconds, error_message, output_directory,
                output_file_path, is_playlist_item, parent_playlist_id,
                retried_from_job_id, created_at, updated_at, title, playlist_title,
                queue_position, retry_count, next_retry_at
            ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25)",
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

    pub fn update_job_progress(
        &self,
        job_id: &str,
        progress_percent: f64,
        speed_bytes_per_sec: Option<i64>,
        eta_seconds: Option<i64>,
    ) -> Result<(), AppError> {
        let conn = self.conn();
        conn.execute(
            "UPDATE download_jobs SET progress_percent = ?1, speed_bytes_per_sec = ?2,
             eta_seconds = ?3, updated_at = ?4 WHERE id = ?5",
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

    pub fn update_job_status(
        &self,
        job_id: &str,
        status: JobStatus,
        error_message: Option<&str>,
    ) -> Result<(), AppError> {
        let conn = self.conn();
        conn.execute(
            "UPDATE download_jobs SET status = ?1, error_message = ?2, updated_at = ?3 WHERE id = ?4",
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

    // ---- truy vấn điều phối hàng đợi ----------------------------------
    //
    // Cả nhóm dưới đây mang `#[allow(dead_code)]`: chúng là tầng truy vấn cho
    // bộ điều phối, và bộ điều phối được thêm ở một bước sau. Tới lúc đó các
    // attribute này phải được gỡ bỏ, không phải để lại — chúng chỉ tồn tại để
    // tầng DB được ghép và kiểm thử trọn vẹn trước khi có người gọi.

    /// Job kế tiếp mà bộ điều phối được phép khởi chạy: đang `queued`, và
    /// không nằm trong khoảng chờ thử lại. `now_rfc3339` được truyền vào thay
    /// vì đọc đồng hồ ở đây để test kiểm soát được thời gian.
    ///
    /// Thứ tự: `queue_position` trước, `created_at` sau. Vế thứ hai giữ cho
    /// các job cũ (đều mang `queue_position = 0` từ migration 0008) vẫn chạy
    /// đúng thứ tự chúng được tạo.
    #[allow(dead_code)]
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
    #[allow(dead_code)]
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
    #[allow(dead_code)]
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

    /// Đánh số lại các job chưa kết thúc thành 1.0, 2.0, 3.0… giữ nguyên thứ tự
    /// hiện tại. Chỉ chạy khi khe hở đã hẹp tới ngưỡng — trong sử dụng bình
    /// thường gần như không bao giờ xảy ra.
    #[allow(dead_code)]
    pub fn renormalize_queue_positions(&self) -> Result<(), AppError> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        renormalize_positions_within(&tx)?;
        tx.commit()?;
        Ok(())
    }

    /// Gọi một lần lúc khởi động: job còn ghi `downloading`/`fetching_metadata`
    /// là tàn dư của một phiên bị đóng đột ngột — tiến trình tải của chúng đã
    /// chết cùng ứng dụng. Chuyển về `paused` để người dùng tiếp tục hoặc huỷ
    /// (FR-115). Trả về số dòng đã đổi.
    #[allow(dead_code)]
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
    #[allow(dead_code)]
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

    /// Đổi trạng thái hàng loạt, trả về id của những job **khớp**
    /// `from_statuses` để tầng gọi biết cần phát sự kiện cho những job nào
    /// (FR-118).
    ///
    /// "Khớp" chứ không phải "đã đổi": nếu `to_status` cũng nằm trong
    /// `from_statuses` thì các dòng vốn đã ở trạng thái đích vẫn được liệt kê,
    /// và phía gọi sẽ phát sự kiện thừa cho chúng. Phía gọi chịu trách nhiệm
    /// không truyền vào tổ hợp đó.
    #[allow(dead_code)]
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
            tx.execute(
                "UPDATE download_jobs SET status = ?1, updated_at = ?2 WHERE id = ?3",
                params![to_status.as_str(), Utc::now().to_rfc3339(), id],
            )?;
        }
        tx.commit()?;
        Ok(ids)
    }

    // ---- downloaded_files ---------------------------------------------

    pub fn insert_downloaded_file(
        &self,
        job_id: &str,
        file_path: &str,
        file_format: &str,
        file_size_bytes: i64,
    ) -> Result<(), AppError> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO downloaded_files (id, job_id, file_path, file_format, file_size_bytes, completed_at)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                uuid::Uuid::new_v4().to_string(),
                job_id,
                file_path,
                file_format,
                file_size_bytes,
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    /// Not called yet — `list_history` (T035) reads `output_file_path`
    /// straight off `DownloadJob` instead, which is enough for the current
    /// UI. Kept for when a richer history view needs `file_size_bytes`/
    /// `file_format` (data-model.md §3), so that data isn't write-only.
    #[allow(dead_code)]
    pub fn get_downloaded_file_for_job(
        &self,
        job_id: &str,
    ) -> Result<Option<DownloadedFile>, AppError> {
        let conn = self.conn();
        conn.query_row(
            "SELECT * FROM downloaded_files WHERE job_id = ?1 ORDER BY completed_at DESC LIMIT 1",
            params![job_id],
            row_to_downloaded_file,
        )
        .optional()
        .map_err(AppError::from)
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
        })
    }

    pub fn update_settings(&self, settings: &AppSettings) -> Result<(), AppError> {
        let conn = self.conn();
        Self::set_setting(&conn, "theme", &settings.theme)?;
        Self::set_setting(&conn, "language", &settings.language)?;
        Self::set_setting(&conn, "default_output_directory", &settings.default_output_directory)?;
        Self::set_setting(&conn, "show_logs_tab", if settings.show_logs_tab { "1" } else { "0" })?;
        Ok(())
    }
}

// Ba mục dưới đây cũng chỉ được gọi từ test cho tới khi bộ điều phối xuất
// hiện — xem ghi chú ở đầu nhóm truy vấn điều phối trong `impl Db`.

/// Khe hở hẹp nhất còn chấp nhận được giữa hai vị trí liền kề.
///
/// `f64` có 52 bit phần định trị, nên trên lý thuyết còn chia đôi được sâu hơn
/// ngưỡng này rất nhiều. Đặt ngưỡng cao hơn giới hạn thật nhiều bậc để không
/// bao giờ chạm tới vùng mà phép lấy điểm giữa trả về đúng bằng một trong hai
/// đầu mút — lúc đó thứ tự sẽ hỏng một cách âm thầm.
#[allow(dead_code)]
const MIN_POSITION_GAP: f64 = 1e-6;

/// Vị trí nằm giữa hai hàng xóm. `None` nghĩa là không có hàng xóm ở phía đó,
/// tức là đang thả vào đầu hoặc cuối danh sách.
#[allow(dead_code)]
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
#[allow(dead_code)]
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
#[allow(dead_code)]
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
#[allow(dead_code)]
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
    })
}

fn row_to_downloaded_file(row: &rusqlite::Row) -> rusqlite::Result<DownloadedFile> {
    Ok(DownloadedFile {
        id: row.get("id")?,
        job_id: row.get("job_id")?,
        file_path: row.get("file_path")?,
        file_format: row.get("file_format")?,
        file_size_bytes: row.get("file_size_bytes")?,
        completed_at: row.get("completed_at")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{JobStatus, MediaType};

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
        let path =
            std::env::temp_dir().join(format!("media-downloader-test-{}.db", uuid::Uuid::new_v4()));
        let mut conn = Connection::open(&path).expect("db opens");
        conn.pragma_update(None, "foreign_keys", "OFF").unwrap();
        migrations()
            .to_version(&mut conn, version)
            .expect("migrates to the requested version");
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        conn
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
}
