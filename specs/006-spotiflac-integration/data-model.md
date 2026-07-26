# Data Model: Tích Hợp SpotiFLAC

**Date**: 2026-07-27 · Tham chiếu: [research.md](./research.md), schema hiện có `src-tauri/src/db/migrations/`.

## 1. Mở rộng `MediaType` (Rust `models.rs` + TS `types/download.ts`)

```
MediaType = Audio | Video | Gallery | Music   // "music" — engine SpotiFLAC
```

- `Music` ⇒ engine SpotiFLAC (tương tự quy ước `Gallery` ⇒ gallery-dl).
- DB: cột `download_jobs.media_type` có CHECK constraint → **migration 0013 rebuild bảng** để thêm `'music'`.

## 2. Mở rộng `JobStatus`

```
JobStatus = Queued | FetchingMetadata | Downloading | WaitingInput | Paused
          | Completed | Failed | Canceled            // WaitingInput = "waiting_input"
```

**State transitions mới** (chỉ áp dụng job `media_type=music`):

```
Downloading ──(cloudflare_challenge)──▶ WaitingInput
WaitingInput ──(submit_cloudflare_grant OK)──▶ Downloading
WaitingInput ──(cancel)──▶ Canceled
WaitingInput ──(timeout 15 phút / grant sai quá 3 lần)──▶ Failed
```

- Job đang `WaitingInput` chiếm 1 slot concurrency (worker process còn sống chờ stdin). Để tránh treo slot vô thời hạn: **timeout tuyệt đối 15 phút** kể từ khi vào `WaitingInput` mà không nhận grant hợp lệ → Rust kill worker, job `Failed` với code `SPOTIFLAC_CHALLENGE_TIMEOUT` (nhả slot; người dùng Retry thủ công được). `reset_interrupted_jobs` khi khởi động app coi `waiting_input` như `downloading` → reset về `queued`.
- DB: nằm trong cùng rebuild của migration 0013 (CHECK constraint status).

## 3. Entity: SpotiFLAC Download Job (tái dùng `DownloadJob`)

Không thêm cột mới vào `download_jobs`. Mapping các trường hiện có:

| Trường spec | Cột/field hiện có | Ghi chú |
|---|---|---|
| URL gốc Spotify/Stream | `source_url` | |
| Metadata (Title/Artist/Album) | `title`, `playlist_title` | Artist nằm trong title hiển thị (`"Artist – Title"` từ preview) |
| Mức chất lượng đã chọn | `audio_quality` | Giá trị = `MusicQualityTier` (xem §5) |
| Trạng thái | `status` | Gồm `waiting_input` (§2) |
| Nguồn phát đang dùng | *(runtime, không persist)* | Trường `provider` trong `JobProgressEvent`; provider cuối persist vào `downloaded_files.source_provider` |
| Album/Playlist/Artist | `is_playlist_item`, `parent_playlist_id` | Mỗi track = 1 job (research R7) |
| Retry/backoff | `retry_count`, `next_retry_at` | Cơ chế hiện có |

**Validation**: `commands/download.rs::validate_quality` thêm nhánh `Music`: tier phải nằm trong danh sách tier của preview đã cache cho đúng URL đó (backend không tin frontend).

## 4. Entity: Provider Profile (Cấu hình nguồn phát) → `AppSettings`

Field mới trong `models::AppSettings` + `types/settings.ts` (bảng `app_settings` key/value — **không cần migration**, `get_setting_or_default` tự chèn):

| Key | Type | Default | Validation (`apply_patch`) |
|---|---|---|---|
| `spotiflac_service_order` | `String` (CSV) | `"tidal,qobuz,deezer,amazon"` | Chỉ chấp nhận subset+permutation của 4 provider; không rỗng |
| `spotiflac_quality` | `String` | `"flac16"` | ∈ {`flac16`,`flac24`,`mp3_320`} |
| `spotiflac_extensions_fallback` | `bool` | `true` | |
| `tg_bot_token` | `String` | `""` | Trim; plaintext (caveat R6) |
| `tg_chat_id` | `String` | `""` | Trim; chỉ chữ số hoặc rỗng |

## 5. Value type: `MusicQualityTier` (TS + Rust enum)

```
MusicQualityTier = "flac16" | "flac24" | "mp3_320"
```

Mapping sang tham số module (thực hiện trong worker, research R3):

| Tier | Tidal | Qobuz | Deezer/Amazon | Hậu xử lý Rust |
|---|---|---|---|---|
| `flac16` | `LOSSLESS` | `"6"` | mặc định module | — |
| `flac24` | `HI_RES_LOSSLESS` | `"27"` | mặc định module | — |
| `mp3_320` | `LOSSLESS` | `"6"` | mặc định module | ffmpeg transcode `libmp3lame 320k`, copy tags+cover, xóa FLAC trung gian |

`allow_fallback=True` luôn bật ở worker (tự hạ tier khi provider không có).

## 6. Entity: Cloudflare Verification State (in-memory, không persist)

Nằm trong `RunningJob` (queue state) khi job `media_type=music`:

```
CloudflareChallenge {
  job_id: String,
  challenge_url: String,     // từ worker event cloudflare_challenge
  requested_at: Instant,     // mốc tính timeout tuyệt đối 15 phút (§2)
  attempts: u8,              // số lần nhập grant sai, giới hạn 3
}
```

- Không persist: app restart ⇒ job reset về `queued`, challenge sẽ tự phát sinh lại khi chạy.
- Command `get_pending_challenge(job_id)` (hoặc trả kèm trong `job:status_changed`) để dialog khôi phục URL sau khi frontend reload.

## 7. Mở rộng `downloaded_files` (Library index)

Migration 0013 thêm cột:

| Cột | Type | Ghi chú |
|---|---|---|
| `source_provider` | `TEXT NULL` | `"tidal" \| "qobuz" \| "deezer" \| "amazon" \| "ext:<name>"` — provider thực tế đã tải; NULL cho file cũ/engine khác |

- `insert_downloaded_file` (UPSERT theo `file_path`) nhận thêm tham số; Library UI hiển thị badge provider (tùy chọn, không bắt buộc trong scope).
- `media_type` của file nhạc ghi `"music"`; `platform` ghi nhãn nguồn gốc (`spotify`, `tidal`, `apple_music`, `pandora` — mở rộng `platform.rs`).

## 8. Migration 0013 (`0013_music_engine.sql`)

Một migration duy nhất, theo pattern rebuild của 0002/0003:

1. Rebuild `download_jobs`: CHECK `media_type IN ('audio','video','gallery','music')`; CHECK `status IN (..., 'waiting_input')`.
2. `ALTER TABLE downloaded_files ADD COLUMN source_provider TEXT` (+ index nếu cần filter sau này — chưa cần trong scope).
3. Đăng ký `M::up(include_str!(...0013...))` trong `db/mod.rs::migrations()`; test `migration_0013_is_registered_and_widens_media_type` theo pattern test 0012.

## 9. Events (payload — chi tiết trong contracts/tauri-interface.md)

- `JobProgressEvent` += `provider: Option<String>` (chỉ job music set).
- Event mới `job:cloudflare_challenge { job_id, challenge_url }`.
- `JobStatusChangedEvent` không đổi shape — `status` nhận thêm giá trị `"waiting_input"`.
