/**
 * Phân tích URL từ văn bản người dùng dán, thả, hoặc từ file danh sách.
 *
 * Quy tắc ở đây phải khớp với phía Rust (lệnh đọc file danh sách URL): cùng một
 * nội dung, dán vào ô nhập hay thả file vào, đều phải cho ra cùng danh sách.
 */

const URL_PATTERN = /https?:\/\/[^\s\r\n]+/g;

/** Ký tự thường dính vào cuối URL khi copy từ văn bản chạy. */
const TRAILING_NOISE = /[,.;:)\]}"']+$/;

export function isValidUrl(candidate: string): boolean {
  try {
    const parsed = new URL(candidate);
    return parsed.protocol === "http:" || parsed.protocol === "https:";
  } catch {
    return false;
  }
}

/**
 * Các host đi qua engine SpotiFLAC thay vì yt-dlp. Phải khớp với
 * `platform.rs::detect_platform` phía Rust — hai bên cùng một danh sách.
 */
const MUSIC_HOSTS = new Set([
  "open.spotify.com",
  "spotify.com",
  "listen.tidal.com",
  "tidal.com",
  "music.apple.com",
  "pandora.com",
  "pandora.app.link",
]);

/**
 * Link nhạc lossless (Spotify/Tidal/Apple Music/Pandora)? So sánh hostname đã
 * parse thật sự, không substring — `https://evil.com/open.spotify.com` phải
 * trả false. Chỉ bỏ tiền tố `www.`; subdomain lạ khác không được tin.
 */
export function isMusicUrl(url: string): boolean {
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    return false;
  }
  if (parsed.protocol !== "http:" && parsed.protocol !== "https:") return false;
  const host = parsed.hostname.toLowerCase().replace(/^www\./, "");
  return MUSIC_HOSTS.has(host);
}

/** Trích mọi URL http(s), bỏ trùng, giữ thứ tự xuất hiện đầu tiên. */
export function extractUrlsFromText(text: string): string[] {
  const matches = text.match(URL_PATTERN) ?? [];
  const cleaned = matches
    .map((match) => match.replace(TRAILING_NOISE, ""))
    .filter((candidate) => isValidUrl(candidate));
  return dedupeUrls(cleaned).unique;
}

export interface DedupeResult {
  unique: string[];
  duplicateCount: number;
}

/**
 * Bỏ trùng và cho biết đã bỏ bao nhiêu, để giao diện nói được với người dùng
 * rằng danh sách của họ đã bị rút ngắn (FR-107).
 */
export function dedupeUrls(urls: string[]): DedupeResult {
  const seen = new Set<string>();
  const unique: string[] = [];
  for (const url of urls) {
    if (!seen.has(url)) {
      seen.add(url);
      unique.push(url);
    }
  }
  return { unique, duplicateCount: urls.length - unique.length };
}
