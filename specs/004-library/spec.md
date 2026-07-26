# Feature Specification: Library — Thư viện media đã tải

**Feature Branch**: `004-library`

**Created**: 2026-07-26

**Status**: Draft

**Phase**: 3/4 (xem `specs/ROADMAP.md`)

**Phụ thuộc**: Phase 1 và Phase 2 — thư viện chỉ hữu ích khi có nhiều file và metadata đầy đủ.

## Bối cảnh kỹ thuật

Hiện trạng đã xác minh trong mã nguồn:

- Bảng `downloaded_files` đã tồn tại từ migration đầu tiên và **đang được ghi** khi tác vụ hoàn tất (`src-tauri/src/db/mod.rs:177-198`), nhưng **chưa bao giờ được đọc** — hàm đọc duy nhất gắn `#[allow(dead_code)]` (`db/mod.rs:204-217`). Trang Lịch sử hiện chỉ đọc cột `output_file_path` trên bảng `download_jobs`.
- Bảng này hiện chỉ lưu: đường dẫn, định dạng, dung lượng, thời điểm hoàn tất, và khoá tới tác vụ. Thiếu tiêu đề, thời lượng, ảnh thu nhỏ, nền tảng — những thứ cần cho một thư viện.
- Ứng dụng **chưa từng phát media**. Để phát file cục bộ trong cửa sổ ứng dụng cần bật giao thức truy cập tài nguyên của Tauri với phạm vi thư mục giới hạn — hiện chưa cấu hình.
- `app.security.csp` đang là `null` (`tauri.conf.json:26`), tức là chính sách bảo mật nội dung đang tắt hoàn toàn. Phase này phải bật CSP có kiểm soát cùng lúc với việc cho phép phát file cục bộ.
- Plugin thao tác tệp chưa được kích hoạt ở phía Rust và không có quyền `fs` nào trong `capabilities/default.json` — gói npm tương ứng đã cài nhưng chưa dùng ở đâu.
- `App.tsx` điều hướng bằng state thủ công, mọi trang render đồng thời và ẩn/hiện bằng class. Thêm một trang có nhiều dữ liệu vào mô hình này cần cân nhắc chi phí render.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Xem lại mọi thứ đã tải (Priority: P1)

Người dùng đã tải 200 file trong ba tháng. Họ mở tab Thư viện và thấy toàn bộ dưới dạng lưới có ảnh thu nhỏ, biết ngay cái nào là nhạc cái nào là video, tải từ đâu, khi nào.

**Why this priority**: Đây là nền của mọi thứ còn lại trong phase; không có danh sách thì không có tìm kiếm, không có phát thử, không có quản lý.

**Independent Test**: Tải vài file với các loại khác nhau, mở tab Thư viện, xác nhận tất cả xuất hiện với ảnh thu nhỏ, tiêu đề, thời lượng, dung lượng và nguồn.

**Acceptance Scenarios**:

1. **Given** người dùng đã tải nhiều file, **When** họ mở tab Thư viện, **Then** mọi file hiện ra kèm ảnh thu nhỏ, tiêu đề, loại nội dung, thời lượng, dung lượng, nền tảng nguồn và ngày tải.
2. **Given** thư viện trống, **When** người dùng mở tab, **Then** hiển thị trạng thái rỗng có hướng dẫn tải file đầu tiên, không phải màn hình trắng.
3. **Given** một file không có ảnh thu nhỏ lưu sẵn, **When** hiển thị trong lưới, **Then** dùng ảnh đại diện theo loại nội dung, không để ô trống vỡ bố cục.
4. **Given** thư viện có hàng nghìn mục, **When** người dùng cuộn, **Then** giao diện vẫn mượt và không nạp toàn bộ ảnh cùng lúc.

---

### User Story 2 - Tìm và lọc (Priority: P1)

Người dùng nhớ mang máng đã tải một bài podcast tháng trước nhưng không nhớ tên đầy đủ. Họ gõ vài từ khoá và lọc theo "chỉ audio", "tháng trước" để tìm ra.

**Why this priority**: Một danh sách 200 mục không tìm được thì không khác gì mở thư mục bằng trình quản lý tệp — giá trị của thư viện nằm ở đây.

**Independent Test**: Tải file với tiêu đề đã biết, gõ một phần tiêu đề vào ô tìm kiếm, xác nhận file xuất hiện; áp bộ lọc loại nội dung, xác nhận danh sách thu hẹp đúng.

**Acceptance Scenarios**:

1. **Given** người dùng gõ từ khoá, **When** kết quả cập nhật, **Then** danh sách chỉ còn các mục có tiêu đề hoặc tên file khớp, cập nhật khi đang gõ.
2. **Given** người dùng áp bộ lọc loại nội dung, nền tảng, định dạng, hoặc khoảng thời gian, **When** bộ lọc có hiệu lực, **Then** danh sách phản ánh đúng và hiển thị rõ đang áp những bộ lọc nào.
3. **Given** nhiều bộ lọc cùng áp, **When** kết quả hiển thị, **Then** các bộ lọc kết hợp theo logic "và", và có nút xoá toàn bộ bộ lọc.
4. **Given** không có kết quả nào khớp, **When** hiển thị, **Then** thông báo rõ và gợi ý nới lỏng bộ lọc.
5. **Given** người dùng chọn tiêu chí sắp xếp (ngày tải, tên, dung lượng, thời lượng), **When** áp dụng, **Then** danh sách sắp xếp đúng và lựa chọn được ghi nhớ cho lần mở sau.

---

### User Story 3 - Nghe và xem thử ngay trong ứng dụng (Priority: P2)

Người dùng muốn kiểm tra nhanh file vừa tải có đúng nội dung không, mà không phải mở ứng dụng khác.

**Why this priority**: Rút ngắn vòng lặp "tải xong — kiểm tra", nhưng người dùng vẫn có trình phát hệ thống nếu thiếu.

**Independent Test**: Bấm vào một file audio và một file video trong thư viện, xác nhận cả hai phát được, tua được, chỉnh âm lượng được.

**Acceptance Scenarios**:

1. **Given** người dùng chọn một mục là audio hoặc video, **When** họ bấm phát, **Then** nội dung phát trong ứng dụng với điều khiển phát/tạm dừng, thanh tua, và âm lượng.
2. **Given** đang phát một mục, **When** người dùng chọn phát mục khác, **Then** mục đang phát dừng lại và mục mới bắt đầu.
3. **Given** định dạng file không phát được trong ứng dụng, **When** người dùng bấm phát, **Then** hiển thị thông báo rõ ràng kèm nút mở bằng ứng dụng mặc định của hệ thống.
4. **Given** đang phát một mục, **When** người dùng chuyển sang tab khác, **Then** hành vi phát (dừng hay tiếp tục) là nhất quán và có thể dự đoán.

---

### User Story 4 - Quản lý file ngay trong ứng dụng (Priority: P2)

Người dùng muốn đổi tên một file cho gọn, xoá vài file không cần nữa, hoặc chuyển một nhóm file sang thư mục khác — mà không phải mở trình quản lý tệp.

**Independent Test**: Đổi tên một mục, xác nhận tên file trên đĩa đổi theo; xoá một mục, xác nhận file không còn và mục biến mất khỏi thư viện.

**Acceptance Scenarios**:

1. **Given** người dùng chọn một mục, **When** họ đổi tên, **Then** cả tên file trên đĩa lẫn thông tin trong thư viện được cập nhật.
2. **Given** người dùng xoá một mục, **When** họ xác nhận, **Then** file được chuyển vào thùng rác của hệ thống (không xoá vĩnh viễn) và mục biến mất khỏi thư viện.
3. **Given** người dùng chọn nhiều mục, **When** họ thực hiện xoá hoặc di chuyển, **Then** thao tác áp cho toàn bộ lựa chọn với một lần xác nhận duy nhất.
4. **Given** người dùng chọn một mục, **When** họ chọn "mở thư mục chứa", **Then** trình quản lý tệp của hệ thống mở đúng vị trí file.
5. **Given** tên mới trùng với file đã tồn tại, **When** người dùng xác nhận đổi tên, **Then** hệ thống cảnh báo và không ghi đè.

---

### User Story 5 - Phát hiện file đã bị xoá bên ngoài (Priority: P2)

Người dùng đã xoá vài file bằng trình quản lý tệp. Thư viện phải biết điều đó thay vì hiển thị mục trỏ vào hư không.

**Independent Test**: Xoá một file bằng trình quản lý tệp, quay lại ứng dụng, xác nhận mục đó được đánh dấu là thiếu và có cách dọn đi.

**Acceptance Scenarios**:

1. **Given** một file đã bị xoá hoặc di chuyển bên ngoài ứng dụng, **When** thư viện được làm mới, **Then** mục đó được đánh dấu rõ là "không tìm thấy file".
2. **Given** có các mục bị đánh dấu thiếu, **When** người dùng chọn dọn dẹp, **Then** các mục đó bị gỡ khỏi thư viện sau khi xác nhận.
3. **Given** một mục bị đánh dấu thiếu, **When** người dùng chọn "tải lại", **Then** một tác vụ mới được tạo với đúng URL và cấu hình gốc.
4. **Given** người dùng chọn "tìm lại file", **When** họ trỏ tới vị trí mới, **Then** mục được liên kết lại thay vì phải tải lại.

---

### User Story 6 - Nhìn tổng quan mức sử dụng (Priority: P3)

Người dùng muốn biết mình đã tải bao nhiêu, chiếm bao nhiêu dung lượng, chủ yếu từ nền tảng nào.

**Independent Test**: Mở phần thống kê, xác nhận tổng số mục và tổng dung lượng khớp với thực tế trong thư viện.

**Acceptance Scenarios**:

1. **Given** thư viện có dữ liệu, **When** người dùng mở phần thống kê, **Then** hiển thị tổng số mục, tổng dung lượng, phân bố theo nền tảng và theo loại nội dung.
2. **Given** thống kê đang hiển thị, **When** người dùng bấm vào một phần trong phân bố, **Then** thư viện lọc theo đúng tiêu chí đó.

---

### User Story 7 - Xuất danh sách phát (Priority: P3)

Người dùng muốn nghe một loạt file đã tải trên trình phát khác theo đúng thứ tự.

**Independent Test**: Chọn vài mục, xuất danh sách phát, mở file kết quả bằng một trình phát phổ biến, xác nhận phát đúng thứ tự.

**Acceptance Scenarios**:

1. **Given** người dùng chọn nhiều mục, **When** họ xuất danh sách phát, **Then** một file danh sách phát được tạo tại vị trí họ chọn, trỏ đúng tới các file.
2. **Given** danh sách phát được tạo từ bộ lọc đang áp, **When** xuất, **Then** thứ tự trong file khớp với thứ tự đang hiển thị.

---

### Edge Cases

- Thư viện có 10.000 mục → cuộn vẫn mượt, tìm kiếm vẫn trả kết quả nhanh.
- File nằm trên ổ đĩa ngoài đã tháo → đánh dấu là thiếu tạm thời, không tự xoá khỏi thư viện.
- Người dùng đổi thư mục lưu mặc định giữa chừng → các mục cũ vẫn trỏ đúng vị trí cũ.
- Hai mục trong thư viện trỏ tới cùng một file (do tải lại) → nhận diện và gộp hoặc đánh dấu trùng.
- Tên file chứa ký tự đặc biệt hoặc emoji → hiển thị, phát, và đổi tên đều hoạt động.
- File rất lớn (vài GB) → không nạp toàn bộ vào bộ nhớ khi phát hoặc lấy ảnh đại diện.
- Người dùng xoá mục đang được phát → dừng phát trước khi xoá.
- Tác vụ tách chương từ Phase 2 tạo ra nhiều file cho một tác vụ → thư viện phải hiển thị đủ mọi file, không chỉ file đầu tiên.
- Người dùng chưa từng tải gì, mở thẳng tab Thư viện → trạng thái rỗng có hướng dẫn.

## Requirements *(mandatory)*

### Functional Requirements

**Chỉ mục**

- **FR-301**: Hệ thống PHẢI lưu, cho mỗi file đã tải: đường dẫn, tiêu đề, loại nội dung, định dạng, dung lượng, thời lượng, nền tảng nguồn, URL gốc, ảnh đại diện, và thời điểm tải.
- **FR-302**: Một tác vụ sinh ra nhiều file (ví dụ tách theo chương) PHẢI được ghi nhận thành nhiều mục trong thư viện, tất cả liên kết về cùng tác vụ gốc.
- **FR-303**: Hệ thống PHẢI nạp được vào thư viện các file đã tải từ trước khi tính năng này tồn tại, dựa trên dữ liệu lịch sử sẵn có.
- **FR-304**: Hệ thống PHẢI lưu ảnh đại diện cục bộ để hiển thị được khi không có mạng.

**Duyệt và tìm kiếm**

- **FR-305**: Hệ thống PHẢI có một trang Thư viện riêng, truy cập được từ thanh điều hướng chính.
- **FR-306**: Thư viện PHẢI hiển thị dạng lưới có ảnh đại diện và dạng danh sách gọn, người dùng chuyển đổi được và lựa chọn được ghi nhớ.
- **FR-307**: Hệ thống PHẢI cho tìm kiếm theo tiêu đề và tên file, cập nhật kết quả trong lúc người dùng gõ.
- **FR-308**: Hệ thống PHẢI cho lọc theo loại nội dung, nền tảng nguồn, định dạng file, và khoảng thời gian tải; các bộ lọc kết hợp theo logic "và".
- **FR-309**: Hệ thống PHẢI cho sắp xếp theo ngày tải, tiêu đề, dung lượng, và thời lượng, theo cả hai chiều.
- **FR-310**: Thư viện PHẢI hiển thị mượt với ít nhất 10.000 mục, không nạp toàn bộ ảnh đại diện cùng lúc.
- **FR-311**: Trạng thái rỗng và trạng thái không có kết quả PHẢI có hướng dẫn cụ thể, không để màn hình trống.

**Phát thử**

- **FR-312**: Hệ thống PHẢI phát được file audio và video đã tải ngay trong ứng dụng, với điều khiển phát/tạm dừng, tua, và âm lượng.
- **FR-313**: Việc phát file cục bộ PHẢI được giới hạn trong phạm vi các thư mục ứng dụng thực sự đã tải file về; KHÔNG được cấp quyền truy cập toàn bộ hệ thống tệp cho tầng giao diện.
- **FR-314**: Chính sách bảo mật nội dung PHẢI được bật và cấu hình tường minh trong phase này, thay cho trạng thái đang tắt hoàn toàn hiện nay.
- **FR-315**: Khi định dạng không phát được trong ứng dụng, hệ thống PHẢI nêu rõ lý do và cho mở bằng ứng dụng mặc định của hệ thống.
- **FR-316**: Tại một thời điểm chỉ một mục được phát; chọn mục mới PHẢI dừng mục đang phát.

**Quản lý file**

- **FR-317**: Người dùng PHẢI đổi tên được file ngay trong thư viện, cập nhật đồng thời tên trên đĩa và trong chỉ mục.
- **FR-318**: Người dùng PHẢI xoá được file; xoá PHẢI đưa file vào thùng rác của hệ thống, KHÔNG xoá vĩnh viễn.
- **FR-319**: Người dùng PHẢI di chuyển được file sang thư mục khác, và chỉ mục PHẢI theo kịp vị trí mới.
- **FR-320**: Người dùng PHẢI chọn được nhiều mục và thực hiện xoá hoặc di chuyển hàng loạt với một lần xác nhận.
- **FR-321**: Người dùng PHẢI mở được thư mục chứa file bằng trình quản lý tệp của hệ thống.
- **FR-322**: Mọi thao tác ghi lên hệ thống tệp PHẢI có xác nhận trước khi thực hiện và KHÔNG được ghi đè file đã tồn tại.

**Đồng bộ với thực tế**

- **FR-323**: Hệ thống PHẢI phát hiện file đã bị xoá hoặc di chuyển bên ngoài ứng dụng và đánh dấu mục đó là thiếu.
- **FR-324**: Người dùng PHẢI dọn được các mục bị đánh dấu thiếu khỏi thư viện.
- **FR-325**: Người dùng PHẢI trỏ lại được một mục thiếu tới vị trí mới của file thay vì phải tải lại.
- **FR-326**: Người dùng PHẢI tạo được tác vụ tải lại cho một mục thiếu, dùng đúng URL và cấu hình gốc.
- **FR-327**: Kiểm tra file tồn tại KHÔNG được chặn giao diện khi thư viện lớn.

**Thống kê và xuất**

- **FR-328**: Hệ thống PHẢI hiển thị tổng số mục, tổng dung lượng, và phân bố theo nền tảng và loại nội dung.
- **FR-329**: Bấm vào một thành phần trong thống kê PHẢI áp bộ lọc tương ứng lên thư viện.
- **FR-330**: Người dùng PHẢI xuất được danh sách phát từ các mục đang chọn hoặc đang lọc, giữ đúng thứ tự hiển thị.

### Key Entities

- **Mục thư viện (Library Item)**: Một file đã tải — đường dẫn, tiêu đề, loại nội dung, định dạng, dung lượng, thời lượng, nền tảng, URL gốc, đường dẫn ảnh đại diện, thời điểm tải, trạng thái tồn tại (có/thiếu), và khoá tới tác vụ đã tạo ra nó.
- **Bộ lọc thư viện (Library Filter)**: Trạng thái duyệt hiện tại — từ khoá tìm kiếm, các bộ lọc đang áp, tiêu chí sắp xếp, kiểu hiển thị. Được ghi nhớ giữa các phiên.
- **Phiên phát (Playback Session)**: Mục đang phát, vị trí hiện tại, trạng thái phát. Chỉ tồn tại trong phiên.

## Success Criteria *(mandatory)*

- **SC-301**: Người dùng tìm ra một file cụ thể trong thư viện 500 mục trong vòng dưới 10 giây bằng tìm kiếm hoặc lọc.
- **SC-302**: Thư viện 10.000 mục mở ra và cuộn mượt, thao tác đầu tiên phản hồi trong dưới 2 giây.
- **SC-303**: 100% file đã tải bằng ứng dụng xuất hiện trong thư viện, kể cả file tải trước khi có tính năng này.
- **SC-304**: File bị xoá bên ngoài được đánh dấu thiếu trong lần làm mới thư viện kế tiếp, 100% trường hợp.
- **SC-305**: Không thao tác xoá nào gây mất dữ liệu không khôi phục được — mọi file xoá đều nằm trong thùng rác hệ thống.
- **SC-306**: Tầng giao diện không truy cập được file nằm ngoài các thư mục tải đã đăng ký — kiểm chứng được bằng test.
- **SC-307**: Số liệu thống kê khớp với dữ liệu thực tế trong thư viện với sai số bằng không.

## Assumptions

- Thư viện chỉ theo dõi file do chính ứng dụng tải về; đây không phải trình quản lý media đa năng quét toàn bộ ổ đĩa của người dùng.
- Người dùng chấp nhận rằng nếu họ di chuyển file bằng công cụ khác, thư viện cần một lần làm mới để bắt kịp.
- Khả năng phát trong ứng dụng phụ thuộc vào những gì webview của từng hệ điều hành hỗ trợ; một số định dạng sẽ phải mở bằng ứng dụng ngoài và điều đó được thông báo rõ thay vì im lặng thất bại.
- Việc bật chính sách bảo mật nội dung có thể làm lộ ra các vi phạm ngầm đang tồn tại trong giao diện hiện tại; cần dự trù thời gian xử lý ở bước lập kế hoạch.
- Ảnh đại diện cho video được lấy từ ảnh thu nhỏ do nguồn cung cấp; việc trích khung hình từ chính file video được gác lại cho Phase 4.
