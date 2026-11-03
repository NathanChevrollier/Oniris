use crate::hotkey::{key_to_vk, HotkeyConfig};
use crate::state::AppState;
use crate::{fullscreen, windows};
use oniris_core::{save_settings, Settings};
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, State};

#[tauri::command]
pub fn panel_open(app: AppHandle) -> tauri::Result<()> {
    windows::open_panel(&app)
}

#[tauri::command]
pub fn panel_close(app: AppHandle) -> tauri::Result<()> {
    windows::close_panel(&app)
}

#[tauri::command]
pub fn panel_toggle(app: AppHandle) -> tauri::Result<()> {
    windows::toggle_panel(&app)
}

#[tauri::command]
pub fn pill_resize(app: AppHandle, width: f64) -> tauri::Result<()> {
    windows::place_pill(&app, width)
}

#[tauri::command]
pub fn settings_get(state: State<AppState>) -> Settings {
    state.settings.read().clone()
}

fn validate(s: &Settings) -> Result<(), String> {
    if s.timeline_start_hour >= s.timeline_end_hour || s.timeline_end_hour > 24 {
        return Err("plage horaire invalide : début < fin <= 24".into());
    }
    if !(100..=1000).contains(&s.hold_threshold_ms) {
        return Err("seuil de maintien hors de 100..=1000 ms".into());
    }
    if key_to_vk(&s.hotkey.key).is_none() {
        return Err(format!("touche inconnue : {}", s.hotkey.key));
    }
    Ok(())
}

#[tauri::command]
pub fn settings_save(app: AppHandle, state: State<AppState>, settings: Settings) -> Result<(), String> {
    validate(&settings)?;
    save_settings(&state.db.lock(), &settings).map_err(|e| e.to_string())?;
    let cfg = HotkeyConfig::from_setting(&settings.hotkey, settings.hold_threshold_ms as u64);
    *state.hotkey.write() = cfg;
    let hide = settings.hide_pill_in_fullscreen;
    let changed = state.hide_pill_in_fullscreen.swap(hide, Ordering::SeqCst) != hide;
    *state.settings.write() = settings.clone();
    if changed {
        fullscreen::reapply(&app);
    }
    app.emit("oniris://settings", settings).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validation() {
        assert!(validate(&Settings::default()).is_ok());
        let bad = |f: fn(&mut Settings)| { let mut s = Settings::default(); f(&mut s); validate(&s).is_err() };
        assert!(bad(|s| s.timeline_start_hour = 23));
        assert!(bad(|s| s.timeline_end_hour = 25));
        assert!(bad(|s| s.hold_threshold_ms = 99));
        assert!(bad(|s| s.hold_threshold_ms = 1001));
        assert!(bad(|s| s.hotkey.key = "Enter".into()));
        let mut ok = Settings::default();
        ok.timeline_end_hour = 24;
        assert!(validate(&ok).is_ok());
    }
}
