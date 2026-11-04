mod commands;
mod events;
mod fullscreen;
mod hotkey;
mod logging;
mod modules;
#[cfg(any(debug_assertions, feature = "perf"))]
mod perf;
mod scheduler;
mod state;
mod tray;
mod windows;

use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            // Second lancement pendant le démarrage : état pas encore géré, rien à ouvrir.
            if app.try_state::<state::AppState>().is_none() {
                return;
            }
            if let Err(e) = windows::open_panel(app) {
                tracing::error!("instance unique : {e}");
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
            modules::agenda::agenda_between,
            modules::agenda::agenda_month_days,
            modules::agenda::agenda_create,
            modules::agenda::agenda_delete,
            modules::reminders::reminders_list,
            modules::reminders::reminders_create,
            modules::reminders::reminders_complete,
            modules::reminders::reminders_delete,
            modules::notes::notes_list,
            modules::notes::notes_create,
            modules::notes::notes_pin,
            modules::notes::notes_delete,
            modules::notes::notes_to_reminder,
            modules::timers::timers_list,
            modules::timers::timers_start,
            modules::timers::focus_start,
            modules::timers::timers_cancel,
            #[cfg(any(debug_assertions, feature = "perf"))]
            perf::perf_frame,
            #[cfg(any(debug_assertions, feature = "perf"))]
            perf::perf_open_cycles
        ])
        .on_window_event(|window, event| {
            if window.label() == "panel" && matches!(event, tauri::WindowEvent::Focused(false)) {
                windows::close_on_blur(window.app_handle().clone());
            }
        })
        .setup(|app| {
            let guard = logging::init(&logging::log_dir());
            let mut st = state::AppState::open(app.handle()).inspect_err(|e| tracing::error!("ouverture de la base impossible : {e}"))?;
            st._log_guard = guard;
            let hotkey_cfg = st.hotkey.clone();
            let autostart = st.settings.read().autostart;
            app.manage(st);
            let _ = app.state::<state::AppState>().scheduler.set(scheduler::Scheduler::start(app.handle().clone()));
            modules::reminders::start(app.handle());
            modules::timers::start(app.handle());
            // Pas encore d'écran (session qui s'ouvre) : on continue sans pilule plutôt que d'échouer.
            if let Err(e) = windows::show_pill(app.handle()) {
                tracing::error!("pilule : affichage au démarrage impossible : {e}");
            }
            tray::build(app.handle())?;
            tray::sync_autostart(app.handle(), autostart);
            hotkey::start_hook(app.handle().clone(), hotkey_cfg);
            fullscreen::start_watcher(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("erreur au lancement d'Oniris");
}
