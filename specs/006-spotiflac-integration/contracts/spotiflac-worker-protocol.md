# Contract: Giao thức Rust ↔ SpotiFLAC Worker

Worker: `scripts/spotiflac_worker.py`, đóng gói PyInstaller onedir → `spotiflac-onedir/`, resolve runtime qua `spotiflac_binary.rs`. Mỗi lần spawn xử lý **một** việc (preview 1 URL, hoặc download 1 track).

## 1. CLI của worker

### Preview (lấy metadata, không tải)

```
spotiflac-worker preview --url <URL>
```

### Download (đúng 1 track)

```
spotiflac-worker download
  --url <track-URL>                # URL track đơn (đã tách từ preview nếu gốc là album/playlist/artist)
  --output-dir <dir>
  --services tidal,qobuz,deezer    # CSV, có thể chứa ext:<name>
  --tier flac16|flac24|mp3_320     # worker tự map sang quality module (mp3_320 tải flac16, Rust transcode)
  [--no-extensions-fallback]       # Rust truyền khi không phát hiện node trong PATH hoặc user tắt
  [--timeout-s N]
```

**Env do Rust set khi cấu hình**: `TG_BOT_TOKEN`, `TG_CHAT_ID` — **worker tự implement** vòng Telegram (gửi challenge URL, poll `getUpdates`, chỉ nhận reply từ đúng chat ID); module không đọc hai biến này (research.md R4). `PYTHONUNBUFFERED=1` luôn set.

## 2. stdout: sentinel JSON-line

Mọi event có dạng một dòng: `SPOTIFLAC_EVENT::{json}`. Dòng không có prefix = log thô (Rust chuyển vào LogBuffer mức debug). Các `type`:

| type | Payload | Ý nghĩa |
|---|---|---|
| `preview_result` | `{kind: "track"\|"album"\|"playlist"\|"artist", title, artist, album?, thumbnail_url?, tracks: [{url, title, artist, album, duration_seconds?, track_number?}]}` | Kết quả preview; `tracks` có đúng 1 phần tử nếu `kind=track` |
| `track_start` | `{provider}` | Bắt đầu thử provider (mỗi lần đổi provider emit lại) |
| `progress` | `{percent: number\|null, downloaded_bytes?: number, speed_bps?: number}` | `percent=null` ⇒ indeterminate (khớp semantics hiện có) |
| `provider_switch` | `{from, to, reason}` | Fallback provider/extension (FR-004/FR-005) |
| `cloudflare_challenge` | `{challenge_url}` | Auto-solver thất bại; worker block chờ grant trên stdin |
| `track_done` | `{file_path, provider, bit_depth?: 16\|24, sample_rate_hz?}` | Tải + tag xong; đường dẫn tuyệt đối file output |
| `error` | `{code, message, provider?}` | Lỗi cuối cùng sau khi đã hết providers/retries (codes: §4) |

Thứ tự chuẩn 1 download thành công: `track_start → progress* → (provider_switch → track_start → progress*)* → track_done`, sau đó process exit 0.

## 3. stdin: lệnh từ Rust xuống worker (JSON-line)

| Lệnh | Khi nào |
|---|---|
| `{"type":"grant","value":"<grant-code>"}` | Người dùng nhập grant từ dialog (command `submit_cloudflare_grant`) |
| `{"type":"cancel"}` | Cancel/pause job — worker dọn file dở rồi exit 130 (Rust vẫn giữ kill-fallback sau 5s như cơ chế hiện hành) |

Sau `grant`: nếu hợp lệ worker tiếp tục (`progress` chảy tiếp); nếu sai worker emit lại `cloudflare_challenge` (tối đa 3 lần, sau đó `error{code:"SPOTIFLAC_CHALLENGE_TIMEOUT"}`).

## 4. Exit codes & error codes

| Exit | Ý nghĩa |
|---|---|
| 0 | Thành công (`track_done`/`preview_result` đã emit) |
| 1 | Lỗi đã emit qua `error` event |
| 2 | Lỗi tham số CLI/khởi tạo (Rust map → `internal`) |
| 130 | Bị cancel |

`error.code` ∈ `SPOTIFLAC_NO_SOURCE` · `SPOTIFLAC_REGION_BLOCKED` · `SPOTIFLAC_NETWORK` (Rust map → `NETWORK_ERROR_CODE`, hưởng retry/backoff) · `SPOTIFLAC_CHALLENGE_TIMEOUT` · `SPOTIFLAC_INTERNAL`.

## 5. Ràng buộc tương thích

- Protocol version hóa bằng hằng `PROTOCOL_VERSION = 1` in ra ở event đầu tiên `{"type":"hello","protocol":1,"module_version":"1.5.5"}` — Rust từ chối chạy nếu protocol lạ (bảo vệ khi bump module).
- Worker không bao giờ in JSON đa dòng; mọi output module gốc bị redirect qua logging capture.
