//! Đọc file danh sách URL do người dùng chọn hoặc thả vào cửa sổ (FR-105,
//! FR-106).
//!
//! Việc đọc tệp nằm ở Rust có chủ đích: tầng giao diện chỉ cần đúng khả năng
//! "đọc một file văn bản người dùng vừa chỉ định", nên không có lý do gì mở
//! quyền hệ thống tệp cho nó.

use std::collections::HashSet;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use url::Url;

use crate::error::AppError;

/// Chặn trên cho kích thước file danh sách. Một file 5 MB toàn URL đã là hàng
/// trăm nghìn dòng — vượt mức đó gần như chắc chắn là chọn nhầm file.
const MAX_FILE_BYTES: u64 = 5 * 1024 * 1024;

/// Nhóm ký tự "khoảng trắng" được viết tay thay vì dùng `\s`.
///
/// Phía giao diện dùng `/https?:\/\/[^\s\r\n]+/` với `\s` của JavaScript, còn
/// `\s` của `regex` là thuộc tính Unicode `White_Space`. Hai tập này lệch nhau
/// đúng hai ký tự: U+FEFF (JS coi là khoảng trắng, Unicode thì không) và U+0085
/// (ngược lại). Liệt kê thẳng tập của JavaScript ở đây để cùng một nội dung cắt
/// URL ra giống hệt nhau ở cả hai phía.
const JS_WHITESPACE: &str = r"\t\n\x{0B}\x{0C}\r \x{A0}\x{1680}\x{2000}-\x{200A}\x{2028}\x{2029}\x{202F}\x{205F}\x{3000}\x{FEFF}";

static URL_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"https?://[^{JS_WHITESPACE}]+")).expect("hằng regex hợp lệ")
});

/// Ký tự thường dính vào cuối URL khi copy từ văn bản chạy. Khớp với
/// `TRAILING_NOISE` (`/[,.;:)\]}"']+$/`) phía giao diện.
const TRAILING_NOISE: [char; 9] = [',', '.', ';', ':', ')', ']', '}', '"', '\''];

/// Trích mọi URL http(s) trong nội dung văn bản, bỏ trùng, giữ thứ tự xuất
/// hiện đầu tiên.
///
/// Cố ý dùng chung quy tắc với `src/lib/url-parsing.ts` phía giao diện: người
/// dùng dán vào ô nhập hay thả file vào thì phải ra cùng một kết quả. Regex chỉ
/// khoanh vùng ứng viên; trọng tài cuối cùng là việc dựng được URL http(s), y
/// như `new URL()` bên kia.
pub fn parse_url_list(content: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut urls = Vec::new();

    for candidate in URL_PATTERN.find_iter(content) {
        let candidate = candidate.as_str().trim_end_matches(TRAILING_NOISE);
        if !is_http_url(candidate) {
            continue;
        }
        // Giữ nguyên chuỗi người dùng viết chứ không lấy dạng chuẩn hoá của
        // `Url`, vì phía giao diện cũng trả lại đúng chuỗi gốc.
        if seen.insert(candidate) {
            urls.push(candidate.to_string());
        }
    }

    urls
}

fn is_http_url(candidate: &str) -> bool {
    Url::parse(candidate).is_ok_and(|parsed| matches!(parsed.scheme(), "http" | "https"))
}

/// Đọc một file danh sách URL và trả về các URL trong đó.
#[tauri::command]
pub fn read_url_list_file(path: String) -> Result<Vec<String>, AppError> {
    let path = Path::new(&path);

    // Hỏi kích thước qua metadata *trước khi* đọc: mục đích của chặn trên là
    // không kéo một file khổng lồ vào bộ nhớ, nên kiểm tra sau khi đọc thì vô
    // nghĩa.
    let metadata = std::fs::metadata(path)
        .map_err(|err| AppError::new("FILE_UNREADABLE", format!("Cannot read file: {err}")))?;
    if metadata.len() > MAX_FILE_BYTES {
        return Err(AppError::new(
            "FILE_TOO_LARGE",
            "URL list file is larger than 5 MB",
        ));
    }

    // Đọc dạng chuỗi UTF-8: file nhị phân sẽ hỏng ngay ở bước này và cho ra
    // thông báo rõ ràng, thay vì âm thầm trả về danh sách rỗng.
    let content = std::fs::read_to_string(path).map_err(|err| match err.kind() {
        std::io::ErrorKind::InvalidData => {
            AppError::new("FILE_NOT_TEXT", "File is not readable text")
        }
        _ => AppError::new("FILE_UNREADABLE", format!("Cannot read file: {err}")),
    })?;

    Ok(parse_url_list(&content))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_http_urls_one_per_line() {
        let content = "https://a.example/1\nhttp://b.example/2\n";
        assert_eq!(
            parse_url_list(content),
            vec![
                "https://a.example/1".to_string(),
                "http://b.example/2".to_string()
            ]
        );
    }

    #[test]
    fn ignores_blank_lines_comments_and_prose() {
        let content = "\n# ghi chú\nkhông phải url\n  https://a.example/1  \n\n";
        assert_eq!(
            parse_url_list(content),
            vec!["https://a.example/1".to_string()]
        );
    }

    #[test]
    fn rejects_non_http_schemes() {
        let content =
            "ftp://c.example/3\nfile:///tmp/x\nmagnet:?xt=urn:btih:abc\nhttps://a.example/1\n";
        assert_eq!(
            parse_url_list(content),
            vec!["https://a.example/1".to_string()]
        );
    }

    #[test]
    fn drops_duplicates_but_keeps_first_seen_order() {
        // Thứ tự mong đợi (z, a, m) khác hẳn thứ tự sắp xếp (a, m, z), nên một
        // cách hiện thực dùng sort/dedup sẽ trượt test này.
        let content = "https://z.example/9\nhttps://a.example/1\nhttps://z.example/9\nhttps://m.example/5\nhttps://a.example/1\n";
        assert_eq!(
            parse_url_list(content),
            vec![
                "https://z.example/9".to_string(),
                "https://a.example/1".to_string(),
                "https://m.example/5".to_string()
            ]
        );
    }

    #[test]
    fn finds_urls_embedded_in_surrounding_text() {
        let content = "xem cái này https://a.example/1 hay lắm";
        assert_eq!(
            parse_url_list(content),
            vec!["https://a.example/1".to_string()]
        );
    }

    #[test]
    fn strips_trailing_punctuation_glued_on_by_prose() {
        let content = concat!(
            "Xem https://a.example/1, rồi https://b.example/2.\n",
            "Nguồn (https://c.example/3) và \"https://d.example/4\"\n",
            "Danh sách [https://e.example/5]; kết {https://f.example/6}\n",
            "Chú thích: 'https://g.example/7':\n"
        );
        assert_eq!(
            parse_url_list(content),
            vec![
                "https://a.example/1".to_string(),
                "https://b.example/2".to_string(),
                "https://c.example/3".to_string(),
                "https://d.example/4".to_string(),
                "https://e.example/5".to_string(),
                "https://f.example/6".to_string(),
                "https://g.example/7".to_string(),
            ]
        );
    }

    #[test]
    fn keeps_query_and_fragment_untouched() {
        // Dấu `=`, `&`, `#` không nằm trong nhóm ký tự nhiễu, và phần trước
        // dấu chấm cuối câu phải còn nguyên.
        let content = "https://a.example/watch?v=abc&list=xyz#t=30.";
        assert_eq!(
            parse_url_list(content),
            vec!["https://a.example/watch?v=abc&list=xyz#t=30".to_string()]
        );
    }

    #[test]
    fn rejects_candidates_that_match_the_pattern_but_are_not_valid_urls() {
        // Regex nhận, nhưng dựng URL thì hỏng — giống hệt phía giao diện, nơi
        // `new URL()` là trọng tài cuối cùng chứ không phải regex.
        let content = "https://[not-an-ip/x\nhttps://a.example/1\n";
        assert_eq!(
            parse_url_list(content),
            vec!["https://a.example/1".to_string()]
        );
    }

    #[test]
    fn returns_empty_for_text_without_urls() {
        assert!(parse_url_list("chỉ là ghi chú thôi\n# không có gì\n").is_empty());
    }

    /// Ghi ra một file tạm và trả về đường dẫn.
    fn write_temp(bytes: &[u8]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "media-downloader-urllist-{}.txt",
            uuid::Uuid::new_v4()
        ));
        std::fs::write(&path, bytes).expect("ghi được file tạm");
        path
    }

    #[test]
    fn reads_and_parses_a_real_file() {
        let path =
            write_temp(b"https://a.example/1\n# note\nhttps://a.example/1\nhttps://b.example/2\n");
        let urls = read_url_list_file(path.to_string_lossy().to_string()).expect("đọc được");
        let _ = std::fs::remove_file(&path);
        assert_eq!(
            urls,
            vec![
                "https://a.example/1".to_string(),
                "https://b.example/2".to_string()
            ]
        );
    }

    #[test]
    fn missing_path_is_file_unreadable() {
        let path = std::env::temp_dir().join(format!("khong-ton-tai-{}.txt", uuid::Uuid::new_v4()));
        let err = read_url_list_file(path.to_string_lossy().to_string())
            .expect_err("đường dẫn không tồn tại phải lỗi");
        assert_eq!(err.code, "FILE_UNREADABLE");
    }

    #[test]
    fn oversized_file_is_rejected_before_reading() {
        let path = write_temp(&vec![b'a'; MAX_FILE_BYTES as usize + 1]);
        let err = read_url_list_file(path.to_string_lossy().to_string())
            .expect_err("file quá lớn phải lỗi");
        let _ = std::fs::remove_file(&path);
        assert_eq!(err.code, "FILE_TOO_LARGE");
    }

    #[test]
    fn file_at_the_size_cap_is_still_accepted() {
        // Chặn là "lớn hơn 5 MB", không phải "đúng 5 MB": kiểm tra ranh giới để
        // một dấu `>=` nhầm chỗ không lọt.
        let mut bytes = vec![b'\n'; MAX_FILE_BYTES as usize];
        bytes[..19].copy_from_slice(b"https://a.example/1");
        let path = write_temp(&bytes);
        let urls = read_url_list_file(path.to_string_lossy().to_string())
            .expect("file đúng bằng chặn trên vẫn phải đọc được");
        let _ = std::fs::remove_file(&path);
        assert_eq!(urls, vec!["https://a.example/1".to_string()]);
    }

    #[test]
    fn binary_file_is_not_text() {
        // 0xFF/0xFE không phải UTF-8 hợp lệ. Nếu đọc bằng byte rồi lossy-convert
        // thì test này sẽ đỗ sai với danh sách rỗng, nên nó chốt luôn là lỗi.
        let path = write_temp(&[0xFF, 0xFE, 0x00, 0x01, 0x02, 0xFF]);
        let err = read_url_list_file(path.to_string_lossy().to_string())
            .expect_err("file nhị phân phải lỗi");
        let _ = std::fs::remove_file(&path);
        assert_eq!(err.code, "FILE_NOT_TEXT");
    }
}
