use std::sync::Arc;

use tauri::State;

use crate::db::{Db, Preset};
use crate::error::AppError;
use crate::models::OutputOptions;

/// Toàn bộ preset đã lưu (FR-228). Bản ghi trả về nguyên vẹn thứ đã lưu, không
/// lọc theo nguồn nào — giao diện cần đúng dữ liệu đó để làm FR-231: đối chiếu
/// mức chất lượng trong preset với danh sách format thật của nguồn hiện tại,
/// chọn mức gần nhất và nói rõ đã đổi gì.
///
/// Cờ mặc định đi kèm từng bản ghi, nên nơi xem trước tự tìm được preset mặc
/// định (FR-230) mà không cần một lệnh riêng.
#[tauri::command]
pub fn list_presets(db: State<Arc<Db>>) -> Result<Vec<Preset>, AppError> {
    db.list_presets()
}

#[tauri::command]
pub fn create_preset(
    db: State<Arc<Db>>,
    name: String,
    options: OutputOptions,
) -> Result<Preset, AppError> {
    db.create_preset(&name, &options)
}

#[tauri::command]
pub fn rename_preset(
    db: State<Arc<Db>>,
    preset_id: String,
    name: String,
) -> Result<Preset, AppError> {
    db.rename_preset(&preset_id, &name)
}

/// Ghi đè cả bộ tuỳ chọn của preset (FR-229). Không phải một bản vá từng
/// trường: bên gọi gửi lên nguyên một `OutputOptions`.
#[tauri::command]
pub fn update_preset(
    db: State<Arc<Db>>,
    preset_id: String,
    options: OutputOptions,
) -> Result<Preset, AppError> {
    db.update_preset_options(&preset_id, &options)
}

#[tauri::command]
pub fn delete_preset(db: State<Arc<Db>>, preset_id: String) -> Result<(), AppError> {
    db.delete_preset(&preset_id)
}

#[tauri::command]
pub fn set_default_preset(db: State<Arc<Db>>, preset_id: String) -> Result<Preset, AppError> {
    db.set_default_preset(&preset_id)
}
