mod commands;
mod fullscreen;
mod hotkey;
mod state;
mod windows;

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
            commands::pill_resize,
            commands::settings_get,
            commands::settings_save
        ])
        .setup(|app| {
            let st = state::AppState::open()?;
            let hotkey_cfg = st.hotkey.clone();
            app.manage(st);
            windows::show_pill(app.handle())?;
            hotkey::start_hook(app.handle().clone(), hotkey_cfg);
            fullscreen::start_watcher(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("erreur au lancement d'Oniris");
}
