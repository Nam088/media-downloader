# Feature Specification: Download Power — Hàng đợi & Nhập liệu

**Feature Branch**: `002-download-power`

**Created**: 2026-07-26

**Status**: Draft

**Phase**: 1/4 (xem `specs/ROADMAP.md`)

**Input**: "nâng cấp app, cải tiến thêm nhiều download cũng như tool hay khác" — nhóm B (queue) + E3 (tray) trong catalog roadmap.

> **Ghi chú phạm vi**: Tính năng theo dõi clipboard (tự bắt link khi người dùng copy) đã được **loại bỏ** theo quyết định ngày 2026-07-26: nó buộc ứng dụng đọc toàn bộ nội dung người dùng copy — bao gồm mật khẩu và tin nhắn riêng tư — một cái giá về quyền riêng tư không tương xứng với tiện ích mang lại. Vì vậy dải **FR-108 đến FR-109 bị bỏ trống có chủ ý**, và **FR-110 được viết lại thành một điều cấm** thay vì một tính năng.

## Bối cảnh kỹ thuật

Những điều dưới đây là **hiện trạng đã xác minh trong mã nguồn**, spec này dựa trên đó:

- `MAX_CONCURRENT_DOWNLOADS = 3` hard-code tại `src-tauri/src/downloader/queue.rs:17`; `Semaphore` khởi tạo một lần ở `:69`.
- `DownloadQueue::enqueue` (`queue.rs:76-80`) gọi `spawn_run` (`:82-113`), tức là **spawn task ngay lập tức** rồi task đó mới `semaphore.acquire().await` ở `:97`. Không tồn tại danh sách chờ có thứ tự.
- Command `list_queue` đã đăng ký (`lib.rs:45`) nhưng **không có chỗ nào trong `src/` gọi tới** — `queue-store.ts` chỉ giữ state trong RAM và không hydrate lúc khởi động.
- Vòng retry `for attempt in 1..=MAX_DOWNLOAD_ATTEMPTS` (`queue.rs:210`, hằng số `:25`) `continue` ngay khi lỗi, **không có delay**, và `classify_ytdlp_error` (`ytdlp.rs:234-251`) gộp mọi lỗi mạng vào `DOWNLOAD_FAILED` nên retry không phân biệt được lỗi tạm thời với lỗi vĩnh viễn.
- Batch mode ở `DownloadForm.tsx:388-438` chạy tuần tự `await` trong for-loop, **ép cứng audio** (`:411-421`) và tự lấy option chất lượng đầu tiên.
- Không có drag-drop ở bất kỳ đâu (`dragDropEnabled` chưa cấu hình, không listen `tauri://drag-drop`).
- Plugin Tauri đã cài: chỉ `opener` và `dialog`. Chưa có `notification`, `tray`, `clipboard-manager`, `fs`, `os`.
- `--continue` đã luôn được truyền cho yt-dlp (`queue.rs:986`) → file `.part` resume được sẵn.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Tải hàng loạt có kiểm soát (Priority: P1)

Người dùng có 20 link cần tải. Họ dán cả 20 link (hoặc kéo một file `.txt`, hoặc kéo trực tiếp link từ trình duyệt) vào ứng dụng, chọn một lần "video 1080p" hoặc "audio", rồi bấm Tải tất cả. Ứng dụng tạo 20 tác vụ, tự tải từng cái theo số luồng đã cấu hình, và hiển thị rõ link nào đang chờ, đang tải, hay lỗi.

**Why this priority**: Đây là chỗ đau lớn nhất hiện tại — batch mode hiện có ép người dùng nhận audio bất kể họ muốn gì, không hiện tiến trình từng link, và chạy tuần tự nên rất chậm.

**Independent Test**: Dán 5 link hợp lệ, chọn "video", bấm Tải tất cả; xác nhận cả 5 xuất hiện trong hàng đợi dưới dạng video (không phải audio), tiến trình cập nhật độc lập, và một link hỏng không chặn 4 link còn lại.

**Acceptance Scenarios**:

1. **Given** người dùng dán nhiều link, **When** họ chọn loại nội dung và mức chất lượng chung rồi bấm Tải tất cả, **Then** mọi link được tạo thành tác vụ đúng loại/chất lượng đã chọn, không bị ép về audio.
2. **Given** người dùng kéo một file `.txt` chứa danh sách URL vào cửa sổ, **When** thả file, **Then** các URL trong file được nạp vào ô nhập và lọc trùng.
3. **Given** người dùng kéo một link từ thanh địa chỉ trình duyệt vào cửa sổ, **When** thả, **Then** link được thêm vào ô nhập.
4. **Given** một trong các link không lấy được thông tin xem trước, **When** quá trình tạo tác vụ chạy, **Then** link đó được đánh dấu lỗi kèm lý do, các link còn lại vẫn được tạo bình thường.

---

### User Story 2 - Điều chỉnh số luồng tải song song (Priority: P1)

Người dùng mạng yếu muốn giảm còn 1 luồng; người dùng mạng khoẻ muốn tăng lên 6. Họ chỉnh trong Cài đặt và thay đổi có hiệu lực ngay, không cần khởi động lại.

**Why this priority**: Rẻ về mặt hiện thực (hạ tầng đã có), nhưng là điều kiện tiên quyết để tải hàng loạt hữu ích.

**Independent Test**: Đặt số luồng = 1, xếp 3 tác vụ, xác nhận chỉ 1 tác vụ ở trạng thái "đang tải" tại một thời điểm; tăng lên 3 trong lúc đang tải, xác nhận 2 tác vụ đang chờ bắt đầu chạy mà không cần khởi động lại.

**Acceptance Scenarios**:

1. **Given** số luồng đặt là N, **When** có nhiều hơn N tác vụ sẵn sàng, **Then** đúng N tác vụ chạy đồng thời, phần còn lại ở trạng thái chờ.
2. **Given** đang có N tác vụ chạy, **When** người dùng tăng số luồng lên M > N, **Then** các tác vụ đang chờ bắt đầu chạy ngay mà không cần khởi động lại ứng dụng.
3. **Given** đang có N tác vụ chạy, **When** người dùng giảm số luồng xuống M < N, **Then** các tác vụ đang chạy được chạy nốt, nhưng không tác vụ chờ nào được khởi động cho tới khi số đang chạy tụt xuống dưới M.

---

### User Story 3 - Hàng đợi sống sót qua khởi động lại (Priority: P1)

Người dùng đang tải dở 10 file thì phải tắt máy. Mở lại ứng dụng, họ thấy nguyên hàng đợi cũ và bấm "Tiếp tục tất cả" để tải nốt phần còn thiếu thay vì tải lại từ đầu.

**Why this priority**: Hiện tại toàn bộ hàng đợi biến mất khi đóng ứng dụng và các tác vụ dở kẹt vĩnh viễn ở trạng thái "đang tải" trong cơ sở dữ liệu — đây là mất dữ liệu, không phải thiếu tiện nghi.

**Independent Test**: Bắt đầu tải một file lớn, đóng ứng dụng giữa chừng, mở lại; xác nhận tác vụ xuất hiện ở trạng thái tạm dừng và khi tiếp tục thì tải nối tiếp phần đã có chứ không bắt đầu lại từ 0%.

**Acceptance Scenarios**:

1. **Given** ứng dụng bị đóng khi có tác vụ đang tải hoặc đang chờ, **When** mở lại ứng dụng, **Then** các tác vụ đó hiển thị lại trong hàng đợi với trạng thái tạm dừng.
2. **Given** một tác vụ tạm dừng có file tải dở, **When** người dùng tiếp tục nó, **Then** phần đã tải được giữ lại và chỉ tải phần còn thiếu.
3. **Given** cơ sở dữ liệu còn tác vụ ghi trạng thái "đang tải" từ phiên trước, **When** ứng dụng khởi động, **Then** trạng thái đó được chuyển thành tạm dừng, không có tác vụ nào kẹt.

---

### User Story 4 - Điều khiển hàng loạt và sắp thứ tự (Priority: P2)

Người dùng có 30 tác vụ trong hàng đợi, muốn ưu tiên 3 cái quan trọng lên đầu, và tạm dừng toàn bộ khi cần nhường băng thông cho việc khác.

**Why this priority**: Chỉ có giá trị khi hàng đợi đủ dài, tức là sau khi User Story 1 hoàn tất.


**Independent Test**: Xếp 5 tác vụ với số luồng = 1, kéo tác vụ cuối lên đầu, xác nhận nó là tác vụ tiếp theo được chạy; bấm "Tạm dừng tất cả", xác nhận không còn tác vụ nào đang tải.

**Acceptance Scenarios**:

1. **Given** nhiều tác vụ đang chờ, **When** người dùng kéo một tác vụ lên vị trí khác, **Then** thứ tự chạy tuân theo vị trí mới và được giữ nguyên sau khi khởi động lại ứng dụng.
2. **Given** có tác vụ đang tải và đang chờ, **When** người dùng bấm "Tạm dừng tất cả", **Then** mọi tác vụ chuyển sang tạm dừng và không tác vụ nào tiếp tục chạy.
3. **Given** nhiều tác vụ đang tạm dừng, **When** người dùng bấm "Tiếp tục tất cả", **Then** chúng quay lại hàng chờ theo đúng thứ tự trước đó.
4. **Given** người dùng bấm "Huỷ tất cả", **When** hộp thoại xác nhận được chấp nhận, **Then** mọi tác vụ chưa hoàn tất chuyển sang trạng thái đã huỷ.

---

### User Story 5 - Retry thông minh khi mạng chập chờn (Priority: P2)

Mạng người dùng rớt 10 giây giữa chừng. Ứng dụng tự đợi rồi thử lại, tải nối tiếp phần dở, không bắt người dùng phải tự bấm. Ngược lại, với video riêng tư thì báo lỗi ngay chứ không thử lại vô ích 3 lần.

**Why this priority**: Cải thiện tỉ lệ thành công thực tế, đồng thời loại bỏ độ trễ vô nghĩa khi gặp lỗi vĩnh viễn.

**Independent Test**: Ngắt mạng giữa lúc tải rồi nối lại sau 15 giây, xác nhận tác vụ tự hoàn tất mà không cần thao tác; đưa vào một link video riêng tư, xác nhận báo lỗi trong lần thử đầu tiên chứ không lặp 3 lần.

**Acceptance Scenarios**:

1. **Given** một tác vụ thất bại vì lỗi tạm thời (mất mạng, hết thời gian chờ, lỗi phía máy chủ), **When** ứng dụng thử lại, **Then** mỗi lần thử cách nhau một khoảng tăng dần, tối đa số lần đã cấu hình.
2. **Given** một tác vụ thất bại vì lỗi vĩnh viễn (nội dung riêng tư, đã bị gỡ, nền tảng không hỗ trợ), **When** lỗi được phân loại, **Then** tác vụ thất bại ngay lập tức mà không thử lại.
3. **Given** một tác vụ đang trong khoảng chờ giữa hai lần thử, **When** người dùng xem hàng đợi, **Then** trạng thái hiển thị rõ là "sẽ thử lại sau N giây (lần thứ K)".
4. **Given** một tác vụ đang chờ thử lại, **When** người dùng huỷ nó, **Then** vòng thử lại dừng ngay.

---

### User Story 6 - Giới hạn tốc độ tải (Priority: P3)

Người dùng dùng chung mạng với người khác, muốn giới hạn ứng dụng ở 2 MB/s để không chiếm hết băng thông.

**Independent Test**: Đặt giới hạn 500 KB/s, tải một file lớn, xác nhận tốc độ hiển thị trong hàng đợi dao động quanh mức đó chứ không vượt xa.

**Acceptance Scenarios**:

1. **Given** người dùng đặt giới hạn tốc độ, **When** một tác vụ chạy, **Then** tốc độ tải bị chặn ở mức đó.
2. **Given** giới hạn đặt là 0 hoặc để trống, **When** một tác vụ chạy, **Then** không có giới hạn nào được áp.

---

### User Story 7 - Chạy nền với biểu tượng khay hệ thống (Priority: P3)

Người dùng đóng cửa sổ nhưng muốn hàng đợi vẫn chạy tiếp, và nhận thông báo hệ thống khi mọi thứ tải xong.

**Independent Test**: Bật tuỳ chọn chạy nền, đóng cửa sổ khi còn tác vụ đang tải, xác nhận biểu tượng khay còn đó và tác vụ vẫn hoàn tất; bấm biểu tượng để mở lại cửa sổ.

**Acceptance Scenarios**:

1. **Given** tuỳ chọn chạy nền đang bật, **When** người dùng đóng cửa sổ trong lúc còn tác vụ chưa xong, **Then** ứng dụng thu về khay hệ thống và tiếp tục tải.
2. **Given** một tác vụ hoàn tất khi cửa sổ không hiển thị, **When** tác vụ kết thúc, **Then** người dùng nhận được thông báo hệ thống có thể bấm để mở thư mục chứa file.
3. **Given** tuỳ chọn chạy nền đang tắt (mặc định), **When** người dùng đóng cửa sổ, **Then** ứng dụng thoát hẳn như hiện nay.

---

### User Story 8 - Link tải trực tiếp và luồng HLS (Priority: P3)

Người dùng có một link `.mp4` trực tiếp hoặc một link `.m3u8`, dán vào và tải được như mọi link khác.

**Independent Test**: Dán một URL trỏ thẳng tới file media và một URL `.m3u8`, xác nhận cả hai tạo được tác vụ và tải thành công.

**Acceptance Scenarios**:

1. **Given** một URL trỏ trực tiếp tới file media, **When** người dùng xem trước, **Then** ứng dụng nhận diện được và cho phép tải, dùng tên file suy ra từ URL khi nguồn không cung cấp tiêu đề.
2. **Given** một URL luồng HLS/DASH, **When** người dùng xem trước, **Then** ứng dụng liệt kê các mức chất lượng có trong manifest và tải được mức đã chọn.
3. **Given** một URL không phải media và không nền tảng nào nhận diện được, **When** người dùng xem trước, **Then** thông báo lỗi nêu rõ đã thử những cách nào.

---

### Edge Cases

- Dán 500 link cùng lúc → ứng dụng không được treo giao diện; xem trước chạy song song có giới hạn và hiển thị tiến độ xử lý.
- Danh sách dán vào có link trùng nhau hoặc trùng với tác vụ đang trong hàng đợi → cảnh báo và bỏ trùng, không tạo tác vụ lặp.
- File `.txt` kéo vào quá lớn hoặc không phải văn bản → báo lỗi rõ ràng, không nạp.
- Giảm số luồng xuống 1 trong khi 3 tác vụ đang chạy → không giết tác vụ đang chạy giữa chừng.
- Tạm dừng rồi tiếp tục một tác vụ rất nhanh liên tiếp → không được để lẫn handle giữa lần chạy cũ và mới (lỗi tiềm ẩn hiện có tại `queue.rs:111`).
- Huỷ một tác vụ gallery trong lúc đang lấy danh sách item → phải dừng được ngay (hiện tại không dừng được, `queue.rs:371-402`).
- Kéo tác vụ đang tải lên/xuống trong danh sách → không làm gián đoạn tác vụ đó.
- Máy tắt đột ngột (mất điện) giữa lúc tải → lần mở sau không có tác vụ nào kẹt ở trạng thái đang tải.
- Đặt giới hạn tốc độ cực thấp (1 KB/s) → tác vụ vẫn chạy, không bị hiểu nhầm thành treo và bị retry.

## Requirements *(mandatory)*

### Functional Requirements

**Nhập liệu**

- **FR-101**: Hệ thống PHẢI cho phép tải hàng loạt nhiều URL với loại nội dung (audio/video) và mức chất lượng do người dùng chọn, áp dụng chung cho cả lô; hệ thống KHÔNG được tự ép về audio như hiện nay.
- **FR-102**: Hệ thống PHẢI lấy thông tin xem trước cho các URL trong lô một cách song song có giới hạn, hiển thị tiến độ xử lý lô, và không chặn giao diện.
- **FR-103**: Ở chế độ hàng loạt, mỗi URL PHẢI hiển thị trạng thái riêng (chờ xử lý, đang xem trước, đã tạo tác vụ, lỗi kèm lý do); một URL lỗi KHÔNG được ngăn các URL còn lại được tạo tác vụ.
- **FR-104**: Hệ thống PHẢI nhận URL thả vào cửa sổ ứng dụng (kéo từ trình duyệt hoặc trình soạn thảo) và nạp vào ô nhập.
- **FR-105**: Hệ thống PHẢI nhận file `.txt` thả vào cửa sổ, đọc từng dòng, trích các URL http(s) hợp lệ và nạp vào ô nhập.
- **FR-106**: Hệ thống PHẢI cho phép chọn file danh sách URL qua hộp thoại chọn file, tương đương với việc kéo-thả.
- **FR-107**: Hệ thống PHẢI loại bỏ URL trùng lặp trong cùng một lô và cảnh báo khi một URL đã tồn tại trong hàng đợi hiện tại, cho người dùng quyết định bỏ qua hay tạo thêm.
- **FR-108 — FR-109**: *(bỏ trống có chủ ý — xem Ghi chú phạm vi ở đầu tài liệu)*
- **FR-110**: Ứng dụng KHÔNG được đọc nội dung clipboard của người dùng ở chế độ nền hay theo chu kỳ. Chỉ chấp nhận thao tác dán do chính người dùng chủ động thực hiện vào ô nhập.

**Hàng đợi**

- **FR-111**: Hệ thống PHẢI có một hàng đợi chờ có thứ tự rõ ràng: tác vụ mới vào cuối hàng, và chỉ được khởi chạy khi có slot trống — thay cho cơ chế spawn ngay rồi chờ semaphore hiện tại.
- **FR-112**: Người dùng PHẢI đặt được số tác vụ chạy song song trong Cài đặt, trong khoảng 1 đến 8, mặc định 3.
- **FR-113**: Thay đổi số luồng song song PHẢI có hiệu lực ngay: tăng thì các tác vụ chờ khởi chạy ngay, giảm thì không giết tác vụ đang chạy mà chỉ ngừng khởi chạy tác vụ mới cho tới khi số đang chạy tụt xuống dưới ngưỡng mới.
- **FR-114**: Hàng đợi PHẢI được khôi phục từ cơ sở dữ liệu khi ứng dụng khởi động, hiển thị lại mọi tác vụ chưa hoàn tất.
- **FR-115**: Khi khởi động, mọi tác vụ còn ghi trạng thái đang tải hoặc đang lấy thông tin từ phiên trước PHẢI được chuyển sang trạng thái tạm dừng để người dùng tiếp tục hoặc huỷ; KHÔNG được để tác vụ kẹt trạng thái.
- **FR-116**: Tiếp tục một tác vụ tạm dừng PHẢI tận dụng phần đã tải, không tải lại từ đầu, khi nguồn hỗ trợ tải nối tiếp.
- **FR-117**: Người dùng PHẢI sắp xếp lại được thứ tự các tác vụ đang chờ bằng thao tác kéo-thả, và thứ tự đó PHẢI được lưu lại qua các lần khởi động.
- **FR-118**: Hệ thống PHẢI cung cấp thao tác hàng loạt: Tạm dừng tất cả, Tiếp tục tất cả, Huỷ tất cả (có xác nhận), và Xoá các tác vụ đã kết thúc khỏi danh sách.
- **FR-119**: Sắp xếp lại thứ tự KHÔNG được làm gián đoạn tác vụ đang chạy.

**Độ tin cậy**

- **FR-120**: Hệ thống PHẢI phân loại lỗi thành tạm thời (mất mạng, hết thời gian chờ, lỗi máy chủ 5xx, giới hạn tần suất) và vĩnh viễn (nội dung riêng tư, đã bị gỡ, nền tảng không hỗ trợ, sai tham số).
- **FR-121**: Hệ thống CHỈ được tự thử lại với lỗi tạm thời, với khoảng chờ tăng dần giữa các lần; lỗi vĩnh viễn PHẢI thất bại ngay ở lần đầu.
- **FR-122**: Trong lúc chờ thử lại, hàng đợi PHẢI hiển thị rõ lần thử thứ mấy và còn bao lâu tới lần thử tiếp theo.
- **FR-123**: Huỷ hoặc tạm dừng một tác vụ đang chờ thử lại PHẢI dừng vòng thử lại ngay lập tức.
- **FR-124**: Thao tác huỷ và tạm dừng PHẢI có hiệu lực ở mọi giai đoạn của tác vụ, bao gồm cả giai đoạn lấy danh sách item của nội dung dạng thư viện ảnh.
- **FR-125**: Tạm dừng rồi tiếp tục một tác vụ liên tiếp trong thời gian ngắn PHẢI không gây mất khả năng điều khiển tác vụ đó.

**Tốc độ và chạy nền**

- **FR-126**: Người dùng PHẢI đặt được giới hạn tốc độ tải tổng thể trong Cài đặt; giá trị 0 hoặc để trống nghĩa là không giới hạn.
- **FR-127**: Hệ thống PHẢI có tuỳ chọn (mặc định TẮT) tiếp tục chạy nền dưới biểu tượng khay hệ thống khi cửa sổ bị đóng, kèm menu khay cho phép mở lại cửa sổ, tạm dừng tất cả, và thoát hẳn.
- **FR-128**: Hệ thống PHẢI gửi thông báo hệ thống khi một tác vụ hoàn tất hoặc thất bại trong lúc cửa sổ không hiển thị; bấm vào thông báo PHẢI mở thư mục chứa file (khi thành công) hoặc mở ứng dụng (khi thất bại).
- **FR-129**: Khi tuỳ chọn chạy nền đang tắt, đóng cửa sổ PHẢI thoát ứng dụng như hành vi hiện tại.

**Nguồn mở rộng**

- **FR-130**: Hệ thống PHẢI tải được URL trỏ trực tiếp tới file media và URL luồng HLS/DASH, dùng cùng luồng xem trước và hàng đợi như các nguồn khác.
- **FR-131**: Khi cả engine tải chính lẫn engine dự phòng đều không nhận diện được URL, thông báo lỗi PHẢI nêu rõ đã thử những cách nào.

**Dọn dẹp kèm theo**

- **FR-132**: Mọi chuỗi hiển thị cho người dùng trong luồng tải và lịch sử PHẢI đi qua hệ thống đa ngôn ngữ; KHÔNG còn chuỗi viết cứng trong mã nguồn giao diện.
- **FR-133**: Dự án PHẢI có kiểm tra tự động phát hiện key dịch tồn tại ở ngôn ngữ này nhưng thiếu ở ngôn ngữ kia, chạy cùng bộ test.
- **FR-134**: Lưới chọn ảnh của nội dung dạng thư viện PHẢI hiển thị đầy đủ các item mà người dùng có thể chọn, không được cắt bớt trong khi vẫn coi phần bị cắt là đã chọn.
- **FR-135**: Cấu hình kiểm tra kiểu PHẢI bao phủ cả thư mục test, để lỗi kiểu trong test bị phát hiện lúc build.

### Key Entities

- **Tác vụ tải (Download Job)**: Bổ sung so với v1 — vị trí trong hàng đợi (số thứ tự dùng để sắp xếp), số lần đã thử lại, thời điểm dự kiến thử lại tiếp theo, và phân loại lỗi gần nhất (tạm thời/vĩnh viễn).
- **Lô tải (Batch)**: Một lần người dùng gửi nhiều URL cùng lúc; gồm danh sách URL, trạng thái xử lý từng URL, lựa chọn loại nội dung và chất lượng chung. Tồn tại trong thời gian phiên làm việc, không cần lưu lâu dài.
- **Cài đặt ứng dụng (App Settings)**: Bổ sung — số luồng song song, giới hạn tốc độ, bật/tắt chạy nền, số lần thử lại tối đa.

## Success Criteria *(mandatory)*

- **SC-101**: Với 20 link hợp lệ dán cùng lúc, toàn bộ tác vụ được tạo trong vòng dưới 15 giây và giao diện vẫn thao tác được suốt quá trình đó.
- **SC-102**: Ở chế độ hàng loạt, 100% tác vụ được tạo đúng loại nội dung và mức chất lượng người dùng đã chọn.
- **SC-103**: Thay đổi số luồng song song có hiệu lực trong vòng dưới 2 giây, không cần khởi động lại ứng dụng.
- **SC-104**: Sau khi buộc đóng ứng dụng và mở lại, 100% tác vụ chưa hoàn tất hiển thị lại trong hàng đợi và 0 tác vụ kẹt ở trạng thái đang tải.
- **SC-105**: Tiếp tục một tác vụ tạm dừng có file tải dở giữ lại được ít nhất 90% phần đã tải với các nguồn hỗ trợ tải nối tiếp.
- **SC-106**: Với lỗi vĩnh viễn, người dùng nhận thông báo thất bại trong vòng dưới 5 giây kể từ lần thử đầu tiên (thay vì phải chờ hết 3 vòng thử như hiện nay).
- **SC-107**: Với mạng gián đoạn dưới 60 giây, tác vụ tự hoàn tất mà không cần bất kỳ thao tác nào từ người dùng.
- **SC-108**: Kéo một tác vụ lên đầu hàng đợi khiến nó là tác vụ tiếp theo được khởi chạy trong 100% trường hợp.
- **SC-109**: Khi bật giới hạn tốc độ, tốc độ tải trung bình đo trong 30 giây không vượt quá 110% mức đã đặt.
- **SC-110**: Mã nguồn không chứa lệnh đọc clipboard nào ngoài trình xử lý sự kiện dán do người dùng chủ động kích hoạt — kiểm chứng được bằng test.
- **SC-111**: Bộ test có kiểm tra parity giữa các file ngôn ngữ và kiểm tra này thất bại khi cố tình thêm key chỉ ở một ngôn ngữ.

## Assumptions

- Người dùng chấp nhận rằng số luồng song song cao không đồng nghĩa tải nhanh hơn; nền tảng nguồn có thể giới hạn tần suất và ứng dụng chỉ cung cấp lựa chọn chứ không tối ưu hộ.
- Giới hạn tốc độ được áp ở mức từng tiến trình tải của engine chứ không phải điều tiết toàn cục chính xác; với N luồng song song, tổng băng thông thực tế có thể xấp xỉ N lần mức đặt nếu engine chỉ nhận giới hạn theo từng tiến trình — cần làm rõ trong bước lập kế hoạch và ghi chú rõ cho người dùng ngay trong giao diện Cài đặt.
- Ứng dụng không đọc clipboard ở chế độ nền dưới bất kỳ hình thức nào. Đây là ranh giới quyền riêng tư đã được chốt, không phải một tuỳ chọn có thể bật lên sau này.
- Chạy nền dưới khay hệ thống hoạt động khác nhau giữa các môi trường desktop Linux; chấp nhận suy giảm nhẹ về trải nghiệm trên một số môi trường miễn là không mất tính năng tải.
- Việc tách nhỏ file giao diện tải xuống hiện tại là điều kiện tiên quyết, không phải việc tuỳ chọn — mọi tính năng mới trong spec này đều chạm vào file đó.
