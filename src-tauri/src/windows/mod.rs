pub mod geometry;

use crate::state::AppState;
use geometry::{panel_rect, pill_rect, Rect};
use std::sync::atomic::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewWindow};

#[derive(Clone, serde::Serialize)]
struct PanelState {
    open: bool,
    at_ms: u64,
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

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
    app.emit("oniris://panel-state", PanelState { open, at_ms: now_ms() })
}

pub fn open_panel(app: &AppHandle) -> tauri::Result<()> {
    let win = window(app, "panel")?;
    apply(&win, panel_rect(primary(app)?.0))?;
    win.show()?;
    win.set_focus()?;
    set_panel_state(app, true)
}

pub fn close_panel(app: &AppHandle) -> tauri::Result<()> {
    window(app, "panel")?.hide()?;
    set_panel_state(app, false)
}

pub fn toggle_panel(app: &AppHandle) -> tauri::Result<()> {
    if app.state::<AppState>().panel_open.load(Ordering::SeqCst) {
        close_panel(app)
    } else {
        open_panel(app)
    }
}
