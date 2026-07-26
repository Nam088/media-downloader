-- Phase 3 (specs/004-library): biến `downloaded_files` từ một bảng CHỈ-GHI
-- thành chỉ mục thật của Thư viện (FR-301 → FR-304, FR-307 → FR-310).
--
-- Bảng đã tồn tại từ 0001 và đã được ghi ở mọi lần tác vụ hoàn tất, nhưng
-- chưa từng có ai đọc (`get_downloaded_file_for_job` vẫn mang
-- `#[allow(dead_code)]`). Nó mới chỉ giữ đường dẫn, định dạng, dung lượng và
-- thời điểm hoàn tất — thiếu đúng những thứ làm nên một thư viện: tiêu đề,
-- loại nội dung, nền tảng, URL gốc, thời lượng, ảnh đại diện.
--
-- KHÔNG tạo bảng mới. Dữ liệu thật của người dùng (105 dòng ở thời điểm viết)
-- đã nằm sẵn ở đây; một bảng thứ hai sẽ biến FR-303 (nạp lại lịch sử cũ)
-- thành một bài toán đồng bộ hai chiều vĩnh viễn thay vì một lần bổ sung cột.
--
-- LƯU Ý VỀ QUY TẮC MIGRATION: `rusqlite_migration` theo dõi tiến độ bằng SỐ
-- LƯỢNG migration, không bằng nội dung (xem đầu file
-- 0005_fix_stale_app_settings_schema.sql). File này một khi đã phát hành thì
-- KHÔNG được sửa nữa — chỉ được thêm file mới. Và thêm file .sql là CHƯA đủ:
-- phải có dòng `M::up(include_str!("migrations/0012_library_index.sql"))`
-- trong `migrations()` ở `db/mod.rs`, nếu không nó bị bỏ qua trong im lặng
-- (test `migration_0012_is_registered_and_is_what_adds_the_library_columns`
-- canh đúng chỗ này).

-- Mọi cột chuỗi đều `NOT NULL DEFAULT ''` chứ không nullable: SQLite chỉ cho
-- ADD COLUMN kèm NOT NULL khi có giá trị mặc định khác NULL, và '' ở đây mang
-- đúng nghĩa "chưa nạp" cho các dòng cũ — `Db::backfill_library_index` chạy
-- ngay sau migration sẽ điền chúng từ `download_jobs`. Nhờ vậy tầng Rust đọc
-- ra `String` chứ không phải `Option<String>`, khớp với hợp đồng TypeScript
-- (`platform: string`, không phải `string | null`).
ALTER TABLE downloaded_files ADD COLUMN title TEXT NOT NULL DEFAULT '';
ALTER TABLE downloaded_files ADD COLUMN media_type TEXT NOT NULL DEFAULT '';
ALTER TABLE downloaded_files ADD COLUMN platform TEXT NOT NULL DEFAULT '';
ALTER TABLE downloaded_files ADD COLUMN source_url TEXT NOT NULL DEFAULT '';

-- Ngược lại, hai cột này nullable là CÓ CHỦ Ý: `NULL` ở đây nghĩa là "không
-- biết", và đó là một câu trả lời khác hẳn với 0 giây hay một ảnh rỗng. Mọi
-- dòng có trước phase này đều rơi vào đúng ô đó — không có chỗ nào để lấy ra
-- thời lượng của một file đã tải ba tháng trước mà không mở lại chính file ấy.
ALTER TABLE downloaded_files ADD COLUMN duration_seconds INTEGER;
ALTER TABLE downloaded_files ADD COLUMN thumbnail_path TEXT;

-- FR-323. Mặc định 0 ("còn đó") chứ không phải "chưa kiểm tra": việc kiểm tra
-- sự tồn tại là một thao tác chạm đĩa cho từng dòng, và FR-327 cấm nó chặn
-- giao diện — nên nó KHÔNG chạy trong migration, cũng không chạy lúc khởi
-- động, mà nằm ở `reconcile_library` (nền, theo lô). Một mục vừa tải xong thì
-- "còn đó" là sự thật, và một mục cũ bị đánh dấu thiếu chậm một nhịp thì tệ
-- hơn hẳn việc cả thư viện hiện ra dưới dạng "thiếu" trong lần mở đầu tiên.
ALTER TABLE downloaded_files ADD COLUMN is_missing INTEGER NOT NULL DEFAULT 0;

-- FR-307: tìm theo tiêu đề VÀ tên file, gõ tới đâu lọc tới đó.
--
-- Cột này là tiêu đề + đường dẫn đã hạ hoa **bằng Rust** (`str::to_lowercase`,
-- theo bảng chữ hoa/thường của Unicode), ghép lại. Nó tồn tại vì `LIKE` của
-- SQLite chỉ không phân biệt hoa thường với ASCII: `'ĐỪNG' LIKE '%đừng%'` trả
-- về FALSE, và `lower()` dựng sẵn của SQLite cũng chỉ đụng tới A-Z. Với một
-- thư viện mà phần lớn tiêu đề là tiếng Việt, tìm kiếm phân biệt hoa thường
-- coi như không dùng được. Hạ hoa một lần lúc ghi, so sánh với từ khoá cũng
-- đã hạ hoa lúc đọc, là cách duy nhất giữ được điều đó mà không phải nạp cả
-- bảng lên bộ nhớ để lọc.
ALTER TABLE downloaded_files ADD COLUMN search_text TEXT NOT NULL DEFAULT '';

-- Gộp các dòng cùng trỏ vào MỘT file (edge case "hai mục trỏ tới cùng một
-- file do tải lại"). Trong CSDL thật của người dùng có 105 dòng nhưng chỉ 66
-- đường dẫn khác nhau: tải lại cùng một link ghi đè lên đúng file cũ, nhưng
-- mỗi lần lại thêm một dòng mới. Hiển thị nguyên trạng thì thư viện có ba ô
-- giống hệt nhau cùng trỏ vào một file duy nhất trên đĩa.
--
-- Giữ lại dòng MỚI NHẤT theo `completed_at` (hoà thì theo rowid): nó mang
-- dung lượng và thời điểm của lần ghi thật sự đang nằm trên đĩa; các dòng cũ
-- mô tả một nội dung đã bị chính lần tải sau đè mất.
DELETE FROM downloaded_files
WHERE rowid NOT IN (
    SELECT rowid FROM (
        SELECT rowid, ROW_NUMBER() OVER (
            PARTITION BY file_path ORDER BY completed_at DESC, rowid DESC
        ) AS rank_in_path
        FROM downloaded_files
    )
    WHERE rank_in_path = 1
);

-- Và biến "trùng đường dẫn" thành trạng thái KHÔNG GHI ĐƯỢC từ đây trở đi,
-- thay vì một thứ mọi nơi gọi phải nhớ kiểm tra. Ràng buộc này là thứ cho
-- phép `insert_downloaded_file` dùng UPSERT (tải lại = cập nhật đúng dòng cũ)
-- và cho phép `rename`/`move`/`relink` phát hiện va chạm bằng chính CSDL —
-- đúng tinh thần FR-322: không bao giờ ghi đè trong im lặng.
CREATE UNIQUE INDEX idx_downloaded_files_path_unique ON downloaded_files (file_path);

-- FR-310: thư viện 10.000 mục phải vẫn mượt. Chi phí đắt nhất ở quy mô đó
-- KHÔNG phải việc lọc mà là việc SẮP XẾP: không có chỉ mục thì mỗi lần mở
-- trang, SQLite phải đọc toàn bộ 10.000 dòng vào một b-tree tạm rồi mới trả
-- được 60 dòng đầu. Mỗi chỉ mục dưới đây tồn tại để đúng một câu ORDER BY
-- (hoặc một cặp lọc-rồi-sắp) đọc được kết quả theo thứ tự sẵn có và dừng lại
-- ngay sau LIMIT.
--
-- Thứ tự mặc định của thư viện, và cũng là câu truy vấn chạy nhiều nhất.
CREATE INDEX idx_downloaded_files_completed_at ON downloaded_files (completed_at DESC);

-- Ba bộ lọc của FR-308 (và FR-329: bấm vào một phần trong thống kê). Cột lọc
-- đứng trước, cột sắp xếp mặc định đứng sau, nên "chỉ audio, mới nhất trước"
-- là một lần quét dải liên tục chứ không phải lọc rồi sắp lại từ đầu.
CREATE INDEX idx_downloaded_files_media_type ON downloaded_files (media_type, completed_at DESC);
CREATE INDEX idx_downloaded_files_platform ON downloaded_files (platform, completed_at DESC);
CREATE INDEX idx_downloaded_files_format ON downloaded_files (file_format, completed_at DESC);

-- Ba tiêu chí sắp xếp còn lại của FR-309. `COLLATE NOCASE` phải nằm ngay
-- trong chỉ mục, nếu không câu `ORDER BY title COLLATE NOCASE` sẽ không dùng
-- được nó (SQLite chỉ khớp chỉ mục khi collation của chỉ mục và của câu lệnh
-- trùng nhau).
CREATE INDEX idx_downloaded_files_title ON downloaded_files (title COLLATE NOCASE);
CREATE INDEX idx_downloaded_files_size ON downloaded_files (file_size_bytes);
CREATE INDEX idx_downloaded_files_duration ON downloaded_files (duration_seconds);

-- FR-324 (dọn các mục thiếu). Chỉ mục một phần: chỉ đánh chỉ mục đúng những
-- dòng đang thiếu, vốn là thiểu số áp đảo — một chỉ mục đầy đủ trên một cột
-- gần như toàn số 0 thì vừa to vừa vô dụng.
CREATE INDEX idx_downloaded_files_missing ON downloaded_files (is_missing) WHERE is_missing = 1;
