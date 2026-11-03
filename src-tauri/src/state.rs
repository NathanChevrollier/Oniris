use std::sync::atomic::AtomicBool;

pub struct AppState {
    pub panel_open: AtomicBool,
    /// Masquer la pilule quand une application est en plein écran (câblé aux réglages par la tâche 6).
    pub hide_pill_in_fullscreen: AtomicBool,
}

impl Default for AppState {
    fn default() -> Self {
        Self { panel_open: AtomicBool::new(false), hide_pill_in_fullscreen: AtomicBool::new(true) }
    }
}
