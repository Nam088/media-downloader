-- Adds GalleryMode::ImagesOnly (keep images, drop audio — the mirror image
-- of AudioOnly). Same SQLite table-rebuild pattern as migration 0002 (CHECK
-- constraints can't be ALTERed in place); `Db::open` already wraps the whole
-- `to_latest()` call with `PRAGMA foreign_keys=OFF/ON`, so this doesn't need
-- to repeat that itself.

CREATE TABLE download_jobs_new (
    id TEXT PRIMARY KEY,
    source_url TEXT NOT NULL,
    platform TEXT NOT NULL,
    media_type TEXT NOT NULL CHECK (media_type IN ('audio', 'video', 'gallery')),
    audio_quality TEXT,
    video_quality TEXT,
    gallery_mode TEXT CHECK (gallery_mode IN ('files', 'audio_only', 'images_only', 'slideshow')),
    status TEXT NOT NULL CHECK (
        status IN (
            'queued', 'fetching_metadata', 'downloading', 'paused',
            'completed', 'failed', 'canceled'
        )
    ),
    progress_percent REAL NOT NULL DEFAULT 0,
    speed_bytes_per_sec INTEGER,
    eta_seconds INTEGER,
    error_message TEXT,
    output_directory TEXT NOT NULL,
    output_file_path TEXT,
    is_playlist_item INTEGER NOT NULL DEFAULT 0,
    parent_playlist_id TEXT,
    retried_from_job_id TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

INSERT INTO download_jobs_new (
    id, source_url, platform, media_type, audio_quality, video_quality,
    gallery_mode, status, progress_percent, speed_bytes_per_sec, eta_seconds,
    error_message, output_directory, output_file_path, is_playlist_item,
    parent_playlist_id, retried_from_job_id, created_at, updated_at
)
SELECT
    id, source_url, platform, media_type, audio_quality, video_quality,
    gallery_mode, status, progress_percent, speed_bytes_per_sec, eta_seconds,
    error_message, output_directory, output_file_path, is_playlist_item,
    parent_playlist_id, retried_from_job_id, created_at, updated_at
FROM download_jobs;

DROP TABLE download_jobs;
ALTER TABLE download_jobs_new RENAME TO download_jobs;

CREATE INDEX idx_download_jobs_status ON download_jobs (status);
CREATE INDEX idx_download_jobs_updated_at ON download_jobs (updated_at);
