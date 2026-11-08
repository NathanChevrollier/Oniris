//! Détection du plein écran par crochets WinEvent (aucune interrogation périodique), sur un thread dédié
//! qui possède sa boucle de messages. Émet `oniris://fullscreen` seulement quand l'état change.
//! LOCATIONCHANGE n'est écouté que pour le processus au premier plan (re-ciblé à chaque changement) :
//! un crochet global réveillerait le thread à chaque mouvement de souris ou de curseur texte.

use crate::state::AppState;
use crate::windows::geometry::Rect;
use ::windows::Win32::Foundation::{HWND, RECT};
use ::windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
};
use ::windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use ::windows::Win32::UI::Shell::{
    SHQueryUserNotificationState, QUNS_PRESENTATION_MODE, QUNS_RUNNING_D3D_FULL_SCREEN,
};
use ::windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetForegroundWindow, GetMessageW, GetWindowLongW, GetWindowRect,
    GetWindowThreadProcessId, IsZoomed,
    GWL_STYLE, WS_CAPTION, EVENT_OBJECT_LOCATIONCHANGE,
    EVENT_SYSTEM_FOREGROUND, MSG, OBJID_WINDOW, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS,
};
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use tauri::{AppHandle, Emitter, Manager};

static APP: OnceLock<AppHandle> = OnceLock::new();
static ACTIVE: AtomicBool = AtomicBool::new(false);

/// Une application plein écran est au premier plan.
pub fn active() -> bool { ACTIVE.load(Ordering::SeqCst) }
const FLAGS: u32 = WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS;

thread_local! {
    /// Crochet LOCATIONCHANGE du processus au premier plan (thread du détecteur seulement).
    static LOCATION_HOOK: Cell<Option<HWINEVENTHOOK>> = const { Cell::new(None) };
}

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

/// Fenêtre à ignorer : bureau/barre des tâches, ou fenêtre maximisée classique (son cadre invisible déborde
/// de l'écran avec une barre des tâches masquée automatiquement). Un jeu sans bordure n'a pas de légende.
fn is_excluded_window(class_is_shell: bool, is_zoomed: bool, has_caption: bool) -> bool {
    class_is_shell || (is_zoomed && has_caption)
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
        .is_ok_and(|s| s == QUNS_RUNNING_D3D_FULL_SCREEN || s == QUNS_PRESENTATION_MODE)
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
    let (zoomed, caption) = unsafe {
        (IsZoomed(hwnd).as_bool(), GetWindowLongW(hwnd, GWL_STYLE) as u32 & WS_CAPTION.0 == WS_CAPTION.0)
    };
    let excluded = is_excluded_window(is_shell_class(hwnd), zoomed, caption);
    is_fullscreen(rect_of(wr), rect_of(mi.rcMonitor), excluded, quns_busy())
}

fn apply(active: bool) {
    if ACTIVE.swap(active, Ordering::SeqCst) == active {
        return;
    }
    let Some(app) = APP.get() else { return };
    let _ = app.emit("oniris://fullscreen", FullscreenState { active });
    if active {
        crate::modules::iris::unload_on_fullscreen(app); // une fois par entrée : `swap` ci-dessus filtre les répétitions
        crate::modules::voice::pipeline::unload_on_fullscreen();
    }
    if let Some(pill) = app.get_webview_window("pill") {
        if !active {
            let _ = pill.show();
        } else if app.state::<AppState>().hide_pill_in_fullscreen.load(Ordering::SeqCst) {
            let _ = pill.hide();
        }
    }
}

/// Réapplique la visibilité de la pilule selon l'état plein écran courant (après un changement de réglage).
pub fn reapply(app: &AppHandle) {
    if !ACTIVE.load(Ordering::SeqCst) {
        return;
    }
    if let Some(pill) = app.get_webview_window("pill") {
        let _ = if app.state::<AppState>().hide_pill_in_fullscreen.load(Ordering::SeqCst) {
            pill.hide()
        } else {
            pill.show()
        };
    }
}

fn hook(ev: u32, pid: u32) -> Option<HWINEVENTHOOK> {
    let h = unsafe { SetWinEventHook(ev, ev, None, Some(on_event), pid, 0, FLAGS) };
    if h.is_invalid() {
        tracing::error!("oniris: SetWinEventHook a échoué pour l'événement {ev:#x}");
        return None;
    }
    Some(h)
}

/// Nouvelle fenêtre au premier plan : LOCATIONCHANGE re-ciblé sur son processus, puis évaluation.
fn on_foreground(fg: HWND) {
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(fg, Some(&mut pid)) };
    if let Some(old) = LOCATION_HOOK.take() {
        let _ = unsafe { UnhookWinEvent(old) };
    }
    if pid != 0 {
        LOCATION_HOOK.set(hook(EVENT_OBJECT_LOCATIONCHANGE, pid));
    }
    apply(evaluate(fg));
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
    if event == EVENT_SYSTEM_FOREGROUND {
        on_foreground(fg);
    } else if id_object == OBJID_WINDOW.0 && hwnd == fg {
        // LOCATIONCHANGE reste bavard dans le processus ciblé (curseur, contrôles) : seule la fenêtre compte.
        apply(evaluate(fg));
    }
}

pub fn start_watcher(app: AppHandle) {
    let _ = APP.set(app);
    std::thread::spawn(|| unsafe {
        hook(EVENT_SYSTEM_FOREGROUND, 0);
        // État initial : la fenêtre au premier plan au démarrage n'émettra pas d'événement FOREGROUND.
        let fg = GetForegroundWindow();
        if !fg.0.is_null() {
            on_foreground(fg);
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
    #[test] fn maximisee_avec_legende_exclue() { assert!(is_excluded_window(false, true, true)); }
    #[test] fn sans_bordure_non_exclue() { assert!(!is_excluded_window(false, true, false)); }
    #[test] fn shell_exclue() { assert!(is_excluded_window(true, false, false)); }
}
