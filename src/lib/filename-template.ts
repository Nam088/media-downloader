/**
 * Ô xem trước tên file (FR-213): người dùng gõ mẫu, thấy ngay tên file sẽ nhận.
 *
 * Đây là bản sao có chủ ý của `src-tauri/src/downloader/filename.rs`. Ô xem
 * trước phải chạy **đúng** luật mà bộ tải chạy — cả phần đổ trường lẫn phần làm
 * sạch — vì một ô xem trước nói dối còn tệ hơn không có ô nào: người dùng gõ
 * `AC/DC`, thấy `AC/DC`, rồi nhận về `AC_DC` và không hiểu chuyện gì xảy ra.
 * Hai bản phải khớp tới từng ký tự; danh sách trường, giá trị dự phòng và luật
 * làm sạch dưới đây được giữ song song với hằng cùng tên bên Rust.
 *
 * Cú pháp là `{field}` chứ không phải `%(field)s` của yt-dlp. Lý do đầy đủ nằm
 * ở đầu file Rust; tóm tắt: mẫu của người dùng không bao giờ được truyền thẳng
 * cho yt-dlp, vì `%(...)s` cho phép chèn dấu tách thư mục và ghi file ra ngoài
 * thư mục đích. Ở đây ta thay thế trong mã của mình, với danh sách trường cố
 * định, rồi làm sạch kết quả.
 */

/** Xem `MAX_FILENAME_BYTES` bên Rust. Tính theo byte UTF-8. */
export const MAX_FILENAME_BYTES = 255;

/** Tên dùng khi mẫu sinh ra thứ không lưu được. */
export const UNTITLED = "untitled";

const REPLACEMENT = "_";

/** Ký tự Windows cấm. macOS và Linux cấm ít hơn, nên tập này phủ cả ba hệ. */
const FORBIDDEN = new Set(["\\", "/", ":", "*", "?", '"', "<", ">", "|"]);

/**
 * Ký tự điều khiển (C0 và C1). Không hệ nào lưu được, mà xuống dòng lọt vào
 * tiêu đề là chuyện thường gặp. So bằng mã điểm thay vì regex để khỏi phải nhét
 * ký tự điều khiển nguyên văn vào mã nguồn.
 */
function isControlCharacter(character: string): boolean {
  const code = character.codePointAt(0) ?? 0;
  return code < 0x20 || (code >= 0x7f && code <= 0x9f);
}

/**
 * Tên thiết bị Windows dành riêng — không mở được dưới dạng file **kể cả khi có
 * phần mở rộng**, vì Windows chỉ xét phần đứng trước dấu chấm đầu tiên.
 */
const RESERVED_DEVICE_NAMES = new Set([
  "CON",
  "PRN",
  "AUX",
  "NUL",
  "COM1",
  "COM2",
  "COM3",
  "COM4",
  "COM5",
  "COM6",
  "COM7",
  "COM8",
  "COM9",
  "LPT1",
  "LPT2",
  "LPT3",
  "LPT4",
  "LPT5",
  "LPT6",
  "LPT7",
  "LPT8",
  "LPT9",
]);

const MAX_EXTENSION_BYTES = 16;

/** Các trường dùng được trong mẫu (FR-212). Đúng thứ tự bên Rust. */
export const TEMPLATE_FIELDS = [
  "title",
  "channel",
  "playlist_index",
  "upload_date",
  "resolution",
  "ext",
] as const;

export type TemplateField = (typeof TEMPLATE_FIELDS)[number];

/** Mẫu mặc định: đúng bằng hành vi hiện tại của bộ tải. */
export const DEFAULT_TEMPLATE = "{title}";

/**
 * Giá trị thay cho trường mà nguồn không cung cấp (FR-216). Phải khớp từng ký
 * tự với các hằng `FALLBACK_*` bên Rust.
 */
export const TEMPLATE_FALLBACKS: Record<TemplateField, string> = {
  title: "untitled",
  channel: "unknown-channel",
  playlist_index: "00",
  upload_date: "unknown-date",
  resolution: "unknown-resolution",
  ext: "bin",
};

/**
 * Dữ liệu nguồn để đổ vào mẫu — bản đối chiếu của `TemplateFields` bên Rust.
 *
 * Cố tình không nhập `MediaSource`: hàm này chỉ cần sáu trường, và phần nối dây
 * là chỗ biết lấy chúng ở đâu trong dữ liệu xem trước.
 */
export interface TemplateSource {
  title?: string | null;
  channel?: string | null;
  playlistIndex?: number | null;
  /** `YYYYMMDD` (dạng yt-dlp trả) hoặc `YYYY-MM-DD`. Cả hai đều hiện ra dạng có dấu gạch. */
  uploadDate?: string | null;
  resolution?: string | null;
  ext?: string | null;
}

export interface TemplatePreview {
  /** Tên file đã đổ trường và làm sạch — đúng cái sẽ nằm trên đĩa. */
  filename: string;
  /**
   * Các `{tên}` trong mẫu không phải trường hợp lệ, theo thứ tự xuất hiện và
   * không lặp. Giao diện dùng để cảnh báo lỗi gõ; chúng vẫn nằm nguyên văn
   * trong `filename` để người dùng thấy hậu quả chứ không bị nuốt mất.
   */
  unknownFields: string[];
}

const encoder = new TextEncoder();

function byteLength(value: string): number {
  return encoder.encode(value).length;
}

/**
 * Cắt `value` sao cho không quá `budget` byte UTF-8, không bao giờ cắt đôi một
 * ký tự. `for...of` duyệt theo code point nên cặp thay thế (emoji) luôn còn
 * nguyên — bản Rust dùng ranh giới `char` và cho cùng kết quả.
 */
function cutToBytes(value: string, budget: number): string {
  if (byteLength(value) <= budget) {
    return value;
  }
  let used = 0;
  let out = "";
  for (const character of value) {
    const size = byteLength(character);
    if (used + size > budget) {
      break;
    }
    used += size;
    out += character;
  }
  return out;
}

/**
 * Tách `["Bài hát", ".mp3"]`. Trả `[name, ""]` khi không có gì đáng coi là phần
 * mở rộng — điều kiện chặt để `(2026. Deluxe Edition)` không bị nhận nhầm.
 */
function splitExtension(name: string): [string, string] {
  const dot = name.lastIndexOf(".");
  if (dot <= 0) {
    return [name, ""];
  }
  const ext = name.slice(dot);
  const isExtension =
    ext.length > 1 && byteLength(ext) <= MAX_EXTENSION_BYTES && /^\.[0-9a-z]+$/i.test(ext);
  return isExtension ? [name.slice(0, dot), ext] : [name, ""];
}

/** Rút gọn về `maxBytes`, ưu tiên giữ phần mở rộng. */
function truncateToBytes(name: string, maxBytes: number): string {
  if (byteLength(name) <= maxBytes) {
    return name;
  }
  const [stem, ext] = splitExtension(name);
  const extBytes = byteLength(ext);
  if (extBytes < maxBytes) {
    const cut = cutToBytes(stem, maxBytes - extBytes);
    if (cut.length > 0) {
      return cut + ext;
    }
  }
  return cutToBytes(name, maxBytes);
}

function trimTrailingDotsAndSpaces(value: string): string {
  return value.replace(/[.\s]+$/u, "");
}

/**
 * Đúng khi chuỗi không còn ký tự nào mang nghĩa. `___` hợp lệ về kỹ thuật nhưng
 * vô nghĩa với người dùng, nên vẫn rơi về `UNTITLED`.
 */
function isMeaningless(value: string): boolean {
  return /^[_.\s]*$/u.test(value);
}

function guardReservedDeviceName(name: string): string {
  const dot = name.indexOf(".");
  const baseLength = dot === -1 ? name.length : dot;
  const base = name.slice(0, baseLength);
  if (RESERVED_DEVICE_NAMES.has(base.trim().toUpperCase())) {
    return base + REPLACEMENT + name.slice(baseLength);
  }
  return name;
}

/**
 * Biến một chuỗi bất kỳ thành tên file lưu được trên Windows, macOS và Linux
 * (FR-214). Bản sao của `sanitize_filename` bên Rust.
 */
export function sanitizeFilename(name: string, maxBytes: number = MAX_FILENAME_BYTES): string {
  let replaced = "";
  for (const character of name) {
    replaced +=
      FORBIDDEN.has(character) || isControlCharacter(character) ? REPLACEMENT : character;
  }

  const trimmed = trimTrailingDotsAndSpaces(replaced.trim());
  if (isMeaningless(trimmed)) {
    return truncateToBytes(UNTITLED, maxBytes);
  }

  const truncated = truncateToBytes(guardReservedDeviceName(trimmed), maxBytes);
  const settled = trimTrailingDotsAndSpaces(truncated);
  return isMeaningless(settled) ? truncateToBytes(UNTITLED, maxBytes) : settled;
}

function nonEmpty(value: string | null | undefined): string | null {
  if (value === null || value === undefined) {
    return null;
  }
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : null;
}

/** `20260726` → `2026-07-26`. Dạng khác giữ nguyên. */
function normalizeDate(value: string): string {
  if (/^\d{8}$/.test(value)) {
    return `${value.slice(0, 4)}-${value.slice(4, 6)}-${value.slice(6)}`;
  }
  return value;
}

/** `null` khi `name` không phải trường được phép. */
function fieldValue(name: string, source: TemplateSource): string | null {
  switch (name) {
    case "title":
      return nonEmpty(source.title) ?? TEMPLATE_FALLBACKS.title;
    case "channel":
      return nonEmpty(source.channel) ?? TEMPLATE_FALLBACKS.channel;
    case "playlist_index":
      // Đệm 2 chữ số để thứ tự chữ cái trùng thứ tự số trong trình quản lý file.
      return source.playlistIndex === null || source.playlistIndex === undefined
        ? TEMPLATE_FALLBACKS.playlist_index
        : String(source.playlistIndex).padStart(2, "0");
    case "upload_date": {
      const raw = nonEmpty(source.uploadDate);
      return raw === null ? TEMPLATE_FALLBACKS.upload_date : normalizeDate(raw);
    }
    case "resolution":
      return nonEmpty(source.resolution) ?? TEMPLATE_FALLBACKS.resolution;
    case "ext":
      return nonEmpty(source.ext) ?? TEMPLATE_FALLBACKS.ext;
    default:
      return null;
  }
}

/**
 * Đổ `source` vào `template` rồi làm sạch — ra đúng tên file bộ tải sẽ ghi.
 *
 * Trường lạ được giữ nguyên văn (`{titel}` vẫn là `{titel}`) và liệt kê trong
 * `unknownFields`, để người dùng thấy ngay lỗi gõ thay vì mất im lặng một phần
 * tên.
 */
export function renderTemplatePreview(template: string, source: TemplateSource): TemplatePreview {
  const unknownFields: string[] = [];
  let out = "";
  let rest = template;

  for (;;) {
    const open = rest.indexOf("{");
    if (open === -1) {
      break;
    }
    out += rest.slice(0, open);
    const after = rest.slice(open + 1);
    const close = after.indexOf("}");
    if (close === -1) {
      // `{` không có `}` đóng: phần còn lại là văn bản thường.
      out += rest.slice(open);
      rest = "";
      break;
    }
    const name = after.slice(0, close);
    const value = fieldValue(name, source);
    if (value === null) {
      out += `{${name}}`;
      if (!unknownFields.includes(name)) {
        unknownFields.push(name);
      }
    } else {
      out += value;
    }
    rest = after.slice(close + 1);
  }
  out += rest;

  return { filename: sanitizeFilename(out), unknownFields };
}
