-- Phase 2 (specs/003-media-output): preset — một bộ `Tuỳ chọn đầu ra` có tên,
-- lưu lâu dài, kèm cờ đánh dấu bộ mặc định (FR-228 → FR-233).
--
-- `output_options` là ĐÚNG cùng một blob JSON mà `download_jobs.output_options`
-- (migration 0010) đang lưu, không phải một bản sao có lược đồ riêng. Đó là
-- toàn bộ ý của bảng này: một preset *là* tuỳ chọn đầu ra của một tác vụ, chỉ
-- thêm cái tên. Dùng chung một dạng tuần tự hoá nghĩa là hai bên không thể trôi
-- khỏi nhau khi phase này còn thêm tiếp tuỳ chọn (phụ đề, cắt đoạn, chapter),
-- và `#[serde(default)]` trên `models::OutputOptions` lo luôn FR-233 cho cả
-- hai: preset lưu từ phiên bản trước vẫn đọc được, tuỳ chọn mới nhận giá trị
-- mặc định thay vì làm hỏng cả bản ghi.
--
-- Vì thế KHÔNG có cột nào ở đây tương ứng với một lựa chọn cụ thể. Thêm một
-- lựa chọn mới trong phase này không được kéo theo migration nào cho bảng này.
CREATE TABLE presets (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    output_options TEXT NOT NULL,
    is_default INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0, 1)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- Tên preset là duy nhất. Hai preset trùng tên thì danh sách chọn trở thành
-- một câu đố ("cái nào là cái mình vừa sửa?"), còn thao tác đổi tên/xoá thì
-- không còn cách nào để người dùng nhắm đúng mục. Ràng buộc nằm ở CSDL để
-- không phụ thuộc vào việc mọi nơi gọi đều nhớ kiểm tra trước; tầng Rust chỉ
-- cắt khoảng trắng thừa và dịch vi phạm này thành mã lỗi `PRESET_NAME_TAKEN`.
CREATE UNIQUE INDEX presets_name_unique ON presets (name);

-- ĐÚNG MỘT preset mặc định, do CSDL bảo đảm chứ không phải do mã ứng dụng.
--
-- Chỉ mục một phần (partial index) trên đúng những dòng `is_default = 1` biến
-- "có hai preset cùng là mặc định" thành một trạng thái *không ghi được*, thay
-- vì một trạng thái mà mọi nơi gọi phải nhớ tránh. Chế độ hỏng mà nó chặn là
-- cái đắt nhất và khó thấy nhất: một lần đặt mặc định mới nhưng quên xoá cờ cũ
-- (hoặc chết giữa chừng) để lại hai dòng cùng nhận, và từ đó "preset mặc định"
-- trở thành thứ phụ thuộc vào thứ tự dòng trả về.
--
-- Lưu ý cho người viết câu lệnh: SQLite kiểm tra ràng buộc UNIQUE ngay tại
-- từng dòng bị ghi, không hoãn tới cuối câu lệnh hay cuối giao dịch. Nên bước
-- đặt mặc định mới PHẢI xoá cờ cũ TRƯỚC rồi mới bật cờ mới (xem
-- `Db::set_default_preset`), cả hai trong một giao dịch — làm ngược lại sẽ
-- chạm chỉ mục này giữa chừng và thất bại.
--
-- KHÔNG có preset nào mang cờ mặc định cũng là một trạng thái hợp lệ: đó là
-- tình trạng của mọi cài đặt mới, và là tình trạng còn lại sau khi xoá chính
-- preset mặc định.
CREATE UNIQUE INDEX presets_single_default ON presets (is_default) WHERE is_default = 1;
