//! Thư viện media đã tải (`specs/004-library`) — nửa backend.
//!
//! Ranh giới của module này: chỉ mục nằm trong CSDL, còn file thật nằm trên
//! đĩa, và mọi lệnh ở đây giữ hai thứ đó khớp nhau. Không có lệnh nào ghi đè
//! một file đã tồn tại (FR-322), không có lệnh nào xoá vĩnh viễn (FR-318), và
//! không có lệnh nào chạm vào toàn bộ thư viện trong một nhịp đồng bộ
//! (FR-327).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_opener::OpenerExt;

use crate::db::{Db, LibraryFileState, FILE_EXISTS_ERROR_CODE};
use crate::downloader::filename::sanitize_filename;
use crate::downloader::queue::DownloadQueue;
use crate::error::AppError;
use crate::models::{DownloadJob, LibraryItem, LibraryQuery, LibraryStats};

/// Số mục được `stat` trong một lô của vòng đối soát.
///
/// Đủ lớn để 10.000 mục chỉ mất khoảng 40 lô, và đủ nhỏ để mỗi lô kết thúc
/// trong vài mili-giây — giữa hai lô luôn có một điểm `.await`, nên bộ chạy
/// bất đồng bộ (và mọi lệnh khác đang chờ) không bị một vòng lặp chạm đĩa dài
/// giữ chân.
const RECONCILE_BATCH_SIZE: i64 = 256;

// ---- duyệt, tìm, lọc, thống kê ------------------------------------------

/// FR-307 → FR-310. Việc lọc/sắp/phân trang nằm hết trong SQL — xem
/// `Db::list_library` và các chỉ mục của migration 0012.
#[tauri::command]
pub fn list_library(
    db: State<Arc<Db>>,
    query: Option<LibraryQuery>,
) -> Result<Vec<LibraryItem>, AppError> {
    db.list_library(&query.unwrap_or_default())
}

/// FR-328. Nhận cùng bộ lọc mà `list_library` vừa nhận, nên con số luôn mô tả
/// đúng tập người dùng đang nhìn (SC-307).
#[tauri::command]
pub fn library_stats(
    db: State<Arc<Db>>,
    query: Option<LibraryQuery>,
) -> Result<LibraryStats, AppError> {
    db.library_stats(&query.unwrap_or_default())
}

/// FR-302: mọi file của một tác vụ, kể cả khi tách chương sinh ra hàng chục
/// file cho đúng một dòng hàng đợi.
#[tauri::command]
pub fn library_items_for_job(
    db: State<Arc<Db>>,
    job_id: String,
) -> Result<Vec<LibraryItem>, AppError> {
    db.library_items_for_job(&job_id)
}

// ---- đối soát với thực tế ------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct LibraryReconcileReport {
    pub checked: i64,
    /// Tổng số mục đang bị đánh dấu thiếu SAU vòng quét này.
    pub missing: i64,
    /// Mục đã đổi trạng thái trong chính vòng này (thiếu → còn hoặc ngược
    /// lại). Giao diện chỉ cần vẽ lại chừng ấy ô.
    pub changed_item_ids: Vec<String>,
}

/// Sự kiện phát sau MỖI lô, để lưới cập nhật dần thay vì đứng im tới khi quét
/// xong cả thư viện.
#[derive(Debug, Clone, Serialize)]
struct LibraryReconciledEvent {
    changed_item_ids: Vec<String>,
    checked: i64,
}

/// FR-323 + FR-327: phát hiện file bị xoá hoặc di chuyển bên ngoài ứng dụng,
/// mà không chặn giao diện.
///
/// Ba lớp bảo vệ, và cả ba đều cần thiết:
///
/// 1. **Không nằm trên đường mở thư viện.** `list_library` chỉ đọc chỉ mục,
///    không `stat` file nào, nên lưới hiện ra ngay. Đối soát là một lệnh
///    riêng mà giao diện gọi SAU khi đã vẽ xong.
/// 2. **Không nằm trên luồng giao diện.** Lệnh này là `async`, nên Tauri chạy
///    nó trên bộ chạy bất đồng bộ chứ không phải luồng chính; và phần `stat`
///    thật sự — thao tác chặn duy nhất ở đây — được đẩy sang
///    `spawn_blocking`, đúng nơi dành cho lời gọi hệ thống đồng bộ.
/// 3. **Theo lô, có điểm nhả.** Mỗi `RECONCILE_BATCH_SIZE` mục là một lần
///    `.await`, nên một thư viện 10.000 mục không biến thành một tác vụ chạy
///    liền mạch giữ chỗ trên bộ chạy; các lệnh khác vẫn chen vào được giữa
///    hai lô.
///
/// Kết quả từng lô được phát ngay qua sự kiện `library:reconciled` — người
/// dùng thấy các mục thiếu hiện ra dần thay vì chờ một cục.
#[tauri::command]
pub async fn reconcile_library(
    app: AppHandle,
    db: State<'_, Arc<Db>>,
    batch_size: Option<i64>,
) -> Result<LibraryReconcileReport, AppError> {
    let db = Arc::clone(&db);
    let batch_size = batch_size
        .filter(|size| *size > 0)
        .unwrap_or(RECONCILE_BATCH_SIZE);

    let mut offset = 0i64;
    let mut checked = 0i64;
    let mut missing = 0i64;
    let mut changed_item_ids = Vec::new();

    loop {
        let page = db.library_reconcile_page(offset, batch_size)?;
        if page.is_empty() {
            break;
        }
        let page_len = page.len() as i64;

        let observed: Vec<LibraryFileState> =
            tokio::task::spawn_blocking(move || page.into_iter().map(observe).collect())
                .await
                .map_err(AppError::internal)?;
        let changed = db.apply_library_file_states(&observed)?;

        checked += page_len;
        missing += observed.iter().filter(|state| state.is_missing).count() as i64;
        if !changed.is_empty() {
            let _ = app.emit(
                "library:reconciled",
                LibraryReconciledEvent {
                    changed_item_ids: changed.clone(),
                    checked,
                },
            );
            changed_item_ids.extend(changed);
        }

        offset += page_len;
    }

    Ok(LibraryReconcileReport {
        checked,
        missing,
        changed_item_ids,
    })
}

/// Đọc trạng thái thật của một mục trên đĩa.
///
/// "Thiếu" ở đây là "không tồn tại", chứ không phải "không đọc được": một ổ
/// đĩa ngoài đã tháo cũng cho ra `false` ở `Path::exists`, và spec nói rõ
/// trường hợp đó là **thiếu tạm thời** — mục vẫn nằm nguyên trong thư viện,
/// chỉ đổi nhãn, và tự sáng lại ở vòng đối soát sau khi cắm ổ vào.
fn observe(state: LibraryFileState) -> LibraryFileState {
    match std::fs::metadata(&state.file_path) {
        Ok(metadata) => LibraryFileState {
            is_missing: false,
            file_size_bytes: metadata.len() as i64,
            ..state
        },
        // Giữ nguyên dung lượng đã biết thay vì ghi 0: khi mục quay lại (cắm
        // lại ổ, tìm lại file) thì con số cũ vẫn đúng, còn số 0 thì đã kịp
        // làm sai lệch thống kê của FR-328 trong suốt thời gian chờ.
        Err(_) => LibraryFileState {
            is_missing: true,
            ..state
        },
    }
}

/// FR-324: gỡ các mục thiếu khỏi thư viện. KHÔNG đụng tới đĩa — nếu file thật
/// sự vẫn còn ở đâu đó (ổ ngoài chưa cắm), người dùng chỉ đang dọn chỉ mục.
#[tauri::command]
pub fn remove_library_items(db: State<Arc<Db>>, item_ids: Vec<String>) -> Result<usize, AppError> {
    db.remove_library_items(&item_ids)
}

/// FR-325: trỏ một mục thiếu tới vị trí mới của file thay vì phải tải lại.
#[tauri::command]
pub fn relink_library_item(
    db: State<Arc<Db>>,
    item_id: String,
    new_path: String,
) -> Result<LibraryItem, AppError> {
    relink_item(&db, &item_id, &new_path)
}

fn relink_item(db: &Db, item_id: &str, new_path: &str) -> Result<LibraryItem, AppError> {
    let metadata =
        std::fs::metadata(new_path).map_err(|_| AppError::not_found("File at the given path"))?;
    if !metadata.is_file() {
        return Err(AppError::new("NOT_A_FILE", "The chosen path is not a file"));
    }
    let item = require_item(db, item_id)?;
    let relinked = db.set_library_item_path(item_id, new_path, metadata.len() as i64)?;
    sync_job_output_path(db, &item, new_path);
    Ok(relinked)
}

/// FR-326: tạo tác vụ tải lại cho một mục, dùng ĐÚNG URL và cấu hình gốc.
///
/// Uỷ thác cho `DownloadQueue::retry`, vốn đã tái tạo một tác vụ từ bản ghi
/// gốc (URL, nền tảng, chất lượng, thư mục đích, và cả `output_options` của
/// FR-235). Viết lại logic ấy ở đây sẽ tạo ra một bản sao thứ hai có quyền
/// trôi khỏi bản gốc — và "tải lại một mục thiếu" với "thử lại một tác vụ"
/// vốn là cùng một thao tác nhìn từ hai màn hình khác nhau.
#[tauri::command]
pub async fn redownload_library_item(
    db: State<'_, Arc<Db>>,
    queue: State<'_, DownloadQueue>,
    item_id: String,
) -> Result<DownloadJob, AppError> {
    let item = require_item(&db, &item_id)?;
    queue.retry(&item.job_id).await
}

// ---- thao tác trên file --------------------------------------------------

/// FR-317: đổi tên file, cập nhật đồng thời tên trên đĩa và trong chỉ mục.
///
/// `new_name` là **tên file**, không phải đường dẫn: nó đi qua
/// `sanitize_filename`, nên một chuỗi như `../../etc/passwd` trở thành một
/// cái tên vô hại trong đúng thư mục cũ thay vì một đường thoát ra ngoài.
///
/// Thiếu phần mở rộng thì giữ nguyên phần mở rộng cũ — người dùng đổi tên cho
/// gọn, không phải để đổi định dạng, và một file `.mp3` mất đuôi là một file
/// mà hệ điều hành không còn mở được.
#[tauri::command]
pub fn rename_library_item(
    db: State<Arc<Db>>,
    item_id: String,
    new_name: String,
) -> Result<LibraryItem, AppError> {
    rename_item(&db, &item_id, &new_name)
}

fn rename_item(db: &Db, item_id: &str, new_name: &str) -> Result<LibraryItem, AppError> {
    let item = require_item(db, item_id)?;
    let current = PathBuf::from(&item.file_path);
    let parent = current
        .parent()
        .ok_or_else(|| AppError::internal("Library item has no containing directory"))?;
    let target = parent.join(target_file_name(&current, new_name)?);

    if target == current {
        return Ok(item);
    }
    reject_existing(&target)?;
    std::fs::rename(&current, &target)?;

    let size = std::fs::metadata(&target)
        .map(|meta| meta.len() as i64)
        .unwrap_or(item.file_size_bytes);
    let renamed = db.set_library_item_path(item_id, &target.to_string_lossy(), size)?;
    sync_job_output_path(db, &item, &target.to_string_lossy());
    Ok(renamed)
}

/// FR-319 + FR-320: di chuyển một hoặc nhiều mục sang thư mục khác.
///
/// Mọi đích đến được kiểm TRƯỚC khi động vào file đầu tiên. Đó là điều khác
/// biệt giữa "một thao tác hàng loạt thất bại" và "một thao tác hàng loạt
/// thất bại giữa chừng": va chạm tên phát hiện ở mục thứ bảy mà sáu mục đầu
/// đã bị chuyển đi thì người dùng không còn cách nào quay lại trạng thái cũ.
/// FR-322 cấm ghi đè, nên câu trả lời đúng là từ chối cả lô.
#[tauri::command]
pub fn move_library_items(
    db: State<Arc<Db>>,
    item_ids: Vec<String>,
    target_directory: String,
) -> Result<Vec<LibraryItem>, AppError> {
    move_items(&db, &item_ids, &target_directory)
}

fn move_items(
    db: &Db,
    item_ids: &[String],
    target_directory: &str,
) -> Result<Vec<LibraryItem>, AppError> {
    let directory = PathBuf::from(target_directory);
    if !directory.is_dir() {
        return Err(AppError::not_found("Target directory"));
    }

    let items = db.library_items(item_ids)?;
    let mut planned: Vec<(LibraryItem, PathBuf)> = Vec::new();
    for item in items {
        let source = PathBuf::from(&item.file_path);
        let file_name = source
            .file_name()
            .ok_or_else(|| AppError::internal("Library item has no file name"))?;
        let target = directory.join(file_name);
        if target == source {
            continue;
        }
        reject_existing(&target)?;
        // Hai mục cùng tên đến từ hai thư mục khác nhau sẽ đè lên nhau ngay
        // trong chính lô này, ở một chỗ mà phép kiểm tra trên đĩa phía trên
        // không nhìn thấy vì file thứ hai chưa được ghi.
        if planned.iter().any(|(_, existing)| *existing == target) {
            return Err(AppError::new(
                FILE_EXISTS_ERROR_CODE,
                format!(
                    "Two of the selected items would both become {}",
                    target.to_string_lossy()
                ),
            ));
        }
        planned.push((item, target));
    }

    let mut moved = Vec::new();
    for (item, target) in planned {
        std::fs::rename(&item.file_path, &target)?;
        let target_str = target.to_string_lossy().into_owned();
        moved.push(db.set_library_item_path(&item.id, &target_str, item.file_size_bytes)?);
        sync_job_output_path(db, &item, &target_str);
    }
    Ok(moved)
}

/// FR-318 + FR-320: xoá một hoặc nhiều mục — vào **thùng rác của hệ thống**,
/// không bao giờ `unlink`.
///
/// Đây là chỗ duy nhất trong toàn bộ ứng dụng gỡ một file người dùng đã tải
/// về khỏi vị trí của nó, và SC-305 nói không thao tác xoá nào được gây mất
/// dữ liệu không khôi phục được. `std::fs::remove_file` sẽ vi phạm điều đó
/// một cách im lặng và không thể sửa được sau khi đã chạy — nên nó không xuất
/// hiện ở đây, kể cả như một nhánh dự phòng khi thùng rác không dùng được.
/// Thùng rác hỏng thì thao tác thất bại, và file vẫn còn.
///
/// Chỉ mục chỉ được cập nhật SAU khi thùng rác nhận file. Ngược lại thì một
/// lần xoá thất bại sẽ để lại file trên đĩa mà không còn mục nào trỏ tới —
/// tức là làm mất file theo một kiểu khác.
#[tauri::command]
pub async fn delete_library_items(
    db: State<'_, Arc<Db>>,
    item_ids: Vec<String>,
) -> Result<usize, AppError> {
    delete_items(&db, &item_ids).await
}

async fn delete_items(db: &Db, item_ids: &[String]) -> Result<usize, AppError> {
    let items = db.library_items(item_ids)?;
    if items.is_empty() {
        return Ok(0);
    }
    // Mục đang thiếu thì không có gì để đưa vào thùng rác; nó vẫn phải biến
    // khỏi thư viện, nếu không thì "xoá" một mục thiếu sẽ không làm gì cả.
    let (present, absent): (Vec<_>, Vec<_>) = items
        .into_iter()
        .partition(|item| Path::new(&item.file_path).exists());

    if !present.is_empty() {
        let paths: Vec<String> = present.iter().map(|item| item.file_path.clone()).collect();
        tokio::task::spawn_blocking(move || trash::delete_all(&paths))
            .await
            .map_err(AppError::internal)?
            .map_err(|err| {
                AppError::new(
                    "TRASH_FAILED",
                    format!("Could not move the file(s) to the system trash: {err}"),
                )
            })?;
    }

    let removed_ids: Vec<String> = present
        .iter()
        .chain(absent.iter())
        .map(|item| item.id.clone())
        .collect();
    db.remove_library_items(&removed_ids)
}

/// FR-321: mở thư mục chứa file bằng trình quản lý tệp của hệ thống.
#[tauri::command]
pub fn reveal_library_item(
    app: AppHandle,
    db: State<Arc<Db>>,
    item_id: String,
) -> Result<(), AppError> {
    let item = require_item(&db, &item_id)?;
    app.opener()
        .reveal_item_in_dir(&item.file_path)
        .map_err(AppError::internal)
}

// ---- xuất danh sách phát -------------------------------------------------

/// FR-330: xuất danh sách phát từ các mục đang chọn hoặc đang lọc, **giữ đúng
/// thứ tự hiển thị**.
///
/// Thứ tự đến từ `item_ids` và chỉ từ đó. Sắp xếp lại ở đây theo bất kỳ tiêu
/// chí nào của backend sẽ phá đúng cái yêu cầu này, vì thứ tự đang hiển thị
/// là kết quả của bộ lọc + tiêu chí sắp xếp + lựa chọn của người dùng — một
/// trạng thái chỉ tồn tại ở phía giao diện. Xem `Db::library_items`.
///
/// Định dạng là M3U mở rộng: dòng `#EXTINF` mang thời lượng và tiêu đề, nên
/// trình phát hiện đúng tên bài thay vì tên file. Thời lượng chưa biết ghi
/// `-1`, đúng quy ước của định dạng cho "không rõ" — không phải `0`, vốn có
/// nghĩa là một file rỗng.
#[tauri::command]
pub fn export_library_playlist(
    db: State<Arc<Db>>,
    item_ids: Vec<String>,
    destination_path: String,
) -> Result<String, AppError> {
    export_playlist(&db, &item_ids, &destination_path)
}

fn export_playlist(
    db: &Db,
    item_ids: &[String],
    destination_path: &str,
) -> Result<String, AppError> {
    let items = db.library_items(item_ids)?;
    if items.is_empty() {
        return Err(AppError::new(
            "EMPTY_PLAYLIST",
            "No library items were selected for export",
        ));
    }
    let destination = PathBuf::from(destination_path);
    reject_existing(&destination)?;
    std::fs::write(&destination, render_m3u(&items))?;
    Ok(destination.to_string_lossy().into_owned())
}

/// Phần thuần của [`export_library_playlist`], tách ra để kiểm chứng được thứ
/// tự và nội dung mà không cần ghi ra đĩa.
fn render_m3u(items: &[LibraryItem]) -> String {
    let mut out = String::from("#EXTM3U\n");
    for item in items {
        out.push_str(&format!(
            "#EXTINF:{},{}\n{}\n",
            item.duration_seconds.unwrap_or(-1),
            item.title.replace(['\r', '\n'], " "),
            item.file_path
        ));
    }
    out
}

// ---- phần dùng chung -----------------------------------------------------

fn require_item(db: &Db, item_id: &str) -> Result<LibraryItem, AppError> {
    db.library_item(item_id)?
        .ok_or_else(|| AppError::not_found("Library item"))
}

/// FR-322. Một câu `if exists { Err }` chứ không phải một bước tự đổi tên:
/// spec nói người dùng phải được **cảnh báo** và thao tác **không** ghi đè,
/// nên "resolve" âm thầm thành `tên (2).mp3` cũng là một câu trả lời sai —
/// chỉ là sai theo hướng dễ chịu hơn.
fn reject_existing(target: &Path) -> Result<(), AppError> {
    if target.exists() {
        return Err(AppError::new(
            FILE_EXISTS_ERROR_CODE,
            format!("A file already exists at {}", target.to_string_lossy()),
        ));
    }
    Ok(())
}

/// Tên file đích của một lần đổi tên, đã làm sạch và đã bù phần mở rộng.
fn target_file_name(current: &Path, new_name: &str) -> Result<String, AppError> {
    let cleaned = sanitize_filename(new_name.trim());
    if cleaned.is_empty() {
        return Err(AppError::new(
            "INVALID_FILE_NAME",
            "The new name is empty after removing characters the filesystem cannot store",
        ));
    }
    let has_extension = Path::new(&cleaned).extension().is_some();
    match current.extension().and_then(|ext| ext.to_str()) {
        Some(extension) if !has_extension => Ok(format!("{cleaned}.{extension}")),
        _ => Ok(cleaned),
    }
}

/// Giữ `download_jobs.output_file_path` khớp với vị trí mới.
///
/// Trang Lịch sử đọc thẳng cột đó (chứ không qua chỉ mục Thư viện), nên một
/// lần đổi tên trong Thư viện mà không đụng tới nó sẽ để lại một nút "mở thư
/// mục chứa" trỏ vào hư không ở màn hình bên cạnh.
///
/// Chỉ ghi khi đường dẫn cũ khớp CHÍNH XÁC: một tác vụ tách chương có nhiều
/// mục thư viện nhưng `output_file_path` chỉ trỏ vào file gốc, nên đổi tên
/// một file chương không được phép kéo theo cột đó.
///
/// Thất bại ở đây cố ý bị nuốt: chỉ mục Thư viện đã được cập nhật đúng, và
/// một cột hiển thị ở màn hình khác không đáng để làm hỏng cả thao tác đổi
/// tên vừa thành công trên đĩa.
fn sync_job_output_path(db: &Db, item: &LibraryItem, new_path: &str) {
    let Ok(Some(job)) = db.get_job(&item.job_id) else {
        return;
    };
    if job.output_file_path.as_deref() == Some(item.file_path.as_str()) {
        let _ = db.set_job_output_file(&item.job_id, new_path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{JobStatus, MediaType, NewLibraryFile, OutputOptions};

    /// Một CSDL tạm cùng một thư mục tạm riêng cho từng test — các test đụng
    /// tới đĩa thật, nên chúng không được dùng chung chỗ nào.
    fn test_db() -> (Db, PathBuf) {
        let unique = uuid::Uuid::new_v4().to_string();
        let root = std::env::temp_dir().join(format!("media-downloader-lib-{unique}"));
        std::fs::create_dir_all(&root).unwrap();
        let db = Db::open(&root.join("test.db")).unwrap();
        (db, root)
    }

    /// Tạo một file thật trên đĩa và một mục thư viện trỏ vào nó.
    fn add_real_item(
        db: &Db,
        root: &Path,
        name: &str,
        title: &str,
        duration: Option<i64>,
    ) -> LibraryItem {
        let path = root.join(name);
        std::fs::write(&path, b"contents").unwrap();
        let job_id = uuid::Uuid::new_v4().to_string();
        db.insert_job(&crate::models::DownloadJob {
            id: job_id.clone(),
            source_url: format!("https://example.com/{job_id}"),
            platform: "youtube".to_string(),
            media_type: MediaType::Audio,
            audio_quality: None,
            video_quality: None,
            gallery_mode: None,
            selected_gallery_indices: None,
            status: JobStatus::Completed,
            progress_percent: 100.0,
            speed_bytes_per_sec: None,
            eta_seconds: None,
            error_message: None,
            output_directory: root.to_string_lossy().into_owned(),
            output_file_path: Some(path.to_string_lossy().into_owned()),
            is_playlist_item: false,
            parent_playlist_id: None,
            retried_from_job_id: None,
            created_at: "2026-07-26T00:00:00Z".to_string(),
            updated_at: "2026-07-26T00:00:00Z".to_string(),
            title: Some(title.to_string()),
            playlist_title: None,
            queue_position: 0.0,
            retry_count: 0,
            next_retry_at: None,
            output_options: OutputOptions::default(),
        })
        .unwrap();
        db.insert_downloaded_file(&NewLibraryFile {
            job_id: job_id.clone(),
            file_path: path.to_string_lossy().into_owned(),
            file_format: "mp3".to_string(),
            file_size_bytes: 8,
            title: title.to_string(),
            media_type: MediaType::Audio,
            platform: "youtube".to_string(),
            source_url: format!("https://example.com/{job_id}"),
            duration_seconds: duration,
            thumbnail_path: None,
        })
        .unwrap();
        db.library_items_for_job(&job_id).unwrap().remove(0)
    }

    #[test]
    fn renaming_updates_both_the_file_on_disk_and_the_index() {
        // FR-317: hai nửa của cùng một thao tác. Chỉ đổi trong chỉ mục thì
        // mục trỏ vào hư không; chỉ đổi trên đĩa thì thư viện trỏ nhầm.
        let (db, root) = test_db();
        let item = add_real_item(&db, &root, "cu.mp3", "Tên cũ", None);

        let renamed = rename_item(&db, &item.id, "Tên mới").unwrap();

        assert_eq!(
            renamed.file_path,
            root.join("Tên mới.mp3").to_string_lossy()
        );
        assert!(root.join("Tên mới.mp3").exists());
        assert!(!root.join("cu.mp3").exists());
        // Cột `output_file_path` của tác vụ (trang Lịch sử đọc thẳng nó) cũng
        // phải theo kịp, nếu không nút "mở thư mục chứa" bên đó sẽ hỏng.
        let job = db.get_job(&renamed.job_id).unwrap().unwrap();
        assert_eq!(
            job.output_file_path.as_deref(),
            Some(renamed.file_path.as_str())
        );
    }

    #[test]
    fn renaming_onto_an_existing_file_is_refused_and_changes_nothing() {
        // FR-322 + kịch bản 5 của User Story 4: cảnh báo và KHÔNG ghi đè.
        // Phép thử thật nằm ở ba khẳng định cuối — một cài đặt tự "giải
        // quyết" va chạm bằng cách đổi thành `b (2).mp3` cũng sẽ trả về Ok và
        // qua được một test chỉ kiểm mã lỗi.
        let (db, root) = test_db();
        let a = add_real_item(&db, &root, "a.mp3", "A", None);
        add_real_item(&db, &root, "b.mp3", "B", None);

        let err = rename_item(&db, &a.id, "b.mp3").unwrap_err();

        assert_eq!(err.code, FILE_EXISTS_ERROR_CODE);
        assert!(
            root.join("a.mp3").exists(),
            "file gốc phải còn nguyên chỗ cũ"
        );
        assert_eq!(
            std::fs::read(root.join("b.mp3")).unwrap(),
            b"contents",
            "file đích không được bị ghi đè"
        );
        assert_eq!(
            db.library_item(&a.id).unwrap().unwrap().file_path,
            a.file_path,
            "chỉ mục không được đổi khi thao tác trên đĩa đã bị từ chối"
        );
        assert_eq!(
            std::fs::read_dir(&root).unwrap().count(),
            3,
            "không được sinh thêm file nào"
        );
    }

    #[test]
    fn a_new_name_without_an_extension_keeps_the_old_one() {
        let (db, root) = test_db();
        let item = add_real_item(&db, &root, "bai-hat.mp3", "Bài hát", None);

        let renamed = rename_item(&db, &item.id, "Bài hát hay").unwrap();

        assert!(renamed.file_path.ends_with("Bài hát hay.mp3"));
        assert_eq!(renamed.file_format, "mp3");
    }

    #[test]
    fn a_new_name_cannot_escape_its_own_directory() {
        let (db, root) = test_db();
        let item = add_real_item(&db, &root, "trong.mp3", "Trong", None);

        let renamed = rename_item(&db, &item.id, "../../thoat.mp3").unwrap();

        assert_eq!(
            PathBuf::from(&renamed.file_path).parent().unwrap(),
            root,
            "tên mới phải nằm nguyên trong thư mục cũ"
        );
    }

    #[test]
    fn moving_a_batch_is_refused_entirely_when_one_target_is_taken() {
        // FR-320 + FR-322. Kiểm TRƯỚC khi động vào file đầu tiên: phát hiện va
        // chạm ở mục thứ hai mà mục thứ nhất đã chuyển đi rồi thì người dùng
        // không còn cách nào quay lại.
        let (db, root) = test_db();
        let source_dir = root.join("nguon");
        let target_dir = root.join("dich");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::create_dir_all(&target_dir).unwrap();
        let first = add_real_item(&db, &source_dir, "mot.mp3", "Một", None);
        let second = add_real_item(&db, &source_dir, "hai.mp3", "Hai", None);
        std::fs::write(target_dir.join("hai.mp3"), b"da co san").unwrap();

        let err = move_items(
            &db,
            &[first.id.clone(), second.id.clone()],
            &target_dir.to_string_lossy(),
        )
        .unwrap_err();

        assert_eq!(err.code, FILE_EXISTS_ERROR_CODE);
        assert!(
            source_dir.join("mot.mp3").exists(),
            "mục đứng trước mục va chạm cũng không được phép di chuyển"
        );
        assert_eq!(
            std::fs::read(target_dir.join("hai.mp3")).unwrap(),
            b"da co san"
        );
    }

    #[test]
    fn moving_a_batch_takes_the_whole_selection_with_it() {
        let (db, root) = test_db();
        let source_dir = root.join("nguon");
        let target_dir = root.join("dich");
        std::fs::create_dir_all(&source_dir).unwrap();
        std::fs::create_dir_all(&target_dir).unwrap();
        let first = add_real_item(&db, &source_dir, "mot.mp3", "Một", None);
        let second = add_real_item(&db, &source_dir, "hai.mp3", "Hai", None);

        let moved = move_items(
            &db,
            &[first.id.clone(), second.id.clone()],
            &target_dir.to_string_lossy(),
        )
        .unwrap();

        assert_eq!(moved.len(), 2);
        assert!(target_dir.join("mot.mp3").exists());
        assert!(target_dir.join("hai.mp3").exists());
        for item in moved {
            assert_eq!(
                PathBuf::from(&item.file_path).parent().unwrap(),
                target_dir,
                "chỉ mục phải theo kịp vị trí mới (FR-319)"
            );
        }
    }

    #[tokio::test]
    async fn deleting_sends_the_file_to_the_system_trash_instead_of_unlinking() {
        // FR-318 + SC-305. Khẳng định quan trọng không phải "file biến khỏi
        // đường dẫn cũ" — `remove_file` cũng làm được đúng thế — mà là nó
        // XUẤT HIỆN trong thùng rác của hệ thống, tức người dùng lấy lại được.
        let (db, root) = test_db();
        let unique = format!("md-trash-test-{}.mp3", uuid::Uuid::new_v4());
        let item = add_real_item(&db, &root, &unique, "Xoá thử", None);

        let removed = delete_items(&db, std::slice::from_ref(&item.id))
            .await
            .unwrap();

        assert_eq!(removed, 1);
        assert!(
            !Path::new(&item.file_path).exists(),
            "file phải rời khỏi vị trí cũ"
        );
        assert!(
            db.library_item(&item.id).unwrap().is_none(),
            "mục phải biến khỏi thư viện"
        );
        let in_trash = system_trash_dir().join(&unique);
        assert!(
            in_trash.exists(),
            "file phải nằm trong thùng rác hệ thống ({}), không bị xoá vĩnh viễn",
            in_trash.to_string_lossy()
        );
        let _ = std::fs::remove_file(&in_trash);
    }

    #[cfg(target_os = "macos")]
    fn system_trash_dir() -> PathBuf {
        PathBuf::from(std::env::var("HOME").expect("HOME is set")).join(".Trash")
    }

    #[cfg(target_os = "linux")]
    fn system_trash_dir() -> PathBuf {
        PathBuf::from(std::env::var("HOME").expect("HOME is set")).join(".local/share/Trash/files")
    }

    #[cfg(target_os = "windows")]
    fn system_trash_dir() -> PathBuf {
        // Thùng rác của Windows không phơi ra một thư mục đọc được bằng tên
        // file gốc, nên trên nền tảng này chỉ khẳng định được vế thứ nhất.
        PathBuf::from("C:\\$Recycle.Bin")
    }

    #[tokio::test]
    async fn deleting_a_missing_item_still_clears_it_from_the_library() {
        // Không có gì để đưa vào thùng rác, nhưng "xoá" vẫn phải làm được một
        // việc gì đó — nếu không thì nút xoá trên một mục thiếu là một nút
        // chết.
        let (db, root) = test_db();
        let item = add_real_item(&db, &root, "bien-mat.mp3", "Biến mất", None);
        std::fs::remove_file(&item.file_path).unwrap();

        let removed = delete_items(&db, std::slice::from_ref(&item.id))
            .await
            .unwrap();

        assert_eq!(removed, 1);
        assert!(db.library_item(&item.id).unwrap().is_none());
    }

    #[test]
    fn the_playlist_keeps_the_order_it_was_given() {
        // FR-330. Danh sách được xin theo thứ tự ngược với thứ tự chèn, nên
        // một cài đặt sắp xếp lại theo bất kỳ tiêu chí nào của backend (ngày,
        // tên, rowid) đều sẽ cho ra thứ tự khác.
        let (db, root) = test_db();
        let first = add_real_item(&db, &root, "1.mp3", "Bài một", Some(61));
        let second = add_real_item(&db, &root, "2.mp3", "Bài hai", None);
        let third = add_real_item(&db, &root, "3.mp3", "Bài ba", Some(200));
        let destination = root.join("danh-sach.m3u");

        export_playlist(
            &db,
            &[third.id.clone(), first.id.clone(), second.id.clone()],
            &destination.to_string_lossy(),
        )
        .unwrap();

        let written = std::fs::read_to_string(&destination).unwrap();
        let lines: Vec<&str> = written.lines().collect();
        assert_eq!(lines[0], "#EXTM3U");
        assert_eq!(lines[1], "#EXTINF:200,Bài ba");
        assert_eq!(lines[2], third.file_path);
        assert_eq!(lines[3], "#EXTINF:61,Bài một");
        assert_eq!(lines[4], first.file_path);
        // Thời lượng chưa biết ghi `-1`, đúng quy ước M3U cho "không rõ" —
        // `0` sẽ bị trình phát hiểu là một file rỗng.
        assert_eq!(lines[5], "#EXTINF:-1,Bài hai");
        assert_eq!(lines[6], second.file_path);
    }

    #[test]
    fn exporting_onto_an_existing_file_is_refused() {
        let (db, root) = test_db();
        let item = add_real_item(&db, &root, "1.mp3", "Bài một", None);
        let destination = root.join("da-co.m3u");
        std::fs::write(&destination, b"noi dung cu").unwrap();

        let err = export_playlist(&db, &[item.id], &destination.to_string_lossy()).unwrap_err();

        assert_eq!(err.code, FILE_EXISTS_ERROR_CODE);
        assert_eq!(std::fs::read(&destination).unwrap(), b"noi dung cu");
    }

    #[test]
    fn relinking_needs_a_file_that_is_really_there() {
        let (db, root) = test_db();
        let item = add_real_item(&db, &root, "goc.mp3", "Gốc", None);

        let err =
            relink_item(&db, &item.id, &root.join("khong-co.mp3").to_string_lossy()).unwrap_err();

        assert_eq!(err.code, "NOT_FOUND");
        assert_eq!(
            db.library_item(&item.id).unwrap().unwrap().file_path,
            item.file_path
        );
    }

    #[test]
    fn relinking_points_the_item_at_the_file_the_user_found() {
        let (db, root) = test_db();
        let item = add_real_item(&db, &root, "goc.mp3", "Gốc", None);
        let elsewhere = root.join("noi-khac.mp3");
        std::fs::rename(&item.file_path, &elsewhere).unwrap();

        let relinked = relink_item(&db, &item.id, &elsewhere.to_string_lossy()).unwrap();

        assert_eq!(relinked.file_path, elsewhere.to_string_lossy());
        assert!(!relinked.is_missing);
    }

    #[test]
    fn observing_a_vanished_file_keeps_its_last_known_size() {
        // Ghi 0 vào dung lượng sẽ làm sai lệch thống kê của FR-328 suốt thời
        // gian một ổ đĩa ngoài chưa được cắm lại.
        let state = LibraryFileState {
            id: "x".to_string(),
            file_path: "/tmp/khong-bao-gio-ton-tai-c0ffee.mp3".to_string(),
            is_missing: false,
            file_size_bytes: 12_345,
        };

        let observed = observe(state);

        assert!(observed.is_missing);
        assert_eq!(observed.file_size_bytes, 12_345);
    }
}
