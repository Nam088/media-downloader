# Contract: Tauri IPC Commands (Frontend ↔ Backend)

**Feature**: 001-media-downloader-desktop | **Date**: 2026-07-25

Đây là ứng dụng desktop độc lập (không có API mạng công khai) — "contract" ở đây là giao diện IPC giữa frontend React và backend Rust qua cơ chế `invoke` của Tauri, cùng các sự kiện (`event`) đẩy từ backend về frontend theo thời gian thực. Mọi thay đổi ở đây là breaking change đối với frontend và ngược lại.

## Quy ước chung

- Mọi command trả về `Result<T, AppError>`; `AppError` luôn có `code` (machine-readable) và `message` (đã bản địa hoá theo `AppSettings.language`, hiển thị trực tiếp cho người dùng — FR-009).
- Toàn bộ thời gian dùng ISO 8601 UTC.

## Commands

### `preview_media(source_url: string) -> MediaSource | AppError`

Gọi yt-dlp ở chế độ chỉ lấy metadata (không tải), phục vụ FR-002. `source_url` KHÔNG bị lọc trước bằng danh sách domain cố định — mọi liên kết đều được chuyển thẳng cho yt-dlp; chỉ khi chính yt-dlp báo không có extractor phù hợp thì mới trả `AppError{code: "UNSUPPORTED_PLATFORM"}` (FR-014, ~1.600+ trang yt-dlp hỗ trợ, không riêng 6 nền tảng bắt buộc). Nếu nội dung riêng tư/yêu cầu đăng nhập/DRM → `AppError{code: "ACCESS_DENIED"}` (FR-012).

**Trả về**: `MediaSource` (xem `data-model.md` §2), gồm cờ `is_playlist` để frontend quyết định có hỏi người dùng theo FR-013 hay không, cùng `available_audio_formats` và `available_video_qualities` lấy trực tiếp từ format thực tế của liên kết này. Frontend PHẢI dựng danh sách lựa chọn chất lượng hiển thị cho người dùng hoàn toàn từ hai trường này — không được có bất kỳ danh sách chất lượng cố định nào viết sẵn trong code frontend (FR-004, FR-019).

### `create_download_job(input: CreateJobInput) -> DownloadJob[] | AppError`

Tạo một hoặc nhiều `DownloadJob` mới ở trạng thái `queued` và đưa vào hàng đợi xử lý. Trả về mảng thay vì một object đơn vì khi `playlist_scope: "entire_playlist"`, một lệnh gọi tạo ra N job (một job mỗi mục trong playlist, cùng `parent_playlist_id`) — trường hợp thông thường trả về mảng có đúng 1 phần tử.

```text
CreateJobInput {
  source_url: string
  media_type: "audio" | "video"
  audio_quality?: string   // PHẢI khớp {bitrate_kbps}kbps của 1 phần tử trong MediaSource.available_audio_formats; bắt buộc nếu media_type = "audio"
  video_quality?: string   // PHẢI khớp .label của 1 phần tử trong MediaSource.available_video_qualities; bắt buộc nếu media_type = "video"
  output_directory: string
  playlist_scope?: "single_item" | "entire_playlist"  // bắt buộc nếu preview_media trả is_playlist = true
}
```

Backend PHẢI validate `audio_quality`/`video_quality` gửi lên so với kết quả `preview_media` gần nhất cho `source_url` (không tin tưởng mù quáng giá trị frontend gửi); trả `AppError{code: "INVALID_QUALITY_OPTION"}` nếu không khớp (FR-019).

### `pause_job(job_id: string) -> void | AppError`
### `resume_job(job_id: string) -> void | AppError`
### `cancel_job(job_id: string) -> void | AppError`
### `retry_job(job_id: string) -> DownloadJob | AppError`

Bốn command điều khiển vòng đời job theo FR-006. `retry_job` tạo job mới (xem quy tắc ở `data-model.md` §1) và trả về job mới.

### `list_queue() -> DownloadJob[]`

Trả về các job có `status IN (queued, fetching_metadata, downloading, paused)`, sắp theo `created_at ASC`.

### `list_history(filter?: HistoryFilter) -> DownloadJob[]`

Trả về các job có `status IN (completed, failed, canceled)`, sắp theo `updated_at DESC`. Phục vụ FR-007.

### `open_containing_folder(job_id: string) -> void | AppError`

Mở trình quản lý tệp của hệ điều hành tại thư mục chứa `DownloadedFile.file_path` của job.

### `get_settings() -> AppSettings`
### `update_settings(patch: Partial<AppSettings>) -> AppSettings`

Đọc/ghi `theme`, `language`, `default_output_directory` (FR-016, FR-017, FR-008).

## Events (Backend → Frontend, đẩy real-time qua `tauri::Emit`)

### `job:progress`

```text
{ job_id: string, progress_percent: number, speed_bytes_per_sec?: number, eta_seconds?: number }
```

Phát định kỳ (khuyến nghị tối đa 4 lần/giây) trong lúc `status = downloading`, phục vụ FR-005 mà không cần frontend poll liên tục.

### `job:status_changed`

```text
{ job_id: string, status: DownloadJob["status"], error_message?: string }
```

Phát mỗi khi `status` đổi (bao gồm chuyển sang `failed` kèm `error_message` đã bản địa hoá — FR-009).

## Ràng buộc

- Frontend KHÔNG được tự parse output thô của yt-dlp/ffmpeg; toàn bộ việc đó nằm ở `src-tauri/src/downloader/` và chỉ lộ ra dưới dạng các field đã chuẩn hoá ở trên.
- Mọi command liên quan tới DRM/nội dung riêng tư PHẢI trả lỗi thay vì âm thầm bỏ qua giới hạn truy cập (FR-012) — không có command nào được thiết kế để bypass xác thực nền tảng nguồn.
