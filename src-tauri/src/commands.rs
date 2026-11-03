use crate::windows;
use tauri::AppHandle;

#[tauri::command]
pub fn panel_open(app: AppHandle) -> tauri::Result<()> {
    windows::open_panel(&app)
}

#[tauri::command]
pub fn panel_close(app: AppHandle) -> tauri::Result<()> {
    windows::close_panel(&app)
}

#[tauri::command]
pub fn panel_toggle(app: AppHandle) -> tauri::Result<()> {
    windows::toggle_panel(&app)
}

#[tauri::command]
pub fn pill_resize(app: AppHandle, width: f64) -> tauri::Result<()> {
    windows::place_pill(&app, width)
}
