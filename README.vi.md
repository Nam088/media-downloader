# Media Downloader

*[English](README.md)*

Ứng dụng desktop tải video, âm thanh và nhạc lossless từ nhiều nền tảng — chạy trên Windows, macOS và Linux, đóng gói sẵn mọi công cụ cần thiết, không yêu cầu người dùng cài thêm Python hay bất kỳ dependency nào.

Xây trên [Tauri 2](https://tauri.app/) (Rust + React), dùng [yt-dlp](https://github.com/yt-dlp/yt-dlp) và [gallery-dl](https://github.com/mikf/gallery-dl) làm engine tải video/ảnh, và [SpotiFLAC](https://github.com/BartolomeoRusso9/SpotiFLAC-Module-Version) làm engine nhạc lossless.

## Tính năng

### Tải video & âm thanh
- Dán một hoặc nhiều liên kết cùng lúc (nhập tay hoặc từ file danh sách), tự nhận diện nền tảng nguồn và hiển thị xem trước (tiêu đề, ảnh thu nhỏ, thời lượng) trước khi tải.
- Hỗ trợ YouTube, TikTok, Facebook, Instagram, X (Twitter), SoundCloud, cùng ~1.600 site khác mà yt-dlp hỗ trợ, và fallback tự động sang gallery-dl cho các post ảnh/gallery (Pixiv, Reddit, slideshow TikTok...) mà yt-dlp không đọc được.
- Chọn giữa chỉ-âm-thanh hoặc video đầy đủ; danh sách chất lượng luôn lấy trực tiếp từ định dạng thật của nguồn, không bịa mức chất lượng không tồn tại.
- Tải cả playlist, chọn từng video muốn tải kèm chất lượng riêng cho mỗi video.

### Hàng đợi mạnh
- Hàng đợi thật sự lưu trên đĩa — sống sót qua khởi động lại, không mất tác vụ đang chờ.
- Kéo-thả sắp xếp lại thứ tự, chỉnh số luồng tải song song, tạm dừng/tiếp tục/huỷ từng tác vụ hoặc cả hàng loạt.
- Tự động thử lại khi gặp lỗi mạng tạm thời (có thời gian chờ tăng dần), phân biệt rõ với lỗi vĩnh viễn (nội dung riêng tư, đã gỡ) để không thử lại vô ích.
- Giới hạn tốc độ tải, chạy nền với biểu tượng khay hệ thống, thông báo khi tác vụ hoàn tất.

### Đầu ra tuỳ biến
- Nhiều định dạng audio (MP3, M4A, Opus, WAV, FLAC) và video (MP4, MKV, giữ nguyên container gốc), tuỳ chọn codec theo hướng tương thích hoặc chất lượng.
- Tự nhúng metadata (tiêu đề, nghệ sĩ, album) và ảnh bìa vào file khi định dạng hỗ trợ.
- Đặt tên file theo mẫu tuỳ chỉnh, tải phụ đề, cắt một đoạn cụ thể, tách video theo chương thành nhiều file.
- Lưu một cấu hình đầu ra thành preset để dùng lại cho lần tải sau.

### Nhạc lossless — SpotiFLAC
- Dán liên kết Spotify, Tidal, Apple Music hoặc Pandora để tải bản nhạc **FLAC lossless thật** (16-bit hoặc 24-bit Hi-Res) từ Tidal, Qobuz, Deezer, Amazon Music — không cần tài khoản của bất kỳ dịch vụ nào.
- Tự động thử lần lượt các nhà cung cấp theo thứ tự ưu tiên do người dùng cấu hình, kèm fallback qua JS extension khi nguồn chính gặp sự cố (yêu cầu Node.js).
- Tùy chọn tier MP3 320kbps (tải bản lossless rồi chuyển mã bằng ffmpeg đã đóng gói sẵn).
- Xử lý mượt khi gặp xác minh Cloudflare: hộp thoại nhập mã xác minh ngay trong ứng dụng, có thể cấu hình thêm Telegram Bot để nhận thông báo xác minh từ xa.
- Album/playlist/nghệ sĩ được tách thành từng bài tải độc lập trong cùng hàng đợi.

### Thư viện
- Mọi file đã tải được lập chỉ mục tự động — xem lại theo lưới hoặc danh sách, tìm kiếm và lọc theo loại nội dung, nền tảng, định dạng.
- Nghe/xem thử ngay trong ứng dụng, đổi tên/xoá/mở thư mục chứa file mà không cần rời ứng dụng.
- Tự phát hiện file đã bị xoá bên ngoài ứng dụng, thống kê tổng quan dung lượng và số lượng đã tải, xuất danh sách phát M3U.

### Đa nền tảng, đa ngôn ngữ
- Một bộ tính năng nhất quán trên Windows, macOS, Linux — không có tính năng nào chỉ chạy trên một hệ điều hành.
- Giao diện tiếng Việt và tiếng Anh, có thể đổi ngay trong ứng dụng.

## Cài đặt

Tải bản cài đặt cho hệ điều hành của bạn tại trang [Releases](../../releases) — mỗi bản phát hành đi kèm:

| Hệ điều hành | Định dạng |
|---|---|
| Windows | `.msi`, `.exe` (NSIS) |
| macOS (Apple Silicon) | `.dmg` |
| Linux | `.deb`, `.rpm`, `.AppImage` |

Không cần cài Python, Node.js hay ffmpeg riêng — toàn bộ đã được đóng gói sẵn trong bộ cài.

> **macOS**: vì ứng dụng chưa được ký bằng chứng chỉ Apple Developer, lần đầu mở có thể bị Gatekeeper báo "is damaged and can't be opened". Chạy `xattr -cr "/Applications/Media Downloader.app"` để gỡ cờ quarantine rồi mở lại bình thường.

## Phát triển

Yêu cầu: Node.js 22+, pnpm, Rust stable, Python 3 (chỉ để build sidecar).

```bash
pnpm install
bash scripts/fetch-dev-binaries.sh   # tải/dựng yt-dlp, gallery-dl, spotiflac-worker, ffmpeg cho máy dev
pnpm tauri dev
```

Kiểm thử:

```bash
pnpm test              # frontend (vitest)
cd src-tauri && cargo test   # backend (Rust)
```

## Kiến trúc

- **Frontend**: React 19 + TypeScript, Zustand cho state, i18next cho đa ngôn ngữ.
- **Backend**: Rust (Tauri 2), SQLite (rusqlite) lưu hàng đợi/lịch sử/thư viện/cài đặt.
- **Engine tải**: ba engine chạy song song theo loại nội dung — `yt-dlp` (video/audio đa nền tảng), `gallery-dl` (ảnh/gallery), `spotiflac-worker` (nhạc lossless, tự viết bọc quanh module Python SpotiFLAC).
- Mỗi công cụ Python được đóng gói dạng PyInstaller onedir và giải nén vào thư mục dữ liệu ứng dụng ở lần chạy đầu — không cần người dùng cài Python.

Tài liệu thiết kế chi tiết từng tính năng nằm trong [`specs/`](specs/), theo phương pháp spec-driven development.
