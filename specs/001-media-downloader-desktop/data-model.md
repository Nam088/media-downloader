# Data Model: Trình Tải Media Đa Nền Tảng

**Feature**: 001-media-downloader-desktop | **Date**: 2026-07-25

Nguồn: mục "Key Entities" trong `spec.md`, cụ thể hoá thành schema SQLite (`rusqlite`) dùng chung cho hàng đợi và lịch sử.

## 1. DownloadJob (Tác vụ tải)

Đại diện một yêu cầu tải cụ thể, tồn tại từ lúc người dùng bấm Tải đến khi hoàn tất/thất bại/bị huỷ. Là cả "hàng đợi" (job đang active) lẫn "lịch sử" (job đã kết thúc) — phân biệt bằng trường `status`.

| Field | Type | Ghi chú |
|---|---|---|
| `id` | TEXT (UUID), PK | |
| `source_url` | TEXT | URL gốc người dùng dán vào |
| `platform` | TEXT | Nhãn nền tảng: `youtube`, `tiktok`, `facebook`, `instagram`, `twitter_x`, `soundcloud` cho 6 nền tảng bắt buộc (FR-014); với mọi liên kết khác mà yt-dlp vẫn nhận diện được, lấy trực tiếp từ `extractor_key` của yt-dlp (viết thường), không giới hạn trước bằng danh sách cố định |
| `media_type` | TEXT enum | `audio` \| `video` (FR-003) |
| `audio_quality` | TEXT, nullable | giá trị người dùng chọn từ `MediaSource.available_audio_formats` của chính liên kết đó (không phải enum cố định trong app) — vd `"128kbps"`, `"64kbps"` tuỳ nguồn thực có gì, chỉ áp dụng khi `media_type = audio` (FR-004, FR-019) |
| `video_quality` | TEXT, nullable | giá trị người dùng chọn từ `MediaSource.available_video_qualities` của chính liên kết đó (vd `1080p`), chỉ áp dụng khi `media_type = video` (FR-003, FR-019, US2) |
| `status` | TEXT enum | `queued` \| `fetching_metadata` \| `downloading` \| `paused` \| `completed` \| `failed` \| `canceled` |
| `progress_percent` | REAL | 0.0 - 100.0 |
| `speed_bytes_per_sec` | INTEGER, nullable | dùng để hiển thị tốc độ (FR-005) |
| `eta_seconds` | INTEGER, nullable | thời gian còn lại ước tính (FR-005) |
| `error_message` | TEXT, nullable | thông báo lỗi dễ hiểu khi `status = failed` (FR-009) |
| `output_directory` | TEXT | thư mục người dùng chọn lưu (FR-008) |
| `output_file_path` | TEXT, nullable | điền khi hoàn tất, tham chiếu tới `DownloadedFile` |
| `is_playlist_item` | BOOLEAN | true nếu job này được tách ra từ một playlist đã xác nhận (FR-013) |
| `parent_playlist_id` | TEXT, nullable | nhóm các job cùng thuộc 1 playlist đã xác nhận tải toàn bộ |
| `retried_from_job_id` | TEXT, nullable | trỏ về `id` của job cũ khi job này được tạo ra bởi thao tác "thử lại" (FR-006); giữ liên kết lịch sử retry |
| `created_at` | TEXT (ISO 8601) | |
| `updated_at` | TEXT (ISO 8601) | |

**State transitions** (trạng thái `status`):

```text
queued → fetching_metadata → downloading → completed
                                        ↘ failed → (retry) → queued
downloading → paused → downloading
queued|downloading|paused → canceled
```

Retry (FR-006) tạo lại job ở trạng thái `queued` giữ nguyên `source_url`/lựa chọn ban đầu, không sửa bản ghi lịch sử cũ mà tạo `id` mới, giữ liên kết `retried_from_job_id` để tra cứu (xem thêm ở DownloadHistory).

## 2. MediaSource (Nguồn media — xem trước)

Thông tin xem trước lấy được từ yt-dlp (`--dump-json`, không tải) trước khi người dùng xác nhận tải. Không nhất thiết lưu lâu dài trong DB — có thể cache tạm trong bộ nhớ theo phiên làm việc, chỉ persist khi job được tạo.

| Field | Type | Ghi chú |
|---|---|---|
| `source_url` | TEXT | |
| `title` | TEXT | (FR-002) |
| `thumbnail_url` | TEXT, nullable | (FR-002) |
| `duration_seconds` | INTEGER, nullable | (FR-002) |
| `platform` | TEXT | |
| `is_playlist` | BOOLEAN | (FR-013) |
| `playlist_item_count` | INTEGER, nullable | chỉ điền khi `is_playlist = true` |
| `available_video_qualities` | Object[] (JSON array) | Lấy trực tiếp từ danh sách format thực tế yt-dlp trả về cho liên kết này (không hard-code), mỗi phần tử gồm `{ label: string (vd "1080p"), filesize_bytes: number \| null }` — `filesize_bytes` là ước tính dung lượng tải về (video + audio tốt nhất tương ứng), lấy từ `filesize`/`filesize_approx` của yt-dlp, `null` nếu nguồn không cung cấp; dùng để gợi ý mức gần nhất khi chất lượng chọn không có sẵn (US2, Acceptance #2; FR-019) |
| `available_audio_formats` | Object[] (JSON array) | Lấy trực tiếp từ danh sách format âm thanh thực tế yt-dlp trả về cho liên kết này, mỗi phần tử gồm `{ bitrate_kbps: number, codec: string, filesize_bytes: number \| null }`; là nguồn duy nhất để dựng danh sách mức chất lượng MP3 hiển thị cho người dùng — KHÔNG dùng danh sách cố định viết sẵn trong ứng dụng (FR-004, FR-019) |

## 3. DownloadedFile (Tệp đã tải)

Một dòng tương ứng 1 job đã hoàn tất thành công (`DownloadJob.status = completed`).

| Field | Type | Ghi chú |
|---|---|---|
| `id` | TEXT (UUID), PK | |
| `job_id` | TEXT, FK → DownloadJob.id | |
| `file_path` | TEXT | đường dẫn tuyệt đối |
| `file_format` | TEXT | `mp3` \| định dạng video gốc (vd `mp4`) |
| `file_size_bytes` | INTEGER | |
| `completed_at` | TEXT (ISO 8601) | |

## 4. DownloadHistory (Lịch sử tải)

Không phải bảng riêng — là view/truy vấn trên `DownloadJob` lọc theo `status IN (completed, failed, canceled)`, sắp theo `updated_at DESC`, JOIN với `DownloadedFile` khi có. Việc "mở thư mục chứa tệp" và "thử tải lại" (FR-007) thao tác trực tiếp trên `file_path` và `source_url` của bản ghi tương ứng.

## 5. AppSettings (Cài đặt ứng dụng)

Bảng đơn dòng (hoặc key-value) lưu lựa chọn của người dùng, phục vụ FR-016/FR-017.

| Field | Type | Ghi chú |
|---|---|---|
| `theme` | TEXT enum | `system` \| `light` \| `dark`, mặc định `system` (FR-016) |
| `language` | TEXT enum | `system` \| `en` \| `vi`, mặc định `system` → fallback `en` nếu ngôn ngữ hệ điều hành không được hỗ trợ (FR-017, edge case) |
| `default_output_directory` | TEXT | (FR-008) |

## Ràng buộc toàn vẹn dữ liệu chung

- `platform` PHẢI thuộc tập nền tảng đã hỗ trợ (FR-014); URL không khớp bất kỳ platform nào bị từ chối ngay ở bước xem trước với lỗi rõ ràng (FR-009).
- `DownloadJob.audio_quality`/`video_quality` PHẢI được chọn từ tập giá trị nằm trong `MediaSource.available_audio_formats`/`available_video_qualities` của chính request `preview_media` gần nhất cho `source_url` đó; backend PHẢI từ chối (`AppError`) nếu giá trị gửi lên không khớp bất kỳ phần tử nào trong danh sách thực tế đã trả về, để đảm bảo không có tuỳ chọn "ảo" bị viết cứng ở phía frontend (FR-019).
- Không tạo `DownloadJob` cho nội dung mà bước xem trước (`MediaSource`) trả về cờ yêu cầu đăng nhập/DRM — chặn tại tầng dữ liệu trước khi tới hàng đợi (FR-012).
- Toàn bộ thao tác ghi SQLite chạy qua lõi Rust (`src-tauri`), frontend không truy cập DB trực tiếp — chỉ gọi qua Tauri IPC commands (xem `contracts/tauri-commands.md`).
