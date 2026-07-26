# Phase 0 Research: Tích Hợp SpotiFLAC

**Date**: 2026-07-27 · **Nguồn**: README + source `BartolomeoRusso9/SpotiFLAC-Module-Version` (v1.5.5), khảo sát kiến trúc codebase hiện tại.

## Tóm tắt module SpotiFLAC (facts đã xác minh)

- Python module trên PyPI (`pip install SpotiFLAC`), phiên bản mới nhất v1.5.5. Có 3 mặt tiếp xúc: sync API `SpotiFLAC(...)`, async API `AsyncSpotiFLAC` (có `download_track`, `get_playlist` trả metadata không cần tải), và CLI/standalone executables (Windows/macOS/Linux x86_64+arm64, 27–41MB/asset trên GitHub Releases).
- Providers: `tidal, qobuz, deezer, amazon, soundcloud, youtube, apple, pandora, joox, netease, migu, kuwo` + JS extensions `ext:<name>` (vd `ext:tidal-web`), trộn tự do trong danh sách ưu tiên `services`. Fallback sang extension tự động bật mặc định (`use_extensions_fallback=True`).
- Quality: tham số `quality` theo provider — Tidal: `HI_RES_LOSSLESS / LOSSLESS / HIGH / LOW / DOLBY_ATMOS`; Qobuz: `"6"` (CD 16-bit) / `"7"` / `"27"` (Hi-Res Max). Có `allow_fallback=True` tự hạ tier khi không có.
- URL input hỗ trợ: Spotify (track/album/playlist/artist), Tidal, Apple Music, SoundCloud, YouTube, Pandora. SoundCloud/YouTube chỉ ra MP3 (không lossless).
- Metadata: tự gắn tag + cover art, MusicBrainz enrichment, lyrics nhúng (LRC), download validation (phát hiện file preview/corrupt và tự thử provider kế).
- Retry: `track_max_retries` (exponential backoff, xoay vòng providers), `timeout_s` per-track, `loop` re-queue phút.
- Cloudflare: `core/solver.py` tự giải bằng **nodriver + Chrome hệ thống** (bắt `grant` từ network). Khi solver hụt, `core/signed_session_desktop.py` rơi về `_run_manual_terminal_verification()` đọc `input()` — chi tiết và hệ quả ở [R4](#r4--luồng-cloudflare-challenge--grant-code-fr-007-us3).
- **Telegram KHÔNG nằm trong module**: `TG_BOT_TOKEN`/`TG_CHAT_ID` chỉ được `telegram_wrapper.py` (script ngoài ở gốc repo upstream) đọc — xem R4.
- JS Extensions cần Node.js; nếu thiếu, module **tự ý cài Node** qua package manager hệ thống (apt/brew/winget/choco).

---

## R1 — Hình thức tích hợp: Worker Python tự đóng gói (PyInstaller onedir), không dùng executable chính chủ

**Decision**: Viết `scripts/spotiflac_worker.py` bọc Python API của module, đóng gói bằng PyInstaller `--onedir` thành `src-tauri/binaries/spotiflac-onedir/`, khai báo trong `tauri.conf.json > bundle.resources`, giải nén runtime bằng `bundled_tool::ensure_cached_onedir` (OnceCell) — sao chép nguyên xi cơ chế của gallery-dl (`scripts/build-gallery-dl-onedir.sh`, `gallery_dl_binary.rs`).

**Rationale**:
- App đã có sẵn toàn bộ hạ tầng cho pattern này (copy-on-first-use vào `app_data_dir`, `.bundled-version` marker, chmod, matrix build trong `release.yml`) → thỏa FR-010 không cần cơ chế mới.
- Worker tự viết cho phép: (a) emit **progress có cấu trúc** (module có `core/progress.py` hook được; executable chính chủ chỉ in output cho người đọc), (b) mở kênh **stdin để bơm grant code** Cloudflare vào process (FR-007), (c) chạy mode `preview` lấy metadata track/playlist mà không tải (dùng `AsyncSpotiFLAC.get_playlist` và tương đương cho track) — khớp cơ chế preview-trước-validate-sau của `commands/download.rs::validate_quality`.

**Alternatives considered**:
- *Executable chính chủ (SpotiFLAC-macOS/…)*: nhẹ công build, nhưng stdout không có contract ổn định để parse tiến trình, không có kênh grant-injection, không có preview mode tách rời → bị loại.
- *Bắt người dùng cài pip/Python*: vi phạm FR-010 → loại.
- *Nhúng qua PyO3*: kéo Python runtime vào process Rust, phức tạp build vượt xa lợi ích → loại.

## R2 — Routing engine: thêm `MediaType::Music`, không refactor trait Engine

**Decision**: Thêm biến thể thứ ba `MediaType::Music` (`"music"`) làm discriminator engine (nhất quán với cách `Gallery` ⇒ gallery-dl hiện nay), kèm nhánh thứ ba tại `queue::run_job` (`queue.rs:640`) → `run_music_job`, và **nhận diện URL nhạc TRƯỚC khi gọi yt-dlp** trong `commands::media::preview_media`.

**Rationale**: Codebase không có trait/enum Engine — 2 engine hiện tại là 2 module song song, route bằng if/else trên `MediaType`. Thêm nhánh thứ ba theo đúng hình dạng đó là thay đổi nhỏ nhất, dễ review; refactor trait là việc riêng, ngoài scope spec này. Phải route **trước** yt-dlp vì yt-dlp không lỗi "sạch" với link Spotify (cơ chế `looks_empty_handed` fallback hiện tại không bắt được đáng tin cậy).

**Phạm vi route sang SpotiFLAC**: host `open.spotify.com`, `listen.tidal.com`, `music.apple.com`, `pandora.com`/`pandora.app.link`. **SoundCloud và YouTube giữ nguyên yt-dlp** dù SpotiFLAC nhận được — hai nguồn này vốn không lossless, pipeline yt-dlp hiện có xử lý tốt hơn (đủ thỏa FR-001: hệ thống vẫn "nhận diện" và xử lý các URL đó, chỉ khác engine).

**Alternatives considered**: cột `engine` riêng trong DB (tách khái niệm media/engine — đúng về lý thuyết nhưng lệch khỏi quy ước hiện hành, tăng diện tích migration/UI); refactor trait Engine trước (trì hoãn giá trị người dùng, PR khổng lồ) → đều loại.

## R3 — Tier chất lượng & mapping provider

**Decision**: App định nghĩa 3 tier hướng người dùng (FR-003): `flac16` (Lossless Standard), `flac24` (Hi-Res), `mp3_320`. Worker map tier → tham số `quality` theo provider đang thử: `flac16` → Tidal `LOSSLESS` / Qobuz `"6"`; `flac24` → Tidal `HI_RES_LOSSLESS` / Qobuz `"27"`; luôn bật `allow_fallback=True`. Tier `mp3_320`: tải FLAC 16-bit rồi **transcode bằng ffmpeg đã bundle** (`libmp3lame -b:a 320k`, giữ tags + cover) ở bước hậu xử lý phía Rust.

**Rationale**: SpotiFLAC không xuất MP3 320 từ các provider lossless (MP3 chỉ có từ SoundCloud/YouTube/Pandora). App đã bundle ffmpeg và có sẵn tiền lệ hậu xử lý audio → tự transcode là đường ngắn nhất, chất lượng nguồn tốt nhất. Deezer/Amazon không có tham số quality riêng trong module — worker chỉ truyền services, module tự chọn stream FLAC tốt nhất.

**Alternatives considered**: expose thẳng ma trận quality theo provider (Tidal 5 mức × Qobuz 3 mức…) — quá phức tạp cho người dùng, spec chỉ yêu cầu 3 tier → loại.

## R4 — Luồng Cloudflare challenge & grant code (FR-007, US3)

**Decision**: Ba lớp, thứ tự ưu tiên:
1. **Auto-solver của module** (nodriver + Chrome): để nguyên — đa số challenge tự giải, người dùng không thấy gì.
2. **In-app dialog**: worker chiếm chỗ prompt thủ công của module → emit `cloudflare_challenge {challenge_url}` qua stdout → Rust chuyển job sang `waiting_input`, emit `job:cloudflare_challenge` → frontend hiện dialog → `submit_cloudflare_grant(job_id, grant)` → Rust ghi JSON xuống **stdin worker** → worker trả grant về cho module, job quay lại `downloading`.
3. **Telegram**: chạy song song với dialog — worker gửi challenge URL qua Bot API rồi poll `getUpdates` chờ reply từ đúng `TG_CHAT_ID`; grant nào tới trước thì thắng.

**Hook đã XÁC MINH** (đọc source v1.5.5, `SpotiFLAC/core/signed_session_desktop.py::run_community_verification`): module giải challenge theo ba mode — (1) handler GUI đăng ký qua `set_community_verification_handlers`, (2) auto-solver `solver.solve_with_callback`, (3) `_run_manual_terminal_verification(challenge_url)` đọc `input()` từ stdin. Worker **cố ý KHÔNG đăng ký handler GUI** (mode 1 chạy trước và sẽ chặn mất auto-solver), mà thay hàm của mode 3 — nó đã nhận sẵn challenge URL hoàn chỉnh và chỉ cần trả về chuỗi grant. Đúng thứ tự lớp ở trên, không phải patch gì sâu hơn.

**SỬA GIẢ ĐỊNH BAN ĐẦU — Telegram KHÔNG có sẵn trong module**: `TG_BOT_TOKEN`/`TG_CHAT_ID` chỉ được đọc bởi `telegram_wrapper.py` ở gốc repo upstream — một tiến trình **bên ngoài**, spawn CLI rồi regex output để bắt challenge URL và ghi grant vào stdin của nó. Đó đúng bằng vai trò mà app này đang đóng, nên truyền env xuống rồi trông chờ module tự xử lý là vô nghĩa: FR-008 do **chính worker implement** (gửi `sendMessage`, poll `getUpdates`, chỉ chấp nhận reply từ đúng chat ID đã cấu hình — thứ chặn người lạ tìm ra bot rồi chiếm phiên).

**Alternatives considered**: đăng ký `set_community_verification_handlers` (API công khai, sạch hơn) — bị loại vì mode 1 chạy TRƯỚC auto-solver, biến mọi challenge thành một lần hỏi người dùng dù máy thừa sức tự giải.

## R5 — JS Extensions & Node.js (FR-005, edge case thiếu Node)

**Decision**: Mặc định bật extensions fallback (theo module) **chỉ khi phát hiện `node` trong PATH** lúc khởi động job; nếu không có Node → worker chạy với `use_extensions_fallback=False` và Rust emit cảnh báo một lần (toast + log) hướng dẫn cài Node để có fallback — **không cho phép module tự ý cài Node** (hành vi auto-install bằng apt/brew/winget từ trong desktop app là xâm lấn, gây prompt sudo/UAC khó hiểu).

**Rationale**: Thỏa cả FR-005 (fallback tự động khi có môi trường) lẫn edge case trong spec ("tự nhận biết và thông báo hỗ trợ người dùng cài đặt... hoặc chuyển sang native provider"). Toggle `spotiflac_extensions_fallback` trong Settings cho người dùng tắt hẳn.

## R6 — Settings & lưu trữ Telegram token (FR-004, FR-008)

**Decision**: 5 field mới trong `AppSettings` (bảng `app_settings` key/value — **không cần migration** nhờ `get_setting_or_default`): `spotiflac_service_order` (CSV, default `"tidal,qobuz,deezer,amazon"`), `spotiflac_quality` (default `"flac16"`), `spotiflac_extensions_fallback` (default `true`), `tg_bot_token`, `tg_chat_id` (default rỗng). UI: section "Nhạc Lossless (SpotiFLAC)" trong `Settings.tsx` — provider order dạng danh sách sắp xếp được, token input dạng password-masked.

**Caveat ghi nhận**: token lưu **plaintext trong SQLite** — app hiện không có secret storage; hiển thị chú thích trong UI. Nâng cấp keychain là việc ngoài scope.

## R7 — Mô hình job cho Album/Playlist/Artist (US1-AS2)

**Decision**: Preview URL nhạc trả về danh sách track (từ worker mode `preview`); album/playlist/artist đi qua đường `create_playlist_download_jobs` hiện có — **mỗi track = 1 `DownloadJob` độc lập** (`parent_playlist_id`, `is_playlist_item`), mỗi job spawn 1 lần worker cho đúng 1 track (dùng `output_path`/single-track mode). Progress, retry, pause/cancel per-track dùng nguyên cơ chế queue hiện hành (pause = kill + tải lại track đó khi resume; FLAC track nhỏ nên chấp nhận không resume giữa file — khớp edge case "cho phép thử lại tác vụ").

**Rationale**: Thỏa đúng câu chữ US1-AS2 ("từng bài hát vào hàng đợi với tiến trình độc lập") và tái dùng 100% hạ tầng queue/retry/reorder; tránh phải parse tiến trình multi-track trong 1 process.

## R8 — Giao thức tiến trình worker (FR-009)

**Decision**: Mọi dòng stdout của worker có prefix sentinel `SPOTIFLAC_EVENT::` theo sau là JSON một dòng (chi tiết trong `contracts/spotiflac-worker-protocol.md`): `preview_result`, `track_start{provider}`, `progress{percent,downloaded_bytes,speed_bps}`, `provider_switch{from,to}`, `cloudflare_challenge{challenge_url}`, `track_done{file_path,provider}`, `error{code,message}`. Rust parse từng dòng (như `ytdlp::parse_progress`) → map sang `JobProgressEvent` (+ field mới `provider`) và các event mới.

**Rationale**: Đúng quy ước sentinel đã có (`MEDIA_DOWNLOADER_FILEPATH::`, `MEDIA_DOWNLOADER_GALLERY_FILE_DONE::`); JSON-line dễ mở rộng và test. Hook tiến trình phía Python: dùng `SpotiFLAC.core.progress` / logging capture — nếu module không expose % byte-level cho một provider, worker emit progress dạng indeterminate (`percent: null`) — khớp semantics `None` percent hiện có của app.

## R9 — Error taxonomy & retry

**Decision**: `classify_spotiflac_error()` trong `spotiflac.rs` map lỗi worker → error codes: `SPOTIFLAC_NO_SOURCE` (không provider nào có bài — edge case 1, gợi ý fallback yt-dlp trong message), `SPOTIFLAC_REGION_BLOCKED` (đã tự xoay hết providers), `NETWORK_ERROR_CODE` (tái dùng, hưởng retry/backoff của `retry.rs`), `SPOTIFLAC_NODE_MISSING` (cảnh báo, không fail job), `SPOTIFLAC_CHALLENGE_TIMEOUT`. Mỗi code có entry `errors.*` trong cả `en.json` và `vi.json` (ErrorBanner map 1:1).

**Rationale**: Khớp cấu trúc `AppError{code,message}` + `decide_outcome` hiện hành; edge case mất mạng giữa chừng tự động được cơ chế retry hiện có xử lý.
