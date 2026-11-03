mod commands;
mod fullscreen;
mod hotkey;
#[cfg(any(debug_assertions, feature = "perf"))]
mod perf;
mod state;
mod tray;
mod windows;

use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Err(e) = windows::open_panel(app) {
                eprintln!("instance unique : {e}");
            }
        }))
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
            commands::settings_save,
            commands::tool_resolve,
            commands::tool_undo,
            commands::journal_list,
            #[cfg(any(debug_assertions, feature = "perf"))]
            perf::perf_frame,
            #[cfg(any(debug_assertions, feature = "perf"))]
            perf::perf_open_cycles
        ])
        .setup(|app| {
            let st = state::AppState::open()?;
            let hotkey_cfg = st.hotkey.clone();
            let autostart = st.settings.read().autostart;
            app.manage(st);
            windows::show_pill(app.handle())?;
            tray::build(app.handle())?;
            tray::sync_autostart(app.handle(), autostart);
            hotkey::start_hook(app.handle().clone(), hotkey_cfg);
            fullscreen::start_watcher(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("erreur au lancement d'Oniris");
}
