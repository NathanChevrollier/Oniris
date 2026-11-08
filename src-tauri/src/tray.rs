//! Zone de notification, démarrage automatique et notifications Windows.
use crate::modules::onboarding::{self, autostart_target, Onboarding};
use crate::state::AppState;
use crate::windows;
use std::sync::atomic::Ordering;
use tauri::menu::{CheckMenuItemBuilder, MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Listener, Manager};
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
    // Un enregistrement fait depuis le panneau (ou ailleurs) : les cases cochées suivent l'état réel.
    let (pause_item, clipboard_item, handle) = (pause.clone(), clipboard.clone(), app.clone());
    app.listen("oniris://settings", move |_| {
        let st = handle.state::<AppState>();
        let (clip, paused) = menu_checks(&st.settings.read(), st.voice_paused.load(Ordering::SeqCst));
        let _ = clipboard_item.set_checked(clip);
        let _ = pause_item.set_checked(paused);
    });
    let mut tray = TrayIconBuilder::with_id("oniris")
        .tooltip("Oniris")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, ev| match ev.id().as_ref() {
            // « Paramètres » ouvre le panneau, où la feuille Paramètres est un bouton de l'en-tête.
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

/// Cases cochées du menu (presse-papiers, voix en pause) pour des réglages et un état de pause donnés.
fn menu_checks(s: &oniris_core::Settings, voice_paused: bool) -> (bool, bool) {
    (s.clipboard_enabled, voice_paused)
}

/// Ne touche à l'OS que si l'état souhaité diffère de l'état courant.
fn autostart_action(desired: bool, current: bool) -> Option<bool> {
    (desired != current).then_some(desired)
}

/// Entrée de démarrage automatique de l'OS (clé Run) ; un faux la remplace dans les tests.
pub trait Autostart {
    fn is_enabled(&self) -> Result<bool, String>;
    fn enable(&self) -> Result<(), String>;
    fn disable(&self) -> Result<(), String>;
}

struct PluginAutostart<'a>(&'a AppHandle);

impl Autostart for PluginAutostart<'_> {
    fn is_enabled(&self) -> Result<bool, String> {
        self.0.autolaunch().is_enabled().map_err(|e| e.to_string())
    }
    fn enable(&self) -> Result<(), String> {
        self.0.autolaunch().enable().map_err(|e| e.to_string())
    }
    fn disable(&self) -> Result<(), String> {
        self.0.autolaunch().disable().map_err(|e| e.to_string())
    }
}

/// Aligne l'entrée sur le réglage, mais seulement si l'accueil est terminé avec un choix explicite.
fn sync_with(a: &dyn Autostart, ob: &Onboarding, setting: bool) -> Result<(), String> {
    let Some(target) = autostart_target(ob, setting) else { return Ok(()) };
    match autostart_action(target, a.is_enabled()?) {
        Some(true) => a.enable(),
        Some(false) => a.disable(),
        None => Ok(()),
    }
}

/// Aligne l'entrée de démarrage automatique sur le réglage (source de vérité). Erreurs journalisées.
pub fn sync_autostart(app: &AppHandle, enabled: bool) {
    // Un build de dev ou de mesure inscrirait son exe de target/ dans la clé Run, qui lui survivrait.
    if cfg!(debug_assertions) || cfg!(feature = "perf") {
        static LOGGED: std::sync::Once = std::sync::Once::new();
        LOGGED.call_once(|| tracing::info!("autostart: ignoré en build de debug ou de mesure"));
        return;
    }
    let ob = onboarding::load(&app.state::<AppState>().db.lock());
    if let Err(e) = sync_with(&PluginAutostart(app), &ob, enabled) {
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
    use super::{autostart_action, menu_checks, sync_with, Autostart};
    use crate::modules::onboarding::Onboarding;
    use std::cell::{Cell, RefCell};

    /// Faux registre : compte les écritures, ne touche jamais à la vraie clé Run.
    #[derive(Default)]
    struct Fake {
        on: Cell<bool>,
        writes: RefCell<Vec<&'static str>>,
    }
    impl Autostart for Fake {
        fn is_enabled(&self) -> Result<bool, String> {
            Ok(self.on.get())
        }
        fn enable(&self) -> Result<(), String> {
            self.on.set(true);
            self.writes.borrow_mut().push("enable");
            Ok(())
        }
        fn disable(&self) -> Result<(), String> {
            self.on.set(false);
            self.writes.borrow_mut().push("disable");
            Ok(())
        }
    }
    fn ob(done: bool, consent: Option<bool>) -> Onboarding {
        Onboarding { step: 5, done, autostart_consent: consent }
    }
    #[test]
    fn rien_n_est_ecrit_tant_que_l_accueil_ou_le_consentement_manque() {
        for o in [ob(false, Some(true)), ob(false, None), ob(true, None)] {
            let f = Fake::default();
            sync_with(&f, &o, true).unwrap();
            assert!(f.writes.borrow().is_empty(), "{o:?}");
        }
        let f = Fake::default();
        f.on.set(true); // une entrée déjà là reste intacte aussi
        sync_with(&f, &ob(false, Some(false)), false).unwrap();
        assert!(f.writes.borrow().is_empty() && f.on.get());
    }
    #[test]
    fn oui_ecrit_non_retire() {
        let f = Fake::default();
        sync_with(&f, &ob(true, Some(true)), true).unwrap();
        assert_eq!(*f.writes.borrow(), ["enable"]);
        sync_with(&f, &ob(true, Some(true)), true).unwrap(); // déjà en place : aucune écriture
        assert_eq!(f.writes.borrow().len(), 1);
        sync_with(&f, &ob(true, Some(false)), false).unwrap();
        assert_eq!(*f.writes.borrow(), ["enable", "disable"]);
    }
    #[test]
    fn le_menu_suit_les_reglages_enregistres() {
        let mut s = oniris_core::Settings::default();
        assert_eq!(menu_checks(&s, false), (true, false));
        s.clipboard_enabled = false;
        assert_eq!(menu_checks(&s, true), (false, true));
    }
    #[test]
    fn autostart_n_agit_que_si_different() {
        assert_eq!(autostart_action(true, false), Some(true));
        assert_eq!(autostart_action(false, true), Some(false));
        assert_eq!(autostart_action(true, true), None);
        assert_eq!(autostart_action(false, false), None);
    }
}
