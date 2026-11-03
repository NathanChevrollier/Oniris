mod commands;
mod hotkey;
mod state;
mod windows;

use parking_lot::RwLock;
use std::sync::Arc;
use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|_, _, _| {}))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![
            commands::panel_open,
            commands::panel_close,
            commands::panel_toggle,
            commands::pill_resize
        ])
        .setup(|app| {
            app.manage(state::AppState::default());
            windows::show_pill(app.handle())?;
            hotkey::start_hook(
                app.handle().clone(),
                Arc::new(RwLock::new(hotkey::HotkeyConfig::default())),
            );
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("erreur au lancement d'Oniris");
}
