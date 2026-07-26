# Tasks: Tích Hợp SpotiFLAC Tải Nhạc Lossless FLAC

**Input**: Design documents from `/specs/006-spotiflac-integration/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/, quickstart.md

**Tests**: Repo có guard tests bắt buộc (locale-parity, no-hardcoded-strings, per-migration tests) — các task test dưới đây là **bắt buộc theo quy ước repo**, không phải TDD tùy chọn.

**Organization**: Task nhóm theo user story; mỗi story là một increment kiểm chứng độc lập được.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Chạy song song được (khác file, không phụ thuộc task chưa xong)
- **[Story]**: US1 / US2 / US3 (map với spec.md)

---

## Phase 1: Setup (Worker & Bundling Infrastructure)

**Purpose**: Dựng worker Python + pipeline đóng gói onedir theo tiền lệ gallery-dl

- [X] T001 Tạo `scripts/spotiflac_worker.py` skeleton: argparse 2 subcommand `preview`/`download` (đúng flags trong `contracts/spotiflac-worker-protocol.md` §1), emitter sentinel `SPOTIFLAC_EVENT::{json}` một dòng, event `hello{protocol:1, module_version}`, redirect toàn bộ output module gốc qua logging capture, exit codes 0/1/2/130
- [X] T002 Tạo `scripts/build-spotiflac-onedir.sh`: venv + `pip install SpotiFLAC==1.5.5 pyinstaller` + `pyinstaller --onedir` → `src-tauri/binaries/spotiflac-onedir/` (tham khảo `scripts/build-gallery-dl-onedir.sh`, thêm hidden-imports hook nếu module import động — kiểm tra `SpotiFLAC/providers/`)
- [X] T003 [P] Thêm bước build/copy `spotiflac-onedir` vào `scripts/fetch-dev-binaries.sh` cho môi trường dev
- [X] T004 [P] Khai báo `"binaries/spotiflac-onedir": "spotiflac-onedir"` vào `bundle.resources` trong `src-tauri/tauri.conf.json`
- [X] T005 [P] Thêm step build spotiflac-onedir vào matrix per-platform trong `.github/workflows/release.yml` (Windows/macOS/Linux, đặt cạnh step gallery-dl)

**Checkpoint**: `spotiflac-worker preview --url <spotify-track>` chạy tay in ra `hello` + `preview_result` (sau khi T010 ở Phase 3 xong phần preview; ở mức Setup chỉ cần `hello` + lỗi tham số chuẩn)

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Migration, models, engine plumbing mà MỌI user story cần

**⚠️ CRITICAL**: Không bắt đầu user story nào trước khi phase này xong

- [X] T006 Tạo migration `src-tauri/src/db/migrations/0013_music_engine.sql`: rebuild `download_jobs` (CHECK `media_type IN ('audio','video','gallery','music')`, CHECK status thêm `'waiting_input'`) + `ALTER TABLE downloaded_files ADD COLUMN source_provider TEXT`; **trước khi viết SQL, xác minh schema hiện tại của `downloaded_files` trong `0001_init.sql`/`0012_library_index.sql` — nếu bảng có CHECK constraint trên `media_type` thì phải rebuild cả bảng này để chấp nhận `'music'`**; đăng ký `M::up(...)` trong `migrations()` tại `src-tauri/src/db/mod.rs`; viết test `migration_0013_is_registered_and_widens_media_type` theo pattern test 0012 (`db/mod.rs`)
- [X] T007 Mở rộng `src-tauri/src/models.rs`: `MediaType::Music` ("music"), `JobStatus::WaitingInput` ("waiting_input") + `as_str`/`from_str` + unit tests; 5 field mới trong `AppSettings` (`spotiflac_service_order`, `spotiflac_quality`, `spotiflac_extensions_fallback`, `tg_bot_token`, `tg_chat_id`) với defaults theo `data-model.md` §4
- [X] T008 [P] Mở rộng TS types: `src/types/download.ts` (`MediaType` thêm `"music"`, type `MusicQualityTier`, `JobStatus` thêm `"waiting_input"`, `JobProgressEvent.provider?`, `JobCloudflareChallengeEvent`, `MediaSource.available_music_tiers?`) và `src/types/settings.ts` (5 field mới)
- [X] T009 [P] Tạo `src-tauri/src/downloader/spotiflac_binary.rs`: `resolve_spotiflac_executable()` dùng `OnceCell` + `bundled_tool::ensure_cached_onedir(app, "spotiflac-onedir", ...)` với tên exe per-OS (theo hình dạng `gallery_dl_binary.rs`); khai báo module trong `src-tauri/src/downloader/mod.rs`
- [X] T010 Tạo `src-tauri/src/downloader/spotiflac.rs` (core engine): structs event khớp protocol (`Hello`, `PreviewResult`, `TrackStart`, `Progress`, `ProviderSwitch`, `CloudflareChallenge`, `TrackDone`, `WorkerError`), hàm `parse_worker_line()` parse sentinel `SPOTIFLAC_EVENT::`, spawn helper (stdout reader + stdin writer handle, env `PYTHONUNBUFFERED`), `classify_spotiflac_error()` map sang codes mới + `NETWORK_ERROR_CODE` (tái dùng `retry.rs`); unit tests cho parser + classifier inline `#[cfg(test)]`
- [X] T011 [P] Thêm error codes vào `src-tauri/src/error.rs`: `SPOTIFLAC_NO_SOURCE`, `SPOTIFLAC_REGION_BLOCKED`, `SPOTIFLAC_CHALLENGE_TIMEOUT`, `SPOTIFLAC_NODE_MISSING` (+ constructors); thêm entry `errors.*` tương ứng vào cả `src/locales/en.json` và `src/locales/vi.json`
- [X] T012 Nối settings backend: `get_settings`/`update_settings` trong `src-tauri/src/db/mod.rs` đọc/ghi 5 key mới (qua `get_setting_or_default` — không cần migration); `apply_patch` + validation (CSV provider whitelist/permutation, tier enum, chat_id chỉ số) trong `src-tauri/src/commands/settings.rs` + unit tests; mở rộng `UpdateSettingsInput`
- [X] T013 [P] Mở rộng `src-tauri/src/platform.rs::detect_platform`: nhận diện host `open.spotify.com`→`spotify`, `listen.tidal.com`→`tidal`, `music.apple.com`→`apple_music`, `pandora.com`/`pandora.app.link`→`pandora`; unit tests; mirror sniff trong `src/lib/url-parsing.ts` (`isMusicUrl()`) + label trong `src/lib/format.ts` `CURATED_PLATFORMS`

**Checkpoint**: `cargo test` + `pnpm test` xanh; migration 0013 chạy trên DB temp; chưa có hành vi người dùng mới

---

## Phase 3: User Story 1 - Tải nhạc FLAC từ liên kết Spotify (Priority: P1) 🎯 MVP

**Goal**: Dán link Spotify (track/album/playlist/artist) → preview → tải FLAC có tag + cover về máy, mỗi bài một job độc lập

**Independent Test**: Dán `https://open.spotify.com/track/...`, chọn FLAC 16-bit, bấm Tải → file `.flac` đúng thư mục, đủ ID3 tags + cover art, phát được (quickstart.md US1)

### Implementation for User Story 1

- [X] T014 [P] [US1] Implement mode `preview` trong `scripts/spotiflac_worker.py`: dùng API metadata của module (`AsyncSpotiFLAC.get_playlist` và tương đương cho track/album/artist — xác minh API thực tế khi code) → emit `preview_result{kind, title, artist, tracks[]}` đúng shape contract §2
- [X] T015 [P] [US1] Implement mode `download` trong `scripts/spotiflac_worker.py`: tải đúng 1 track qua sync/async API, map `--tier` → tham số `quality` per-provider (R3: flac16→LOSSLESS/"6", flac24→HI_RES_LOSSLESS/"27", mp3_320→tải như flac16), `allow_fallback=True`, hook `SpotiFLAC.core.progress`/logging để emit `track_start{provider}` + `progress{percent,speed_bps}` (percent=null nếu không đo được) + `track_done{file_path,provider,bit_depth}`, emit `error{code}` chuẩn khi hết provider
- [X] T016 [US1] Routing preview trong `src-tauri/src/commands/media.rs`: hàm `try_music_preview()` gọi worker `preview` (qua `spotiflac.rs`), đặt **trước** nhánh yt-dlp khi `is_music_url`; map `preview_result` → `MediaSource{media_type: Music, available_music_tiers: [flac16,flac24,mp3_320], entries[]}`; lỗi worker → fallback chuỗi yt-dlp→gallery-dl hiện hành; cache preview cho validate
- [X] T017 [US1] Nhánh validate music trong `src-tauri/src/commands/download.rs::validate_quality`: `media_type=music` yêu cầu `audio_quality ∈ available_music_tiers` của preview cache; từ chối output_options video/subtitle cho job music; đảm bảo `create_playlist_download_jobs` chấp nhận entries music (mỗi track 1 job, `parent_playlist_id`) + unit tests phần thuần
- [X] T018 [US1] `run_music_job()` trong `src-tauri/src/downloader/queue.rs`: nhánh thứ ba tại `run_job()` (cạnh check Gallery hiện có ~dòng 640) — spawn worker `download` với services/tier từ `AppSettings`, stream events → `JobProgressEvent{provider}` qua `job:progress`, `track_done` → `insert_downloaded_file(..., source_provider, media_type="music")` + status `completed`; cancel/pause = lệnh stdin `cancel` + kill-fallback 5s theo cơ chế hiện hành; lỗi → `classify_spotiflac_error` + `decide_outcome` (retry backoff cho NETWORK)
- [X] T019 [US1] Cập nhật `src-tauri/src/db/mod.rs::insert_downloaded_file` nhận `source_provider: Option<String>` (UPSERT giữ cột mới) + test; các call-site engine cũ truyền `None`
- [X] T020 [P] [US1] Frontend form: `src/components/DownloadForm.tsx` render tier picker (3 lựa chọn FLAC 16-bit / FLAC 24-bit Hi-Res / MP3 320) khi `media_type === "music"` (ẩn quality video/gallery); `src/lib/build-job-input.ts` build payload job music (`audio_quality = tier`); default tier từ settings store
- [X] T021 [P] [US1] Frontend queue: `src/stores/queue-store.ts` áp `provider` từ `job:progress` vào `liveProgress`; `src/components/QueueList.tsx` hiển thị nhãn provider đang dùng + tốc độ (FR-009)
- [X] T022 [US1] i18n keys US1 vào cả `src/locales/en.json` + `src/locales/vi.json`: `downloadForm.musicTier.*`, `queue.provider`, label platform nhạc — pass guard `locale-parity` + `no-hardcoded-strings`
- [X] T023 [P] [US1] Vitest: `tests/unit/DownloadForm-music.test.tsx` (tier picker hiện đúng khi preview music, payload đúng tier), cập nhật `tests/unit/queue-store.test.ts` (provider trong progress event), test `isMusicUrl` trong `tests/unit/url-parsing.test.ts` (tạo mới nếu chưa có)

**Checkpoint**: US1 chạy end-to-end theo quickstart.md — MVP giao được

---

## Phase 4: User Story 2 - Cấu hình nguồn phát & chất lượng (Priority: P2)

**Goal**: Người dùng sắp xếp ưu tiên provider (Tidal/Qobuz/Deezer/Amazon), chọn tier mặc định (kể cả MP3 320 qua transcode), bật/tắt JS extensions fallback

**Independent Test**: Đổi ưu tiên sang Qobuz + Hi-Res 24-bit trong Settings, tải 1 bài → provider hiển thị `qobuz`, `ffprobe` cho bit depth 24; provider chính lỗi → tự chuyển nguồn kế (quickstart.md US2)

### Implementation for User Story 2

- [X] T024 [P] [US2] Section SpotiFLAC trong `src/pages/Settings.tsx`: danh sách provider sắp xếp được (lên/xuống, đủ 4 provider), select tier mặc định, toggle extensions fallback — persist qua `updateSettings(patch)` của `src/stores/settings-store.ts` (mở rộng store/types nếu T008 chưa đủ)
- [X] T025 [US2] Truyền cấu hình runtime vào worker trong `queue.rs::run_music_job` + `spotiflac.rs`: build `--services` từ `spotiflac_service_order`, `--tier` từ job, phát hiện `node` trong PATH lúc spawn (`which`-style check trong `spotiflac.rs`) → thêm `--no-extensions-fallback` khi thiếu node hoặc user tắt; thiếu node lần đầu → log + toast cảnh báo `SPOTIFLAC_NODE_MISSING` (không fail job) + unit test cho hàm build args
- [X] T026 [P] [US2] Worker fallback events trong `scripts/spotiflac_worker.py`: emit `provider_switch{from,to,reason}` khi module xoay provider/extension (hook logging của module), truyền `use_extensions_fallback` + services chứa `ext:*` xuống module đúng contract
- [X] T027 [US2] Hậu xử lý MP3 320 trong `queue.rs::run_music_job`: khi tier `mp3_320`, sau `track_done` chạy ffmpeg bundled (`resolve_ffmpeg_path`) transcode `libmp3lame -b:a 320k` giữ tags + cover, xóa FLAC trung gian, ghi `downloaded_files.file_format="mp3"`; unit test cho hàm build ffmpeg args
- [X] T028 [US2] Hiển thị `provider_switch` trong queue/log: append dòng vào `LogBuffer` (`src-tauri/src/logging.rs`) + cập nhật nhãn provider live trong `src/components/QueueList.tsx` khi event đổi provider
- [X] T029 [US2] i18n keys US2 (`settings.spotiflac.*` gồm serviceOrder/quality/extensionsFallback + mô tả, cảnh báo thiếu Node) vào cả `en.json` + `vi.json`
- [X] T030 [P] [US2] Vitest `tests/unit/Settings-spotiflac.test.tsx`: render section, reorder provider gọi đúng patch, toggle fallback; cargo test cho validation `apply_patch` các giá trị CSV sai (bổ sung vào `commands/settings.rs` tests nếu T012 chưa phủ)

**Checkpoint**: US1 + US2 độc lập đều chạy; cấu hình ảnh hưởng đúng job mới

---

## Phase 5: User Story 3 - Xử lý Cloudflare CAPTCHA (Priority: P3)

**Goal**: Challenge Cloudflare không giết job: auto-solver → Telegram (nếu cấu hình) → dialog in-app nhập grant code, job tự nối lại

**Independent Test**: Giả lập challenge (mock worker script emit `cloudflare_challenge`) → job sang trạng thái Chờ CAPTCHA, dialog hiện link + ô nhập; nhập grant đúng → job tiếp tục; cấu hình TG bot → nhận tin nhắn Telegram (quickstart.md US3)

### Implementation for User Story 3

- [X] T031 [US3] Worker challenge bridge trong `scripts/spotiflac_worker.py`: đọc `SpotiFLAC/core/solver.py` + provider call-sites để xác định hook bắt challenge (R4 — **nhiệm vụ xác minh**; fallback đã định trong research.md: tự bắt exception challenge và tự chạy vòng verify với grant nhập ngoài); emit `cloudflare_challenge{challenge_url}`, block đọc stdin chờ `{"type":"grant"}`, inject grant, sai 3 lần → `error{SPOTIFLAC_CHALLENGE_TIMEOUT}`
- [X] T032 [US3] Trạng thái `WaitingInput` trong `src-tauri/src/downloader/queue.rs`: nhận event challenge → set status `waiting_input` + lưu `CloudflareChallenge{challenge_url, requested_at, attempts}` vào `RunningJob`, emit `job:cloudflare_challenge` + `job:status_changed`; grant OK (progress chảy lại) → về `downloading`; **timeout tuyệt đối 15 phút ở `waiting_input` không nhận grant hợp lệ → kill worker, job `failed` với `SPOTIFLAC_CHALLENGE_TIMEOUT` (nhả slot concurrency)**; `reset_interrupted_jobs` trong `db/mod.rs` coi `waiting_input` như `downloading` (reset về `queued`) + test
- [X] T033 [US3] Tạo `src-tauri/src/commands/music.rs`: `submit_cloudflare_grant(job_id, grant)` (precondition status/worker sống, ghi stdin) + `get_pending_challenge(job_id)`; đăng ký cả hai trong `invoke_handler` tại `src-tauri/src/lib.rs`; unit test phần thuần (validate precondition)
- [X] T034 [US3] Truyền env Telegram trong `queue.rs`/`spotiflac.rs`: set `TG_BOT_TOKEN`/`TG_CHAT_ID` vào worker process khi settings không rỗng (module tự xử lý vòng Telegram — US3-AS2)
- [X] T035 [P] [US3] Tạo `src/components/CloudflareGrantDialog.tsx`: hiện challenge URL (nút mở trình duyệt qua opener hiện có), ô nhập grant, đếm attempts, gọi `submit_cloudflare_grant`; khôi phục sau reload qua `get_pending_challenge`
- [X] T036 [US3] Nối frontend: listener `job:cloudflare_challenge` trong `src/stores/queue-store.ts` (mở/cập nhật dialog state), `src/components/QueueList.tsx` hiển thị trạng thái "Chờ CAPTCHA" (`waiting_input`) với action mở dialog
- [X] T037 [US3] Settings Telegram trong `src/pages/Settings.tsx`: input `tg_bot_token` (password-masked) + `tg_chat_id` + chú thích lưu plaintext (R6 caveat)
- [X] T038 [US3] i18n keys US3 (`music.challenge.*`, `queue.waitingInput`, `settings.spotiflac.tgBotToken/tgChatId/plaintextWarning`) vào cả `en.json` + `vi.json`
- [X] T039 [P] [US3] Vitest: `tests/unit/CloudflareGrantDialog.test.tsx` (render, submit gọi invoke đúng payload, attempts), cập nhật `tests/unit/queue-store.test.ts` (listener challenge event → state); cargo test transition `waiting_input` trong `queue.rs`/`models.rs`

**Checkpoint**: Cả 3 user story độc lập hoạt động

---

## Phase 6: Polish & Cross-Cutting Concerns

- [ ] T040 [P] Chạy toàn bộ kịch bản `specs/006-spotiflac-integration/quickstart.md` (US1→US3 + edge cases) trên máy dev, ghi kết quả vào PR description — **CHƯA CHẠY**: cần `bash scripts/build-spotiflac-onedir.sh` (pip install SpotiFLAC==1.5.5 + PyInstaller) mà môi trường agent chặn cài package bên thứ ba; chạy tay script đó rồi `pnpm tauri dev` để hoàn tất
- [X] T041 [P] Kiểm tra protocol guard: Rust từ chối worker có `hello.protocol != 1` với thông báo lỗi rõ (test trong `spotiflac.rs`); pin `SpotiFLAC==1.5.5` trong build script có comment hướng dẫn bump
- [X] T042 Edge-case message: lỗi `SPOTIFLAC_NO_SOURCE` hiển thị gợi ý "thử tải thường qua yt-dlp" trong `ErrorBanner` (message từ `errors.*` — xác nhận cả en/vi); job Retry thủ công hoạt động với job music
- [X] T043 [P] Library: hiển thị badge `source_provider` cho file music trong `src/components/LibraryGrid.tsx` (tùy chọn, nhỏ); đảm bảo filter/search Library không vỡ với `media_type="music"` + cập nhật `tests/unit/Library.test.tsx`
- [X] T044 Vệ sinh cuối: `cargo clippy`, `cargo test`, `pnpm test`, `pnpm lint` xanh toàn bộ; rà soát log worker không leak token TG vào LogBuffer

---

## Dependencies & Execution Order

### Phase Dependencies

- **Phase 1 (Setup)**: bắt đầu ngay; T003/T004/T005 song song sau T002 (T004/T005 chỉ cần biết tên thư mục, có thể làm song song với T002)
- **Phase 2 (Foundational)**: cần T001 (protocol shape) để viết parser T010; T006→T007 (models dùng giá trị mới) → còn lại; **chặn mọi user story**
- **Phase 3 (US1)**: sau Phase 2. T014/T015 (worker, Python) song song với T016–T019 (Rust) sau khi contract cố định; frontend T020/T021 song song với Rust; T022–T023 cuối
- **Phase 4 (US2)**: sau Phase 2; độc lập kiểm chứng nhưng demo trọn vẹn cần US1 (T025/T027/T028 đụng `queue.rs` — làm sau T018 để tránh conflict file)
- **Phase 5 (US3)**: sau Phase 2; T031 (worker) độc lập; T032 đụng `queue.rs` — làm sau T018/T025/T027
- **Phase 6 (Polish)**: sau các story mong muốn

### User Story Dependencies

- **US1 (P1)**: chỉ cần Foundational — MVP
- **US2 (P2)**: không phụ thuộc logic US1, nhưng các task `queue.rs` phải xếp sau T018 (cùng file)
- **US3 (P3)**: không phụ thuộc logic US1/US2, nhưng T032 cùng file `queue.rs` — xếp cuối chuỗi file đó

### Parallel Opportunities

- Phase 1: T003, T004, T005 song song
- Phase 2: T008, T009, T011, T013 song song (khác file); T010 sau T001; T012 sau T007
- US1: cặp Python (T014, T015) ∥ chuỗi Rust (T016→T017→T018→T019) ∥ cặp frontend (T020, T021)
- US2: T024, T026, T030 song song; T025→T027→T028 tuần tự (cùng `queue.rs`)
- US3: T031 ∥ T035; T032→T033→T034 tuần tự backend; T036–T038 sau T035

## Parallel Example: User Story 1

```bash
# Sau khi Phase 2 xong, 3 luồng song song:
Luồng A (Python):  T014 preview mode  → T015 download mode   (scripts/spotiflac_worker.py)
Luồng B (Rust):    T016 preview route → T017 validate → T018 run_music_job → T019 insert_downloaded_file
Luồng C (Frontend):T020 tier picker  ∥ T021 provider trong queue
Hợp nhất:          T022 i18n → T023 vitest
```

## Implementation Strategy

### MVP First (US1)

1. Phase 1 + Phase 2 (T001–T013)
2. Phase 3 (T014–T023) → **DỪNG, VALIDATE** theo quickstart US1 → demo được: dán link Spotify ra file FLAC
3. Ship/demo MVP trước khi làm tiếp

### Incremental Delivery

- +US2 (T024–T030): cấu hình provider/tier/MP3-320 → validate độc lập
- +US3 (T031–T039): luồng Cloudflare → validate độc lập (mock worker được, không cần gặp challenge thật)
- Phase 6 polish → PR

### Ghi chú rủi ro

- **T031 là task rủi ro cao nhất** (hook challenge chưa xác minh — research.md R4 có fallback định sẵn). Nếu kẹt, US3 vẫn giao được mức Telegram-only (T034) + trạng thái `waiting_input` hiển thị hướng dẫn.
- Các task đụng `queue.rs` (T018, T025, T027, T028, T032) tuyệt đối không chạy song song với nhau.
