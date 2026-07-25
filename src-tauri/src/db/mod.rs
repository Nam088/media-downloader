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
        conn.execute(
            "INSERT INTO download_jobs (
                id, source_url, platform, media_type, audio_quality, video_quality,
                gallery_mode, status, progress_percent, speed_bytes_per_sec, eta_seconds,
                error_message, output_directory, output_file_path, is_playlist_item,
                parent_playlist_id, retried_from_job_id, created_at, updated_at
            ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",
            params![
                job.id,
                job.source_url,
                job.platform,
                media_type_str(&job.media_type),
                job.audio_quality,
                job.video_quality,
                job.gallery_mode.as_ref().map(gallery_mode_str),
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

    // ---- app_settings ---------------------------------------------------

    pub fn get_settings(&self) -> Result<AppSettings, AppError> {
        let conn = self.conn();
        conn.query_row(
            "SELECT theme, language, default_output_directory FROM app_settings WHERE id = 1",
            [],
            |row| {
                Ok(AppSettings {
                    theme: row.get(0)?,
                    language: row.get(1)?,
                    default_output_directory: row.get(2)?,
                })
            },
        )
        .map_err(AppError::from)
    }

    pub fn update_settings(&self, settings: &AppSettings) -> Result<(), AppError> {
        let conn = self.conn();
        conn.execute(
            "UPDATE app_settings SET theme = ?1, language = ?2, default_output_directory = ?3 WHERE id = 1",
            params![settings.theme, settings.language, settings.default_output_directory],
        )?;
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
