mod commands;
mod events;
mod fullscreen;
mod hotkey;
mod launch;
mod logging;
mod modules;
mod oauth_loopback;
#[cfg(any(debug_assertions, feature = "perf"))]
mod perf;
mod scheduler;
mod secrets_win;
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
        .plugin(tauri_plugin_dialog::init())
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
            modules::clipboard::clipboard_list,
            modules::clipboard::clipboard_pin,
            modules::clipboard::clipboard_delete,
            modules::clipboard::clipboard_paste,
            modules::favorites::favorites_list,
            modules::favorites::favorites_reorder,
            modules::favorites::favorites_rename,
            modules::favorites::favorites_add,
            modules::favorites::favorites_delete,
            modules::favorites::favorites_open,
            modules::favorites::favorites_import_browsers,
            modules::favorites::favorites_import_file,
            modules::projects::projects_list,
            modules::projects::projects_pin,
            modules::projects::projects_hide,
            modules::projects::projects_set_roots,
            modules::projects::projects_resume,
            modules::weather::weather_get,
            modules::accounts::accounts_list,
            modules::accounts::accounts_set_google_app,
            modules::accounts::accounts_set_spotify_app,
            modules::accounts::accounts_connect,
            modules::accounts::accounts_cancel,
            modules::accounts::accounts_remove,
            modules::accounts::accounts_set_color,
            modules::accounts::accounts_intro_take,
            modules::accounts::accounts_open_guide,
            modules::mail::mail_list,
            modules::mail::mail_read,
            modules::mail::mail_set_read,
            modules::mail::mail_archive,
            modules::mail::mail_delete,
            modules::mail::mail_send,
            modules::mail::mail_open_web,
            modules::mail::mail_open_link,
            modules::mail::mail_status,
            modules::smtc::music_now,
            modules::smtc::music_toggle,
            modules::smtc::music_next,
            modules::smtc::music_previous,
            modules::smtc::music_seek,
            modules::indexer::index_status,
            modules::search::search_query,
            modules::search::search_open,
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
            // La pilule d'abord : rattrapages et purge partent sur d'autres fils.
            // Pas encore d'écran (session qui s'ouvre) : on continue sans pilule plutôt que d'échouer.
            if let Err(e) = windows::show_pill(app.handle()) {
                tracing::error!("pilule : affichage au démarrage impossible : {e}");
            }
            modules::reminders::start(app.handle());
            modules::timers::start(app.handle());
            modules::clipboard::start(app.handle());
            modules::weather::load(app.handle());
            modules::mail::start(app.handle());
            modules::favicons::spawn(app.handle());
            modules::apps::start(app.handle());
            modules::projects::refresh(app.handle());
            let h = app.handle().clone();
            tauri::async_runtime::spawn_blocking(move || modules::indexer::start(&h));
            tray::build(app.handle())?;
            tray::sync_autostart(app.handle(), autostart);
            hotkey::start_hook(app.handle().clone(), hotkey_cfg);
            fullscreen::start_watcher(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("erreur au lancement d'Oniris");
}

#[cfg(test)]
mod tests {
    /// Une commande synchrone tourne sur le fil principal (celui des fenêtres) : toute commande qui verrouille
    /// la base doit être `#[tauri::command(async)]` ou `async fn`. Même règle pour l'ouverture du panneau.
    #[test]
    fn la_base_n_est_jamais_verrouillee_sur_le_fil_principal() {
        let sources = [
            ("commands.rs", include_str!("commands.rs")),
            ("accounts.rs", include_str!("modules/accounts.rs")),
            ("agenda.rs", include_str!("modules/agenda.rs")),
            ("clipboard.rs", include_str!("modules/clipboard.rs")),
            ("favorites.rs", include_str!("modules/favorites.rs")),
            ("mail.rs", include_str!("modules/mail.rs")),
            ("notes.rs", include_str!("modules/notes.rs")),
            ("projects.rs", include_str!("modules/projects.rs")),
            ("reminders.rs", include_str!("modules/reminders.rs")),
            ("search.rs", include_str!("modules/search.rs")),
            ("smtc.rs", include_str!("modules/smtc.rs")),
            ("system.rs", include_str!("modules/system.rs")),
            ("timers.rs", include_str!("modules/timers.rs")),
            ("weather.rs", include_str!("modules/weather.rs")),
        ];
        let touches_db = |body: &str| body.contains("db.lock") || body.contains(".journal(");
        for (file, src) in sources {
            for cmd in src.split("#[tauri::command]").skip(1) {
                let body = &cmd[..cmd.find("\n}").unwrap_or(cmd.len())];
                let head = body.trim_start().lines().next().unwrap_or("");
                assert!(!(head.starts_with("pub fn") && touches_db(body)), "{file} : commande synchrone qui touche la base : {head}");
            }
        }
        for module in [include_str!("modules/weather.rs"), include_str!("modules/mail.rs")] {
            let open = module.split("pub fn on_panel_open").nth(1).expect("on_panel_open");
            assert!(!touches_db(&open[..open.find("\n}").unwrap()]), "on_panel_open ne doit pas verrouiller la base");
        }
    }
}
