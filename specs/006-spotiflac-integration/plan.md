# Implementation Plan: Tích Hợp SpotiFLAC Tải Nhạc Lossless FLAC

**Branch**: `006-spotiflac-integration` | **Date**: 2026-07-27 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/006-spotiflac-integration/spec.md`

## Summary

Tích hợp SpotiFLAC (pip module `SpotiFLAC` v1.5.5, repo `BartolomeoRusso9/SpotiFLAC-Module-Version`) làm **engine tải nhạc thứ ba** bên cạnh `yt-dlp` và `gallery-dl`, chuyên tải FLAC lossless từ link Spotify/Tidal/Apple Music mà không cần tài khoản. Cách tiếp cận kỹ thuật: đóng gói một **Python worker tự viết** (`spotiflac_worker.py`, bọc quanh Python API của module) bằng PyInstaller `--onedir` — đúng theo tiền lệ `gallery-dl-onedir` hiện có — giao tiếp với Rust qua **giao thức sentinel JSON-line trên stdout/stdin**. Backend thêm `MediaType::Music`, nhánh routing URL trước yt-dlp trong `preview_media`, nhánh `run_music_job` trong queue, migration 0013 (mở rộng CHECK constraint + trạng thái `waiting_input` cho luồng Cloudflare grant). Frontend thêm tier chất lượng (FLAC 16-bit / 24-bit Hi-Res / MP3 320), mục Settings cấu hình thứ tự provider + Telegram Bot, và dialog nhập grant code khi gặp Cloudflare challenge.

## Technical Context

**Language/Version**: Rust (Tauri 2, edition 2021) cho backend; TypeScript + React 19 cho frontend; Python 3.11+ chỉ ở build-time (PyInstaller đóng gói worker)

**Primary Dependencies**: Tauri 2, rusqlite + rusqlite_migration, tokio; React 19, zustand, i18next, Radix UI; pip `SpotiFLAC==1.5.5` (+ PyInstaller) cho worker; ffmpeg (đã bundle sẵn) cho transcode MP3 320

**Storage**: SQLite (`app_data_dir()/media-downloader.db`) — `download_jobs` (migration 0013: thêm `media_type='music'`, status `waiting_input`), `downloaded_files` (thêm cột `source_provider`), `app_settings` key/value (settings mới không cần migration)

**Testing**: vitest 4 + Testing Library (`tests/unit/`, có guard test locale-parity & no-hardcoded-strings); `cargo test` inline theo module (pattern per-migration test trong `db/mod.rs`)

**Target Platform**: Desktop Windows / macOS / Linux (Tauri bundle; worker onedir build theo matrix trong `release.yml`)

**Project Type**: desktop-app (Tauri 2: React frontend + Rust backend + Python tool bundle)

**Performance Goals**: Progress event mỗi ≤500ms/track khi đang tải; queue concurrency hiện có 1–8 job song song giữ nguyên; preview metadata Spotify track < 5s mạng bình thường

**Constraints**: Không yêu cầu đăng nhập tài khoản nhạc; worker phải chạy offline-bundled (không bắt người dùng cài Python); Node.js là optional (chỉ cho JS Extensions fallback, có phát hiện & thông báo khi thiếu); TG token lưu plaintext trong SQLite (không có secret storage — ghi rõ caveat)

**Scale/Scope**: Ứng dụng desktop 1 người dùng; hàng đợi hàng trăm job (album/playlist tách mỗi bài = 1 job qua cơ chế `parent_playlist_id` hiện có); ~1 engine module Rust mới (~2 file), 1 Python worker, 1 migration, mở rộng 3 màn hình UI (DownloadForm, Queue, Settings)

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

`.specify/memory/constitution.md` hiện vẫn là **template chưa được điền** (toàn placeholder `[PRINCIPLE_*]`), do đó không có gate cụ thể nào để enforce. Áp dụng các nguyên tắc mặc định của repo thay thế:

| Gate | Trạng thái | Ghi chú |
|---|---|---|
| Không sửa migration đã phát hành — chỉ thêm `0013_*.sql` mới + đăng ký trong `migrations()` | PASS (thiết kế tuân thủ) | Kèm test `migration_0013_is_registered...` theo pattern hiện có |
| Locale parity en/vi + không hardcode string UI | PASS (thiết kế tuân thủ) | Mọi key mới thêm đồng thời vào `en.json` và `vi.json` |
| Tuân theo tiền lệ bundle tool Python (onedir + `ensure_cached_onedir`) | PASS | Không phát minh cơ chế sidecar mới |
| Backend không tin quality string từ frontend (`validate_quality`) | PASS | Thêm nhánh validate tier nhạc trong `commands/download.rs` |

**Re-check sau Phase 1**: PASS — thiết kế trong `data-model.md`/`contracts/` không vi phạm gate nào ở trên; không có mục nào cần ghi vào Complexity Tracking.

## Project Structure

### Documentation (this feature)

```text
specs/006-spotiflac-integration/
├── plan.md              # File này (/speckit-plan output)
├── research.md          # Phase 0 output
├── data-model.md        # Phase 1 output
├── quickstart.md        # Phase 1 output
├── contracts/
│   ├── spotiflac-worker-protocol.md   # Giao thức stdout/stdin Rust ↔ Python worker
│   └── tauri-interface.md             # Commands + events + settings mở rộng
├── checklists/requirements.md         # (đã có từ /speckit-checklist)
└── tasks.md             # Phase 2 output (/speckit-tasks — KHÔNG tạo ở bước này)
```

### Source Code (repository root)

```text
src-tauri/
├── binaries/
│   └── spotiflac-onedir/            # NEW (build-time output, gitignored)
├── src/
│   ├── models.rs                    # MediaType::Music, JobStatus::WaitingInput, AppSettings mới
│   ├── platform.rs                  # nhận diện host spotify/tidal/apple-music/pandora
│   ├── error.rs                     # error codes mới (SPOTIFLAC_*)
│   ├── commands/
│   │   ├── media.rs                 # route URL nhạc → preview qua worker TRƯỚC yt-dlp
│   │   ├── download.rs              # validate music tier, tạo job music
│   │   ├── music.rs                 # NEW: submit_cloudflare_grant
│   │   └── settings.rs              # patch các field SpotiFLAC + Telegram
│   ├── downloader/
│   │   ├── spotiflac.rs             # NEW: spawn worker, parse SPOTIFLAC_EVENT::, classify errors
│   │   ├── spotiflac_binary.rs      # NEW: OnceCell + ensure_cached_onedir("spotiflac-onedir",…)
│   │   └── queue.rs                 # nhánh run_music_job tại run_job()
│   └── db/
│       ├── mod.rs                   # đăng ký M::up(0013), method mới
│       └── migrations/0013_music_engine.sql   # NEW: rebuild download_jobs + downloaded_files.source_provider
├── tauri.conf.json                  # bundle.resources += "binaries/spotiflac-onedir"

scripts/
├── build-spotiflac-onedir.sh        # NEW: PyInstaller onedir cho worker
├── spotiflac_worker.py              # NEW: Python worker (preview/download, JSON protocol)
└── fetch-dev-binaries.sh            # thêm bước build/copy spotiflac-onedir cho dev

src/
├── types/download.ts                # MediaType 'music', MusicQualityTier, event types
├── types/settings.ts                # field SpotiFLAC/Telegram
├── lib/build-job-input.ts           # nhánh payload job music
├── lib/url-parsing.ts               # sniff URL nhạc
├── stores/queue-store.ts            # listener job:cloudflare_challenge, provider live
├── components/
│   ├── DownloadForm.tsx             # tier FLAC16/FLAC24/MP3-320 khi media_type=music
│   ├── QueueList.tsx                # hiển thị provider + trạng thái Chờ CAPTCHA
│   └── CloudflareGrantDialog.tsx    # NEW: link mở trình duyệt + ô nhập grant code
├── pages/Settings.tsx               # section SpotiFLAC (provider order, quality, TG bot)
└── locales/{en,vi}.json             # namespaces music/settings/errors mới

tests/
├── unit/                            # test store/component mới (vitest)
└── (Rust) inline #[cfg(test)] trong spotiflac.rs, db/mod.rs, commands/*
```

**Structure Decision**: Giữ nguyên cấu trúc single Tauri app hiện có; SpotiFLAC được thêm như **module engine ngang hàng** trong `src-tauri/src/downloader/` theo đúng hình dạng cặp file của gallery-dl (`gallery_dl.rs` + `gallery_dl_binary.rs`), không refactor sang trait Engine trong scope này (xem research.md §R2).

## Complexity Tracking

> Không có vi phạm Constitution Check nào cần biện minh — bảng bỏ trống.
