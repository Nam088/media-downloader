/**
 * Các hàm định dạng dùng chung cho hàng đợi, lịch sử, và khu vực xem trước.
 *
 * Trước đây mỗi component tự viết một bản; ba bản đó đã trôi khác nhau (một
 * bản không đệm số 0 cho phần giây). Giữ ở một chỗ để mọi màn hình hiển thị
 * giống nhau.
 */

const PLACEHOLDER_TIME = "--:--";
const PLACEHOLDER_VALUE = "--";

/** Giây → `m:ss`, hoặc `h:mm:ss` khi từ một tiếng trở lên. */
export function formatDuration(seconds: number | null | undefined): string {
  if (seconds === null || seconds === undefined || !Number.isFinite(seconds) || seconds < 0) {
    return PLACEHOLDER_TIME;
  }

  const total = Math.floor(seconds);
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const secs = total % 60;

  if (hours > 0) {
    return `${hours}:${String(minutes).padStart(2, "0")}:${String(secs).padStart(2, "0")}`;
  }
  return `${minutes}:${String(secs).padStart(2, "0")}`;
}

const SIZE_UNITS = ["B", "KB", "MB", "GB", "TB"] as const;

/** Byte → chuỗi có đơn vị. Đơn vị byte không có phần thập phân. */
export function formatFileSize(bytes: number | null | undefined): string {
  if (bytes === null || bytes === undefined || !Number.isFinite(bytes) || bytes < 0) {
    return PLACEHOLDER_VALUE;
  }

  let value = bytes;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < SIZE_UNITS.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }

  const formatted = unitIndex === 0 ? String(Math.round(value)) : value.toFixed(1);
  return `${formatted} ${SIZE_UNITS[unitIndex]}`;
}

/** Byte mỗi giây → chuỗi tốc độ. */
export function formatSpeed(bytesPerSecond: number | null | undefined): string {
  const size = formatFileSize(bytesPerSecond);
  return size === PLACEHOLDER_VALUE ? PLACEHOLDER_VALUE : `${size}/s`;
}

/** Thời gian còn lại, dùng chung định dạng với thời lượng. */
export function formatEta(seconds: number | null | undefined): string {
  return formatDuration(seconds);
}
