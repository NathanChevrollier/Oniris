pub mod backdrop;
pub mod geometry;

use crate::state::AppState;
use geometry::{panel_rect, pill_rect, Rect};
use std::sync::atomic::{AtomicU64, Ordering};
use oniris_core::time_util::now_ms;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewWindow};

#[derive(Clone, serde::Serialize)]
struct PanelState {
    open: bool,
    at_ms: u64,
}

/// Délai avant de fermer sur perte de focus : un clic sur la zone de notification le reprend un instant.
const BLUR_DEBOUNCE: Duration = Duration::from_millis(150);
/// Une bascule juste après une fermeture ne rouvre pas : le clic (pilule, zone de notification) qui a fait
/// perdre le focus au panneau arrive après sa fermeture.
const REOPEN_GUARD_MS: u64 = 400;
static LAST_CLOSE_MS: AtomicU64 = AtomicU64::new(0);
/// Fermeture animée (panel.css, ~190 ms) : la fenêtre se masque ensuite. Marge pour la livraison de l'événement.
const CLOSE_ANIM: Duration = Duration::from_millis(220);
/// Incrémenté à chaque ouverture et fermeture : un masquage différé périmé (panneau rouvert entre-temps) s'abstient.
static PANEL_GEN: AtomicU64 = AtomicU64::new(0);

/// Écran principal (repli : premier écran disponible) : rectangle physique + échelle.
fn primary(app: &AppHandle) -> tauri::Result<(Rect, f64)> {
    let m = match app.primary_monitor()? {
        Some(m) => m,
        None => app
            .available_monitors()?
            .into_iter()
            .next()
            .ok_or_else(|| tauri::Error::WindowNotFound)?,
    };
    let (p, s) = (m.position(), m.size());
    Ok((Rect { x: p.x, y: p.y, w: s.width, h: s.height }, m.scale_factor()))
}

fn window(app: &AppHandle, label: &str) -> tauri::Result<WebviewWindow> {
    app.get_webview_window(label).ok_or(tauri::Error::WindowNotFound)
}

fn apply(win: &WebviewWindow, r: Rect) -> tauri::Result<()> {
    win.set_size(PhysicalSize::new(r.w, r.h))?;
    win.set_position(PhysicalPosition::new(r.x, r.y))
}

pub fn place_pill(app: &AppHandle, width_logical: f64, asleep: bool) -> tauri::Result<()> {
    let (m, scale) = primary(app)?;
    apply(&window(app, "pill")?, pill_rect(m, scale, width_logical, asleep))
}

pub fn show_pill(app: &AppHandle) -> tauri::Result<()> {
    place_pill(app, 420.0, false)?;
    window(app, "pill")?.show()
}

fn set_panel_state(app: &AppHandle, open: bool) -> tauri::Result<()> {
    app.state::<AppState>().panel_open.store(open, Ordering::SeqCst);
    app.emit("oniris://panel-state", PanelState { open, at_ms: now_ms() as u64 })
}

/// Fenêtre au premier plan si elle n'est pas à nous (0 sinon).
fn foreign_foreground() -> isize {
    use ::windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    let fg = unsafe { GetForegroundWindow() };
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(fg, Some(&mut pid)) };
    if fg.0.is_null() || pid == std::process::id() { 0 } else { fg.0 as isize }
}

pub fn open_panel(app: &AppHandle) -> tauri::Result<()> {
    let win = window(app, "panel")?;
    let state = app.state::<AppState>();
    // Avant de prendre le focus : cible du collage. Panneau déjà ouvert : la cible déjà notée reste.
    if !state.panel_open.load(Ordering::SeqCst) {
        state.prev_foreground.store(foreign_foreground(), Ordering::SeqCst);
    }
    PANEL_GEN.fetch_add(1, Ordering::SeqCst);
    let m = primary(app)?.0;
    // Fenêtre déjà visible (fermeture en cours) : la capture verrait le panneau, la valeur précédente reste.
    if state.settings.read().glass_mode == "auto" && !win.is_visible().unwrap_or(true) {
        backdrop::sample(app, m);
    }
    apply(&win, panel_rect(m))?;
    win.show()?;
    win.set_focus()?;
    set_panel_state(app, true)?;
    crate::modules::system::set_sampling(app, true);
    crate::modules::weather::on_panel_open(app);
    crate::modules::iris_briefing::on_panel_open(app);
    crate::modules::mail::on_panel_open(app);
    crate::modules::music::on_panel_open(app);
    crate::modules::apps::on_panel_open(app);
    crate::modules::projects::on_panel_open(app);
    crate::modules::smtc::set_listening(app, true);
    crate::modules::zenytt::on_panel_open(app);
    Ok(())
}

/// Animations coupées (réglage d'Oniris ou « Afficher les animations » de Windows, qui pilote prefers-reduced-motion).
fn motion_reduced(app: &AppHandle) -> bool {
    use ::windows::Win32::UI::WindowsAndMessaging::{SystemParametersInfoW, SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS};
    let mut on = ::windows::core::BOOL(1);
    let ok = unsafe { SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, Some((&mut on as *mut ::windows::core::BOOL).cast()), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)) };
    app.state::<AppState>().settings.read().reduce_motion || (ok.is_ok() && !on.as_bool())
}

/// État « fermé » émis tout de suite (le panneau joue sa remontée), fenêtre masquée après `CLOSE_ANIM`
/// sauf réouverture entre-temps.
pub fn close_panel(app: &AppHandle) -> tauri::Result<()> {
    crate::modules::system::set_sampling(app, false);
    crate::modules::smtc::set_listening(app, false);
    let win = window(app, "panel")?;
    let gen = PANEL_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    if motion_reduced(app) {
        win.hide()?;
    } else {
        let a = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(CLOSE_ANIM);
            // Sur le fil principal, comme `show` : une ouverture passée avant ce point incrémente PANEL_GEN.
            let _ = a.clone().run_on_main_thread(move || {
                if PANEL_GEN.load(Ordering::SeqCst) == gen {
                    if let Err(e) = window(&a, "panel").and_then(|w| w.hide()) {
                        tracing::error!("oniris: masquage du panneau : {e}");
                    }
                }
            });
        });
    }
    LAST_CLOSE_MS.store(now_ms() as u64, Ordering::SeqCst);
    let r = set_panel_state(app, false);
    crate::modules::zenytt::on_panel_close(); // après l'état « fermé » : le compte des 10 min démarre
    r
}

fn toggle_reopens(now: u64, last_close: u64) -> bool {
    now.saturating_sub(last_close) >= REOPEN_GUARD_MS
}

pub fn toggle_panel(app: &AppHandle) -> tauri::Result<()> {
    if app.state::<AppState>().panel_open.load(Ordering::SeqCst) {
        close_panel(app)
    } else if toggle_reopens(now_ms() as u64, LAST_CLOSE_MS.load(Ordering::SeqCst)) {
        open_panel(app)
    } else {
        Ok(())
    }
}

/// Le panneau a perdu le focus : fermé s'il ne l'a pas retrouvé après `BLUR_DEBOUNCE`.
pub fn close_on_blur(app: AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(BLUR_DEBOUNCE);
        let refocused = window(&app, "panel").and_then(|w| w.is_focused()).unwrap_or(false);
        if !refocused && app.state::<AppState>().panel_open.load(Ordering::SeqCst) {
            if let Err(e) = close_panel(&app) {
                tracing::error!("oniris: fermeture du panneau sur perte de focus : {e}");
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bascule_juste_apres_fermeture_ne_rouvre_pas() {
        assert!(!toggle_reopens(10_000, 10_000 - REOPEN_GUARD_MS + 1));
        assert!(toggle_reopens(10_000, 10_000 - REOPEN_GUARD_MS));
        assert!(toggle_reopens(10_000, 0));
    }
}
