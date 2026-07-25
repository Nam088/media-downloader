# Quickstart: Trình Tải Media Đa Nền Tảng

**Feature**: 001-media-downloader-desktop | **Date**: 2026-07-25

Hướng dẫn này dùng để xác nhận luồng cốt lõi (User Story 1 — tải âm thanh từ một liên kết) chạy được đầu cuối trong môi trường phát triển. Không thay thế cho `tasks.md` (kế hoạch triển khai chi tiết, tạo bởi `/speckit-tasks`).

## Prerequisites

- Rust stable (`rustup show` ≥ 1.78) và Tauri CLI (`cargo install tauri-cli` hoặc `cargo tauri --version`)
- Node.js 20+ và package manager (pnpm khuyến nghị)
- Binary `yt-dlp` và `ffmpeg` tương ứng hệ điều hành đang phát triển đặt vào `src-tauri/binaries/` theo tên target-triple mà Tauri yêu cầu cho `externalBin` (xem `research.md` §2). **Chỉ cần cho môi trường dev** — bản build release đã tự động kèm sẵn qua CI, người dùng cuối không cần bước này (FR-018).

## Setup

```bash
pnpm install
cd src-tauri && cargo check && cd ..
```

## Run (dev mode)

```bash
pnpm tauri dev
```

Ứng dụng mở lên với giao diện theo theme hệ thống (FR-016) và ngôn ngữ theo hệ điều hành (FR-017).

## Validation scenario (User Story 1 — Independent Test)

1. Dán một liên kết YouTube công khai hợp lệ (video ngắn, không riêng tư, không phải playlist) vào ô nhập.
2. Xác nhận khu vực xem trước hiển thị đúng tiêu đề, hình thu nhỏ, thời lượng (`preview_media`, xem `contracts/tauri-commands.md`).
3. Chọn "Chỉ âm thanh", mức chất lượng "Tiêu chuẩn (~128kbps)", chọn thư mục đầu ra.
4. Bấm Tải → xác nhận:
   - Job xuất hiện trong hàng đợi với `status: downloading` và tiến trình cập nhật liên tục qua sự kiện `job:progress`.
   - Sau khi hoàn tất, `status: completed`, và một tệp `.mp3` tồn tại đúng tại thư mục đầu ra đã chọn.
   - Tệp phát được và có nội dung âm thanh khớp với video gốc.
5. Mở mục Lịch sử → xác nhận job vừa tải xuất hiện với đúng tên, nguồn, thời gian, trạng thái `completed` (FR-007).

## Kiểm tra nhanh các yêu cầu bổ sung

- **Theme**: Toggle Sáng/Tối ở góc giao diện → xác nhận toàn bộ UI đổi theme ngay lập tức, không cần khởi động lại (SC-006).
- **Ngôn ngữ**: Đổi ngôn ngữ trong Cài đặt giữa Tiếng Việt/Tiếng Anh → xác nhận toàn bộ văn bản trên màn hình đổi theo, không còn chữ sót lại ở ngôn ngữ cũ (SC-007).
- **Không cài thủ công**: Trên một máy sạch (chưa từng cài Python/ffmpeg), cài đặt ứng dụng từ trình cài đặt release và lặp lại bước 1-5 ở trên mà không có bước cài đặt phụ nào khác (SC-008, FR-018).
- **Playlist**: Dán một liên kết playlist → xác nhận ứng dụng hỏi rõ "chỉ mục này" hay "cả danh sách" trước khi tạo job (FR-013), không tự động tải toàn bộ.
- **Nội dung bị chặn**: Dán một liên kết video riêng tư/yêu cầu đăng nhập → xác nhận `preview_media` trả lỗi rõ ràng, không tạo được job (FR-009, FR-012).

## Automated coverage tương ứng

- `cargo test` (trong `src-tauri/tests/`): unit test cho parser tiến trình yt-dlp, state machine của `DownloadJob`, truy vấn SQLite.
- `pnpm test` (Vitest): component `DownloadForm`, `QueueList`, `ThemeToggle`, `LanguageSwitcher`.
- `pnpm test:e2e` (tauri-driver + WebdriverIO): tự động hoá đúng kịch bản bước 1-5 ở trên với một video test công khai cố định.
