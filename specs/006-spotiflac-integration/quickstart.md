# Quickstart: Kiểm chứng Tích Hợp SpotiFLAC

Hướng dẫn chạy & xác nhận feature hoạt động end-to-end. Chi tiết shape dữ liệu xem [data-model.md](./data-model.md), giao thức xem [contracts/](./contracts/).

## Prerequisites

- Toolchain hiện có của repo: `pnpm`, Rust stable, Tauri 2 CLI.
- **Python 3.11+ và `pyinstaller`** (chỉ build-time, để đóng gói worker).
- ffmpeg dev binary (đã có qua `scripts/fetch-dev-binaries.sh`).
- Tùy chọn: Node.js trong PATH (để test JS Extensions fallback), Chrome/Chromium (auto-solver Cloudflare).

## Setup

```bash
# 1. Build worker onedir (mới)
scripts/build-spotiflac-onedir.sh          # pip install SpotiFLAC==1.5.5 + pyinstaller --onedir scripts/spotiflac_worker.py
ls src-tauri/binaries/spotiflac-onedir/    # phải chứa executable spotiflac-worker*

# 2. Dev binaries còn lại + chạy app
scripts/fetch-dev-binaries.sh
pnpm install
pnpm tauri dev
```

## Kiểm thử tự động

```bash
pnpm test          # vitest — gồm test mới: CloudflareGrantDialog, tier picker, queue-store challenge listener,
                   # locale-parity & no-hardcoded-strings phải pass với keys mới
cd src-tauri && cargo test   # gồm: migration_0013_is_registered..., parse SPOTIFLAC_EVENT lines,
                             # classify_spotiflac_error, apply_patch cho settings mới, validate music tier
```

### Test riêng giao thức worker (không cần app)

```bash
src-tauri/binaries/spotiflac-onedir/spotiflac-worker preview \
  --url "https://open.spotify.com/track/4cOdK2wGLETKBW3PvgPWqT"
# Kỳ vọng: dòng SPOTIFLAC_EVENT::{"type":"hello","protocol":1,...}
#         rồi SPOTIFLAC_EVENT::{"type":"preview_result","kind":"track",...}  — exit 0
```

## Kịch bản xác nhận theo User Story

### US1 — Tải FLAC từ link Spotify (P1)

1. Dán `https://open.spotify.com/track/...` vào ô URL → preview hiện media type **music**, 3 tier chất lượng.
2. Chọn **FLAC 16-bit**, bấm Tải → job vào queue, hiển thị % + tốc độ + **provider đang dùng** (FR-009).
3. Kỳ vọng: file `.flac` xuất hiện ở thư mục output, có tag Title/Artist/Album + cover art (mở bằng player kiểm tra), row mới trong Library với `media_type=music`.
4. Dán link **Album/Playlist** → xác nhận danh sách → mỗi bài thành 1 job độc lập trong queue (US1-AS2).

### US2 — Cấu hình provider & chất lượng (P2)

1. Settings → section **SpotiFLAC**: đổi thứ tự ưu tiên (Qobuz lên đầu), chọn **FLAC 24-bit Hi-Res**.
2. Tải 1 bài → queue hiển thị provider `qobuz`; kiểm tra file: `ffprobe file.flac` cho bit depth 24 (nếu nguồn có Hi-Res; nếu không, tự fallback tier — chấp nhận).
3. Tắt mạng tới provider đầu (hoặc chọn provider hỏng) → xác nhận job tự chuyển provider kế tiếp (event `provider_switch` hiện trong queue/log) (US2-AS1); nếu Node.js có mặt và bật extensions fallback → thấy provider dạng `ext:*` khi native fail (US2-AS2).
4. Chọn tier **MP3 320kbps** → file output `.mp3` 320k có đủ tag + cover (transcode ffmpeg).

### US3 — Cloudflare challenge (P3)

1. Kích hoạt trường hợp challenge (provider Amazon thường gặp; hoặc mock worker bằng script emit `cloudflare_challenge`).
2. Kỳ vọng: job chuyển trạng thái **Chờ CAPTCHA** (`waiting_input`), dialog hiện link mở trình duyệt + ô nhập grant; nhập grant đúng → job tự tiếp tục về `downloading` (US3-AS1).
3. Cấu hình `TG Bot Token` + `Chat ID` trong Settings → gặp challenge → nhận tin nhắn Telegram chứa URL xác minh (US3-AS2, do module xử lý native).

### Edge cases

- Link Spotify bài không tồn tại trên providers → lỗi `SPOTIFLAC_NO_SOURCE` với gợi ý tải thường (edge 1).
- Ngắt mạng giữa chừng → job tự retry theo backoff (`NETWORK_ERROR`), hoặc bấm Retry thủ công (edge 3).
- Gỡ Node khỏi PATH + bật extensions fallback → job vẫn chạy native, có cảnh báo `SPOTIFLAC_NODE_MISSING` hướng dẫn cài Node (edge 4).

## Success Criteria mapping

| Tiêu chí spec | Cách đo trong quickstart |
|---|---|
| ≥95% khớp nguồn | Chạy list 20 track Spotify phổ biến → ≥19 tải thành công |
| Đúng định dạng FLAC + tag + cover | `ffprobe` + mở tag editor trên file output |
| Không cần đăng nhập | Toàn bộ kịch bản trên không nhập credential nào |
| 100% fallback khi provider chính lỗi | Kịch bản US2 bước 3 |
