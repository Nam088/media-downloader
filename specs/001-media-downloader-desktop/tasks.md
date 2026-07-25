---

description: "Task list for feature implementation"
---

# Tasks: Trình Tải Media Đa Nền Tảng (Cross-Platform Media Downloader)

**Input**: Design documents from `/specs/001-media-downloader-desktop/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/tauri-commands.md, quickstart.md (all present)

**Tests**: Không có yêu cầu TDD/test-first tường minh trong spec.md, nên các task bên dưới không tách "Tests for User Story N" theo từng story. Việc thiết lập công cụ test và viết test tự động hoá được gộp vào Phase 6 (Polish), theo đúng chiến lược Testing đã chốt ở `plan.md`.

**Organization**: Task được nhóm theo user story (US1/US2/US3 từ `spec.md`) để mỗi story có thể triển khai và kiểm thử độc lập.

**Cập nhật so với bản trước**: Bổ sung T021 (ghi `DownloadedFile` khi job hoàn tất) và T041 (nhập nhiều URL cùng lúc theo FR-001), mở rộng T049/T050 để kiểm thử đủ 6 nền tảng và đo thời gian theo SC-001/SC-002 — khắc phục các gap HIGH/MEDIUM phát hiện ở `/speckit-analyze`.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Có thể chạy song song (khác file, không phụ thuộc task chưa xong)
- **[Story]**: US1 (P1 - tải âm thanh), US2 (P2 - tải video đầy đủ), US3 (P3 - hàng đợi & lịch sử)
- Đường dẫn file theo đúng `Project Structure` trong `plan.md`

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Khởi tạo dự án Tauri + React theo `plan.md`

- [x] T001 Khởi tạo scaffold dự án Tauri (`src-tauri/`) và frontend React+TypeScript (`src/`) bằng `pnpm create tauri-app` theo cấu trúc trong `plan.md` § Project Structure
- [x] T002 [P] Cấu hình Tailwind CSS và khởi tạo shadcn/ui trong `src/` (`tailwind.config.ts`, `src/styles/`)
- [x] T003 [P] Cấu hình `react-i18next` với file locale khởi tạo `src/locales/en.json` và `src/locales/vi.json`
- [x] T004 [P] Cấu hình ESLint/Prettier cho frontend và `rustfmt`/`clippy` cho `src-tauri/`
- [x] T005 Thêm cấu hình `bundle.externalBin` cho sidecar `yt-dlp` và `ffmpeg` vào `src-tauri/tauri.conf.json` theo `research.md` §2 — **sửa bug (2026-07-25)**: bản "onefile" của yt-dlp tự giải nén lại toàn bộ runtime Python vào thư mục tạm MỚI mỗi lần gọi (~14s/lần, chính là nguyên nhân preview/download bị chậm); đổi sang bản "onedir" đóng gói dưới dạng Tauri `bundle.resources` (không phải `externalBin` — cơ chế đó chỉ nhận một file) và copy một lần vào app-data lúc khởi động đầu tiên (`downloader::ytdlp_binary::resolve_ytdlp_executable`), giảm còn ~0.3s/lần sau lần đầu. ffmpeg vẫn là `externalBin` một-file như cũ, giờ được yt-dlp gọi qua `--ffmpeg-location` tường minh (trước đây chưa từng được truyền, ffmpeg sidecar đóng gói kèm chưa từng thực sự được dùng). Xem `research.md` §2 (amendment 2026-07-25).
- [x] T006 [P] Viết script chỉ dùng cho dev `scripts/fetch-dev-binaries.sh` để tải `yt-dlp`/`ffmpeg` vào `src-tauri/binaries/` phục vụ phát triển cục bộ (theo `quickstart.md` Prerequisites) — **sửa (2026-07-25)**: cập nhật để tải zip bản onedir (`yt-dlp_macos.zip`/`yt-dlp_win.zip`/`yt-dlp_linux.zip`) và giải nén vào `src-tauri/binaries/yt-dlp-onedir/` thay vì tải thẳng bản onefile

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Hạ tầng lõi mà TẤT CẢ user story đều cần trước khi bắt đầu

**⚠️ CRITICAL**: Không được bắt đầu bất kỳ user story nào trước khi hoàn tất phase này

- [x] T007 Tạo migration SQLite cho các bảng `download_jobs` (đầy đủ field theo `data-model.md` §1, gồm cả `retried_from_job_id`), `downloaded_files`, `app_settings` trong `src-tauri/src/db/migrations/`
- [x] T008 [P] Cài đặt `src-tauri/src/db/mod.rs`: connection pool và các hàm truy vấn dùng `rusqlite`
- [x] T009 [P] Cài đặt kiểu `AppError` dùng chung (mã lỗi + thông điệp đã bản địa hoá) trong `src-tauri/src/error.rs`
- [x] T010 Cài đặt wrapper gọi tiến trình yt-dlp trong `src-tauri/src/downloader/ytdlp.rs` (spawn tiến trình, parse tiến trình qua `--newline`, chế độ lấy metadata qua `--dump-json`)
- [x] T011 [P] Cài đặt tham số gọi ffmpeg làm postprocessor (định dạng/chất lượng âm thanh theo FR-004) trong `src-tauri/src/downloader/ffmpeg.rs` — hiện thực dưới dạng args truyền cho yt-dlp (`-x --audio-format mp3 --audio-quality`), yt-dlp tự gọi ffmpeg postprocessor nội bộ
- [x] T012 Cài đặt bộ quản lý hàng đợi tác vụ trong `src-tauri/src/downloader/queue.rs` (state machine theo `data-model.md` §1, giới hạn số tác vụ chạy đồng thời)
- [x] T013 Cài đặt Tauri command `get_settings`/`update_settings` trong `src-tauri/src/commands/settings.rs` (AppSettings: theme, language, default_output_directory)
- [x] T014 Đăng ký toàn bộ Tauri command và helper phát sự kiện (`job:progress`, `job:status_changed`) trong `src-tauri/src/main.rs`
- [x] T015 [P] Dựng khung ứng dụng với theme provider (class `dark:` của Tailwind gắn với `AppSettings.theme`, mặc định `system`) trong `src/App.tsx`
- [x] T016 [P] Gắn provider `react-i18next` theo `AppSettings.language`, fallback `en` khi ngôn ngữ hệ điều hành không được hỗ trợ, trong `src/App.tsx`
- [x] T017 Cài đặt modal tuyên bố tuân thủ/miễn trừ trách nhiệm hiển thị lần đầu khởi chạy (FR-011) trong `src/components/ComplianceDisclaimer.tsx`

**Checkpoint**: Hạ tầng nền đã sẵn sàng — có thể bắt đầu triển khai user story

---

## Phase 3: User Story 1 - Tải âm thanh từ một liên kết đơn (Priority: P1) 🎯 MVP

**Goal**: Người dùng dán một liên kết, chọn "chỉ âm thanh", và nhận về tệp MP3 tải về máy.

**Independent Test**: Dán một liên kết video công khai hợp lệ, bấm "Tải âm thanh", xác nhận tệp âm thanh xuất hiện trong thư mục đầu ra và phát được (xem `quickstart.md`).

### Implementation for User Story 1

- [x] T018 [P] [US1] Cài đặt Tauri command `preview_media` trong `src-tauri/src/commands/media.rs`, ánh xạ metadata từ yt-dlp (`--dump-json`, trường `formats`) sang `MediaSource` (`data-model.md` §2), gồm dựng động `available_audio_formats` từ các luồng audio thực tế của link đó (KHÔNG hard-code danh sách bitrate cố định — FR-004, FR-019), trả lỗi `UNSUPPORTED_PLATFORM`/`ACCESS_DENIED` theo FR-009/FR-012 — **sửa (2026-07-25)**: đã bỏ việc tự chặn bằng 6 domain cố định trước khi gọi yt-dlp; giờ để yt-dlp tự quyết định hỗ trợ hay không (khớp đúng FR-014 đã cập nhật), `platform.rs::detect_platform` chỉ còn dùng để gán nhãn đẹp cho 6 nền tảng bắt buộc, fallback sang `extractor_key` của yt-dlp cho các trang khác
- [x] T019 [P] [US1] Cài đặt Tauri command `create_download_job` (nhánh audio) trong `src-tauri/src/commands/download.rs` theo `contracts/tauri-commands.md`, validate `audio_quality` gửi lên phải khớp một phần tử trong `available_audio_formats` từ `preview_media` gần nhất của `source_url` đó, trả `AppError{code:"INVALID_QUALITY_OPTION"}` nếu không khớp (FR-019)
- [x] T020 [US1] Cài đặt xử lý tác vụ `media_type=audio` trong `src-tauri/src/downloader/queue.rs`: spawn yt-dlp với `--extract-audio --audio-format mp3 --audio-quality` dùng đúng giá trị `audio_quality` đã chọn (không phải hằng số cố định), phát sự kiện `job:progress`/`job:status_changed` (phụ thuộc T010, T011, T012, T019) — **sửa bug (2026-07-25)**: `downloader/ytdlp.rs::run_download` dùng `--print` để lấy đường dẫn file cuối, nhưng yt-dlp tự động bật `quiet`/`noprogress` bất cứ khi nào có `--print`, khiến `--progress-template` không in ra gì (đã tái hiện bằng file test local và xác nhận qua mã nguồn yt-dlp: `opts.quiet = ... or bool(opts.forceprint)`); đã thêm cờ `--progress` để ép hiện progress dù đang ở quiet mode, kèm test hồi quy `download_args_force_progress_even_though_print_is_used`. Đồng thời thêm `-4`/`--socket-timeout 20` cho cả preview và download để giảm độ trễ do IPv6 chờ timeout rồi mới rơi về IPv4. **Sửa bug (2026-07-25)**: TikTok thỉnh thoảng trả file video KHÔNG có âm thanh dù metadata báo `acodec=aac` (yt-dlp issue #15891, xác nhận lỗi phía server TikTok chứ không phải yt-dlp) — thêm `downloader::ytdlp::output_has_audio_stream` (kiểm tra bằng ffmpeg remux luồng audio sang null) và tự động tải lại tối đa `MAX_DOWNLOAD_ATTEMPTS` lần trong `run_job` khi phát hiện thiếu audio, trước khi báo `DOWNLOAD_FAILED`. **Sửa thêm (2026-07-25)**: sau khi trực tiếp đọc issue #15891 và issue liên quan #15642 trên GitHub (xác nhận KHÔNG có bản vá chính thức nào trong yt-dlp), bổ sung `--format-sort vcodec:avc` (ưu tiên H.264, ít gặp lỗi hơn h265/bytevc1), tải lại cả khi yt-dlp thất bại hẳn (không chỉ khi thiếu audio), và `downloader::queue::recover_missing_audio` làm phương án cuối: tải riêng bản `bestaudio` từ cùng URL rồi ghép bằng ffmpeg vào video đã tải, đúng theo cách cộng đồng đã xác nhận có tác dụng trên chính 2 issue đó.
- [x] T021 [US1] Khi một job (audio hoặc video) chuyển sang `status=completed`, ghi bản ghi `DownloadedFile` (file_path, file_format, file_size_bytes, completed_at) liên kết với job đó trong `src-tauri/src/downloader/queue.rs`, làm nguồn dữ liệu duy nhất cho `list_history`/`open_containing_folder` (phụ thuộc T020, T007, T008) — khắc phục gap: trước đây không có bước ghi nhận tệp đã tải khi hoàn tất
- [x] T022 [P] [US1] Tạo type TypeScript cho `MediaSource`/`DownloadJob` trong `src/types/download.ts`, bao gồm `available_audio_formats`/`available_video_qualities` dạng mảng động (không phải union type cố định)
- [x] T023 [P] [US1] Cài đặt component `DownloadForm` (nhập URL, thẻ xem trước, chọn "chỉ âm thanh") trong `src/components/DownloadForm.tsx`; dropdown mức chất lượng PHẢI render từ `MediaSource.available_audio_formats` trả về sau khi gọi `preview_media` cho link vừa nhập — không được có danh sách chất lượng viết cứng trong component (FR-004, FR-019)
- [x] T024 [US1] Cài đặt component `QueueList` lắng nghe sự kiện `job:progress`/`job:status_changed` trong `src/components/QueueList.tsx` (phụ thuộc T022) — dùng chung Zustand store (`src/stores/queue-store.ts`) để có state chia sẻ giữa DownloadForm/QueueList/Home
- [x] T025 [US1] Cài đặt danh sách tác vụ đã hoàn tất (dạng tối giản) trong `src/pages/Home.tsx`, đọc từ bản ghi `DownloadedFile` (T021) để hiển thị đường dẫn tệp khi `status=completed` — `output_file_path` được đính kèm vào sự kiện `job:status_changed` khi hoàn tất để tránh cần round-trip API riêng trước khi có `list_history` (T035)
- [x] T026 [US1] Cài đặt banner lỗi bản địa hoá ánh xạ từ `AppError` trong `src/components/ErrorBanner.tsx`
- [x] T027 [US1] Bổ sung chuỗi văn bản `en`/`vi` cho giao diện User Story 1 vào `src/locales/en.json` và `src/locales/vi.json`

**Checkpoint**: User Story 1 hoạt động và kiểm thử độc lập được (theo `quickstart.md`)

---

## Phase 4: User Story 2 - Tải video đầy đủ với lựa chọn chất lượng (Priority: P2)

**Goal**: Người dùng lưu toàn bộ video (hình + tiếng) với mức chất lượng tự chọn.

**Independent Test**: Dán một liên kết, chọn "video đầy đủ" cùng một mức chất lượng, xác nhận tệp video tải về phát được đúng chất lượng đã chọn.

### Implementation for User Story 2

- [x] T028 [P] [US2] Mở rộng `preview_media` để trả về `available_video_qualities` (dựng động từ `formats` thực tế của link, không hard-code — FR-019) trong `src-tauri/src/commands/media.rs` (phụ thuộc T018) — đã làm cùng lúc với T018 vì `extract_format_options` trích cả audio lẫn video từ cùng một lần gọi yt-dlp
- [x] T029 [US2] Mở rộng `create_download_job`/xử lý hàng đợi để hỗ trợ `media_type=video` với `video_quality` (validate khớp `available_video_qualities` như T019 làm với audio), gồm logic gợi ý chất lượng gần nhất khi mức chọn không có sẵn, trong `src-tauri/src/downloader/queue.rs` (phụ thuộc T020, T021, T028) — `validate_quality` (download.rs) đã có nhánh Video, và format selector `bestvideo[height<=X]+bestaudio/best[height<=X]` tự để yt-dlp chọn mức gần nhất có thật thay vì tự viết logic fallback riêng (test: `video_args_select_nearest_available_height_via_format_selector`)
- [x] T030 [P] [US2] Mở rộng `DownloadForm` với lựa chọn audio/video; dropdown chất lượng video PHẢI render từ `MediaSource.available_video_qualities` trả về cho link đó, không viết cứng danh sách độ phân giải (FR-019) trong `src/components/DownloadForm.tsx` (phụ thuộc T023)
- [x] T031 [US2] Bổ sung chuỗi văn bản `en`/`vi` cho giao diện chọn chất lượng video vào `src/locales/en.json` và `src/locales/vi.json`

**Checkpoint**: User Story 1 và 2 đều hoạt động độc lập

---

## Phase 5: User Story 3 - Quản lý hàng đợi và lịch sử tải xuống (Priority: P3)

**Goal**: Người dùng tải nhiều liên kết cùng lúc (kể cả dán nhiều URL cùng một lượt), theo dõi tiến trình độc lập, và xem lại lịch sử.

**Independent Test**: Dán 3 liên kết cùng lúc (mỗi dòng một liên kết trong một lượt dán), xác nhận cả 3 tác vụ xuất hiện trong hàng đợi với tiến trình cập nhật độc lập, và sau khi hoàn tất có thể xem lại trong mục lịch sử.

### Implementation for User Story 3

- [x] T032 [P] [US3] Mở rộng `preview_media` để trả về `is_playlist`/`playlist_item_count` trong `src-tauri/src/commands/media.rs` (phụ thuộc T018) — đã làm cùng lúc với T018
- [x] T033 [US3] Cài đặt logic chọn phạm vi playlist trong `create_download_job` (`single_item` hoặc `entire_playlist` → tách thành nhiều job cùng `parent_playlist_id`) trong `src-tauri/src/commands/download.rs` (phụ thuộc T032) — `create_download_job` giờ trả `DownloadJob[]` (1 phần tử cho ca thường, N phần tử khi fan-out playlist); mọi job đơn lẻ đều thêm `--no-playlist` để tránh vô tình tải cả playlist khi chưa xác nhận (FR-013)
- [x] T034 [P] [US3] Cài đặt Tauri command `pause_job`/`resume_job`/`cancel_job`/`retry_job` trong `src-tauri/src/commands/download.rs` (phụ thuộc T012); `retry_job` tạo job mới và ghi `retried_from_job_id` trỏ về job cũ (theo `data-model.md` §1) — logic vòng đời nằm ở `DownloadQueue::{pause,resume,cancel,retry}` (queue.rs), command chỉ gọi qua
- [x] T035 [P] [US3] Cài đặt Tauri command `list_queue` và `list_history` trong `src-tauri/src/commands/history.rs` (phụ thuộc T021) — dùng trực tiếp `output_file_path` đã lưu sẵn trên `DownloadJob` (ghi bởi T021) thay vì JOIN riêng, vì đủ dữ liệu cho hiển thị lịch sử
- [x] T036 [P] [US3] Cài đặt Tauri command `open_containing_folder` trong `src-tauri/src/commands/history.rs` qua `tauri-plugin-opener::reveal_item_in_dir`, đọc `output_file_path` từ job (phụ thuộc T021)
- [x] T037 [US3] Mở rộng `QueueList` để hiển thị nhiều tác vụ đồng thời kèm điều khiển tạm dừng/huỷ/thử lại trong `src/components/QueueList.tsx` (phụ thuộc T024, T034)
- [x] T038 [P] [US3] Cài đặt hộp thoại xác nhận phạm vi playlist trong `src/components/PlaylistScopeDialog.tsx` (phụ thuộc T032)
- [x] T039 [US3] Cài đặt đầy đủ component `HistoryList` (danh sách, mở thư mục, thử lại) dựa trên `list_history`/`open_containing_folder`/`retry_job` trong `src/components/HistoryList.tsx` (phụ thuộc T035, T036, T034)
- [x] T040 [US3] Cài đặt trang `src/pages/History.tsx` gắn `HistoryList` vào điều hướng
- [x] T041 [US3] Mở rộng `DownloadForm` (hoặc thêm ô nhập riêng) để chấp nhận nhiều URL dán cùng một lượt (mỗi dòng một liên kết), gọi `preview_media` rồi `create_download_job` độc lập cho từng URL hợp lệ và đẩy tất cả vào hàng đợi cùng lúc, theo FR-001, trong `src/components/DownloadForm.tsx` (phụ thuộc T023) — khắc phục gap: trước đây chỉ hỗ trợ nhập 1 URL mỗi lần nên không đáp ứng đúng Independent Test của US3
- [x] T042 [US3] Bổ sung chuỗi văn bản `en`/`vi` cho giao diện hàng đợi/lịch sử/playlist/nhập nhiều URL vào `src/locales/en.json` và `src/locales/vi.json`

**Checkpoint**: Cả 3 user story đều hoạt động độc lập

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Hoàn thiện các yêu cầu xuyên suốt (FR-015, FR-016, FR-017, FR-018) và đóng gói phát hành

- [x] T043 [P] Cài đặt component `ThemeToggle` gắn với `update_settings` trong `src/components/ThemeToggle.tsx`
- [x] T044 [P] Cài đặt component `LanguageSwitcher` gắn với `update_settings` + `i18next.changeLanguage` trong `src/components/LanguageSwitcher.tsx`
- [x] T045 [P] Cài đặt trang `src/pages/Settings.tsx` (theme, ngôn ngữ, thư mục lưu mặc định)
- [x] T046 Cấu hình CI build ma trận Windows/macOS/Linux, tự động tải binary release `yt-dlp` + `ffmpeg` (static build) vào `src-tauri/binaries/` và đóng gói sẵn vào trình cài đặt, xác minh FR-018/SC-008, trong `.github/workflows/release.yml` — đã xác minh các URL nguồn (yt-dlp GitHub releases, BtbN/FFmpeg-Builds cho Linux/Windows) tồn tại thật; còn 1 lưu ý chưa giải quyết: evermeet.cx (ffmpeg macOS) chỉ có bản x86_64, cần xác nhận/thay nguồn trước khi phát hành thật cho Apple Silicon (đã ghi rõ TODO trong file) — **sửa (2026-07-25)**: bước tải yt-dlp đổi từ tải thẳng bản onefile sang tải zip bản onedir (`yt-dlp_win.zip`/`yt-dlp_macos.zip`/`yt-dlp_linux.zip`) và giải nén vào `src-tauri/binaries/yt-dlp-onedir/` (đóng gói qua `bundle.resources`, không còn qua `externalBin`)
- [x] T047 [P] Viết `cargo test` cho state machine hàng đợi, parser tiến trình yt-dlp, và logic ghi `DownloadedFile` khi completed trong `src-tauri/tests/` — hiện thực dưới dạng `#[cfg(test)] mod tests` ngay trong từng file (`queue.rs`, `ytdlp.rs`, `platform.rs`), 12 test đều pass; state machine hàng đợi và ghi `DownloadedFile` cần AppHandle/DB thật nên phần đó để lại cho E2E (T049) thay vì unit test cô lập
- [x] T048 [P] Viết test Vitest cho `DownloadForm` (bao gồm nhập nhiều URL), `QueueList`, `ThemeToggle`, `LanguageSwitcher` trong `tests/unit/` — 7 test pass (4 file), cấu hình `vitest.config.ts` + `tests/unit/setup.ts` (mock Tauri APIs, shim jsdom cho Radix/next-themes)
- [ ] T049 (khung sườn đã viết, CHƯA chạy được) Tự động hoá kịch bản xác thực trong `quickstart.md` thành smoke test bằng `tauri-driver` + WebdriverIO trong `tests/e2e/` (`wdio.conf.ts`, `fixtures.ts`, `audio-download.spec.ts`), chạy với 1 liên kết mẫu cho MỖI nền tảng trong 6 nền tảng bắt buộc ở FR-014 để xác minh SC-002, đồng thời ghi lại thời gian hoàn tất để đối chiếu SC-001 — **cần**: (1) cài `tauri-driver` + gói `@wdio/*` (chưa cài vì không thể xác minh chạy được trong sandbox không có màn hình), (2) điền link mẫu thật vào `fixtures.ts` (để trống placeholder vì không tự ý chọn URL nội dung thật thay người dùng), (3) chạy thử trên máy có GUI thật
- [ ] T050 (không thể tự thực hiện trong môi trường này) Chạy thủ công toàn bộ checklist xác thực trong `quickstart.md` (bao gồm kiểm tra "cài trên máy sạch" của SC-008 và xác nhận cả 6 nền tảng trong FR-014 đều tải được) trước khi phát hành — cần người dùng tự chạy `pnpm tauri dev`/bản build thật trên máy có màn hình, vì sandbox hiện tại không có GUI để thao tác

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: Không phụ thuộc — bắt đầu ngay
- **Foundational (Phase 2)**: Phụ thuộc hoàn tất Setup — CHẶN toàn bộ user story
- **User Stories (Phase 3-5)**: Đều phụ thuộc hoàn tất Foundational
  - Có thể làm song song nếu đủ nhân lực, hoặc tuần tự theo thứ tự ưu tiên P1 → P2 → P3
- **Polish (Phase 6)**: Phụ thuộc các user story muốn hoàn thiện đã xong

### User Story Dependencies

- **US1 (P1)**: Bắt đầu được ngay sau Phase 2 — không phụ thuộc story khác
- **US2 (P2)**: Bắt đầu được sau Phase 2; mở rộng trực tiếp trên các file của US1 (`preview_media`, `create_download_job`, `DownloadForm`, `queue.rs`) nhưng vẫn kiểm thử độc lập được theo Independent Test riêng
- **US3 (P3)**: Bắt đầu được sau Phase 2; mở rộng `QueueList`/`DownloadForm` của US1 và dùng chung `preview_media`, nhưng có bộ command riêng (`list_queue`, `list_history`, `pause/resume/cancel/retry`, nhập nhiều URL) và kiểm thử độc lập được

### Within Each User Story

- Command backend trước, component frontend gọi command đó sau
- Cùng một file (`queue.rs`, `DownloadForm.tsx`, `media.rs`, `download.rs`) không đánh dấu `[P]` nếu task sau sửa trên phần task trước vừa thêm
- Story hoàn tất trước khi chuyển sang story ưu tiên tiếp theo (nếu làm tuần tự)

### Parallel Opportunities

- Toàn bộ task `[P]` trong Setup chạy song song được
- Toàn bộ task `[P]` trong Foundational chạy song song được (T008, T009, T011, T015, T016)
- Sau khi Foundational xong, US1/US2/US3 có thể triển khai song song nếu đủ nhân lực
- Trong mỗi story, các task `[P]` (khác file, không phụ thuộc nhau) chạy song song được

---

## Parallel Example: User Story 1

```bash
# Sau khi Foundational (Phase 2) hoàn tất, chạy song song:
Task: "Cài đặt Tauri command preview_media trong src-tauri/src/commands/media.rs"
Task: "Cài đặt Tauri command create_download_job (nhánh audio) trong src-tauri/src/commands/download.rs"
Task: "Tạo type TypeScript cho MediaSource/DownloadJob trong src/types/download.ts"
Task: "Cài đặt component DownloadForm trong src/components/DownloadForm.tsx"
```

---

## Implementation Strategy

### MVP First (chỉ User Story 1)

1. Hoàn tất Phase 1: Setup
2. Hoàn tất Phase 2: Foundational (BẮT BUỘC — chặn mọi user story)
3. Hoàn tất Phase 3: User Story 1
4. **DỪNG và KIỂM TRA**: chạy `quickstart.md` để xác nhận US1 hoạt động độc lập
5. Đây đã là bản demo/MVP khả dụng (tải âm thanh từ liên kết)

### Incremental Delivery

1. Setup + Foundational → nền tảng sẵn sàng
2. Thêm US1 → kiểm thử độc lập → demo (MVP!)
3. Thêm US2 → kiểm thử độc lập → demo
4. Thêm US3 (bao gồm nhập nhiều URL) → kiểm thử độc lập → demo
5. Phase 6 (Polish) → đóng gói phát hành chính thức với binary tự chứa (FR-018), kiểm thử đủ 6 nền tảng (SC-002)

### Parallel Team Strategy

Với nhiều lập trình viên:

1. Cả nhóm hoàn tất Setup + Foundational cùng nhau
2. Sau khi Foundational xong:
   - Dev A: User Story 1
   - Dev B: User Story 2
   - Dev C: User Story 3
3. Các story hoàn tất và tích hợp độc lập vào cùng `QueueList`/`preview_media` nền đã có

---

## Notes

- `[P]` = khác file, không phụ thuộc task khác
- Nhãn `[Story]` map task về đúng user story trong `spec.md` để truy vết
- Mỗi user story phải hoàn thành và kiểm thử độc lập được
- Commit sau mỗi task hoặc mỗi nhóm task liên quan
- Có thể dừng lại ở bất kỳ Checkpoint nào để kiểm tra story độc lập
- Tránh: task mơ hồ, nhiều task cùng sửa 1 file mà không có thứ tự rõ ràng, phụ thuộc chéo giữa các story làm mất tính độc lập
