# Feature Specification: Media Output — Định dạng, Metadata, Phụ đề, Cắt ghép

**Feature Branch**: `003-media-output`

**Created**: 2026-07-26

**Status**: Draft

**Phase**: 2/4 (xem `specs/ROADMAP.md`)

**Phụ thuộc**: Phase 1 (`002-download-power`) — preset chỉ có ý nghĩa khi áp được cho lô nhiều URL.

## Bối cảnh kỹ thuật

Hiện trạng đã xác minh trong mã nguồn:

- Audio **luôn luôn** là MP3: `--audio-format mp3` viết cứng tại `src-tauri/src/downloader/queue.rs:931`. Bitrate lấy từ lựa chọn người dùng qua `--audio-quality {N}K` (`:932-948`).
- Video **luôn luôn** là MP4/H.264/AAC: `--merge-output-format mp4` (`:966`), `--format-sort vcodec:avc` (`:972`), chuỗi chọn format ưu tiên `avc1`+`mp4a` (`:1000-1011`).
- Nhãn giao diện đang nói sai một nửa: `DownloadForm.tsx:112` hiển thị `MP3 / {codec nguồn}` (ví dụ "MP3 / OPUS"), `:119` là chuỗi literal `"MP4 / H264 / AAC"` không suy ra từ dữ liệu.
- Template đặt tên file viết cứng `"{output_directory}/%(title)s.%(ext)s"` tại `queue.rs:206`.
- Không có cờ nào liên quan tới metadata, thumbnail, phụ đề, cắt đoạn, hay chapter trong `build_ytdlp_args` (`queue.rs:905-988`).
- ffmpeg đã được bundle sẵn dưới dạng sidecar (`tauri.conf.json:38`) và đã được truyền cho yt-dlp qua `--ffmpeg-location` (`ytdlp.rs:119-120`) — mọi hậu xử lý của yt-dlp đều dùng được ngay.
- Bảng `download_jobs` hiện có các cột riêng cho từng lựa chọn (`audio_quality`, `video_quality`, `gallery_mode`, `selected_gallery_urls`). Spec này bổ sung nhiều lựa chọn đầu ra mới; cần quyết định ở bước lập kế hoạch giữa việc thêm cột cho từng lựa chọn hay gom vào một cột JSON — khuyến nghị gom JSON vì các lựa chọn này chỉ được đọc bởi bộ chạy tác vụ, không bao giờ dùng để truy vấn hay lọc.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Chọn định dạng đầu ra (Priority: P1)

Người dùng muốn file M4A để bỏ vào thư viện Apple Music, hoặc FLAC để nghe không mất chất lượng, thay vì luôn nhận MP3. Với video, họ muốn giữ nguyên chất lượng gốc (VP9/AV1 trong MKV) thay vì bị ép chuyển sang H.264.

**Why this priority**: Đây là giới hạn cứng rõ ràng nhất của v1 và cũng là thứ người dùng nhận ra ngay lập tức.

**Independent Test**: Tải cùng một link ba lần với ba định dạng audio khác nhau, xác nhận ba file có đúng phần mở rộng và đúng codec bên trong.

**Acceptance Scenarios**:

1. **Given** người dùng đang xem trước một liên kết, **When** họ chọn một định dạng audio trong danh sách, **Then** file tải về có đúng định dạng đó.
2. **Given** người dùng chọn một định dạng không nén (không mất dữ liệu), **When** tải xong, **Then** hệ thống không áp mức bitrate và không hiện lựa chọn bitrate cho định dạng đó.
3. **Given** người dùng chọn "giữ nguyên định dạng gốc", **When** tải xong, **Then** không có bước chuyển mã nào chạy và file giữ nguyên codec từ nguồn.
4. **Given** nguồn không có định dạng người dùng chọn ở dạng gốc, **When** tải, **Then** hệ thống chuyển mã sang định dạng đó và báo cho người dùng biết có bước chuyển mã.
5. **Given** người dùng đang xem lựa chọn chất lượng, **When** nhãn hiển thị, **Then** nhãn phản ánh đúng định dạng đầu ra sẽ nhận được, không hiển thị nhãn cố định như hiện nay.

---

### User Story 2 - Nhúng thông tin và ảnh bìa vào file (Priority: P1)

Người dùng tải một album nhạc. Mở lên trong trình phát, họ thấy đúng tên bài, nghệ sĩ, và ảnh bìa thay vì một file trống trơn chỉ có tên.

**Why this priority**: Với người tải nhạc — tệp người dùng chính của ứng dụng — đây là khác biệt giữa "một file" và "một bài hát trong thư viện".

**Independent Test**: Tải một link nhạc với tuỳ chọn nhúng bật, mở file bằng trình phát bất kỳ, xác nhận thấy tiêu đề, nghệ sĩ và ảnh bìa.

**Acceptance Scenarios**:

1. **Given** tuỳ chọn nhúng thông tin đang bật, **When** tải xong một file audio, **Then** file chứa tiêu đề, nghệ sĩ, tên nguồn và ngày đăng lấy từ nguồn.
2. **Given** tuỳ chọn nhúng ảnh bìa đang bật và nguồn có ảnh thu nhỏ, **When** tải xong, **Then** ảnh bìa được nhúng vào file.
3. **Given** định dạng đầu ra không hỗ trợ nhúng ảnh bìa, **When** tải, **Then** hệ thống bỏ qua bước nhúng và ghi rõ lý do trong nhật ký, không coi là lỗi.
4. **Given** nguồn không cung cấp thông tin nghệ sĩ, **When** tải xong, **Then** các trường thiếu để trống chứ không điền giá trị bịa.

---

### User Story 3 - Đặt tên file theo mẫu (Priority: P2)

Người dùng tải nhiều playlist và muốn file có dạng `01 - Tên bài.mp3` hoặc `[Kênh] Tiêu đề (2026).mp4` thay vì chỉ mỗi tiêu đề.

**Why this priority**: Quan trọng với người tải nhiều, nhưng người dùng thông thường vẫn ổn với mặc định.

**Independent Test**: Đặt mẫu tên file có chứa số thứ tự và tên kênh, tải một playlist, xác nhận tên file khớp mẫu.

**Acceptance Scenarios**:

1. **Given** người dùng nhập một mẫu tên file, **When** họ đang nhập, **Then** hệ thống hiển thị ngay ví dụ tên file kết quả dựa trên nội dung đang xem trước.
2. **Given** mẫu tên file chứa trường mà nguồn không có, **When** tải, **Then** trường đó được thay bằng giá trị dự phòng đã định nghĩa chứ không tạo tên file rỗng hoặc lỗi.
3. **Given** mẫu sinh ra tên file chứa ký tự hệ điều hành không cho phép hoặc dài quá giới hạn, **When** tải, **Then** hệ thống tự làm sạch và rút gọn tên, đảm bảo lưu được trên cả ba hệ điều hành.
4. **Given** hai nội dung khác nhau sinh ra cùng một tên file, **When** tải cả hai, **Then** file thứ hai được thêm hậu tố phân biệt, không ghi đè file thứ nhất.

---

### User Story 4 - Tải phụ đề (Priority: P2)

Người dùng tải video nước ngoài và muốn kèm phụ đề tiếng Việt và tiếng Anh, hoặc nhúng thẳng phụ đề vào file để xem trên TV.

**Independent Test**: Tải một video có phụ đề với hai ngôn ngữ được chọn, xác nhận hai file phụ đề xuất hiện cạnh video và mở được.

**Acceptance Scenarios**:

1. **Given** một video có phụ đề, **When** người dùng xem trước, **Then** danh sách ngôn ngữ phụ đề thực tế có sẵn được hiển thị để chọn — không phải danh sách cố định.
2. **Given** người dùng chọn "lưu thành file riêng", **When** tải xong, **Then** các file phụ đề nằm cạnh file media với tên tương ứng.
3. **Given** người dùng chọn "nhúng vào file", **When** tải xong và định dạng đầu ra hỗ trợ, **Then** phụ đề nằm trong file media dưới dạng track chọn được.
4. **Given** video chỉ có phụ đề tự động sinh, **When** người dùng xem trước, **Then** hệ thống ghi rõ đó là phụ đề tự động và cho phép chọn riêng.
5. **Given** video không có phụ đề nào, **When** người dùng xem trước, **Then** phần chọn phụ đề bị ẩn hoặc vô hiệu hoá kèm giải thích, không hiện danh sách rỗng khó hiểu.

---

### User Story 5 - Cắt một đoạn của video (Priority: P2)

Người dùng chỉ muốn phút 12:30 đến 15:00 của một video dài 2 tiếng, không muốn tải cả file rồi tự cắt.

**Independent Test**: Nhập khoảng thời gian, tải, xác nhận file kết quả có đúng thời lượng mong đợi (sai số vài giây chấp nhận được) và nội dung đúng đoạn đã chọn.

**Acceptance Scenarios**:

1. **Given** người dùng nhập thời điểm bắt đầu và kết thúc, **When** tải, **Then** file kết quả chỉ chứa đoạn đó.
2. **Given** người dùng chỉ nhập thời điểm bắt đầu, **When** tải, **Then** file chứa từ thời điểm đó tới hết.
3. **Given** người dùng nhập khoảng thời gian không hợp lệ (kết thúc trước bắt đầu, hoặc vượt quá thời lượng nội dung), **When** họ rời khỏi ô nhập, **Then** hệ thống báo lỗi ngay tại chỗ và không cho tạo tác vụ.
4. **Given** người dùng bật tuỳ chọn cắt chính xác, **When** tải, **Then** điểm cắt khớp thời gian yêu cầu, đổi lại thời gian xử lý lâu hơn và hệ thống báo trước điều đó.

---

### User Story 6 - Tách video theo chương (Priority: P3)

Một video podcast dài có sẵn các chương. Người dùng muốn mỗi chương thành một file riêng.

**Independent Test**: Tải một video có chương với tuỳ chọn tách bật, xác nhận số file kết quả bằng số chương và tên file chứa tên chương.

**Acceptance Scenarios**:

1. **Given** nội dung có sẵn danh sách chương, **When** người dùng xem trước, **Then** số chương được hiển thị và tuỳ chọn tách theo chương khả dụng.
2. **Given** tuỳ chọn tách theo chương đang bật, **When** tải xong, **Then** mỗi chương là một file riêng, đặt tên theo mẫu có chứa tên chương.
3. **Given** nội dung không có chương, **When** người dùng xem trước, **Then** tuỳ chọn tách theo chương bị vô hiệu hoá kèm giải thích.
4. **Given** tác vụ tách chương hoàn tất, **When** người dùng xem hàng đợi và lịch sử, **Then** tác vụ hiển thị là một mục với số file kết quả, không phải nhiều mục rời rạc.

---

### User Story 7 - Lưu cấu hình thành preset (Priority: P3)

Người dùng luôn tải podcast ở dạng M4A 128 kbps có metadata, và luôn tải phim ở MKV 1080p có phụ đề. Họ lưu hai bộ cấu hình và chọn bằng một cú bấm thay vì chỉnh lại mỗi lần.

**Independent Test**: Lưu một preset, tắt mở lại ứng dụng, áp preset đó cho một link mới, xác nhận mọi tuỳ chọn khớp với lúc lưu.

**Acceptance Scenarios**:

1. **Given** người dùng đã cấu hình xong các lựa chọn đầu ra, **When** họ lưu thành preset có tên, **Then** preset xuất hiện trong danh sách và tồn tại qua các lần khởi động.
2. **Given** một preset đã lưu, **When** người dùng áp nó cho một liên kết, **Then** mọi lựa chọn đầu ra được điền sẵn theo preset.
3. **Given** preset chứa mức chất lượng mà nguồn hiện tại không có, **When** áp preset, **Then** hệ thống chọn mức gần nhất có sẵn và báo rõ đã thay đổi gì.
4. **Given** người dùng đang tải một lô nhiều URL, **When** họ chọn một preset, **Then** preset áp cho toàn bộ lô.
5. **Given** người dùng đặt một preset làm mặc định, **When** họ xem trước một liên kết mới, **Then** các lựa chọn được điền sẵn theo preset mặc định đó.

---

### Edge Cases

- Chọn định dạng không mất dữ liệu cho một nguồn vốn chỉ có audio nén → cảnh báo rõ rằng chuyển đổi không khôi phục được chất lượng đã mất, file chỉ to hơn.
- Chọn giữ nguyên định dạng gốc nhưng nguồn trả về container lạ → vẫn lưu được file, ghi rõ định dạng thực tế trong lịch sử.
- Nhúng ảnh bìa cho định dạng không hỗ trợ → bỏ qua có thông báo, không làm hỏng file.
- Mẫu tên file sinh ra đường dẫn vượt giới hạn độ dài của Windows → rút gọn an toàn, không thất bại.
- Mẫu tên file chứa dấu tách thư mục → quyết định rõ ràng: cho phép tạo thư mục con hay từ chối, và nhất quán trên cả ba hệ điều hành.
- Chọn cắt đoạn cho nội dung là luồng trực tiếp đang phát → không hỗ trợ, thông báo rõ.
- Tách chapter kết hợp với cắt đoạn → hai tuỳ chọn loại trừ nhau, giao diện phải phản ánh điều đó.
- Phụ đề tự động sinh cho ngôn ngữ không có trong danh sách chuẩn → vẫn hiển thị được, không lọc mất.
- Preset lưu từ phiên bản cũ, sau này có thêm tuỳ chọn mới → tuỳ chọn mới nhận giá trị mặc định, preset không bị hỏng.
- Nội dung dạng thư viện ảnh không áp dụng được phần lớn các tuỳ chọn ở spec này → giao diện phải ẩn chúng thay vì hiện rồi bỏ qua âm thầm.

## Requirements *(mandatory)*

### Functional Requirements

**Định dạng đầu ra**

- **FR-201**: Người dùng PHẢI chọn được định dạng audio đầu ra trong số ít nhất: MP3, M4A/AAC, Opus, WAV, FLAC, và tuỳ chọn "giữ nguyên định dạng gốc".
- **FR-202**: Khi người dùng chọn "giữ nguyên định dạng gốc", hệ thống KHÔNG được chạy bước chuyển mã nào.
- **FR-203**: Lựa chọn bitrate CHỈ được hiển thị cho các định dạng có nén mất dữ liệu; với định dạng không mất dữ liệu hoặc giữ nguyên gốc, lựa chọn bitrate PHẢI bị ẩn.
- **FR-204**: Người dùng PHẢI chọn được container video đầu ra trong số ít nhất: MP4, MKV, và "giữ nguyên định dạng gốc".
- **FR-205**: Người dùng PHẢI chọn được giữa "ưu tiên tương thích" (mã hoá H.264/AAC như hiện nay) và "ưu tiên chất lượng" (nhận codec tốt nhất nguồn có, kể cả VP9/AV1); mặc định là ưu tiên tương thích.
- **FR-206**: Nhãn hiển thị của mỗi lựa chọn chất lượng PHẢI phản ánh đúng định dạng và codec đầu ra thực tế sẽ nhận được, suy ra từ dữ liệu nguồn và lựa chọn hiện tại — KHÔNG được là chuỗi cố định viết cứng.
- **FR-207**: Khi lựa chọn của người dùng buộc phải chuyển mã, hệ thống PHẢI báo cho họ biết trước khi tạo tác vụ.

**Metadata**

- **FR-208**: Hệ thống PHẢI có tuỳ chọn (mặc định BẬT) nhúng thông tin nội dung — tiêu đề, tác giả/kênh, nguồn, ngày đăng — vào file kết quả.
- **FR-209**: Hệ thống PHẢI có tuỳ chọn (mặc định BẬT) nhúng ảnh thu nhỏ làm ảnh bìa khi định dạng đầu ra hỗ trợ.
- **FR-210**: Khi định dạng đầu ra không hỗ trợ nhúng thông tin hoặc ảnh bìa, hệ thống PHẢI bỏ qua bước đó, ghi lý do vào nhật ký, và KHÔNG coi tác vụ là thất bại.
- **FR-211**: Các trường thông tin không có ở nguồn PHẢI để trống, KHÔNG được điền giá trị suy đoán.

**Đặt tên file**

- **FR-212**: Người dùng PHẢI đặt được mẫu tên file, với tối thiểu các trường: tiêu đề, tên kênh/tác giả, số thứ tự trong playlist, ngày đăng, độ phân giải, phần mở rộng.
- **FR-213**: Giao diện PHẢI hiển thị ví dụ tên file kết quả ngay khi người dùng đang chỉnh mẫu, dựa trên nội dung đang xem trước.
- **FR-214**: Hệ thống PHẢI làm sạch tên file sinh ra để hợp lệ trên Windows, macOS và Linux, và rút gọn khi vượt giới hạn độ dài đường dẫn.
- **FR-215**: Khi tên file sinh ra trùng với file đã tồn tại, hệ thống PHẢI thêm hậu tố phân biệt thay vì ghi đè.
- **FR-216**: Trường trong mẫu mà nguồn không cung cấp PHẢI được thay bằng giá trị dự phòng xác định trước, không tạo tên rỗng.

**Phụ đề**

- **FR-217**: Ở bước xem trước, hệ thống PHẢI liệt kê các ngôn ngữ phụ đề thực tế có sẵn tại nguồn, phân biệt rõ phụ đề do người tạo cung cấp và phụ đề tự động sinh; KHÔNG được hiển thị danh sách ngôn ngữ cố định.
- **FR-218**: Người dùng PHẢI chọn được nhiều ngôn ngữ phụ đề cùng lúc.
- **FR-219**: Người dùng PHẢI chọn được giữa lưu phụ đề thành file riêng hoặc nhúng vào file media.
- **FR-220**: Khi định dạng đầu ra không hỗ trợ nhúng phụ đề, tuỳ chọn nhúng PHẢI bị vô hiệu hoá kèm giải thích.
- **FR-221**: Khi nguồn không có phụ đề nào, phần chọn phụ đề PHẢI bị ẩn hoặc vô hiệu hoá kèm giải thích.

**Cắt đoạn và chương**

- **FR-222**: Người dùng PHẢI nhập được thời điểm bắt đầu và/hoặc kết thúc để chỉ tải một đoạn của nội dung.
- **FR-223**: Hệ thống PHẢI kiểm tra tính hợp lệ của khoảng thời gian ngay tại giao diện và chặn tạo tác vụ khi không hợp lệ.
- **FR-224**: Hệ thống PHẢI có tuỳ chọn cắt chính xác tại thời điểm yêu cầu, kèm cảnh báo rằng bước này làm tăng thời gian xử lý.
- **FR-225**: Khi nội dung có sẵn danh sách chương, hệ thống PHẢI hiển thị số chương và cho phép tách mỗi chương thành một file riêng.
- **FR-226**: Tuỳ chọn tách chương và tuỳ chọn cắt đoạn PHẢI loại trừ lẫn nhau trong giao diện.
- **FR-227**: Một tác vụ tách chương PHẢI hiển thị là một mục duy nhất trong hàng đợi và lịch sử, kèm số file kết quả.

**Preset**

- **FR-228**: Người dùng PHẢI lưu được cấu hình đầu ra hiện tại thành một preset có tên, và preset PHẢI tồn tại qua các lần khởi động.
- **FR-229**: Người dùng PHẢI áp, sửa, đổi tên, và xoá được preset.
- **FR-230**: Người dùng PHẢI đặt được một preset làm mặc định, tự áp cho mọi liên kết mới xem trước.
- **FR-231**: Khi áp preset mà nguồn hiện tại không có mức chất lượng trong preset, hệ thống PHẢI chọn mức gần nhất có sẵn và nêu rõ đã thay đổi gì.
- **FR-232**: Preset PHẢI áp được cho toàn bộ một lô nhiều URL.
- **FR-233**: Preset lưu từ phiên bản trước PHẢI vẫn dùng được khi có tuỳ chọn mới được thêm vào; tuỳ chọn mới nhận giá trị mặc định.

**Chung**

- **FR-234**: Với nội dung dạng thư viện ảnh, các tuỳ chọn không áp dụng được PHẢI bị ẩn khỏi giao diện thay vì hiển thị rồi bị bỏ qua âm thầm.
- **FR-235**: Mọi lựa chọn đầu ra PHẢI được lưu cùng tác vụ để việc thử lại tái tạo đúng cấu hình ban đầu.

### Key Entities

- **Tuỳ chọn đầu ra (Output Options)**: Tập hợp lựa chọn gắn với một tác vụ — định dạng audio, bitrate, container video, ưu tiên codec, bật/tắt nhúng thông tin, bật/tắt ảnh bìa, mẫu tên file, danh sách ngôn ngữ phụ đề, chế độ phụ đề, khoảng thời gian cắt, bật/tắt tách chương.
- **Preset**: Một `Tuỳ chọn đầu ra` có tên, lưu lâu dài, có cờ đánh dấu mặc định.
- **Phụ đề có sẵn (Available Subtitle)**: Thông tin xem trước — mã ngôn ngữ, tên hiển thị, có phải tự động sinh hay không.
- **Chương (Chapter)**: Thông tin xem trước — tiêu đề, thời điểm bắt đầu, thời điểm kết thúc.
- **Tác vụ tải (Download Job)**: Bổ sung so với Phase 1 — mang theo `Tuỳ chọn đầu ra` đã dùng, và số file kết quả (lớn hơn 1 khi tách chương).

## Success Criteria *(mandatory)*

- **SC-201**: Người dùng tải được cùng một nội dung ra ít nhất 5 định dạng audio khác nhau, mỗi lần file kết quả đúng định dạng đã chọn trong 100% trường hợp.
- **SC-202**: Với tuỳ chọn "giữ nguyên định dạng gốc", thời gian hoàn tất giảm đáng kể so với có chuyển mã, và không có tiến trình chuyển mã nào được chạy.
- **SC-203**: 100% file audio tải với tuỳ chọn nhúng bật hiển thị đúng tiêu đề và nghệ sĩ trong ít nhất hai trình phát phổ biến khác nhau.
- **SC-204**: Nhãn lựa chọn chất lượng khớp với định dạng file thực tế nhận được trong 100% trường hợp — không còn trường hợp nhãn ghi một đằng file một nẻo.
- **SC-205**: Mẫu tên file do người dùng đặt cho ra tên file hợp lệ và lưu được trên cả ba hệ điều hành trong 100% trường hợp thử nghiệm, kể cả với tiêu đề chứa emoji, ký tự tiếng Việt có dấu, và ký tự đặc biệt.
- **SC-206**: Danh sách ngôn ngữ phụ đề hiển thị khớp hoàn toàn với danh sách nguồn thực tế cung cấp, không thừa không thiếu.
- **SC-207**: Cắt đoạn với tuỳ chọn cắt chính xác cho sai số dưới 1 giây so với thời điểm yêu cầu.
- **SC-208**: Áp một preset đã lưu điền đủ mọi tuỳ chọn đầu ra trong 1 thao tác và dưới 1 giây.
- **SC-209**: Không có lựa chọn đầu ra nào bị bỏ qua âm thầm: mọi tuỳ chọn không áp dụng được đều hiển thị lý do cho người dùng.

## Assumptions

- Toàn bộ hậu xử lý dựa trên công cụ xử lý media đã được bundle sẵn từ v1; spec này không thêm phụ thuộc bên ngoài nào mà người dùng phải tự cài.
- Chuyển mã sang định dạng không mất dữ liệu từ nguồn đã nén không khôi phục được chất lượng — ứng dụng cung cấp lựa chọn và cảnh báo, không hứa hẹn điều ngược lại.
- Danh sách định dạng đầu ra là cố định và do ứng dụng định nghĩa (khác với danh sách chất lượng vốn phải lấy động từ nguồn theo FR-019 của v1) — đây là những gì công cụ xử lý media có thể tạo ra, không phải những gì nguồn có.
- Người dùng phổ thông sẽ không đụng tới mẫu tên file và preset; các tuỳ chọn này nằm trong khu vực nâng cao, mặc định thu gọn, để không làm rối luồng cơ bản.
- Số lượng tuỳ chọn tăng lên đáng kể trong phase này; giao diện xem trước cần được tổ chức lại theo nhóm thu gọn được, nếu không sẽ vi phạm yêu cầu về tính dễ dùng của v1.
