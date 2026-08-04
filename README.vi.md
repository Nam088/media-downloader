# Media Downloader

*[English](README.md)*

Ứng dụng desktop tải video, âm thanh và nhạc lossless từ nhiều nền tảng — chạy trên Windows, macOS và Linux, đóng gói sẵn mọi công cụ cần thiết, không yêu cầu người dùng cài thêm Python hay bất kỳ dependency nào.

Xây trên [Tauri 2](https://tauri.app/) (Rust + React), dùng [yt-dlp](https://github.com/yt-dlp/yt-dlp) và [gallery-dl](https://github.com/mikf/gallery-dl) làm engine tải.

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
| macOS (Apple Silicon) | `.dmg` (tên file kết thúc bằng `_aarch64.dmg`) |
| macOS (Intel) | `.dmg` (tên file kết thúc bằng `_x64.dmg`) |
| Linux | `.deb`, `.rpm`, `.AppImage` |

Không biết Mac của bạn dùng chip gì? Vào menu Apple → **About This Mac** → **Chip**: "Apple M1/M2/M3/M4" là Apple Silicon, "Intel" là Intel.

Không cần cài Python, Node.js hay ffmpeg riêng — toàn bộ đã được đóng gói sẵn trong bộ cài.

> **macOS báo "is damaged and can't be opened"?** App không hề bị hỏng — **cứ bỏ qua thông báo này, đừng bấm Move to Trash.** Do ứng dụng chưa được ký bằng chứng chỉ Apple Developer, nên macOS luôn hiện đúng thông báo này với bất kỳ app chưa ký nào tải từ internet, bất kể bạn dùng trình duyệt gì (Safari, Chrome, Yandex... đều bị như nhau).
>
> Sửa một lần bằng Terminal:
> ```bash
> xattr -cr "/Applications/Media Downloader.app"
> ```
> Rồi mở lại app bình thường. Mỗi lần tải/cài bản mới đều cần chạy lại lệnh này.

## Phát triển

Yêu cầu: Node.js 22+, pnpm, Rust stable, Python 3 (chỉ để build sidecar).

```bash
pnpm install
bash scripts/fetch-dev-binaries.sh   # tải/dựng yt-dlp, gallery-dl, ffmpeg cho máy dev
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
- **Engine tải**: hai engine chạy song song theo loại nội dung — `yt-dlp` (video/audio đa nền tảng), `gallery-dl` (ảnh/gallery).
- Mỗi công cụ Python được đóng gói dạng PyInstaller onedir và giải nén vào thư mục dữ liệu ứng dụng ở lần chạy đầu — không cần người dùng cài Python.

Tài liệu thiết kế chi tiết từng tính năng nằm trong [`specs/`](specs/), theo phương pháp spec-driven development.
