//! Đặt tên file theo mẫu: làm sạch, rút gọn, và không ghi đè.
//!
//! Module này là *toàn bộ* phần quyết định tên file, và cố tình không chạm vào
//! đĩa: mọi hàm ở đây là hàm thuần, nhận vào chuỗi và trả ra chuỗi. Phép kiểm
//! tra "file đã tồn tại chưa" được tiêm vào qua tham số (`deduplicate_path`)
//! nên logic chống ghi đè kiểm thử được mà không cần thư mục tạm.
//!
//! # Cú pháp mẫu: `{field}`, không phải `%(field)s`
//!
//! yt-dlp có sẵn cú pháp mẫu của nó (`%(title)s`). Chúng ta **không** phơi nó
//! ra cho người dùng và **không bao giờ** truyền mẫu của người dùng thẳng cho
//! yt-dlp. Lý do là an toàn chứ không phải thẩm mỹ:
//!
//! - `%(...)s` của yt-dlp cho phép truy cập trường tuỳ ý, chỉ định định dạng
//!   kiểu Python, và — quan trọng nhất — **dấu tách thư mục**. Một mẫu như
//!   `%(title)s/../../../id_rsa` sẽ khiến yt-dlp ghi ra ngoài thư mục đích mà
//!   người dùng đã chọn. Đó là ghi file tuỳ ý, không phải một tính năng đặt
//!   tên.
//! - Ngoài ra yt-dlp còn hiểu tiền tố loại đầu ra (`thumbnail:`, `subtitle:`)
//!   ngay trong đối số `-o`, tức người dùng có thể đổi hướng ghi cho cả những
//!   file mà giao diện không hề nhắc tới.
//!
//! Nên: mẫu dùng cú pháp `{field}` với **danh sách trường cho phép cố định**
//! ([`TEMPLATE_FIELDS`]). [`render_filename`] tự thay thế trong Rust, rồi làm
//! sạch kết quả bằng [`sanitize_filename`] — bước này biến `/` và `\` thành
//! `_`, nên không mẫu nào tạo được thư mục con hay đi ngược lên cây thư mục.
//! Tên đã làm sạch được đưa cho yt-dlp dưới dạng **hằng**, sau khi qua
//! [`escape_for_ytdlp_template`] để một dấu `%` do người dùng gõ không bị
//! yt-dlp diễn giải lại thành mẫu. Trường không nằm trong danh sách cho phép
//! được giữ nguyên văn (`{foo}` vẫn là `{foo}`) chứ không bị thay bằng rỗng:
//! người dùng thấy ngay lỗi gõ của mình trong ô xem trước (FR-213).
//!
//! Bản TypeScript `src/lib/filename-template.ts` là bản sao của cùng luật này
//! để ô xem trước hiển thị đúng cái tên sẽ nhận được. Hai bản phải khớp nhau
//! tới từng ký tự — một ô xem trước nói dối còn tệ hơn không có ô nào.

// Module mới hạ cánh trước phần nối dây (`queue.rs` đang do việc khác giữ).
// Khi `queue.rs` gọi tới `render_filename`/`deduplicate_path`, xoá dòng này.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

/// Giới hạn độ dài một thành phần tên file, tính theo **byte UTF-8**.
///
/// 255 là mức chung của ext4, APFS và NTFS. NTFS đếm theo đơn vị UTF-16 chứ
/// không phải byte, nhưng mọi ký tự đều tốn số byte UTF-8 lớn hơn hoặc bằng số
/// đơn vị UTF-16 của nó (ASCII 1↔1, chữ có dấu 2–3 byte ↔ 1 đơn vị, emoji 4
/// byte ↔ 2 đơn vị), nên đo bằng byte là cận trên an toàn cho cả ba hệ.
pub const MAX_FILENAME_BYTES: usize = 255;

/// Tên dùng khi mẫu sinh ra thứ không lưu được (rỗng, hoặc chỉ toàn ký tự bị
/// cấm). Không bao giờ trả về chuỗi rỗng — một file không tên là một lỗi ghi
/// đĩa, không phải một cái tên xấu.
pub const UNTITLED: &str = "untitled";

/// Ký tự thay cho ký tự bị cấm. Dùng `_` chứ không xoá hẳn để `a:b` thành
/// `a_b` thay vì `ab` — ranh giới giữa hai từ vẫn còn.
const REPLACEMENT: char = '_';

/// Ký tự Windows cấm trong tên file. macOS chỉ cấm `/` (và `:` ở tầng Finder),
/// Linux chỉ cấm `/`; Windows là tập lớn nhất nên dùng nó cho cả ba hệ.
const FORBIDDEN: [char; 9] = ['\\', '/', ':', '*', '?', '"', '<', '>', '|'];

/// Tên thiết bị Windows dành riêng. Không mở được dưới dạng file **kể cả khi
/// có phần mở rộng** — `CON.mp3` vẫn là thiết bị console, vì Windows chỉ xét
/// phần đứng trước dấu chấm đầu tiên.
const RESERVED_DEVICE_NAMES: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Độ dài tối đa (kể cả dấu chấm) của thứ được coi là phần mở rộng. Chặn để
/// `Bản Remaster (2026. Deluxe)` không bị hiểu là có đuôi ` Deluxe)`.
const MAX_EXTENSION_BYTES: usize = 16;

/// Số hậu tố tối đa thử khi tên bị trùng. Chạm tới mức này nghĩa là thư mục đã
/// có 9999 file cùng tên — thực tế không xảy ra, nhưng vòng lặp vẫn phải có
/// điểm dừng vì hàm `exists` là do bên ngoài truyền vào.
const MAX_DEDUP_ATTEMPTS: u32 = 9999;

/// Các trường dùng được trong mẫu. Danh sách cho phép: mọi thứ ngoài đây không
/// được thay thế (FR-212).
pub const TEMPLATE_FIELDS: [&str; 6] = [
    "title",
    "channel",
    "playlist_index",
    "upload_date",
    "resolution",
    "ext",
];

/// Mẫu mặc định: đúng bằng hành vi cũ (`%(title)s`), để người dùng không đụng
/// tới mục nâng cao này vẫn nhận được y hệt cái họ vẫn nhận.
pub const DEFAULT_TEMPLATE: &str = "{title}";

// Giá trị dự phòng cho trường mà nguồn không cung cấp (FR-216). Phải khớp
// từng ký tự với `TEMPLATE_FALLBACKS` bên `src/lib/filename-template.ts`.
pub const FALLBACK_TITLE: &str = "untitled";
pub const FALLBACK_CHANNEL: &str = "unknown-channel";
pub const FALLBACK_PLAYLIST_INDEX: &str = "00";
pub const FALLBACK_UPLOAD_DATE: &str = "unknown-date";
pub const FALLBACK_RESOLUTION: &str = "unknown-resolution";
pub const FALLBACK_EXT: &str = "bin";

/// Dữ liệu nguồn để đổ vào mẫu. Mọi trường đều tuỳ chọn: nguồn nào cũng có thể
/// thiếu bất kỳ trường nào, và thiếu thì rơi vào giá trị dự phòng chứ không
/// làm hỏng tên file.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct TemplateFields {
    pub title: Option<String>,
    pub channel: Option<String>,
    pub playlist_index: Option<u32>,
    /// `YYYYMMDD` (dạng yt-dlp trả về) hoặc `YYYY-MM-DD`. Cả hai đều được
    /// chuẩn hoá về `YYYY-MM-DD` khi hiện trong tên file.
    pub upload_date: Option<String>,
    pub resolution: Option<String>,
    pub ext: Option<String>,
}

/// Đổ `fields` vào `template` rồi làm sạch kết quả.
///
/// Kết quả luôn là một tên file hợp lệ trên cả ba hệ điều hành: hàm này gọi
/// [`sanitize_filename`] ở bước cuối, nên không có đường nào để một giá trị từ
/// nguồn (tiêu đề chứa `/`, tên kênh chứa `:`) lọt ra thành đường dẫn.
pub fn render_filename(template: &str, fields: &TemplateFields) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;

    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) => {
                let name = &after[..close];
                match field_value(name, fields) {
                    Some(value) => out.push_str(&value),
                    // Trường lạ: giữ nguyên văn để người dùng thấy lỗi gõ
                    // trong ô xem trước, thay vì im lặng nuốt mất một phần tên.
                    None => {
                        out.push('{');
                        out.push_str(name);
                        out.push('}');
                    }
                }
                rest = &after[close + 1..];
            }
            // `{` không có `}` đóng: phần còn lại là văn bản thường.
            None => {
                out.push_str(&rest[open..]);
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);

    sanitize_filename(&out)
}

/// `None` khi `name` không phải trường được phép; `Some` khi được phép, đã
/// thay bằng giá trị dự phòng nếu nguồn không có.
fn field_value(name: &str, fields: &TemplateFields) -> Option<String> {
    let value = match name {
        "title" => non_empty(fields.title.as_deref()).unwrap_or(FALLBACK_TITLE).to_string(),
        "channel" => non_empty(fields.channel.as_deref())
            .unwrap_or(FALLBACK_CHANNEL)
            .to_string(),
        // Đệm 2 chữ số để thứ tự chữ cái trùng với thứ tự số trong trình quản
        // lý file: `02` đứng trước `10`, còn `2` thì không.
        "playlist_index" => fields
            .playlist_index
            .map(|index| format!("{index:02}"))
            .unwrap_or_else(|| FALLBACK_PLAYLIST_INDEX.to_string()),
        "upload_date" => non_empty(fields.upload_date.as_deref())
            .map(normalize_date)
            .unwrap_or_else(|| FALLBACK_UPLOAD_DATE.to_string()),
        "resolution" => non_empty(fields.resolution.as_deref())
            .unwrap_or(FALLBACK_RESOLUTION)
            .to_string(),
        "ext" => non_empty(fields.ext.as_deref()).unwrap_or(FALLBACK_EXT).to_string(),
        _ => return None,
    };
    Some(value)
}

/// Chuỗi rỗng hoặc chỉ có khoảng trắng cũng là "nguồn không cung cấp" — đối xử
/// y như `None`, nếu không mẫu sẽ sinh ra khoảng trống giữa tên file.
fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

/// `20260726` → `2026-07-26`. Dạng nào khác thì giữ nguyên: đoán tiếp sẽ sai
/// nhiều hơn đúng.
fn normalize_date(value: &str) -> String {
    if value.len() == 8 && value.bytes().all(|byte| byte.is_ascii_digit()) {
        format!("{}-{}-{}", &value[..4], &value[4..6], &value[6..])
    } else {
        value.to_string()
    }
}

/// Biến một chuỗi bất kỳ thành tên file lưu được trên Windows, macOS và Linux,
/// dài không quá [`MAX_FILENAME_BYTES`] (FR-214).
pub fn sanitize_filename(name: &str) -> String {
    sanitize_filename_with_limit(name, MAX_FILENAME_BYTES)
}

/// Như [`sanitize_filename`] nhưng tự đặt trần độ dài.
///
/// Có mặt vì Windows còn giới hạn **cả đường dẫn** (260 ký tự với API cũ):
/// người gọi biết thư mục đích dài bao nhiêu nên chỉ họ mới tính được phần còn
/// lại cho tên file.
pub fn sanitize_filename_with_limit(name: &str, max_bytes: usize) -> String {
    // 1. Ký tự Windows cấm, cộng ký tự điều khiển (xuống dòng trong tiêu đề là
    //    chuyện thường gặp, và không hệ nào lưu được).
    let replaced: String = name
        .chars()
        .map(|c| {
            if FORBIDDEN.contains(&c) || c.is_control() {
                REPLACEMENT
            } else {
                c
            }
        })
        .collect();

    // 2. Windows cấm tên kết thúc bằng dấu chấm hoặc khoảng trắng — Explorer
    //    lặng lẽ cắt đi, rồi ứng dụng đi tìm cái tên nó vừa ghi thì không thấy.
    //    Bước này cũng là thứ giết `..`: nó rụng hết thành rỗng.
    let trimmed = trim_trailing_dots_and_spaces(replaced.trim());

    if is_meaningless(trimmed) {
        return truncate_to_bytes(UNTITLED, max_bytes);
    }

    // 3. Tên thiết bị dành riêng, kể cả khi có phần mở rộng.
    let guarded = guard_reserved_device_name(trimmed);

    // 4. Rút gọn theo byte, giữ lại phần mở rộng.
    let truncated = truncate_to_bytes(&guarded, max_bytes);

    // 5. Cắt ở bước 4 có thể để lại dấu chấm/khoảng trắng ở cuối (`Bài hát .`),
    //    nên phải trim lại lần nữa.
    let settled = trim_trailing_dots_and_spaces(truncated.trim_end());
    if is_meaningless(settled) {
        return truncate_to_bytes(UNTITLED, max_bytes);
    }
    settled.to_string()
}

/// Đúng khi chuỗi không còn ký tự nào mang nghĩa — rỗng, hoặc chỉ gồm ký tự
/// thay thế, dấu chấm và khoảng trắng. `___` là tên file hợp lệ về mặt kỹ
/// thuật nhưng vô nghĩa với người dùng, nên vẫn rơi về [`UNTITLED`].
fn is_meaningless(value: &str) -> bool {
    value
        .chars()
        .all(|c| c == REPLACEMENT || c == '.' || c.is_whitespace())
}

fn trim_trailing_dots_and_spaces(value: &str) -> &str {
    value.trim_end_matches(|c: char| c == '.' || c.is_whitespace())
}

/// `CON.mp3` → `CON_.mp3`, `CON.tar.gz` → `CON_.tar.gz`, `CON` → `CON_`.
///
/// Chèn `_` ngay sau phần đứng trước dấu chấm **đầu tiên**, vì đó chính là
/// phần Windows đem đi so với danh sách thiết bị.
fn guard_reserved_device_name(name: &str) -> String {
    let base_len = name.find('.').unwrap_or(name.len());
    let base = &name[..base_len];
    if is_reserved_device_name(base) {
        format!("{}{}{}", base, REPLACEMENT, &name[base_len..])
    } else {
        name.to_string()
    }
}

fn is_reserved_device_name(base: &str) -> bool {
    // `CON ` (có khoảng trắng đuôi) cũng mở ra thiết bị: Windows bỏ khoảng
    // trắng trước khi so.
    let normalized = base.trim().to_ascii_uppercase();
    RESERVED_DEVICE_NAMES.contains(&normalized.as_str())
}

/// Tách `("Bài hát", ".mp3")`. Trả về `("...", "")` khi không có gì đáng coi là
/// phần mở rộng.
///
/// Điều kiện chặt có chủ ý — chỉ chữ và số sau dấu chấm, và tối đa
/// [`MAX_EXTENSION_BYTES`] — vì hàm rút gọn tin vào kết quả này để quyết định
/// giữ lại phần nào. Nhận nhầm một đoạn tiêu đề là "phần mở rộng" sẽ giữ lại
/// đúng cái đoạn vô dụng và vứt đi phần có nghĩa.
fn split_extension(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(dot) if dot > 0 => {
            let ext = &name[dot..];
            let is_extension = ext.len() > 1
                && ext.len() <= MAX_EXTENSION_BYTES
                && ext[1..].chars().all(|c| c.is_ascii_alphanumeric());
            if is_extension {
                (&name[..dot], ext)
            } else {
                (name, "")
            }
        }
        _ => (name, ""),
    }
}

/// Chỉ số byte lớn nhất `<= index` mà cắt ở đó vẫn ra UTF-8 hợp lệ.
///
/// `str::floor_char_boundary` còn là API chưa ổn định, nên tự viết. Đây là
/// mấu chốt của việc rút gọn: một tiêu đề tiếng Việt có dấu tốn 2–3 byte cho
/// mỗi chữ và emoji tốn 4 — cắt phăng ở byte thứ 255 sẽ tạo ra chuỗi không
/// phải UTF-8, và Rust thì panic ngay tại chỗ slice.
fn floor_char_boundary(value: &str, index: usize) -> usize {
    if index >= value.len() {
        return value.len();
    }
    let mut boundary = index;
    while boundary > 0 && !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    boundary
}

/// Rút gọn về `max_bytes` byte, ưu tiên giữ phần mở rộng.
///
/// Mất `.mp3` tệ hơn mất mấy chữ cuối của tiêu đề: hệ điều hành không còn biết
/// mở file bằng gì.
fn truncate_to_bytes(name: &str, max_bytes: usize) -> String {
    if name.len() <= max_bytes {
        return name.to_string();
    }

    let (stem, ext) = split_extension(name);
    if ext.len() < max_bytes {
        let cut = floor_char_boundary(stem, max_bytes - ext.len());
        if cut > 0 {
            return format!("{}{}", &stem[..cut], ext);
        }
    }

    // Phần mở rộng một mình đã dài hơn cả trần: không giữ được nữa, cắt thẳng.
    name[..floor_char_boundary(name, max_bytes)].to_string()
}

/// Đường dẫn ghi được cho `desired`, thêm hậu tố phân biệt nếu đã có file
/// trùng tên (FR-215).
///
/// `exists` được tiêm vào thay vì gọi `Path::exists` bên trong: nhờ vậy hàm
/// này là hàm thuần, kiểm thử được mọi tình huống trùng tên mà không cần thư
/// mục tạm, và người gọi tự chọn hỏi đĩa hay hỏi một danh sách tên đã đặt cho
/// lô đang chạy.
///
/// Hậu tố là ` (2)`, ` (3)`, ... đặt trước phần mở rộng, giống quy ước của
/// trình quản lý file trên cả ba hệ.
pub fn deduplicate_path(desired: &Path, exists: impl Fn(&Path) -> bool) -> PathBuf {
    if !exists(desired) {
        return desired.to_path_buf();
    }

    // Tên không phải UTF-8 (hiếm, nhưng có trên Linux): không cắt ghép an toàn
    // được, trả lại nguyên trạng thay vì đoán.
    let Some(name) = desired.file_name().and_then(|name| name.to_str()) else {
        return desired.to_path_buf();
    };

    let parent = desired.parent();
    let (stem, ext) = split_extension(name);
    let mut candidate = desired.to_path_buf();

    for attempt in 2..=MAX_DEDUP_ATTEMPTS {
        let suffix = format!(" ({attempt})");
        candidate = join_in(parent, &fit_with_suffix(stem, &suffix, ext, MAX_FILENAME_BYTES));
        if !exists(&candidate) {
            return candidate;
        }
    }

    // Không tới được trong thực tế; trả về ứng viên cuối để hàm luôn có kết quả.
    candidate
}

fn join_in(parent: Option<&Path>, name: &str) -> PathBuf {
    match parent {
        Some(parent) => parent.join(name),
        None => PathBuf::from(name),
    }
}

/// Ghép `stem + suffix + ext` sao cho tổng không vượt `max_bytes`, cắt bớt
/// `stem` nếu cần. Hậu tố phải còn nguyên — nó chính là thứ làm tên khác đi.
fn fit_with_suffix(stem: &str, suffix: &str, ext: &str, max_bytes: usize) -> String {
    let budget = max_bytes.saturating_sub(suffix.len() + ext.len());
    let cut = floor_char_boundary(stem, budget);
    format!("{}{}{}", &stem[..cut], suffix, ext)
}

/// Bọc một chuỗi **hằng** để đưa vào đối số `-o` của yt-dlp.
///
/// yt-dlp đọc `%` là mở đầu một trường mẫu. Một tiêu đề như `100% Real` sẽ
/// khiến nó báo lỗi mẫu hoặc thay bằng thứ khác hẳn. `%%` là cách yt-dlp viết
/// một dấu phần trăm nguyên văn.
pub fn escape_for_ytdlp_template(literal: &str) -> String {
    literal.replace('%', "%%")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_every_character_windows_forbids() {
        // Từng ký tự một, để khi thêm/bớt ký tự trong FORBIDDEN thì test chỉ ra
        // đúng ký tự nào sai chứ không chỉ "chuỗi khác nhau".
        for forbidden in FORBIDDEN {
            let name = format!("a{forbidden}b");
            assert_eq!(sanitize_filename(&name), "a_b", "ký tự {forbidden:?}");
        }
    }

    #[test]
    fn replaces_all_forbidden_characters_in_one_name() {
        assert_eq!(sanitize_filename(r#"a\b/c:d*e?f"g<h>i|j"#), "a_b_c_d_e_f_g_h_i_j");
    }

    #[test]
    fn replaces_control_characters() {
        assert_eq!(sanitize_filename("dòng một\ndòng hai"), "dòng một_dòng hai");
        assert_eq!(sanitize_filename("tab\there"), "tab_here");
    }

    #[test]
    fn guards_reserved_device_names_bare() {
        assert_eq!(sanitize_filename("CON"), "CON_");
        assert_eq!(sanitize_filename("NUL"), "NUL_");
        assert_eq!(sanitize_filename("COM1"), "COM1_");
        assert_eq!(sanitize_filename("LPT9"), "LPT9_");
    }

    #[test]
    fn guards_reserved_device_names_with_extension() {
        // Đây là cái bẫy: `CON.mp3` trông như một file bình thường nhưng
        // Windows vẫn mở ra thiết bị console.
        assert_eq!(sanitize_filename("CON.mp3"), "CON_.mp3");
        assert_eq!(sanitize_filename("con.mp3"), "con_.mp3");
        assert_eq!(sanitize_filename("Aux.TXT"), "Aux_.TXT");
        assert_eq!(sanitize_filename("COM9.tar.gz"), "COM9_.tar.gz");
    }

    #[test]
    fn leaves_names_that_only_look_reserved() {
        assert_eq!(sanitize_filename("CONSOLE"), "CONSOLE");
        assert_eq!(sanitize_filename("COM10"), "COM10");
        assert_eq!(sanitize_filename("MyCON"), "MyCON");
    }

    #[test]
    fn trims_trailing_dot_and_trailing_space() {
        assert_eq!(sanitize_filename("Tên bài."), "Tên bài");
        assert_eq!(sanitize_filename("Tên bài "), "Tên bài");
        assert_eq!(sanitize_filename("Tên bài. . ."), "Tên bài");
        assert_eq!(sanitize_filename("  Tên bài  "), "Tên bài");
    }

    #[test]
    fn falls_back_when_nothing_meaningful_survives() {
        assert_eq!(sanitize_filename("///"), UNTITLED);
        assert_eq!(sanitize_filename(r#"\/:*?"<>|"#), UNTITLED);
        assert_eq!(sanitize_filename(""), UNTITLED);
        assert_eq!(sanitize_filename("   "), UNTITLED);
        // Đường dẫn đi ngược cây thư mục rụng hết ở bước cắt dấu chấm cuối.
        assert_eq!(sanitize_filename(".."), UNTITLED);
    }

    #[test]
    fn strips_path_separators_so_a_template_cannot_escape_the_output_directory() {
        assert_eq!(sanitize_filename("../../etc/passwd"), ".._.._etc_passwd");
        assert_eq!(sanitize_filename(r"..\..\Windows\System32"), ".._.._Windows_System32");
    }

    #[test]
    fn truncation_never_splits_a_multi_byte_character() {
        // "ề" chiếm 3 byte. Trần 10 byte rơi vào giữa ký tự thứ 4 nếu cắt thô.
        let name = "ềềềề";
        assert_eq!(name.len(), 12);
        let truncated = sanitize_filename_with_limit(name, 10);
        assert_eq!(truncated, "ềềề");
        assert!(truncated.len() <= 10);

        // Emoji 4 byte, cộng thêm phần nối ZWJ.
        let emoji = "🎵🎵🎵";
        assert_eq!(emoji.len(), 12);
        assert_eq!(sanitize_filename_with_limit(emoji, 10), "🎵🎵");
    }

    #[test]
    fn truncation_keeps_the_extension() {
        let name = format!("{}.mp3", "a".repeat(300));
        let truncated = sanitize_filename(&name);
        assert!(truncated.ends_with(".mp3"), "mất phần mở rộng: {truncated}");
        assert_eq!(truncated.len(), MAX_FILENAME_BYTES);
        assert_eq!(truncated, format!("{}.mp3", "a".repeat(251)));
    }

    #[test]
    fn truncation_keeps_the_extension_with_multi_byte_stem() {
        // Chữ có dấu 3 byte: 100 chữ = 300 byte, quá trần.
        let name = format!("{}.mp3", "ề".repeat(100));
        let truncated = sanitize_filename(&name);
        assert!(truncated.ends_with(".mp3"));
        assert!(truncated.len() <= MAX_FILENAME_BYTES);
        // 255 - 4 = 251 byte cho phần thân → 83 chữ (249 byte), thừa 2 byte
        // không đủ cho chữ thứ 84.
        assert_eq!(truncated, format!("{}.mp3", "ề".repeat(83)));
    }

    #[test]
    fn truncation_does_not_leave_a_trailing_dot_or_space() {
        let name = format!("{} . x", "a".repeat(253));
        let truncated = sanitize_filename(&name);
        assert!(!truncated.ends_with('.'), "{truncated}");
        assert!(!truncated.ends_with(' '), "{truncated}");
    }

    #[test]
    fn does_not_mistake_a_long_parenthetical_for_an_extension() {
        let name = format!("{} (2026. Deluxe Edition)", "a".repeat(250));
        let truncated = sanitize_filename(&name);
        assert!(truncated.len() <= MAX_FILENAME_BYTES);
        assert!(truncated.starts_with("aaa"));
    }

    #[test]
    fn leaves_short_names_untouched() {
        assert_eq!(sanitize_filename("Chúng ta của tương lai"), "Chúng ta của tương lai");
        assert_eq!(sanitize_filename("01 - Bài hát 🎵.mp3"), "01 - Bài hát 🎵.mp3");
    }

    #[test]
    fn deduplicate_returns_the_desired_path_when_free() {
        let desired = Path::new("/music/Bài hát.mp3");
        assert_eq!(deduplicate_path(desired, |_| false), PathBuf::from("/music/Bài hát.mp3"));
    }

    #[test]
    fn deduplicate_adds_a_suffix_before_the_extension_on_collision() {
        let desired = Path::new("/music/Bài hát.mp3");
        let taken = |path: &Path| path == Path::new("/music/Bài hát.mp3");
        assert_eq!(deduplicate_path(desired, taken), PathBuf::from("/music/Bài hát (2).mp3"));
    }

    #[test]
    fn deduplicate_keeps_counting_past_the_first_free_slot() {
        let desired = Path::new("/music/song.mp3");
        let taken = |path: &Path| {
            matches!(
                path.to_str(),
                Some("/music/song.mp3" | "/music/song (2).mp3" | "/music/song (3).mp3")
            )
        };
        assert_eq!(deduplicate_path(desired, taken), PathBuf::from("/music/song (4).mp3"));
    }

    #[test]
    fn deduplicate_handles_a_name_without_an_extension() {
        let desired = Path::new("/music/song");
        let taken = |path: &Path| path == Path::new("/music/song");
        assert_eq!(deduplicate_path(desired, taken), PathBuf::from("/music/song (2)"));
    }

    #[test]
    fn deduplicate_shortens_the_stem_so_the_suffix_still_fits() {
        let stem = "a".repeat(251);
        let desired = PathBuf::from(format!("/music/{stem}.mp3"));
        let taken = |path: &Path| path == desired;
        let result = deduplicate_path(&desired, taken);
        let name = result.file_name().unwrap().to_str().unwrap();
        assert!(name.ends_with(" (2).mp3"), "{name}");
        assert!(name.len() <= MAX_FILENAME_BYTES, "{} byte", name.len());
    }

    #[test]
    fn deduplicate_leaves_a_relative_name_without_a_parent_alone() {
        let desired = Path::new("song.mp3");
        let taken = |path: &Path| path == Path::new("song.mp3");
        assert_eq!(deduplicate_path(desired, taken), PathBuf::from("song (2).mp3"));
    }

    fn full_fields() -> TemplateFields {
        TemplateFields {
            title: Some("Chúng ta của tương lai".to_string()),
            channel: Some("Sơn Tùng M-TP".to_string()),
            playlist_index: Some(3),
            upload_date: Some("20260726".to_string()),
            resolution: Some("1080p".to_string()),
            ext: Some("mp3".to_string()),
        }
    }

    #[test]
    fn renders_every_supported_field() {
        let rendered = render_filename(
            "{playlist_index} - {channel} - {title} ({upload_date}) [{resolution}].{ext}",
            &full_fields(),
        );
        assert_eq!(
            rendered,
            "03 - Sơn Tùng M-TP - Chúng ta của tương lai (2026-07-26) [1080p].mp3"
        );
    }

    #[test]
    fn each_missing_field_gets_its_own_fallback() {
        let empty = TemplateFields::default();
        assert_eq!(render_filename("{title}", &empty), FALLBACK_TITLE);
        assert_eq!(render_filename("{channel}", &empty), FALLBACK_CHANNEL);
        assert_eq!(render_filename("{playlist_index}", &empty), FALLBACK_PLAYLIST_INDEX);
        assert_eq!(render_filename("{upload_date}", &empty), FALLBACK_UPLOAD_DATE);
        assert_eq!(render_filename("{resolution}", &empty), FALLBACK_RESOLUTION);
        assert_eq!(render_filename("{ext}", &empty), FALLBACK_EXT);
    }

    #[test]
    fn a_field_present_but_blank_counts_as_missing() {
        let blank = TemplateFields {
            title: Some("   ".to_string()),
            channel: Some(String::new()),
            ..TemplateFields::default()
        };
        assert_eq!(render_filename("{title}", &blank), FALLBACK_TITLE);
        assert_eq!(render_filename("{channel}", &blank), FALLBACK_CHANNEL);
    }

    #[test]
    fn a_template_of_only_missing_fields_still_yields_a_usable_name() {
        // FR-216: không bao giờ ra tên rỗng.
        let rendered = render_filename("{title}", &TemplateFields::default());
        assert!(!rendered.is_empty());
    }

    #[test]
    fn pads_the_playlist_index_to_two_digits_but_never_clips_it() {
        let one = TemplateFields {
            playlist_index: Some(1),
            ..TemplateFields::default()
        };
        assert_eq!(render_filename("{playlist_index}", &one), "01");

        let big = TemplateFields {
            playlist_index: Some(137),
            ..TemplateFields::default()
        };
        assert_eq!(render_filename("{playlist_index}", &big), "137");
    }

    #[test]
    fn keeps_an_already_dashed_upload_date_as_is() {
        let fields = TemplateFields {
            upload_date: Some("2026-07-26".to_string()),
            ..TemplateFields::default()
        };
        assert_eq!(render_filename("{upload_date}", &fields), "2026-07-26");
    }

    #[test]
    fn leaves_an_unknown_field_visible_instead_of_swallowing_it() {
        let rendered = render_filename("{titel} - x", &full_fields());
        assert_eq!(rendered, "{titel} - x");
    }

    #[test]
    fn leaves_an_unclosed_brace_as_literal_text() {
        assert_eq!(render_filename("{title", &full_fields()), "{title");
    }

    #[test]
    fn sanitizes_field_values_that_came_from_the_source() {
        let fields = TemplateFields {
            title: Some("AC/DC: Back in Black?".to_string()),
            ..TemplateFields::default()
        };
        assert_eq!(render_filename("{title}", &fields), "AC_DC_ Back in Black_");
    }

    #[test]
    fn a_template_cannot_build_a_subdirectory_or_climb_out() {
        let fields = TemplateFields {
            channel: Some("kênh".to_string()),
            title: Some("bài".to_string()),
            ..TemplateFields::default()
        };
        let rendered = render_filename("{channel}/../../{title}", &fields);
        assert!(!rendered.contains('/'), "{rendered}");
        assert!(!rendered.contains('\\'), "{rendered}");
        assert_eq!(rendered, "kênh_.._.._bài");
    }

    #[test]
    fn the_default_template_reproduces_the_old_behaviour() {
        assert_eq!(
            render_filename(DEFAULT_TEMPLATE, &full_fields()),
            "Chúng ta của tương lai"
        );
    }

    #[test]
    fn escapes_percent_so_ytdlp_cannot_reinterpret_a_literal_name() {
        assert_eq!(escape_for_ytdlp_template("100% Real"), "100%% Real");
        assert_eq!(escape_for_ytdlp_template("%(title)s"), "%%(title)s");
        assert_eq!(escape_for_ytdlp_template("no percent"), "no percent");
    }

    #[test]
    fn template_fields_and_fallbacks_stay_in_step() {
        // Mọi trường được quảng cáo phải thật sự thay thế được, và phải có giá
        // trị dự phòng — nếu không FR-216 hở một lỗ đúng ở trường mới thêm.
        let empty = TemplateFields::default();
        for field in TEMPLATE_FIELDS {
            let rendered = render_filename(&format!("{{{field}}}"), &empty);
            assert_ne!(rendered, format!("{{{field}}}"), "trường {field} không được thay");
            assert!(!rendered.is_empty(), "trường {field} cho ra tên rỗng");
        }
    }
}
