/** Thư viện media đã tải (`specs/004-library`) — hợp đồng với backend.
 *
 * Mọi kiểu ở đây là bản sao đúng của một struct trong `src-tauri/src/models.rs`
 * (hoặc `commands/library.rs`). Tên trường bên trong một struct giữ nguyên
 * `snake_case` vì Tauri chỉ đổi sang camelCase cho **tham số của lệnh**, không
 * cho nội dung của một đối tượng được tuần tự hoá.
 */

import type { MediaType } from "./download";

/** Một file đã tải, kèm mọi thứ cần để vẽ nó lên lưới mà không cần hỏi lại
 * nguồn (FR-301). Khớp `models::LibraryItem`. */
export interface LibraryItem {
  id: string;
  file_path: string;
  title: string;
  media_type: MediaType;
  file_format: string;
  file_size_bytes: number;
  /** `null` = **không biết**, không phải 0 giây. Mọi mục nạp lại từ lịch sử
   * trước Phase 3 đều như vậy: thời lượng chỉ đo được bằng cách mở chính file
   * ấy, và làm việc đó cho cả thư viện lúc khởi động là thứ FR-327 cấm. Giao
   * diện phải để trống ô thời lượng ở đó, không hiện `0:00`. */
  duration_seconds: number | null;
  platform: string;
  source_url: string;
  /** Đường dẫn ảnh đại diện **trên máy** (FR-304), không phải URL — cần đi
   * qua `convertFileSrc` để hiện được trong webview. `null` khi nguồn không
   * có ảnh hoặc mục được nạp lại từ lịch sử cũ; khi đó dùng ảnh thay thế theo
   * `media_type` chứ không để ô trống làm vỡ bố cục. */
  thumbnail_path: string | null;
  downloaded_at: string;
  /** File không còn ở `file_path` tại lần đối soát gần nhất (FR-323). Chưa
   * chắc đã mất hẳn — một ổ đĩa ngoài đã tháo cũng cho ra `true`. */
  is_missing: boolean;
  job_id: string;
  /** Nguồn phát **thật sự đã giao file** khi engine SpotiFLAC tải một mục
   * `media_type = "music"`: `"tidal" | "qobuz" | "deezer" | "amazon"` hoặc
   * `"ext:<tên extension>"`. `null` cho mọi file của engine khác.
   *
   * Optional vì cột `downloaded_files.source_provider` đã có trong CSDL nhưng
   * struct Rust `models::LibraryItem` CHƯA expose trường này — cho tới khi
   * backend thêm nó vào `LibraryItem` + `row_to_library_item`, mọi payload đều
   * vắng field và giao diện phải coi đó là "không có badge", không được crash. */
  source_provider?: string | null;
}

/** FR-309. */
export type LibrarySort = "downloaded_at" | "title" | "file_size" | "duration";

export type SortDirection = "asc" | "desc";

/** Trạng thái duyệt thư viện (FR-307 → FR-310). Khớp `models::LibraryQuery`.
 *
 * Mọi trường đều có mặc định ở phía Rust, nên `list_library({ query: {} })`
 * (hoặc bỏ hẳn `query`) là hợp lệ và có nghĩa "mọi thứ, mới nhất trước".
 *
 * Các bộ lọc KHÁC nhau kết hợp theo "và" (FR-308); nhiều giá trị TRONG cùng
 * một bộ lọc kết hợp theo "hoặc". */
export interface LibraryQuery {
  /** Khớp tiêu đề hoặc tên file, không phân biệt hoa thường kể cả với tiếng
   * Việt (backend so trên một cột đã hạ hoa theo Unicode). */
  search?: string | null;
  media_types?: MediaType[];
  platforms?: string[];
  formats?: string[];
  /** Chuỗi RFC 3339, bao gồm cả hai đầu. */
  downloaded_from?: string | null;
  downloaded_to?: string | null;
  /** `null`/bỏ qua = không quan tâm; `true` = chỉ các mục đang thiếu (màn
   * hình dọn dẹp của FR-324). */
  is_missing?: boolean | null;
  sort?: LibrarySort;
  direction?: SortDirection;
  /** FR-310: xin từng trang khi cuộn thay vì nhận cả 10.000 dòng một lần.
   * Bỏ trống = không giới hạn (dùng khi xuất danh sách phát theo bộ lọc). */
  limit?: number | null;
  offset?: number | null;
}

/** Một dòng trong phân bố của FR-328. `key` là giá trị thô (`"youtube"`,
 * `"audio"`) — cũng chính là giá trị đem đặt vào `LibraryQuery` khi người
 * dùng bấm vào nó (FR-329). */
export interface LibraryBreakdownEntry {
  key: string;
  item_count: number;
  total_size_bytes: number;
}

/** FR-328. Tính trên đúng bộ lọc đang áp, nên luôn khớp với thứ đang hiển thị
 * (SC-307). */
export interface LibraryStats {
  total_items: number;
  total_size_bytes: number;
  missing_items: number;
  by_platform: LibraryBreakdownEntry[];
  by_media_type: LibraryBreakdownEntry[];
  /** Các định dạng thật sự có trong thư viện — dùng để dựng bộ lọc định dạng
   * thay vì một danh sách cứng. */
  formats: string[];
}

/** Kết quả một vòng đối soát (FR-323). */
export interface LibraryReconcileReport {
  checked: number;
  /** Tổng số mục đang bị đánh dấu thiếu sau vòng này. */
  missing: number;
  changed_item_ids: string[];
}

/** Sự kiện phát sau MỖI lô của `reconcile_library`, để lưới cập nhật dần thay
 * vì đứng im tới khi quét xong cả thư viện (FR-327). Tên sự kiện:
 * `"library:reconciled"`. */
export interface LibraryReconciledEvent {
  changed_item_ids: string[];
  checked: number;
}

/** Mã lỗi riêng của Thư viện, để `ErrorBanner` dịch được thay vì hiện chuỗi
 * tiếng Anh của backend.
 *
 * `FILE_EXISTS` là mã của FR-322: đã có file ở đường dẫn đích, và hệ thống
 * **không** ghi đè, cũng không tự đổi tên — thao tác dừng lại và người dùng
 * chọn tên khác. */
export const LIBRARY_ERROR_CODES = {
  FILE_EXISTS: "FILE_EXISTS",
  TRASH_FAILED: "TRASH_FAILED",
  INVALID_FILE_NAME: "INVALID_FILE_NAME",
  NOT_A_FILE: "NOT_A_FILE",
  EMPTY_PLAYLIST: "EMPTY_PLAYLIST",
} as const;

export type LibraryErrorCode =
  (typeof LIBRARY_ERROR_CODES)[keyof typeof LIBRARY_ERROR_CODES];
