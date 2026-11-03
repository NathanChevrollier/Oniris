use std::sync::atomic::AtomicBool;

#[derive(Default)]
pub struct AppState {
    pub panel_open: AtomicBool,
}
