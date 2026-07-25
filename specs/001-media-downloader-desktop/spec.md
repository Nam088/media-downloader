# Feature Specification: Trình Tải Media Đa Nền Tảng (Cross-Platform Media Downloader)

**Feature Branch**: `001-media-downloader-desktop`

**Created**: 2026-07-25

**Status**: Draft

**Input**: User description: "giúp tôi lên phương án để làm 1 desktop app chạy được trên mọi nền tảng có ui để download mọi loại audio media từ ytb tiktok balbla chọn tech stack cho nó ok nhé"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Tải âm thanh từ một liên kết đơn (Priority: P1)

Người dùng dán một liên kết video (YouTube, TikTok, ...) vào ứng dụng, chọn tải "chỉ âm thanh", và nhận về một tệp âm thanh có thể phát ngay trên máy.

**Why this priority**: Đây là giá trị cốt lõi mà người dùng mong muốn nhất (nghe lại nội dung dạng âm thanh, offline). Chỉ riêng luồng này đã tạo thành một sản phẩm khả dụng tối thiểu (MVP).

**Independent Test**: Dán một liên kết video công khai hợp lệ, bấm "Tải âm thanh", xác nhận một tệp âm thanh xuất hiện trong thư mục đầu ra và phát được đúng nội dung.

**Acceptance Scenarios**:

1. **Given** người dùng đã dán một liên kết YouTube hợp lệ, **When** họ chọn "chỉ âm thanh" và bấm Tải, **Then** ứng dụng hiển thị tiến trình và tạo ra một tệp âm thanh hoàn chỉnh trong thư mục đầu ra đã chọn.
2. **Given** người dùng đã dán một liên kết TikTok hợp lệ, **When** họ chọn "chỉ âm thanh" và bấm Tải, **Then** kết quả tương tự như với YouTube (không có khác biệt trải nghiệm giữa các nền tảng).

---

### User Story 2 - Tải video đầy đủ với lựa chọn chất lượng (Priority: P2)

Người dùng muốn lưu lại toàn bộ video (hình + tiếng), không chỉ phần âm thanh, và có thể chọn mức chất lượng trước khi tải.

**Why this priority**: Mở rộng giá trị cốt lõi sang nhu cầu lưu trữ clip đầy đủ (ví dụ video TikTok/Reels ngắn), là nhu cầu phổ biến thứ hai sau tải âm thanh.

**Independent Test**: Dán một liên kết video, chọn "video đầy đủ" cùng một mức chất lượng cụ thể, xác nhận tệp video tải về phát được và đúng mức chất lượng đã chọn.

**Acceptance Scenarios**:

1. **Given** người dùng đã dán một liên kết hợp lệ, **When** họ chọn "video đầy đủ" và một mức chất lượng, **Then** ứng dụng tải về đúng tệp video ở chất lượng đó.
2. **Given** nền tảng nguồn không cung cấp mức chất lượng đã chọn, **When** người dùng bấm Tải, **Then** ứng dụng đề xuất mức chất lượng gần nhất có sẵn thay vì báo lỗi ngay lập tức.

---

### User Story 3 - Quản lý hàng đợi và lịch sử tải xuống (Priority: P3)

Người dùng dán nhiều liên kết cùng lúc, theo dõi tiến trình từng tác vụ độc lập, và xem lại lịch sử các lần tải trước đó.

**Why this priority**: Tăng hiệu quả sử dụng cho người dùng có nhu cầu tải nhiều nội dung, nhưng không bắt buộc để có một sản phẩm khả dụng tối thiểu.

**Independent Test**: Dán 3 liên kết cùng lúc, xác nhận cả 3 tác vụ xuất hiện trong hàng đợi với tiến trình cập nhật độc lập, và sau khi hoàn tất có thể xem lại trong mục lịch sử.

**Acceptance Scenarios**:

1. **Given** người dùng dán nhiều liên kết cùng lúc, **When** họ bấm Tải tất cả, **Then** mỗi liên kết trở thành một tác vụ riêng trong hàng đợi với trạng thái và tiến trình độc lập.
2. **Given** một tác vụ đã hoàn tất hoặc thất bại, **When** người dùng mở mục lịch sử, **Then** họ thấy tên tệp, nguồn, thời gian, và trạng thái của tác vụ đó, và có thể mở thư mục chứa tệp hoặc thử tải lại nếu thất bại.

---

### Edge Cases

- Liên kết không hợp lệ hoặc từ nền tảng chưa được hỗ trợ → hiển thị thông báo lỗi rõ ràng, không làm treo hàng đợi.
- Video riêng tư, đã bị gỡ, hoặc yêu cầu đăng nhập → thông báo không thể tải kèm lý do.
- Nội dung có bản quyền, trả phí, hoặc được bảo vệ bằng DRM → từ chối tải và hiển thị cảnh báo tuân thủ.
- Mất kết nối mạng giữa chừng → tác vụ tạm dừng, cho phép tiếp tục hoặc huỷ khi có mạng trở lại.
- Ổ đĩa không đủ dung lượng trống → cảnh báo trước khi bắt đầu tải, không để tệp bị hỏng dở.
- Liên kết trỏ tới một playlist / nhiều video → hỏi rõ người dùng muốn tải toàn bộ hay chỉ một mục.
- Ngôn ngữ hệ điều hành không nằm trong danh sách ngôn ngữ được hỗ trợ → mặc định hiển thị Tiếng Anh, người dùng vẫn có thể đổi sang ngôn ngữ khác được hỗ trợ trong cài đặt.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: Hệ thống PHẢI cho phép người dùng nhập hoặc dán một hoặc nhiều URL từ các nền tảng được hỗ trợ.
- **FR-002**: Hệ thống PHẢI tự động nhận diện nền tảng nguồn và hiển thị thông tin xem trước (tiêu đề, hình thu nhỏ, thời lượng) trước khi tải.
- **FR-003**: Người dùng PHẢI có thể chọn tải "chỉ âm thanh" hoặc "video đầy đủ" cho mỗi liên kết.
- **FR-004**: Ở bước xem trước, hệ thống PHẢI truy vấn trực tiếp từ nguồn danh sách luồng/định dạng âm thanh thực tế đang có (qua yt-dlp), và xây dựng động các mức chất lượng MP3 hiển thị cho người dùng dựa trên luồng thực tế đó; hệ thống KHÔNG được hiển thị một danh sách mức chất lượng cố định giống nhau cho mọi liên kết bất kể nguồn có hỗ trợ hay không — nếu nguồn chỉ có luồng chất lượng thấp, chỉ hiển thị (các) mức thực sự đạt được từ nguồn đó. Định dạng đầu ra là MP3 trong v1; các định dạng khác (WAV, FLAC, ...) có thể bổ sung ở phiên bản sau.
- **FR-005**: Hệ thống PHẢI hiển thị tiến trình tải (phần trăm, tốc độ, thời gian còn lại ước tính) cho từng tác vụ.
- **FR-006**: Hệ thống PHẢI cho phép tạm dừng, huỷ, hoặc thử lại một tác vụ tải đang diễn ra hoặc đã thất bại.
- **FR-007**: Hệ thống PHẢI lưu lịch sử các lần tải (tên tệp, nguồn, thời gian, trạng thái) và cho phép mở thư mục chứa tệp đã tải trực tiếp từ lịch sử.
- **FR-008**: Hệ thống PHẢI cho phép người dùng chọn thư mục lưu tệp đầu ra.
- **FR-009**: Hệ thống PHẢI hiển thị thông báo lỗi rõ ràng, dễ hiểu khi không thể tải (liên kết không hợp lệ, nội dung riêng tư, đã bị gỡ, vượt quá giới hạn, v.v.).
- **FR-010**: Hệ thống PHẢI hoạt động nhất quán trên Windows, macOS, và Linux với cùng một bộ tính năng, không có khác biệt trải nghiệm giữa các hệ điều hành.
- **FR-011**: Hệ thống PHẢI hiển thị tuyên bố tuân thủ/miễn trừ trách nhiệm khi khởi chạy lần đầu, nhắc người dùng chỉ tải nội dung mà họ có quyền sử dụng, không vi phạm bản quyền hoặc Điều khoản Dịch vụ (ToS) của nền tảng nguồn.
- **FR-012**: Hệ thống KHÔNG được thực hiện hành vi phá khoá DRM hoặc bỏ qua cơ chế xác thực/giới hạn truy cập của nền tảng nguồn (không hỗ trợ tải nội dung trả phí có bảo vệ DRM hoặc nội dung yêu cầu đăng nhập riêng tư).
- **FR-013**: Khi liên kết trỏ tới một playlist/nhiều video, hệ thống PHẢI hỏi người dùng chọn giữa "chỉ tải mục này" hoặc "thêm cả danh sách vào hàng đợi dưới dạng nhiều tác vụ riêng biệt"; không tự động tải toàn bộ playlist mà không hỏi trước.
- **FR-014**: Hệ thống PHẢI hỗ trợ tải từ bất kỳ nền tảng nào mà engine tải (yt-dlp) tự nhận diện được (hiện tại khoảng 1.600+ trang đang hoạt động tốt trong tổng số extractor của yt-dlp), không giới hạn trước bằng một danh sách domain cố định trong ứng dụng; YouTube, TikTok, Facebook, Instagram, Twitter/X, và SoundCloud là 6 nền tảng PHẢI được kiểm thử và đảm bảo hoạt động tốt trong v1 (xem T049/T050), nhưng không phải là giới hạn trên của hệ thống. Hệ thống chỉ được từ chối một liên kết khi chính engine tải xác nhận không nhận diện được nó, không phải do ứng dụng tự chặn trước.
- **FR-015**: Giao diện PHẢI có chất lượng thiết kế hiện đại, chuyên nghiệp, mượt mà (không phải giao diện thô sơ dạng công cụ nội bộ), với bố cục rõ ràng để người dùng phổ thông thao tác được ngay mà không cần đọc hướng dẫn.
- **FR-016**: Hệ thống PHẢI cung cấp chế độ Sáng (Light) và Tối (Dark); PHẢI tự động theo cài đặt giao diện của hệ điều hành khi khởi chạy lần đầu, đồng thời cho phép người dùng ghi đè thủ công và ghi nhớ lựa chọn đó cho lần mở sau.
- **FR-017**: Hệ thống PHẢI hỗ trợ đa ngôn ngữ cho giao diện (tối thiểu Tiếng Việt và Tiếng Anh trong v1), tự động chọn ngôn ngữ theo hệ điều hành khi khởi chạy lần đầu và cho phép người dùng đổi ngôn ngữ thủ công trong phần cài đặt; kiến trúc PHẢI cho phép bổ sung ngôn ngữ mới mà không cần sửa lại logic ứng dụng (tách văn bản hiển thị khỏi mã nguồn).
- **FR-018**: Bộ cài đặt ứng dụng PHẢI tự chứa toàn bộ phần mềm/công cụ phụ trợ cần thiết để tải và xử lý media; người dùng KHÔNG được yêu cầu tự tải, cài đặt, hay cấu hình thủ công bất kỳ công cụ phụ trợ nào (kể cả engine tải media và công cụ xử lý âm thanh) trên bất kỳ hệ điều hành nào — cài đặt xong là dùng được ngay.
- **FR-019**: Toàn bộ tuỳ chọn tải xuống hiển thị cho người dùng (định dạng, mức chất lượng âm thanh, độ phân giải video, có phải playlist hay không, v.v.) PHẢI được xây dựng động từ thông tin lấy trực tiếp từ nguồn tại thời điểm xem trước liên kết đó; hệ thống KHÔNG được viết cứng (hard-code) sẵn một danh sách tuỳ chọn áp dụng chung cho mọi liên kết/nền tảng. Khi nguồn không cung cấp đủ dữ liệu để xác định tuỳ chọn, hệ thống PHẢI báo rõ giới hạn đó thay vì tự ý hiển thị tuỳ chọn không có thật.

### Key Entities

- **Tác vụ tải (Download Job)**: Đại diện cho một yêu cầu tải cụ thể; gồm URL nguồn, nền tảng, loại nội dung (audio/video), định dạng/chất lượng đã chọn, trạng thái (đang chờ, đang tải, tạm dừng, hoàn tất, lỗi), và tiến trình.
- **Nguồn media (Media Source)**: Thông tin xem trước của nội dung tại URL, gồm tiêu đề, hình thu nhỏ, thời lượng, nền tảng gốc, và có phải là playlist/nhiều video hay không.
- **Tệp đã tải (Downloaded File)**: Kết quả sau khi một tác vụ hoàn tất; gồm đường dẫn tệp, định dạng, dung lượng, thời điểm hoàn tất, và tác vụ liên quan.
- **Lịch sử tải (Download History)**: Tập hợp các tác vụ đã hoàn tất hoặc thất bại theo thời gian, dùng để tra cứu và thao tác lại (mở thư mục, tải lại).

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Với một video có thời lượng dưới 10 phút và mạng ổn định, người dùng có tệp âm thanh sẵn sàng nghe trong vòng dưới 30 giây kể từ khi bấm Tải.
- **SC-002**: Ứng dụng tải thành công tối thiểu 95% các liên kết hợp lệ thuộc danh sách nền tảng được công bố hỗ trợ.
- **SC-003**: 90% người dùng lần đầu có thể hoàn thành lượt tải đầu tiên của họ mà không cần xem hướng dẫn bổ sung.
- **SC-004**: Ứng dụng cung cấp trải nghiệm và bộ tính năng giống nhau trên Windows, macOS, và Linux, không có tính năng nào chỉ khả dụng trên một hệ điều hành.
- **SC-005**: Khi một lượt tải thất bại, 100% trường hợp hiển thị lý do đủ rõ để người dùng biết bước tiếp theo cần làm (thử liên kết khác, kiểm tra kết nối mạng, v.v.).
- **SC-006**: Người dùng chuyển đổi giữa chế độ Sáng/Tối chỉ trong 1 thao tác, giao diện cập nhật ngay lập tức mà không cần khởi động lại ứng dụng.
- **SC-007**: Ứng dụng hiển thị đúng ngôn ngữ giao diện đã chọn ở 100% màn hình (không còn văn bản bị bỏ sót ở ngôn ngữ khác), và người dùng đổi ngôn ngữ trong vòng 1 thao tác từ phần cài đặt.
- **SC-008**: Sau khi chạy đúng một trình cài đặt duy nhất, người dùng tải thành công lượt tải đầu tiên ngay mà không phải tự cài đặt, tải về, hay cấu hình thêm bất kỳ phần mềm/công cụ phụ trợ nào khác trên máy.

## Assumptions

- Người dùng tải nội dung cho mục đích cá nhân, hợp pháp, và tự chịu trách nhiệm tuân thủ luật bản quyền cũng như Điều khoản Dịch vụ (ToS) của từng nền tảng nguồn; ứng dụng hiển thị nhắc nhở nhưng không thể xác minh quyền sở hữu nội dung thay người dùng.
- Ứng dụng chỉ lưu tệp trên máy cá nhân của người dùng, không lưu trữ tập trung hay phân phối lại nội dung đã tải.
- Ứng dụng không hỗ trợ và không cố gắng vượt qua nội dung có DRM hoặc yêu cầu tài khoản trả phí/đăng nhập riêng tư của nền tảng nguồn.
- Cần kết nối Internet ổn định để tải; ứng dụng không hoạt động ở chế độ hoàn toàn ngoại tuyến (ngoại trừ phát lại các tệp đã tải trước đó).
- Máy của người dùng có đủ dung lượng ổ đĩa trống cho các tệp đang được tải về.
- Việc lựa chọn công nghệ cụ thể (ở bước `/speckit-plan`) cần ưu tiên các lựa chọn có hỗ trợ tốt sẵn có cho theming Sáng/Tối và quốc tế hoá (i18n) đa ngôn ngữ, để đáp ứng FR-016 và FR-017 mà không phải tự xây dựng từ đầu.
