mod commands;
mod events;
mod fullscreen;
mod hotkey;
mod job;
mod launch;
mod logging;
pub mod mcp_relay;
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

/// Côté app du serveur MCP, pour les tests d'intégration du relais.
#[doc(hidden)]
pub use modules::iris_mcp;

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
            modules::sites::sites_list,
            modules::sites::sites_set,
            modules::zenytt::vps_status,
            modules::zenytt::sites_import_from_zenytt,
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
            modules::iris_briefing::iris_briefing,
            modules::iris_briefing::iris_briefing_act,
            modules::iris::iris_status,
            modules::iris::iris_draft_reply,
            modules::iris::iris_draft_reply_local,
            modules::iris::iris_pull_model,
            modules::iris::iris_history,
            modules::iris::iris_history_clear,
            modules::iris::iris_memories,
            modules::iris::iris_memory_delete,
            modules::iris::iris_conversation_new,
            modules::iris::iris_ask,
            modules::iris::iris_cancel,
            modules::voice::install::voice_status,
            modules::voice::install::voice_install,
            modules::voice::capture::voice_devices,
            modules::mail::mail_open_web,
            modules::mail::mail_open_link,
            modules::mail::mail_status,
            modules::smtc::music_now,
            modules::smtc::music_toggle,
            modules::smtc::music_next,
            modules::smtc::music_previous,
            modules::smtc::music_seek,
            modules::music::music_playlists,
            modules::music::music_set_favorites,
            modules::music::music_devices,
            modules::music::music_play_playlist,
            modules::music::music_play_search,
            modules::music::music_volume,
            modules::music::music_queue,
            modules::music::music_transfer,
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
            let mut st = match state::AppState::open(app.handle()) {
                Ok(st) => st,
                Err(e) => {
                    tracing::error!("démarrage impossible : {e}");
                    use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
                    app.dialog()
                        .message(format!(
                            "Oniris n'a pas pu démarrer : {}. Le journal est dans {}.",
                            startup_error_kind(e.as_ref()),
                            logging::log_dir().display()
                        ))
                        .title("Oniris")
                        .kind(MessageDialogKind::Error)
                        .blocking_show();
                    return Err(e);
                }
            };
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
            modules::iris_briefing::load(app.handle());
            modules::mail::start(app.handle());
            modules::sites::start(app.handle());
            modules::favicons::spawn(app.handle());
            modules::apps::start(app.handle());
            modules::projects::refresh(app.handle());
            let h = app.handle().clone();
            tauri::async_runtime::spawn_blocking(move || modules::indexer::start(&h));
            tray::build(app.handle())?;
            tray::sync_autostart(app.handle(), autostart);
            modules::voice::pipeline::start(app.handle());
            hotkey::start_hook(app.handle().clone(), hotkey_cfg);
            fullscreen::start_watcher(app.handle().clone());
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("erreur au lancement d'Oniris")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                if let Some(st) = app.try_state::<state::AppState>() {
                    // Au plus 2 s pour le verrou puis 2 s pour le checkpoint.
                    if let Some(conn) = st.db.try_lock_for(std::time::Duration::from_secs(2)) {
                        if let Err(e) = oniris_core::checkpoint(&conn) {
                            tracing::warn!("checkpoint du WAL impossible : {e}");
                        }
                    }
                }
                modules::zenytt::shutdown(); // Zenytt ne survit pas à Oniris
            }
        });
}

/// Catégorie fixe affichée à l'utilisateur ; jamais le texte brut de l'erreur (chemins, SQL).
pub fn startup_error_kind(e: &(dyn std::error::Error + 'static)) -> &'static str {
    use rusqlite::ErrorCode::*;
    let mut cur = Some(e);
    while let Some(err) = cur {
        if let Some(rusqlite::Error::SqliteFailure(f, _)) = err.downcast_ref::<rusqlite::Error>() {
            match f.code {
                DatabaseBusy | DatabaseLocked => return "base de données verrouillée",
                DatabaseCorrupt | NotADatabase => return "base de données endommagée",
                CannotOpen | ReadOnly | PermissionDenied => return "dossier inaccessible",
                _ => {}
            }
        }
        if let Some(io) = err.downcast_ref::<std::io::Error>() {
            if matches!(io.kind(), std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::NotFound) {
                return "dossier inaccessible";
            }
        }
        cur = err.source();
    }
    "erreur inattendue"
}

#[cfg(test)]
mod tests {
    use super::startup_error_kind;
    use rusqlite::ffi;

    fn sqlite(code: i32) -> rusqlite::Error {
        rusqlite::Error::SqliteFailure(ffi::Error::new(code), None)
    }
    #[test]
    fn categories_d_erreur_de_demarrage() {
        assert_eq!(startup_error_kind(&sqlite(ffi::SQLITE_BUSY)), "base de données verrouillée");
        assert_eq!(startup_error_kind(&sqlite(ffi::SQLITE_CORRUPT)), "base de données endommagée");
        assert_eq!(startup_error_kind(&sqlite(ffi::SQLITE_NOTADB)), "base de données endommagée");
        let denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        assert_eq!(startup_error_kind(&denied), "dossier inaccessible");
        let wrapped = oniris_core::CoreError::Io(denied);
        assert_eq!(startup_error_kind(&wrapped), "dossier inaccessible");
        assert_eq!(startup_error_kind(&oniris_core::CoreError::Invalid("x".into())), "erreur inattendue");
    }

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
            ("iris.rs", include_str!("modules/iris.rs")),
            ("iris_briefing.rs", include_str!("modules/iris_briefing.rs")),
            ("mail.rs", include_str!("modules/mail.rs")),
            ("music.rs", include_str!("modules/music.rs")),
            ("notes.rs", include_str!("modules/notes.rs")),
            ("projects.rs", include_str!("modules/projects.rs")),
            ("reminders.rs", include_str!("modules/reminders.rs")),
            ("search.rs", include_str!("modules/search.rs")),
            ("sites.rs", include_str!("modules/sites.rs")),
            ("smtc.rs", include_str!("modules/smtc.rs")),
            ("system.rs", include_str!("modules/system.rs")),
            ("timers.rs", include_str!("modules/timers.rs")),
            ("voice/capture.rs", include_str!("modules/voice/capture.rs")),
            ("voice/install.rs", include_str!("modules/voice/install.rs")),
            ("weather.rs", include_str!("modules/weather.rs")),
            ("zenytt.rs", include_str!("modules/zenytt.rs")),
        ];
        let touches_db = |body: &str| body.contains("db.lock") || body.contains(".journal(");
        for (file, src) in sources {
            for cmd in src.split("#[tauri::command]").skip(1) {
                let body = &cmd[..cmd.find("\n}").unwrap_or(cmd.len())];
                let head = body.trim_start().lines().next().unwrap_or("");
                assert!(!(head.starts_with("pub fn") && touches_db(body)), "{file} : commande synchrone qui touche la base : {head}");
            }
        }
        for module in [include_str!("modules/weather.rs"), include_str!("modules/mail.rs"), include_str!("modules/music.rs"), include_str!("modules/iris_briefing.rs")] {
            let open = module.split("pub fn on_panel_open").nth(1).expect("on_panel_open");
            assert!(!touches_db(&open[..open.find("\n}").unwrap()]), "on_panel_open ne doit pas verrouiller la base");
        }
    }
}
