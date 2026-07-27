-- Xoá SpotiFLAC music engine: gỡ 'music' khỏi media_type CHECK,
-- gỡ 'waiting_input' khỏi status CHECK, và xoá cột source_provider.
--
-- Migration 0014 đã thêm chúng; không được xoá file 0014 (đã phát hành).
-- Pattern rebuild giống 0002/0003/0014: SQLite không ALTER CHECK được tại chỗ.

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
    updated_at TEXT NOT NULL,
    selected_gallery_urls TEXT,
    title TEXT,
    playlist_title TEXT,
    queue_position REAL NOT NULL DEFAULT 0,
    retry_count INTEGER NOT NULL DEFAULT 0,
    next_retry_at TEXT,
    output_options TEXT
);

INSERT INTO download_jobs_new (
    id, source_url, platform, media_type, audio_quality, video_quality,
    gallery_mode, status, progress_percent, speed_bytes_per_sec, eta_seconds,
    error_message, output_directory, output_file_path, is_playlist_item,
    parent_playlist_id, retried_from_job_id, created_at, updated_at,
    selected_gallery_urls, title, playlist_title, queue_position, retry_count,
    next_retry_at, output_options
)
SELECT
    id, source_url, platform,
    CASE media_type WHEN 'music' THEN 'audio' ELSE media_type END AS media_type,
    audio_quality, video_quality,
    gallery_mode,
    CASE status WHEN 'waiting_input' THEN 'downloading' ELSE status END AS status,
    progress_percent, speed_bytes_per_sec, eta_seconds,
    error_message, output_directory, output_file_path, is_playlist_item,
    parent_playlist_id, retried_from_job_id, created_at, updated_at,
    selected_gallery_urls, title, playlist_title, queue_position, retry_count,
    next_retry_at, output_options
FROM download_jobs;

DROP TABLE download_jobs;
ALTER TABLE download_jobs_new RENAME TO download_jobs;

CREATE INDEX idx_download_jobs_status ON download_jobs (status);
CREATE INDEX idx_download_jobs_updated_at ON download_jobs (updated_at);
CREATE INDEX idx_download_jobs_dispatch
    ON download_jobs (status, queue_position, created_at);

-- Xoá cột source_provider (chỉ có ở download_music, giờ không dùng nữa).
-- SQLite 3.35+ hỗ ALTER TABLE DROP COLUMN; nếu bản build cũ hơn thì bỏ qua.
ALTER TABLE downloaded_files DROP COLUMN source_provider;
