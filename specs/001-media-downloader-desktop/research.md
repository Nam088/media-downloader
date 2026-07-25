# Research: Trình Tải Media Đa Nền Tảng

**Feature**: 001-media-downloader-desktop | **Date**: 2026-07-25

Tài liệu này tổng hợp các quyết định kỹ thuật cho tính năng, giải quyết toàn bộ điểm cần nghiên cứu phát sinh từ Technical Context trong `plan.md`.

## 1. Khung ứng dụng desktop đa nền tảng

**Decision**: Tauri 2.x (lõi Rust + webview hệ điều hành) làm khung ứng dụng chính.

**Rationale**:
- Dùng webview có sẵn của hệ điều hành (WebView2/WKWebView/WebKitGTK) thay vì đóng gói Chromium riêng như Electron → bộ cài đặt nhỏ hơn nhiều lần (thường 10-30MB so với 80-120MB của Electron), khởi động nhanh hơn, tiêu thụ RAM ít hơn.
- Vẫn cho phép xây giao diện bằng React/TypeScript + Tailwind + shadcn/ui → đạt yêu cầu "giao diện xịn, dễ dùng" (FR-015) mà không phải học ngôn ngữ UI mới, đồng thời có sẵn theming Sáng/Tối và hệ sinh thái i18n trưởng thành của React.
- Lõi Rust phù hợp để quản lý tiến trình con (spawn yt-dlp/ffmpeg), xử lý hàng đợi tải đồng thời, và đọc/ghi SQLite một cách an toàn, hiệu năng cao.
- Hỗ trợ chính thức cả Windows, macOS, Linux từ cùng một codebase (đáp ứng FR-010).

**Alternatives considered**:
- **Electron + React**: hệ sinh thái lớn hơn, dễ tuyển dụng hơn, nhưng bundle nặng (đóng gói cả Chromium + Node runtime), RAM cao hơn đáng kể — không tối ưu cho một ứng dụng tiện ích nhỏ gọn.
- **Flutter Desktop**: giao diện đẹp sẵn (Material/Cupertino) và i18n có `flutter_localizations`, nhưng hệ sinh thái desktop (tray icon, tiến trình con, packaging) kém trưởng thành hơn Tauri/Electron; gọi tiến trình con yt-dlp qua Dart `Process` khả thi nhưng ít tài liệu tham khảo hơn.
- **PyQt/PySide (Python)**: cùng ngôn ngữ với yt-dlp nên tích hợp trực tiếp (import thay vì spawn tiến trình), nhưng đạt giao diện "xịn" hiện đại đòi hỏi tự viết QSS tuỳ chỉnh nhiều, và đóng gói cross-platform bằng PyInstaller thường cho kích thước lớn, không ổn định bằng Tauri.
- **.NET MAUI / Avalonia (C#)**: cross-platform tốt, nhưng hệ sinh thái component UI "đẹp sẵn" và i18n phong phú kém hơn so với React, và ít phù hợp nếu muốn tái sử dụng thư viện JS cho UI.

## 2. Engine tải/trích xuất media

**Decision**: `yt-dlp` (fork chủ động bảo trì của `youtube-dl`) làm engine trích xuất và tải media, gọi qua tiến trình con (đóng gói kèm ứng dụng, không yêu cầu người dùng tự cài đặt).

**Yêu cầu bắt buộc (FR-018)**: yt-dlp và ffmpeg PHẢI được đóng gói kèm ứng dụng — binary thực thi độc lập cho từng hệ điều hành (`.exe` cho Windows, binary macOS, binary Linux) được tải về và nhúng thẳng vào trình cài đặt trong bước build/CI, KHÔNG phải tải về lúc runtime và KHÔNG yêu cầu người dùng cuối cài đặt Python, pip, hay bất kỳ package manager nào. ffmpeg vẫn là "sidecar binary" của Tauri (`externalBin`); yt-dlp thì KHÔNG (xem sửa lại bên dưới).

**Sửa lại (2026-07-25) — kiến trúc đóng gói yt-dlp đổi từ "onefile" sidecar sang "onedir" resource**: Bản build "onefile" ban đầu (một file thực thi tự-giải-nén runtime Python bên trong) là nguyên nhân khiến `preview_media`/download chậm hẳn — mỗi lần gọi yt-dlp, nó phải giải nén lại toàn bộ runtime vào một thư mục tạm MỚI (đo thực tế ~14 giây chỉ cho `--version`), trong khi bản "onedir" (thực thi kèm sẵn thư mục `_internal/` đã giải nén sẵn trên đĩa) chỉ mất ~0.3 giây mỗi lần gọi sau khi đã có mặt trên đĩa thật. Vì `externalBin` của Tauri chỉ hỗ trợ một file duy nhất (không hỗ trợ cả một thư mục), yt-dlp onedir được đóng gói dưới dạng Tauri *resource* (`bundle.resources` trong `tauri.conf.json`, ánh xạ `binaries/yt-dlp-onedir` → `yt-dlp-onedir`) rồi được copy một lần duy nhất vào thư mục app-data của ứng dụng ở lần chạy đầu tiên (`downloader::ytdlp_binary::resolve_ytdlp_executable`, cache theo `OnceCell` cho suốt vòng đời tiến trình + đánh dấu phiên bản để tự copy lại khi ứng dụng được cập nhật). ffmpeg vẫn giữ nguyên là sidecar `externalBin` một-file thông thường vì nó là binary C biên dịch sẵn, không có chi phí tự-giải-nén như onefile Python — chỉ khác là giờ yt-dlp gọi nó qua đường dẫn tường minh `--ffmpeg-location` thay vì để yt-dlp tự dò `PATH` hệ thống (lỗi tiềm ẩn trước đó khiến ffmpeg sidecar đóng gói kèm chưa từng thực sự được yt-dlp dùng tới). Do đó `tauri_plugin_shell` không còn cần thiết (không còn sidecar nào được spawn qua plugin này) và đã được gỡ khỏi `Cargo.toml`/`capabilities/default.json`. Quy trình CI/release đã cập nhật: (1) tải zip bản "onedir" chính thức của yt-dlp (`yt-dlp_win.zip`/`yt-dlp_macos.zip`/`yt-dlp_linux.zip`) và giải nén vào `src-tauri/binaries/yt-dlp-onedir/`, (2) tải binary ffmpeg tĩnh (static build) cho cả 3 hệ điều hành theo quy ước đặt tên target-triple của Tauri, (3) build trình cài đặt cho từng hệ điều hành kèm sẵn cả hai.

**Sửa lại (2026-07-25) — TikTok đôi khi trả file video không có âm thanh dù metadata báo có (yt-dlp issue #15891)**: Ngoài lỗi thiếu `-f "bestaudio/best"` khi tách âm thanh (đã sửa trước đó), có một lỗi RIÊNG do chính TikTok gây ra: cùng một `format id` mà TikTok trả về đôi khi là file có cả video+audio, đôi khi lại là file chỉ có video — dù metadata luôn báo `acodec=aac` — nên yt-dlp không có cách nào tự phát hiện sai lệch này và vẫn báo tải thành công. Nhóm bảo trì yt-dlp xác nhận đây là lỗi phía server TikTok, không phải lỗi parse của yt-dlp, và tải lại thường sẽ nhận được file khác (đúng). Đã vá bằng `downloader::ytdlp::output_has_audio_stream` (dùng ffmpeg remux luồng audio sang null để kiểm tra, không cần đóng gói thêm `ffprobe`) + `downloader::queue::run_job` tự động tải lại tối đa `MAX_DOWNLOAD_ATTEMPTS` lần nếu file tải về thiếu audio, trước khi báo lỗi `DOWNLOAD_FAILED` rõ ràng cho người dùng.

**Sửa thêm (2026-07-25) — tăng cường theo nghiên cứu trực tiếp trên chính issue #15891 và issue liên quan #15642 (cả hai đều KHÔNG có bản vá chính thức nào trong yt-dlp — #15642 chỉ được đóng vì người báo cáo tự tìm cách xử lý phía client, không phải yt-dlp fix)**: dựa theo các workaround được cộng đồng xác nhận có hiệu quả thực tế trong chính các issue đó, đã bổ sung 3 lớp phòng thủ:
  1. **Ưu tiên H.264 mạnh hơn**: thêm `--format-sort vcodec:avc` (đề xuất trực tiếp từ maintainer `DTrombett` trong issue) bên cạnh `-f` selector avc1-first có sẵn — lỗi được báo cáo xảy ra với `bytevc1`/h265 nhiều hơn hẳn h264.
  2. **Tải lại cả khi yt-dlp thất bại hẳn**, không chỉ khi tải "thành công" nhưng thiếu audio — CDN của TikTok có thể làm cả request thất bại hoàn toàn, không chỉ âm thầm mất audio.
  3. **Khôi phục bằng cách ghép audio riêng (last resort)**: nếu hết `MAX_DOWNLOAD_ATTEMPTS` lần mà video vẫn thiếu audio, `downloader::queue::recover_missing_audio` tải riêng một bản `bestaudio` từ cùng URL rồi dùng ffmpeg ghép (`-map 0:v:0 -map 1:a:0 -c copy`) vào video đã tải — mô phỏng đúng cách nhiều người báo cáo trên issue #15642 tự làm thủ công (tải audio riêng từ một request khác rồi ghép vào), vì tải lại NGUYÊN VẸN cùng một format có thể vẫn nhận được đúng file lỗi đó (nhất là với các video TikTok đã tắt tính năng tải xuống, theo báo cáo `Worldgate`/`weskerty` trong issue).

**Rationale**:
- Hỗ trợ sẵn hàng nghìn trang (bản `yt-dlp --list-extractors` thực tế đã kiểm: 1.752 extractor, ~1.611 cái đang hoạt động), bao gồm toàn bộ 6 nền tảng bắt buộc trong FR-014 (YouTube, TikTok, Facebook, Instagram, Twitter/X, SoundCloud) mà không cần tự viết extractor riêng cho từng nền tảng. **Sửa lại (2026-07-25)**: bản cài đặt đầu tiên của `preview_media` tự ý chặn trước bằng danh sách 6 domain cố định, vô tình từ chối mọi liên kết khác dù yt-dlp thừa sức xử lý — đã sửa để hệ thống chỉ hỏi thẳng yt-dlp, chỉ báo `UNSUPPORTED_PLATFORM` khi chính yt-dlp xác nhận không có extractor (xem `commands::media::resolve_platform_label`, `downloader::ytdlp::classify_ytdlp_error`).
- Cộng đồng cập nhật thường xuyên khi các nền tảng thay đổi API/cấu trúc, giảm rủi ro ứng dụng bị hỏng khi YouTube/TikTok... thay đổi.
- Hỗ trợ sẵn output định dạng/chất lượng, tách audio, và giới hạn được các luồng DRM/riêng tư (báo lỗi rõ ràng khi không truy cập được) — phù hợp FR-009, FR-011, FR-012.
- Có sẵn khả năng phát hiện/liệt kê playlist (`--flat-playlist`) phục vụ đúng luồng hỏi người dùng ở FR-013.
- Xuất tiến trình dạng text có thể parse được (`--newline` + progress template) để hiển thị % / tốc độ / ETA theo FR-005.

**Alternatives considered**:
- **Tự viết extractor riêng cho từng nền tảng** (gọi trực tiếp API/giải mã stream): kiểm soát tốt hơn nhưng khối lượng công việc rất lớn, dễ vỡ khi nền tảng thay đổi, và không có lý do để đánh đổi so với một dự án mã nguồn mở đã giải quyết đúng vấn đề này.
- **libmpv/youtube-dl gốc**: `youtube-dl` gốc cập nhật chậm hơn nhiều so với `yt-dlp` (fork năng động hơn) — chọn `yt-dlp`.

## 3. Xử lý âm thanh

**Decision**: `ffmpeg` (đóng gói kèm làm sidecar, yt-dlp gọi ffmpeg làm postprocessor) để trích xuất/chuyển mã sang MP3 ở 2 mức chất lượng theo FR-004.

**Rationale**: yt-dlp đã tích hợp sẵn cơ chế gọi ffmpeg làm postprocessor (`--extract-audio --audio-format mp3 --audio-quality`), không cần tự viết pipeline chuyển mã.

**Alternatives considered**: Thư viện chuyển mã thuần Rust (ví dụ `symphonia`) — chỉ hỗ trợ decode, không đủ để encode MP3/xử lý mux đầy đủ như ffmpeg; không đáng đánh đổi.

## 4. Lưu trữ hàng đợi & lịch sử

**Decision**: SQLite cục bộ qua `rusqlite`.

**Rationale**: Dữ liệu có cấu trúc quan hệ rõ ràng (tác vụ tải, lịch sử, cài đặt) và cần truy vấn/lọc (theo trạng thái, thời gian) — SQLite phù hợp hơn một file JSON phẳng khi lịch sử tăng dần theo thời gian, đồng thời không yêu cầu server riêng (phù hợp ứng dụng desktop đơn người dùng).

**Alternatives considered**: Lưu JSON phẳng — đơn giản hơn ban đầu nhưng sẽ chậm dần và khó truy vấn khi lịch sử lớn; SQLite có chi phí thiết lập không đáng kể qua `rusqlite` nên được chọn ngay từ đầu.

## 5. Đa ngôn ngữ (i18n)

**Decision**: `react-i18next`, văn bản tách thành file JSON theo ngôn ngữ (`src/locales/en.json`, `src/locales/vi.json`), tự phát hiện ngôn ngữ hệ điều hành lúc khởi chạy lần đầu và cho phép người dùng đổi thủ công (FR-017).

**Rationale**: Là thư viện i18n phổ biến và trưởng thành nhất cho React, hỗ trợ tách namespace, fallback ngôn ngữ (đáp ứng edge case "hệ điều hành dùng ngôn ngữ chưa hỗ trợ → mặc định Tiếng Anh"), và thêm ngôn ngữ mới chỉ cần thêm 1 file JSON, không cần sửa logic ứng dụng.

**Alternatives considered**: Tự viết cơ chế i18n tối giản (dictionary lookup thủ công) — đủ dùng cho 2 ngôn ngữ nhưng thiếu các tiện ích (pluralization, interpolation, fallback) mà react-i18next đã cung cấp sẵn; không có lý do để tự xây lại.

## 6. Theming Sáng/Tối

**Decision**: Tailwind CSS `dark:` variant + CSS variables cho design tokens, kết hợp component shadcn/ui (đã hỗ trợ sẵn 2 theme).

**Rationale**: Cho phép chuyển theme tức thời chỉ bằng toggle 1 class trên phần tử gốc (đáp ứng SC-006 "chuyển đổi trong 1 thao tác, cập nhật ngay lập tức"), không cần build lại hay tải lại trang.

**Alternatives considered**: CSS-in-JS theme provider tuỳ chỉnh — khả thi nhưng tốn công thiết lập hơn trong khi Tailwind + shadcn/ui đã giải quyết sẵn vấn đề này.

## 7. Kiểm thử

**Decision**: `cargo test` (Rust), Vitest + React Testing Library (frontend), `tauri-driver` + WebdriverIO (smoke E2E cho luồng P1).

**Rationale**: Đây là bộ công cụ kiểm thử chính thức được khuyến nghị trong tài liệu Tauri, đảm bảo cả logic backend (hàng đợi, gọi tiến trình con, DB) và giao diện đều được kiểm thử mà không cần thêm framework ngoài hệ sinh thái đã chọn.

## 8. Nguyên tắc không viết cứng (hard-code) tuỳ chọn tải

**Decision**: Mọi tuỳ chọn chất lượng/định dạng hiển thị cho người dùng (mức chất lượng âm thanh, độ phân giải video, có phải playlist) PHẢI lấy từ kết quả `preview_media` (yt-dlp `--dump-json`, trường `formats`) của chính liên kết đang xử lý — không dùng danh sách tĩnh viết sẵn trong code (kiểu `["128kbps", "320kbps"]` cố định cho mọi link). Backend validate lại giá trị frontend gửi lên khớp với danh sách đã trả trước đó (FR-019).

**Rationale**: Các nền tảng khác nhau (YouTube, TikTok, SoundCloud, ...) và thậm chí từng video riêng lẻ có bộ luồng audio/video khả dụng khác nhau. Một danh sách chất lượng cố định trong app dễ dẫn tới hiển thị tuỳ chọn không có thật (vd hiện "320kbps" trong khi nguồn chỉ có luồng gốc 64kbps) hoặc chặn nhầm các mức chất lượng cao hơn mà nguồn thực sự có. Lấy động từ `formats` đảm bảo UI luôn phản ánh đúng khả năng thực tế của từng liên kết.

**Alternatives considered**: Bảng ánh xạ tĩnh theo từng platform (vd "YouTube luôn có 128/320kbps") — bị loại vì không chính xác ở cấp video đơn lẻ (phụ thuộc uploader, độ dài, cấu hình riêng của từng video) và phá vỡ nguyên tắc chỉ hiển thị tuỳ chọn thực sự tồn tại.

## Kết luận

Không còn điểm "NEEDS CLARIFICATION" nào trong Technical Context. Toàn bộ lựa chọn công nghệ đã được chốt và sẵn sàng cho Phase 1 (Design & Contracts).
