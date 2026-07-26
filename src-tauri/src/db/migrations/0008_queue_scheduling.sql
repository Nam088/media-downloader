-- Phase 1 (specs/002-download-power): hàng đợi chờ có thứ tự thật sự + retry
-- là trạng thái dữ liệu thay vì vòng lặp trong task.
--
-- `queue_position` là REAL chứ không phải INTEGER vì thứ tự dùng *fractional
-- indexing*: kéo một mục vào giữa hai mục khác chỉ ghi đúng MỘT dòng, với giá
-- trị là điểm giữa của hai hàng xóm.
--
-- Lý do quan trọng hơn cả hiệu năng: nếu mỗi lần kéo phải ghi lại vị trí của
-- cả danh sách, thì một job vừa được thêm vào trong lúc người dùng đang kéo sẽ
-- bị ghi đè vị trí — snapshot mà giao diện gửi lên đã cũ. Chỉ đụng một dòng thì
-- không có tranh chấp đó (FR-117, FR-119).
--
-- Khe hở giữa hai vị trí bị chia đôi mỗi lần chèn vào cùng một chỗ; khi nó nhỏ
-- hơn ngưỡng an toàn, `renormalize_positions_within` đánh số lại 1.0, 2.0, 3.0…
-- Xem `db::position_between` và `db::needs_renormalize`.
--
-- `retry_count` / `next_retry_at`: một job đang chờ thử lại là job có
-- status='queued' và next_retry_at ở tương lai (FR-121, FR-122). Cách này
-- tránh phải thêm giá trị mới vào ràng buộc CHECK trên cột status — SQLite
-- không ALTER được CHECK, sẽ phải rebuild cả bảng.
ALTER TABLE download_jobs ADD COLUMN queue_position REAL NOT NULL DEFAULT 0;
ALTER TABLE download_jobs ADD COLUMN retry_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE download_jobs ADD COLUMN next_retry_at TEXT;

-- Job có sẵn đều mang mặc định 0, tức là hoà nhau hết. Đánh số lại theo rowid
-- (xấp xỉ thứ tự tạo) để chúng có vị trí phân biệt ngay từ đầu, thay vì phải
-- dựa vào `created_at` làm tiêu chí phân định mãi mãi.
UPDATE download_jobs SET queue_position = rowid;

CREATE INDEX idx_download_jobs_dispatch
    ON download_jobs (status, queue_position, created_at);
