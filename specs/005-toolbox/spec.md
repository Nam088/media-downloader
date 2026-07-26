# Feature Specification: Toolbox — Bộ công cụ xử lý media cục bộ

**Feature Branch**: `005-toolbox`

**Created**: 2026-07-26

**Status**: Draft

**Phase**: 4/4 (xem `specs/ROADMAP.md`)

**Phụ thuộc**: Không bắt buộc. Tích hợp với Phase 3 (chọn file từ Thư viện) nếu có; nếu không thì nhận file qua kéo-thả.

## Bối cảnh kỹ thuật

Hiện trạng đã xác minh trong mã nguồn:

- Công cụ xử lý media đã được đóng gói sẵn cùng ứng dụng từ v1 dưới dạng sidecar (`tauri.conf.json:38`) và được gọi trực tiếp ở nhiều chỗ: kiểm tra luồng audio (`ytdlp.rs:178-191`), đo thời lượng (`queue.rs:614-624`), ghép slideshow (`queue.rs:735-829`), ghép lại audio bị thiếu (`queue.rs:867-881`). Phase này **không thêm phụ thuộc mới nào mà người dùng phải tự cài**.
- **Chưa đóng gói công cụ đọc thông tin media**: hiện đang lấy thời lượng bằng cách chạy công cụ xử lý rồi phân tích chuỗi văn bản ở luồng lỗi chuẩn (`queue.rs:609-636`, có ghi chú thừa nhận đây là giải pháp tạm). Một bộ công cụ nghiêm túc cần đọc được thông tin luồng media một cách có cấu trúc — đây là điều kiện tiên quyết của phase này.
- Hạ tầng hàng đợi hiện tại gắn chặt với khái niệm "tác vụ tải": bảng `download_jobs` có `source_url` bắt buộc và ràng buộc kiểm tra trên `media_type` chỉ chấp nhận ba giá trị `audio`/`video`/`gallery`. Tác vụ xử lý cục bộ không có URL nguồn. Bước lập kế hoạch phải quyết định giữa mở rộng mô hình hiện có và tạo mô hình tác vụ riêng — khuyến nghị tạo riêng, vì nhồi tác vụ không-có-URL vào bảng tải sẽ làm hỏng ý nghĩa của mọi truy vấn lịch sử hiện có.
- Cơ chế theo dõi tiến trình hiện tại phân tích đầu ra của công cụ tải, không dùng lại được cho tác vụ xử lý cục bộ; cần cơ chế theo dõi riêng nhưng nên dùng lại đúng mô hình sự kiện và cùng bộ điều phối số luồng song song của Phase 1.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Chuyển đổi định dạng file có sẵn (Priority: P1)

Người dùng có sẵn một thư mục video quay từ điện thoại và muốn chuyển hết sang MP4 để phát trên TV, hoặc trích audio từ một video đã tải về trước đó.

**Why this priority**: Đây là lý do tồn tại của bộ công cụ — mọi thứ khác là biến thể của cùng một cơ chế.

**Independent Test**: Kéo một file video vào ứng dụng, chọn chuyển sang định dạng khác, xác nhận file kết quả phát được và đúng định dạng.

**Acceptance Scenarios**:

1. **Given** người dùng chọn một file media cục bộ, **When** họ chọn định dạng đích và bắt đầu, **Then** file kết quả được tạo ở đúng định dạng, file gốc không bị thay đổi.
2. **Given** người dùng chọn trích audio từ một video, **When** tác vụ hoàn tất, **Then** thu được file audio đúng định dạng đã chọn.
3. **Given** file nguồn đã sẵn ở định dạng đích, **When** người dùng bắt đầu, **Then** hệ thống báo rằng không cần chuyển đổi và đề nghị chỉ sao chép.
4. **Given** người dùng chọn nhiều file cùng lúc, **When** họ áp cùng một thao tác, **Then** mỗi file thành một tác vụ riêng chạy theo cùng cơ chế hàng đợi như tải xuống.

---

### User Story 2 - Cắt và ghép (Priority: P1)

Người dùng muốn cắt phần intro 30 giây khỏi một video, hoặc ghép ba file audio thành một tập podcast duy nhất.

**Why this priority**: Hai thao tác biên tập cơ bản nhất, và là thứ khiến người dùng phải mở phần mềm khác nếu thiếu.

**Independent Test**: Cắt một đoạn từ một file và ghép hai file, xác nhận thời lượng kết quả đúng như mong đợi.

**Acceptance Scenarios**:

1. **Given** người dùng chọn một file và nhập khoảng thời gian, **When** tác vụ hoàn tất, **Then** file kết quả chỉ chứa đoạn đó.
2. **Given** người dùng chọn nhiều file cùng loại, **When** họ ghép và sắp xếp thứ tự, **Then** file kết quả chứa nội dung nối tiếp theo đúng thứ tự đó.
3. **Given** các file cần ghép có định dạng hoặc thông số khác nhau, **When** người dùng ghép, **Then** hệ thống cảnh báo và cho phép chuẩn hoá trước khi ghép.
4. **Given** người dùng nhập khoảng thời gian không hợp lệ, **When** họ rời khỏi ô nhập, **Then** lỗi hiện ngay và không cho bắt đầu tác vụ.
5. **Given** người dùng đang chọn khoảng cắt, **When** họ tua trên thanh thời gian, **Then** thấy được điểm cắt để xác nhận trước khi chạy.

---

### User Story 3 - Nén và tối ưu dung lượng (Priority: P2)

Một video 4K 3GB cần gửi qua ứng dụng nhắn tin có giới hạn 100MB. Người dùng chọn mức nén và nhận về file nhỏ hơn nhiều mà vẫn xem được.

**Independent Test**: Nén một file video, xác nhận dung lượng giảm rõ rệt và file vẫn phát bình thường.

**Acceptance Scenarios**:

1. **Given** người dùng chọn một file video và một mức chất lượng, **When** tác vụ hoàn tất, **Then** file kết quả nhỏ hơn đáng kể và vẫn phát được.
2. **Given** người dùng đặt một giới hạn dung lượng mục tiêu, **When** hệ thống xử lý, **Then** kết quả bám sát giới hạn đó trong biên độ chấp nhận được.
3. **Given** một mức nén được chọn, **When** người dùng xem trước tuỳ chọn, **Then** hệ thống ước lượng dung lượng và thời gian xử lý trước khi chạy.
4. **Given** người dùng giảm độ phân giải xuống, **When** tác vụ hoàn tất, **Then** file kết quả có đúng độ phân giải đã chọn.

---

### User Story 4 - Chuẩn hoá âm lượng (Priority: P2)

Người dùng có một loạt file audio to nhỏ khác nhau, muốn nghe liên tục mà không phải chỉnh âm lượng liên tục.

**Independent Test**: Chuẩn hoá vài file có mức âm lượng chênh lệch, xác nhận sau khi xử lý nghe đều nhau.

**Acceptance Scenarios**:

1. **Given** người dùng chọn nhiều file audio, **When** họ chuẩn hoá âm lượng, **Then** mọi file kết quả có mức âm lượng cảm nhận tương đương nhau.
2. **Given** một file đã ở mức chuẩn, **When** xử lý, **Then** hệ thống báo không cần thay đổi đáng kể thay vì xử lý lại vô ích.

---

### User Story 5 - Tạo ảnh động và trích ảnh (Priority: P3)

Người dùng muốn cắt một đoạn 5 giây trong video thành ảnh động để chia sẻ, hoặc lấy một khung hình làm ảnh đại diện.

**Independent Test**: Tạo một ảnh động từ đoạn video ngắn, xác nhận file mở được và chạy đúng đoạn đã chọn.

**Acceptance Scenarios**:

1. **Given** người dùng chọn một đoạn video và các thông số, **When** tác vụ hoàn tất, **Then** thu được file ảnh động chạy đúng đoạn đó.
2. **Given** người dùng chọn một thời điểm cụ thể, **When** họ trích khung hình, **Then** thu được file ảnh tại đúng thời điểm đó.
3. **Given** đoạn được chọn quá dài khiến file kết quả rất lớn, **When** người dùng chuẩn bị chạy, **Then** hệ thống cảnh báo kèm ước lượng dung lượng.

---

### User Story 6 - Xử lý phụ đề (Priority: P3)

Người dùng có một video kèm file phụ đề rời và muốn nhúng vào để xem trên thiết bị không đọc được file rời.

**Independent Test**: Nhúng một file phụ đề vào video, mở bằng trình phát, xác nhận bật/tắt được phụ đề.

**Acceptance Scenarios**:

1. **Given** người dùng chọn một video và một file phụ đề, **When** họ nhúng dạng track, **Then** file kết quả có phụ đề bật/tắt được trong trình phát.
2. **Given** người dùng chọn ghi phụ đề trực tiếp lên hình, **When** tác vụ hoàn tất, **Then** phụ đề hiện cố định trên video và không tắt được — hệ thống đã cảnh báo trước điều đó.
3. **Given** một video có sẵn track phụ đề, **When** người dùng trích ra, **Then** thu được file phụ đề rời.

---

### User Story 7 - Đổi tên hàng loạt (Priority: P3)

Người dùng có 50 file tên lộn xộn, muốn đổi hết theo một mẫu thống nhất có đánh số.

**Independent Test**: Chọn nhiều file, áp một mẫu tên, xem trước kết quả, xác nhận sau khi áp thì tên trên đĩa khớp bản xem trước.

**Acceptance Scenarios**:

1. **Given** người dùng chọn nhiều file và nhập một mẫu tên, **When** họ xem trước, **Then** danh sách tên cũ và tên mới hiện song song trước khi áp.
2. **Given** bản xem trước có xung đột tên, **When** hiển thị, **Then** các xung đột được đánh dấu rõ và không cho áp cho tới khi giải quyết xong.
3. **Given** người dùng đã áp việc đổi tên, **When** họ chọn hoàn tác ngay sau đó, **Then** tên cũ được khôi phục.

---

### Edge Cases

- File nguồn bị hỏng hoặc không phải file media → báo lỗi rõ ràng, không tạo file kết quả rỗng.
- File nguồn nằm trên ổ đĩa chỉ đọc hoặc thư mục không có quyền ghi → báo trước khi bắt đầu, không thất bại giữa chừng.
- File kết quả trùng tên file nguồn → không bao giờ ghi đè file gốc.
- Ghép các file có tốc độ khung hình hoặc tần số lấy mẫu khác nhau → cảnh báo trước, cung cấp bước chuẩn hoá.
- Tác vụ chạy rất lâu (video vài tiếng) → có tiến trình, huỷ được, và huỷ phải dọn file tạm.
- Huỷ giữa chừng → không để lại file kết quả dở dang gây nhầm lẫn.
- Hết dung lượng ổ đĩa giữa chừng → dừng sạch, báo rõ, dọn file tạm.
- Chạy nhiều tác vụ xử lý nặng cùng lúc → tuân theo cùng giới hạn số luồng song song của Phase 1, không làm treo máy.
- Đường dẫn file chứa khoảng trắng, ký tự tiếng Việt có dấu, hoặc emoji → mọi thao tác vẫn chạy đúng trên cả ba hệ điều hành.
- Người dùng chọn nhầm một file rất lớn → có ước lượng thời gian trước khi bắt đầu.

## Requirements *(mandatory)*

### Functional Requirements

**Nền tảng**

- **FR-401**: Hệ thống PHẢI đóng gói sẵn công cụ đọc thông tin media có cấu trúc, thay cho cách phân tích chuỗi văn bản hiện tại; người dùng KHÔNG phải cài thêm bất cứ thứ gì.
- **FR-402**: Tác vụ xử lý cục bộ PHẢI có mô hình dữ liệu riêng, tách khỏi tác vụ tải xuống, để không làm sai lệch ý nghĩa của lịch sử tải hiện có.
- **FR-403**: Tác vụ xử lý cục bộ PHẢI dùng chung bộ điều phối số luồng song song với tác vụ tải, để tổng tải lên máy vẫn nằm trong giới hạn người dùng đã đặt.
- **FR-404**: Mỗi tác vụ xử lý PHẢI hiển thị tiến trình, huỷ được, và ghi lỗi vào cùng hệ thống nhật ký đang có.
- **FR-405**: Huỷ một tác vụ xử lý PHẢI dọn sạch mọi file tạm và file kết quả dở dang.
- **FR-406**: Hệ thống KHÔNG BAO GIỜ được ghi đè file nguồn; mọi thao tác đều tạo file mới, trừ đổi tên hàng loạt vốn theo bản chất là thao tác tại chỗ và có hoàn tác.

**Nhập file**

- **FR-407**: Người dùng PHẢI đưa file vào bộ công cụ bằng kéo-thả vào cửa sổ và bằng hộp thoại chọn file.
- **FR-408**: Khi Thư viện (Phase 3) có mặt, người dùng PHẢI gửi được mục từ Thư viện thẳng sang bộ công cụ.
- **FR-409**: Hệ thống PHẢI xác minh file đưa vào là file media hợp lệ trước khi cho chọn thao tác, và báo rõ khi không hợp lệ.
- **FR-410**: Người dùng PHẢI chọn được nhiều file và áp cùng một thao tác cho tất cả.

**Các thao tác**

- **FR-411**: Hệ thống PHẢI chuyển đổi được giữa các định dạng audio và video mà công cụ đã đóng gói hỗ trợ.
- **FR-412**: Hệ thống PHẢI trích được luồng audio từ file video ra định dạng người dùng chọn.
- **FR-413**: Hệ thống PHẢI cắt được một đoạn theo thời gian bắt đầu và kết thúc.
- **FR-414**: Hệ thống PHẢI ghép được nhiều file cùng loại thành một, theo thứ tự người dùng sắp xếp.
- **FR-415**: Trước khi ghép, hệ thống PHẢI phát hiện các file không tương thích về thông số và đề nghị chuẩn hoá.
- **FR-416**: Hệ thống PHẢI nén video theo mức chất lượng chọn sẵn hoặc theo dung lượng mục tiêu, và giảm được độ phân giải.
- **FR-417**: Hệ thống PHẢI ước lượng dung lượng kết quả và thời gian xử lý trước khi chạy các thao tác nén.
- **FR-418**: Hệ thống PHẢI chuẩn hoá được âm lượng của một hoặc nhiều file audio về cùng một mức cảm nhận.
- **FR-419**: Hệ thống PHẢI tạo được ảnh động từ một đoạn video và trích được khung hình tại một thời điểm.
- **FR-420**: Hệ thống PHẢI nhúng phụ đề vào video dưới dạng track chọn được, ghi phụ đề cố định lên hình, và trích track phụ đề ra file rời.
- **FR-421**: Khi người dùng chọn ghi phụ đề cố định lên hình, hệ thống PHẢI cảnh báo rằng thao tác này không thể hoàn tác trên file kết quả.
- **FR-422**: Hệ thống PHẢI đổi tên hàng loạt theo mẫu, có xem trước danh sách tên cũ và tên mới trước khi áp.
- **FR-423**: Đổi tên hàng loạt PHẢI phát hiện xung đột tên và chặn thao tác cho tới khi được giải quyết.
- **FR-424**: Đổi tên hàng loạt PHẢI hoàn tác được ngay sau khi áp.

**Trải nghiệm**

- **FR-425**: Bộ công cụ PHẢI có trang riêng, truy cập được từ thanh điều hướng chính.
- **FR-426**: Với các thao tác cần chọn thời điểm, giao diện PHẢI cho xem trước nội dung để người dùng xác nhận điểm cắt.
- **FR-427**: Hệ thống PHẢI kiểm tra quyền ghi vào thư mục đích và dung lượng trống trước khi bắt đầu, thay vì thất bại giữa chừng.
- **FR-428**: Mọi thông báo lỗi PHẢI nêu rõ nguyên nhân và bước tiếp theo người dùng cần làm.
- **FR-429**: Toàn bộ chuỗi hiển thị PHẢI đi qua hệ thống đa ngôn ngữ.

### Key Entities

- **Tác vụ xử lý (Tool Task)**: Một thao tác trên file cục bộ — loại thao tác, danh sách file nguồn, thư mục đích, các tham số riêng của thao tác, trạng thái, tiến trình, thông báo lỗi, danh sách file kết quả, thời điểm tạo và kết thúc.
- **Thao tác (Tool Operation)**: Định nghĩa một loại thao tác — tên, loại file nhận vào, các tham số cần thiết, quy tắc kiểm tra hợp lệ. Do ứng dụng định nghĩa, không do người dùng tạo.
- **Thông tin media (Media Probe)**: Kết quả đọc từ một file nguồn — thời lượng, các luồng có trong file, độ phân giải, tốc độ khung hình, tần số lấy mẫu, codec. Dùng để kiểm tra hợp lệ và ước lượng.

## Success Criteria *(mandatory)*

- **SC-401**: Người dùng hoàn tất một thao tác chuyển đổi định dạng trong dưới 4 thao tác chuột kể từ khi mở bộ công cụ.
- **SC-402**: 100% thao tác tạo file mới, không thao tác nào ghi đè hay làm hỏng file nguồn.
- **SC-403**: Huỷ một tác vụ đang chạy dừng nó trong dưới 2 giây và không để lại file tạm nào.
- **SC-404**: Ước lượng dung lượng kết quả cho thao tác nén sai lệch không quá 25% so với thực tế.
- **SC-405**: Chuẩn hoá âm lượng cho ra mức cảm nhận chênh lệch dưới 1 đơn vị đo tiêu chuẩn giữa các file trong cùng lô.
- **SC-406**: Chạy đồng thời tác vụ tải và tác vụ xử lý không vượt quá giới hạn số luồng song song người dùng đã đặt.
- **SC-407**: Mọi thao tác chạy đúng với đường dẫn chứa khoảng trắng, ký tự tiếng Việt có dấu và emoji, trên cả ba hệ điều hành.
- **SC-408**: Đổi tên hàng loạt hoàn tác được về đúng trạng thái ban đầu trong 100% trường hợp thử nghiệm.

## Assumptions

- Bộ công cụ hoạt động trên file cục bộ có sẵn; nó không tải gì từ mạng và không cần kết nối Internet.
- Ứng dụng không nhắm tới việc thay thế phần mềm dựng phim; phạm vi giới hạn ở các thao tác một bước, chạy theo lô, không có dòng thời gian biên tập nhiều lớp.
- Chất lượng kết quả bị giới hạn bởi những gì công cụ xử lý đã đóng gói hỗ trợ; ứng dụng không tự cài thêm bộ mã hoá bên ngoài.
- Các thao tác nặng có thể chiếm nhiều CPU; ứng dụng tuân theo giới hạn số luồng người dùng đặt nhưng không tự điều tiết theo nhiệt độ hay mức pin của máy.
- Tính năng liên quan tới mô hình học máy (tách giọng hát khỏi nhạc nền, nhận dạng lời nói thành phụ đề) nằm ngoài phạm vi vì sẽ phá vỡ cam kết "cài một lần là dùng được ngay" của v1 do kích thước mô hình.
