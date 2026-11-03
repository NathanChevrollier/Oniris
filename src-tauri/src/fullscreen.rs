//! Détection du plein écran par crochets WinEvent (aucune interrogation périodique), sur un thread dédié
//! qui possède sa boucle de messages. Émet `oniris://fullscreen` seulement quand l'état change.

use crate::state::AppState;
use crate::windows::geometry::Rect;
use ::windows::Win32::Foundation::{HMODULE, HWND, RECT};
use ::windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use ::windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use ::windows::Win32::UI::Shell::{
    SHQueryUserNotificationState, QUNS_BUSY, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
};
use ::windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetForegroundWindow, GetMessageW, GetWindowRect, EVENT_OBJECT_LOCATIONCHANGE,
    EVENT_SYSTEM_FOREGROUND, MSG, OBJID_WINDOW, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use tauri::{AppHandle, Emitter, Manager};

static APP: OnceLock<AppHandle> = OnceLock::new();
static ACTIVE: AtomicBool = AtomicBool::new(false);

#[derive(Clone, serde::Serialize)]
struct FullscreenState {
    active: bool,
}

/// `foreground` couvre l'écran en entier, hors bureau/barre des tâches ; ou Windows signale un mode exclusif.
pub fn is_fullscreen(foreground: Rect, monitor: Rect, is_shell_window: bool, quns_busy: bool) -> bool {
    if quns_busy {
        return true;
    }
    let right = |r: Rect| r.x as i64 + r.w as i64;
    let bottom = |r: Rect| r.y as i64 + r.h as i64;
    !is_shell_window
        && foreground.x <= monitor.x
        && foreground.y <= monitor.y
        && right(foreground) >= right(monitor)
        && bottom(foreground) >= bottom(monitor)
}

fn rect_of(r: RECT) -> Rect {
    Rect { x: r.left, y: r.top, w: (r.right - r.left).max(0) as u32, h: (r.bottom - r.top).max(0) as u32 }
}

fn is_shell_class(hwnd: HWND) -> bool {
    let mut buf = [0u16; 32];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) } as usize;
    matches!(String::from_utf16_lossy(&buf[..n]).as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd")
}

fn quns_busy() -> bool {
    unsafe { SHQueryUserNotificationState() }
        .is_ok_and(|s| s == QUNS_BUSY || s == QUNS_RUNNING_D3D_FULL_SCREEN || s == QUNS_PRESENTATION_MODE)
}

fn evaluate(hwnd: HWND) -> bool {
    let mut wr = RECT::default();
    let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
    unsafe {
        if GetWindowRect(hwnd, &mut wr).is_err()
            || !GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut mi).as_bool()
        {
            return false;
        }
    }
    is_fullscreen(rect_of(wr), rect_of(mi.rcMonitor), is_shell_class(hwnd), quns_busy())
}

fn apply(active: bool) {
    if ACTIVE.swap(active, Ordering::SeqCst) == active {
        return;
    }
    let Some(app) = APP.get() else { return };
    let _ = app.emit("oniris://fullscreen", FullscreenState { active });
    if let Some(pill) = app.get_webview_window("pill") {
        if !active {
            let _ = pill.show();
        } else if app.state::<AppState>().hide_pill_in_fullscreen.load(Ordering::SeqCst) {
            let _ = pill.hide();
        }
    }
}

unsafe extern "system" fn on_event(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    id_object: i32,
    _id_child: i32,
    _thread: u32,
    _time: u32,
) {
    let fg = unsafe { GetForegroundWindow() };
    if fg.0.is_null() {
        return;
    }
    // LOCATIONCHANGE est très bavard : filtrer d'abord, avant tout travail.
    if event == EVENT_OBJECT_LOCATIONCHANGE && (id_object != OBJID_WINDOW.0 || hwnd != fg) {
        return;
    }
    apply(evaluate(fg));
}

pub fn start_watcher(app: AppHandle) {
    let _ = APP.set(app);
    std::thread::spawn(|| unsafe {
        let flags = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;
        for ev in [EVENT_SYSTEM_FOREGROUND, EVENT_OBJECT_LOCATIONCHANGE] {
            SetWinEventHook(ev, ev, Some(HMODULE::default()), Some(on_event), 0, 0, flags);
        }
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {}
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    const M: Rect = Rect { x: 0, y: 0, w: 2560, h: 1440 };
    #[test] fn jeu_plein_ecran() { assert!(is_fullscreen(M, M, false, false)); }
    #[test] fn bureau_n_est_pas_plein_ecran() { assert!(!is_fullscreen(M, M, true, false)); }
    #[test] fn fenetre_maximisee_n_est_pas_plein_ecran() {
        assert!(!is_fullscreen(Rect { x: 0, y: 0, w: 2560, h: 1392 }, M, false, false));
    }
    #[test] fn mode_presentation_signale_par_windows() {
        assert!(is_fullscreen(Rect { x: 10, y: 10, w: 800, h: 600 }, M, false, true));
    }
}
