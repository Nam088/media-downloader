# Implementation Plan: Trình Tải Media Đa Nền Tảng (Cross-Platform Media Downloader)

**Branch**: `001-media-downloader-desktop` | **Date**: 2026-07-25 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/001-media-downloader-desktop/spec.md`

## Summary

Ứng dụng desktop chạy trên Windows/macOS/Linux, cho phép người dùng dán liên kết YouTube/TikTok/Facebook/Instagram/Twitter-X/SoundCloud để tải âm thanh (MP3) hoặc video đầy đủ, với hàng đợi tải, lịch sử, giao diện hiện đại có chế độ Sáng/Tối, và đa ngôn ngữ (Việt/Anh).

Cách tiếp cận kỹ thuật: **Tauri (lõi Rust) + giao diện web React/TypeScript**, dùng **yt-dlp** (đóng gói sẵn dưới dạng sidecar binary) làm engine trích xuất/tải media cho toàn bộ các nền tảng nguồn, và **ffmpeg** (đóng gói kèm) để chuyển đổi sang MP3 theo mức chất lượng yêu cầu. Lựa chọn này tối ưu cho cả 3 tiêu chí người dùng nhấn mạnh: (1) dung lượng cài đặt/bộ nhớ nhỏ hơn nhiều so với Electron vì Tauri dùng webview có sẵn của hệ điều hành thay vì đóng gói Chromium riêng, (2) giao diện "xịn" dễ đạt được nhờ hệ sinh thái React + Tailwind CSS + shadcn/ui vốn có sẵn theming Sáng/Tối, và (3) đa ngôn ngữ tối ưu nhờ react-i18next, thư viện i18n trưởng thành nhất cho React, tách toàn bộ văn bản hiển thị ra file JSON riêng theo ngôn ngữ.

## Technical Context

**Language/Version**: Rust 1.78+ (lõi Tauri, backend) và TypeScript 5.x (giao diện React)

**Primary Dependencies**:
- Backend: Tauri 2.x, `tokio` (async runtime cho tiến trình con), `rusqlite` (lưu lịch sử/hàng đợi), `serde`/`serde_json` (giao tiếp IPC)
- Downloader engine: `yt-dlp` (đóng gói làm sidecar binary theo từng hệ điều hành), `ffmpeg` (đóng gói kèm, dùng để trích xuất/chuyển mã âm thanh)
- Frontend: React 18, Vite, Tailwind CSS, shadcn/ui (bộ component có sẵn dark/light theming), `react-i18next` (đa ngôn ngữ), Zustand (state nhẹ cho hàng đợi/tiến trình)

**Storage**: SQLite cục bộ (file `.db` trong thư mục dữ liệu ứng dụng của hệ điều hành) lưu hàng đợi tải và lịch sử; tệp media đã tải lưu trực tiếp vào thư mục do người dùng chọn trên ổ đĩa.

**Testing**:
- Rust: `cargo test` cho logic hàng đợi, wrapper gọi yt-dlp/ffmpeg, truy cập SQLite
- Frontend: Vitest + React Testing Library cho component/unit test
- End-to-end: `tauri-driver` + WebdriverIO cho kịch bản smoke test luồng tải âm thanh (User Story 1)

**Target Platform**: Desktop — Windows 10+, macOS 12+, Linux (Ubuntu/Debian qua AppImage và .deb)

**Project Type**: desktop-app (một dự án Tauri duy nhất gồm backend Rust `src-tauri/` và frontend React `src/`)

**Performance Goals**: Khởi động nguội dưới 2 giây; thao tác UI phản hồi dưới 100ms; tốc độ tải bị giới hạn bởi băng thông mạng và nền tảng nguồn, không bởi overhead của ứng dụng (đáp ứng SC-001).

**Constraints**: Bộ cài đặt lõi ứng dụng nhỏ (~15-30MB không tính binary đóng gói); có kèm yt-dlp (~20-30MB) và ffmpeg (~60-100MB tuỳ hệ điều hành) dưới dạng sidecar **đóng gói sẵn ngay trong trình cài đặt** — người dùng KHÔNG tự tải/cài/cấu hình ffmpeg hay yt-dlp theo bất kỳ cách nào (FR-018, SC-008); bộ nhớ RAM khi chạy nền dưới 200MB; không thực hiện phá khoá DRM hay bỏ qua xác thực (FR-012); mọi tuỳ chọn chất lượng/định dạng hiển thị PHẢI dựng động từ metadata thực tế lấy về qua `preview_media`, không viết cứng danh sách tuỳ chọn trong code (FR-019, xem `research.md` §8).

**Scale/Scope**: Ứng dụng đơn người dùng, chạy cục bộ; hàng đợi xử lý đồng thời tối đa vài chục tác vụ; hỗ trợ tối thiểu 6 nền tảng nguồn theo FR-014, kiến trúc plugin của yt-dlp cho phép mở rộng thêm nền tảng mà không cần sửa code ứng dụng.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

`.specify/memory/constitution.md` hiện chỉ chứa các placeholder mẫu (`[PROJECT_NAME]`, `[PRINCIPLE_1_NAME]`, ...), chưa có bản hiến pháp dự án nào được phê chuẩn (ratified). Do đó không có gate cụ thể nào để đối chiếu ở thời điểm này — bước này được coi là **PASS (không áp dụng)**. Khuyến nghị: chạy `/speckit-constitution` sau khi có bản kế hoạch này nếu muốn thiết lập các nguyên tắc ràng buộc lâu dài (ví dụ: bắt buộc test-first, giới hạn kích thước bundle, yêu cầu không phá DRM) cho các tính năng sau này.

*Re-check sau Phase 1*: Không có thay đổi — vẫn PASS (không áp dụng), vì chưa có constitution nào được phê chuẩn.

## Project Structure

### Documentation (this feature)

```text
specs/001-media-downloader-desktop/
├── plan.md              # This file (/speckit-plan command output)
├── research.md          # Phase 0 output
├── data-model.md         # Phase 1 output
├── quickstart.md         # Phase 1 output
├── contracts/            # Phase 1 output (Tauri IPC command contract)
└── tasks.md              # Phase 2 output (/speckit-tasks command - NOT created by /speckit-plan)
```

### Source Code (repository root)

```text
src-tauri/                       # Backend Rust (lõi Tauri)
├── src/
│   ├── main.rs                  # Điểm khởi động Tauri, đăng ký commands
│   ├── commands/                # Tauri IPC command handlers
│   │   ├── download.rs          # start/pause/cancel/retry một tác vụ tải
│   │   ├── queue.rs             # quản lý hàng đợi, trạng thái tác vụ
│   │   ├── history.rs           # đọc/ghi lịch sử tải
│   │   └── settings.rs          # theme, ngôn ngữ, thư mục lưu mặc định
│   ├── downloader/               # Wrapper gọi tiến trình con yt-dlp/ffmpeg
│   │   ├── ytdlp.rs              # build lệnh, parse tiến trình (%, tốc độ, ETA)
│   │   └── ffmpeg.rs              # chuyển đổi/trích xuất âm thanh MP3
│   ├── db/                        # Truy cập SQLite (lịch sử, hàng đợi, cài đặt)
│   └── i18n_meta.rs                # (nếu cần) siêu dữ liệu ngôn ngữ phía backend
├── binaries/                       # yt-dlp + ffmpeg tải sẵn theo từng hệ điều hành lúc build (Tauri "externalBin"),
│                                     # đóng gói thẳng vào trình cài đặt — người dùng không tự cài đặt các công cụ này
└── tests/                           # cargo test: unit + integration cho downloader/db

src/                               # Frontend React (giao diện)
├── components/
│   ├── DownloadForm.tsx           # nhập URL, chọn audio/video, chất lượng
│   ├── QueueList.tsx               # danh sách tác vụ đang tải + tiến trình
│   ├── HistoryList.tsx              # lịch sử tải, mở thư mục, thử lại
│   ├── ThemeToggle.tsx               # chuyển Sáng/Tối
│   └── LanguageSwitcher.tsx          # chuyển ngôn ngữ
├── pages/
│   ├── Home.tsx
│   ├── History.tsx
│   └── Settings.tsx
├── locales/
│   ├── en.json                       # văn bản giao diện Tiếng Anh
│   └── vi.json                       # văn bản giao diện Tiếng Việt
├── stores/                            # Zustand: queueStore, themeStore, i18nStore
├── styles/                             # Tailwind config + design tokens Sáng/Tối
└── App.tsx

tests/
├── unit/                                # Vitest cho components/stores
├── integration/                          # gọi yt-dlp thật với 1 video test công khai, xác nhận job hoàn tất
└── e2e/                                    # tauri-driver + WebdriverIO smoke test luồng P1
```

**Structure Decision**: Một dự án Tauri duy nhất (không tách backend/frontend thành hai ứng dụng độc lập) — `src-tauri/` chứa toàn bộ logic nghiệp vụ (hàng đợi, gọi yt-dlp/ffmpeg, SQLite), `src/` chỉ chứa giao diện và gọi các lệnh backend qua Tauri IPC. Cấu trúc này phù hợp vì đây là một ứng dụng desktop độc lập, không có dịch vụ backend từ xa nào cần tách riêng.

## Complexity Tracking

*Không có vi phạm Constitution Check nào cần biện minh (constitution chưa được phê chuẩn).*
