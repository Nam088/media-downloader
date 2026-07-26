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

/**
 * `platform` lưu ở backend là nhãn snake_case riêng của app cho 6 nền tảng
 * chính (`resolve_platform_label` trong `commands/media.rs`), hoặc
 * `extractor_key` thô của yt-dlp/gallery-dl đã viết thường cho ~1.600 site
 * còn lại — nên có những giá trị xấu như `"imgurgallery"`. Hàm này chỉ đổi
 * cách hiển thị; giá trị dùng để lọc/nhóm trong store vẫn giữ nguyên bản gốc.
 */
const PLATFORM_DISPLAY_NAMES: Record<string, string> = {
  youtube: "YouTube",
  tiktok: "TikTok",
  facebook: "Facebook",
  instagram: "Instagram",
  twitter_x: "X (Twitter)",
  soundcloud: "SoundCloud",
  imgurgallery: "Imgur",
  spotify: "Spotify",
  tidal: "TIDAL",
  apple_music: "Apple Music",
  pandora: "Pandora",
  // Id provider của SpotiFLAC (danh sách thứ tự ưu tiên trong Cài đặt);
  // fallback viết hoa chữ đầu sẽ chỉ hiển thị "Amazon" trơn.
  amazon: "Amazon Music",
};

export function formatPlatformLabel(platform: string): string {
  const known = PLATFORM_DISPLAY_NAMES[platform];
  if (known) return known;
  return platform.length > 0 ? platform[0].toUpperCase() + platform.slice(1) : platform;
}

/** The platforms worth advertising as "quick tags" before a preview
 * (`PLATFORM_HOSTS` in `src-tauri/src/platform.rs`): the 6 FR-014 requires
 * plus the 4 music sources the SpotiFLAC engine handles. Everything else
 * `formatPlatformLabel` prettifies is a fallback the app happens to support,
 * not something to showcase as a headline feature. */
export const CURATED_PLATFORMS = [
  "youtube",
  "tiktok",
  "facebook",
  "instagram",
  "twitter_x",
  "soundcloud",
  "spotify",
  "tidal",
  "apple_music",
  "pandora",
] as const;
