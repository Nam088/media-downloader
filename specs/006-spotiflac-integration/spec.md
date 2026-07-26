# Feature Specification: Tích Hợp SpotiFLAC Tải Nhạc Lossless FLAC

**Feature Directory**: `specs/006-spotiflac-integration`

**Created**: 2026-07-26

**Status**: Draft

**Input**: Tích hợp module `BartolomeoRusso9/SpotiFLAC-Module-Version` để tải nhạc âm thanh chất lượng cao (FLAC/Lossless) từ các nguồn Spotify, Tidal, Apple Music, SoundCloud,... không cần tài khoản.

---

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Tải nhạc FLAC chất lượng cao từ liên kết Spotify (Priority: P1)

Người dùng dán liên kết nhạc Spotify (Track, Album, Playlist hoặc Artist) vào ứng dụng. Ứng dụng tự động chuyển đổi metadata Spotify sang luồng nhạc âm thanh Lossless FLAC chất lượng cao từ các nhà cung cấp (Tidal, Qobuz, Deezer, Amazon Music) mà không yêu cầu người dùng phải đăng nhập tài khoản.

**Why this priority**: Đây là mục tiêu chính của việc tích hợp SpotiFLAC, giải quyết nhu cầu thưởng thức nhạc chất lượng cao FLAC mà engine `yt-dlp` thông thường không lấy được âm thanh gốc chất lượng audiophile.

**Independent Test**: Dán một URL bài hát Spotify hợp lệ (`https://open.spotify.com/track/...`), chọn định dạng chất lượng FLAC 16-bit / 24-bit Hi-Res, bấm Tải xuống. Xác nhận tệp FLAC được tải về thư mục đầu ra, có đầy đủ ID3 tags (tiêu đề, ca sĩ, album) và phát âm thanh chuẩn FLAC.

**Acceptance Scenarios**:

1. **Given** người dùng dán một liên kết Spotify Track hợp lệ, **When** họ bấm Tải xuống với thiết lập chất lượng FLAC, **Then** hệ thống khớp metadata và tải thành công tệp âm thanh FLAC về máy kèm đầy đủ bìa đĩa (Cover Art) và thông tin bài hát.
2. **Given** người dùng dán một liên kết Spotify Album hoặc Playlist, **When** người dùng xác nhận tải danh sách, **Then** hệ thống thêm từng bài hát vào hàng đợi tải với định dạng FLAC và tiến trình độc lập.

---

### User Story 2 - Cấu hình nguồn phát (Services) & Chất lượng âm thanh FLAC (Priority: P2)

Người dùng có thể chủ động cấu hình thứ tự ưu tiên các nhà cung cấp nguồn phát âm thanh (Tidal, Qobuz, Deezer, Amazon Music) cũng như mức chất lượng mong muốn (FLAC 16-bit Lossless, FLAC 24-bit Hi-Res, MP3 320kbps).

**Why this priority**: Giúp người dùng linh hoạt tùy chỉnh khi một trong các nhà cung cấp nguồn phát gặp sự cố hoặc giới hạn khu vực, đồng thời kiểm soát được dung lượng lưu trữ.

**Independent Test**: Mở cài đặt SpotiFLAC trong ứng dụng, thay đổi ưu tiên nguồn phát từ Tidal sang Qobuz và chọn mức Hi-Res 24-bit. Tiến hành tải 1 bài hát và kiểm tra bitrate tệp FLAC thu được tương ứng với nguồn Qobuz.

**Acceptance Scenarios**:

1. **Given** người dùng thiết lập danh sách nguồn ưu tiên trong Cài đặt, **When** hệ thống tìm bài hát, **Then** hệ thống sẽ thử lần lượt từ nguồn ưu tiên cao nhất xuống thấp hơn nếu nguồn trước đó không có sẵn.
2. **Given** nguồn phát chính bị lỗi kết nối, **When** tính năng JS Extensions fallback bật, **Then** hệ thống tự động kích hoạt extension tương ứng (`ext:tidal-web`, `ext:qobuz-web`) để tiếp tục tải nhạc không gián đoạn.

---

### User Story 3 - Xử lý thông báo xác minh Cloudflare CAPTCHA mượt mà (Priority: P3)

Khi nhà cung cấp nguồn phát yêu cầu xác minh Cloudflare CAPTCHA, hệ thống cung cấp cơ chế thông báo cho người dùng (qua giao diện app hoặc qua Telegram Bot đã cấu hình) để người dùng lấy mã xác minh (grant code) tiếp tục tải nhạc tự động mà không bị dừng đột ngột.

**Why this priority**: Đảm bảo quy trình tải nhạc hoạt động liên tục ngay cả khi gặp sự cố chặn tự động từ Cloudflare.

**Independent Test**: Giả lập hoặc gặp trường hợp Cloudflare challenge, ứng dụng hiển thị đường dẫn xác minh và ô nhập mã `grant`. Sau khi nhập mã, tiến trình tải nhạc được nối lại thành công.

**Acceptance Scenarios**:

1. **Given** một tác vụ tải gặp thử thách Cloudflare, **When** thử thách xuất hiện, **Then** ứng dụng tạm dừng tác vụ, hiển thị liên kết mở trình duyệt xác minh và ô nhập mã grant code.
2. **Given** người dùng đã cấu hình Telegram Bot Token & Chat ID, **When** gặp thử thách Cloudflare, **Then** hệ thống tự động gửi tin nhắn Telegram kèm URL xác minh đến thiết bị cá nhân của người dùng.

---

### Edge Cases

- Link Spotify bài hát không tồn tại trên các nhà cung cấp (Tidal/Qobuz/Deezer/Amazon) → Thông báo lỗi không tìm thấy nguồn phát FLAC tương ứng và đề xuất chuyển sang phương thức tải thông thường qua `yt-dlp`.
- Bài hát bị giới hạn bản quyền theo quốc gia/khu vực → Tự động chuyển sang nhà cung cấp khả dụng tiếp theo trong danh sách ưu tiên.
- Mất kết nối mạng giữa chừng khi đang tải luồng FLAC → Tự động thử lại theo backoff hoặc cho phép thử lại thủ công; mỗi lần thử tải lại track từ đầu (không resume giữa file — track FLAC dung lượng nhỏ nên chấp nhận được).
- Thiếu môi trường Node.js trên máy người dùng khi dùng JS Extensions → Hệ thống tự nhận biết và thông báo hỗ trợ người dùng cài đặt/kích hoạt Node.js hoặc chuyển sang dùng native provider.

---

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: Hệ thống PHẢI tự động nhận diện các định dạng URL liên kết Spotify (Track, Album, Playlist, Artist) cũng như các dịch vụ stream được hỗ trợ khác (Tidal, Apple Music, SoundCloud, YouTube, Pandora). Trong đó Spotify, Tidal, Apple Music và Pandora được định tuyến sang engine SpotiFLAC; SoundCloud và YouTube tiếp tục được nhận diện và xử lý qua engine `yt-dlp` hiện có (hai nguồn này vốn không phân phối lossless).
- **FR-002**: Hệ thống PHẢI tích hợp engine `SpotiFLAC` (dựa trên repo `BartolomeoRusso9/SpotiFLAC-Module-Version`) dưới dạng một module tải nhạc lossless chuyên dụng bên cạnh `yt-dlp` và `gallery-dl`.
- **FR-003**: Hệ thống PHẢI cho phép người dùng tùy chọn mức chất lượng âm thanh mong muốn bao gồm: FLAC 16-bit (Lossless Standard), FLAC 24-bit (Hi-Res Audio) và MP3 320kbps (High Quality).
- **FR-004**: Hệ thống PHẢI cho phép người dùng thiết lập và sắp xếp thứ tự ưu tiên các nhà cung cấp nguồn âm thanh (Tidal, Qobuz, Deezer, Amazon Music).
- **FR-005**: Hệ thống PHẢI hỗ trợ tính năng tự động chuyển đổi sang JS Extensions (`ext:*`) khi các native API của nhà cung cấp gặp sự cố hoặc thay đổi cấu trúc.
- **FR-006**: Hệ thống PHẢI gắn đầy đủ Metadata (Tiêu đề, Nghệ sĩ, Album, Năm phát hành, Track number) và nhúng bìa đĩa (Cover Art) chất lượng cao vào tệp FLAC/MP3 sau khi tải về.
- **FR-007**: Hệ thống PHẢI tích hợp cơ chế bắt và xử lý Cloudflare challenge, hiển thị URL xác minh và ô nhập mã grant ngay trên giao diện ứng dụng.
- **FR-008**: Hệ thống PHẢI hỗ trợ cấu hình tùy chọn Telegram Bot (`TG_BOT_TOKEN`, `TG_CHAT_ID`) trong Cài đặt để gửi thông báo xác minh CAPTCHA từ xa cho người dùng.
- **FR-009**: Hệ thống PHẢI hiển thị tiến trình tải chi tiết (tên bài hát, nhà cung cấp đang dùng, phần trăm tải, tốc độ tải KB/s hoặc MB/s) trên giao diện hàng đợi.
- **FR-010**: Bộ cài đặt ứng dụng PHẢI đóng gói sẵn công cụ phụ trợ SpotiFLAC executable/sidecar hoặc tự động chuẩn bị môi trường chạy phù hợp mà không bắt buộc người dùng thao tác cài thủ công phức tạp.

---

### Key Entities

- **Tác Vụ Tải SpotiFLAC (SpotiFLAC Download Job)**: Đại diện cho tác vụ tải nhạc FLAC; bao gồm URL gốc Spotify/Stream, Metadata bài hát (Title, Artist, Album), Nguồn phát đang dùng (Tidal/Qobuz/Deezer), Mức chất lượng đã chọn (16-bit/24-bit), Trạng thái (Đang khớp nguồn, Đang tải, Chờ CAPTCHA, Hoàn tất, Lỗi).
- **Cấu Hình Nguồn Phát (Provider Profile)**: Danh sách cài đặt ưu tiên nhà cung cấp, thiết lập fallback extension, và tùy chọn mã hóa đầu ra.
- **Thông Tin Xác Minh Cloudflare (Cloudflare Verification State)**: Lưu trữ URL thử thách và trạng thái mã grant code cho tác vụ bị tạm dừng.

---

## Success Criteria *(mandatory)*

1. **Độ chính xác khớp nguồn**: ≥ 95% bài hát từ các liên kết Spotify phổ biến khớp thành công với nguồn âm thanh FLAC tương ứng.
2. **Tốc độ & Chất lượng**: Bài hát tải về đạt đúng định dạng FLAC (độ phân giải 16-bit hoặc 24-bit chuẩn lossless) với đầy đủ bìa đĩa và ID3 tag.
3. **Trải nghiệm người dùng**: 100% các thao tác từ dán link Spotify đến nhận tệp FLAC hoàn tất mượt mượt trên giao diện ứng dụng mà không yêu cầu tài khoản đăng nhập.
4. **Khả năng phục hồi**: 100% tác vụ gặp lỗi nhà cung cấp chính được tự động chuyển sang nhà cung cấp phụ hoặc extension fallback thành công nếu nguồn phụ có sẵn.
