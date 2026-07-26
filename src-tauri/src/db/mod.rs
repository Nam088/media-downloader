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
}
