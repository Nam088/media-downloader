# Contract: Tauri Commands & Events (mở rộng cho SpotiFLAC)

Frontend (TS) ↔ Backend (Rust) qua `invoke`/`listen`. Chỉ liệt kê phần **mới hoặc thay đổi**; shape hiện có giữ nguyên tương thích ngược.

## 1. Commands thay đổi

### `preview_media(url)` — thay đổi hành vi, shape mở rộng

- **Routing mới**: host ∈ {`open.spotify.com`, `listen.tidal.com`, `music.apple.com`, `pandora.com`, `pandora.app.link`} → preview qua SpotiFLAC worker (`preview` mode) **trước**, không gọi yt-dlp; lỗi worker → fallback chuỗi engine hiện hành (yt-dlp → gallery-dl) trước khi trả `unsupported_after_all_engines`.
- **`MediaSource` mở rộng**:
  - `media_type: "music"` (giá trị mới).
  - `available_music_tiers: MusicQualityTier[]` — luôn `["flac16","flac24","mp3_320"]` trong scope này (worker không probe tier trước).
  - Playlist/album/artist: tái dùng shape playlist hiện có (`entries[]` với `source_url` per-track từ `preview_result.tracks`).

### `create_download_job(input)` / `create_playlist_download_jobs(input)` — nhánh validate mới

- `input.media_type = "music"` yêu cầu `input.audio_quality ∈ available_music_tiers` của preview đã cache cho đúng URL (lỗi `invalid_quality_option` nếu không khớp — backend không tin frontend).
- `output_options` cho music: chỉ phần filename template áp dụng; các option video/subtitle bị từ chối (`invalid_quality_option`).

### `get_settings()` / `update_settings(patch)` — field mới

```ts
// AppSettings & UpdateSettingsInput thêm:
spotiflac_service_order: string;        // CSV "tidal,qobuz,deezer,amazon"
spotiflac_quality: MusicQualityTier;    // default "flac16"
spotiflac_extensions_fallback: boolean; // default true
tg_bot_token: string;                   // default ""
tg_chat_id: string;                     // default ""
```

Validation phía `apply_patch` theo data-model.md §4. Có unit test Rust cho từng ràng buộc.

## 2. Commands mới (`src-tauri/src/commands/music.rs`)

### `submit_cloudflare_grant(job_id: string, grant: string) -> Result<(), AppError>`

- Precondition: job tồn tại, `status == "waiting_input"`, worker còn sống. Sai precondition → `not_found` / `internal`.
- Hành vi: ghi `{"type":"grant","value":...}` xuống stdin worker; **không** đổi status ngay — status đổi khi worker xác nhận (tiếp tục `progress` ⇒ Rust set `downloading`) hoặc thất bại (re-emit challenge).
- Grant sai lần 3 ⇒ job `failed` với code `SPOTIFLAC_CHALLENGE_TIMEOUT`.

### `get_pending_challenge(job_id: string) -> Result<{ challenge_url: string } | null, AppError>`

- Cho dialog khôi phục sau khi frontend reload (state in-memory, data-model.md §6).

## 3. Events

### `job:progress` — payload mở rộng (backward-compatible)

```ts
interface JobProgressEvent {
  job_id: string;
  progress_percent: number | null;
  downloaded_bytes?: number;
  speed_bytes_per_sec?: number;
  eta_seconds?: number;
  provider?: string;   // MỚI — chỉ job music: "tidal" | "qobuz" | "deezer" | "amazon" | "ext:<name>"
}
```

### `job:status_changed` — giá trị status mới

`status` nhận thêm `"waiting_input"`. Shape không đổi.

### `job:cloudflare_challenge` — event MỚI

```ts
interface JobCloudflareChallengeEvent {
  job_id: string;
  challenge_url: string;
}
```

- Emit khi worker báo challenge và job chuyển `waiting_input`. Frontend (queue-store listener) mở/cập nhật `CloudflareGrantDialog`.

## 4. Error codes mới (map `errors.*` trong `en.json` + `vi.json`)

| Code | Khi nào | Retry tự động? |
|---|---|---|
| `SPOTIFLAC_NO_SOURCE` | Không provider nào có bài (edge case 1 — message gợi ý thử tải thường qua yt-dlp) | Không |
| `SPOTIFLAC_REGION_BLOCKED` | Hết providers vì giới hạn khu vực | Không |
| `NETWORK_ERROR` (tái dùng) | Mất mạng giữa chừng | Có (retry.rs backoff) |
| `SPOTIFLAC_CHALLENGE_TIMEOUT` | Không nhận grant hợp lệ (sai 3 lần, hoặc quá 15 phút ở `waiting_input`) | Không (retry thủ công) |
| `SPOTIFLAC_NODE_MISSING` | Thiếu Node khi cần extension — chỉ cảnh báo (toast/log), job vẫn chạy native | — |

## 5. i18n keys mới (cả `en.json` và `vi.json` — guard test locale-parity)

Namespaces: `downloadForm.musicTier.*` (3 tier + mô tả), `queue.waitingInput`, `queue.provider`, `music.challenge.*` (dialog: title, instructions, openBrowser, grantLabel, submit, attemptsLeft), `settings.spotiflac.*` (section, serviceOrder, quality, extensionsFallback, tgBotToken, tgChatId, plaintextWarning), `errors.SPOTIFLAC_*`.
