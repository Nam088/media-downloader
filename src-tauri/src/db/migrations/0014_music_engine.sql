-- Phase 006 (specs/006-spotiflac-integration): thêm engine nhạc lossless
-- SpotiFLAC làm engine thứ ba cạnh yt-dlp/gallery-dl.
--
-- Hai việc, một migration:
--   1. `download_jobs`: nới CHECK `media_type` nhận 'music' và CHECK `status`
--      nhận 'waiting_input' (job đang dừng chờ người dùng nhập Cloudflare
--      grant code — data-model.md §2). CHECK không ALTER tại chỗ được nên
--      dùng đúng pattern rebuild của 0002/0003; `Db::open` đã bọc toàn bộ
--      `to_latest()` trong `PRAGMA foreign_keys=OFF/ON` nên tham chiếu
--      `downloaded_files.job_id` sống sót qua DROP/RENAME.
--   2. `downloaded_files`: thêm cột `source_provider` — provider thật sự đã
--      giao file ('tidal'/'qobuz'/'deezer'/'amazon'/'ext:<name>'), NULL cho
--      file của engine khác và mọi dòng có trước migration này. Bảng này
--      KHÔNG có CHECK trên `media_type` (0012 thêm cột bằng ALTER, không ràng
--      buộc) nên không cần rebuild.
--
-- QUY TẮC MIGRATION (xem 0005/0012): `rusqlite_migration` theo dõi bằng SỐ
-- LƯỢNG — file đã phát hành không được sửa; file mới phải được đăng ký bằng
-- `M::up(include_str!("migrations/0014_music_engine.sql"))` trong
-- `migrations()` ở `db/mod.rs`, nếu không nó bị bỏ qua trong im lặng (test
-- `migration_0014_is_registered_and_widens_media_type` canh chỗ này).

CREATE TABLE download_jobs_new (
    id TEXT PRIMARY KEY,
    source_url TEXT NOT NULL,
    platform TEXT NOT NULL,
    media_type TEXT NOT NULL CHECK (media_type IN ('audio', 'video', 'gallery', 'music')),
    audio_quality TEXT,
    video_quality TEXT,
    gallery_mode TEXT CHECK (gallery_mode IN ('files', 'audio_only', 'images_only', 'slideshow')),
    status TEXT NOT NULL CHECK (
        status IN (
            'queued', 'fetching_metadata', 'downloading', 'paused',
            'completed', 'failed', 'canceled', 'waiting_input'
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
    id, source_url, platform, media_type, audio_quality, video_quality,
    gallery_mode, status, progress_percent, speed_bytes_per_sec, eta_seconds,
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

ALTER TABLE downloaded_files ADD COLUMN source_provider TEXT;
