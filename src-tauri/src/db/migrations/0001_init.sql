-- Initial schema per specs/001-media-downloader-desktop/data-model.md

CREATE TABLE download_jobs (
    id TEXT PRIMARY KEY,
    source_url TEXT NOT NULL,
    platform TEXT NOT NULL,
    media_type TEXT NOT NULL CHECK (media_type IN ('audio', 'video')),
    audio_quality TEXT,
    video_quality TEXT,
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

CREATE INDEX idx_download_jobs_status ON download_jobs (status);
CREATE INDEX idx_download_jobs_updated_at ON download_jobs (updated_at);

CREATE TABLE downloaded_files (
    id TEXT PRIMARY KEY,
    job_id TEXT NOT NULL REFERENCES download_jobs (id),
    file_path TEXT NOT NULL,
    file_format TEXT NOT NULL,
    file_size_bytes INTEGER NOT NULL,
    completed_at TEXT NOT NULL
);

CREATE INDEX idx_downloaded_files_job_id ON downloaded_files (job_id);

CREATE TABLE app_settings (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    theme TEXT NOT NULL DEFAULT 'system' CHECK (theme IN ('system', 'light', 'dark')),
    language TEXT NOT NULL DEFAULT 'system' CHECK (language IN ('system', 'en', 'vi')),
    default_output_directory TEXT NOT NULL DEFAULT ''
);

INSERT INTO app_settings (id, theme, language, default_output_directory) VALUES (1, 'system', 'system', '');
