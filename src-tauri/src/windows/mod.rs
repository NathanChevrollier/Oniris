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

pub fn place_pill(app: &AppHandle, width_logical: f64) -> tauri::Result<()> {
    let (m, scale) = primary(app)?;
    apply(&window(app, "pill")?, pill_rect(m, scale, width_logical))
}

pub fn show_pill(app: &AppHandle) -> tauri::Result<()> {
    place_pill(app, 420.0)?;
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
    apply(&win, panel_rect(primary(app)?.0))?;
    win.show()?;
    win.set_focus()?;
    set_panel_state(app, true)?;
    crate::modules::system::set_sampling(app, true);
    crate::modules::weather::on_panel_open(app);
    crate::modules::mail::on_panel_open(app);
    crate::modules::apps::on_panel_open(app);
    crate::modules::projects::on_panel_open(app);
    crate::modules::smtc::set_listening(app, true);
    Ok(())
}

pub fn close_panel(app: &AppHandle) -> tauri::Result<()> {
    crate::modules::system::set_sampling(app, false);
    crate::modules::smtc::set_listening(app, false);
    window(app, "panel")?.hide()?;
    LAST_CLOSE_MS.store(now_ms() as u64, Ordering::SeqCst);
    set_panel_state(app, false)
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
