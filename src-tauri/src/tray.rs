//! Zone de notification, démarrage automatique et notifications Windows.
use crate::state::AppState;
use crate::windows;
use std::sync::atomic::Ordering;
use tauri::menu::{CheckMenuItemBuilder, MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_notification::NotificationExt as _;

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItemBuilder::with_id("open", "Ouvrir Oniris").build(app)?;
    let settings = MenuItemBuilder::with_id("settings", "Paramètres").build(app)?;
    let pause = CheckMenuItemBuilder::with_id("pause", "Mettre la voix en pause").build(app)?;
    let clip_on = app.state::<AppState>().settings.read().clipboard_enabled;
    let clipboard = CheckMenuItemBuilder::with_id("clipboard", "Historique du presse-papiers").checked(clip_on).build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quitter").build(app)?;
    let menu = MenuBuilder::new(app).items(&[&open, &settings, &pause, &clipboard, &quit]).build()?;
    let mut tray = TrayIconBuilder::with_id("oniris")
        .tooltip("Oniris")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, ev| match ev.id().as_ref() {
            // « Paramètres » : la section viendra au plan 7, on ouvre le panneau.
            "open" | "settings" => {
                if let Err(e) = windows::open_panel(app) {
                    tracing::error!("tray: ouverture du panneau : {e}");
                }
            }
            "pause" => {
                // Relu par le pipeline vocal à chaque appui.
                let on = pause.is_checked().unwrap_or(false);
                app.state::<AppState>().voice_paused.store(on, Ordering::SeqCst);
            }
            "clipboard" => {
                // Windows coche ou décoche l'élément avant l'événement : on enregistre son nouvel état, hors du
                // fil principal (la sauvegarde verrouille la base). Échec : l'élément revient à l'état enregistré.
                let on = clipboard.is_checked().unwrap_or(false);
                let (app, item) = (app.clone(), clipboard.clone());
                tauri::async_runtime::spawn_blocking(move || {
                    let mut s = app.state::<AppState>().settings.read().clone();
                    s.clipboard_enabled = on;
                    if let Err(e) = crate::commands::apply_settings(&app, s) {
                        tracing::error!("tray: historique du presse-papiers : {e}");
                        let _ = item.set_checked(!on);
                    }
                });
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, ev| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = ev {
                if let Err(e) = windows::toggle_panel(tray.app_handle()) {
                    tracing::error!("tray: bascule du panneau : {e}");
                }
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

/// Ne touche à l'OS que si l'état souhaité diffère de l'état courant.
fn autostart_action(desired: bool, current: bool) -> Option<bool> {
    (desired != current).then_some(desired)
}

/// Aligne l'entrée de démarrage automatique sur le réglage (source de vérité). Erreurs journalisées.
pub fn sync_autostart(app: &AppHandle, enabled: bool) {
    // Un build de dev ou de mesure inscrirait son exe de target/ dans la clé Run, qui lui survivrait.
    if cfg!(debug_assertions) || cfg!(feature = "perf") {
        static LOGGED: std::sync::Once = std::sync::Once::new();
        LOGGED.call_once(|| tracing::info!("autostart: ignoré en build de debug ou de mesure"));
        return;
    }
    let mgr = app.autolaunch();
    let current = match mgr.is_enabled() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("autostart: lecture de l'état : {e}");
            return;
        }
    };
    let res = match autostart_action(enabled, current) {
        Some(true) => mgr.enable(),
        Some(false) => mgr.disable(),
        None => return,
    };
    if let Err(e) = res {
        tracing::error!("autostart: mise à jour : {e}");
    }
}

pub fn notify(app: &AppHandle, title: &str, body: &str) {
    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        tracing::error!("notification: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::autostart_action;
    #[test]
    fn autostart_n_agit_que_si_different() {
        assert_eq!(autostart_action(true, false), Some(true));
        assert_eq!(autostart_action(false, true), Some(false));
        assert_eq!(autostart_action(true, true), None);
        assert_eq!(autostart_action(false, false), None);
    }
}
