use crate::hotkey::HotkeyConfig;
use crate::state::AppState;
use crate::{fullscreen, windows};
use oniris_core::tools::{JournalEntry, ToolBroker, ToolCall, ToolOutcome};
use oniris_core::{save_settings, Settings};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State, Webview};

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

#[tauri::command]
pub fn settings_save(app: AppHandle, state: State<AppState>, settings: Settings) -> Result<(), String> {
    settings.validate()?;
    save_settings(&state.db.lock(), &settings).map_err(|e| e.to_string())?;
    let cfg = HotkeyConfig::from_setting(&settings.hotkey, settings.hold_threshold_ms as u64);
    *state.hotkey.write() = cfg;
    let hide = settings.hide_pill_in_fullscreen;
    let changed = state.hide_pill_in_fullscreen.swap(hide, Ordering::SeqCst) != hide;
    *state.settings.write() = settings.clone();
    crate::tray::sync_autostart(&app, settings.autostart);
    if changed {
        fullscreen::reapply(&app);
    }
    app.emit("oniris://settings", settings).map_err(|e| e.to_string())
}

/// Point d'entrée des futurs appelants (IA, MCP, UI) : ouvre le panneau et émet `oniris://confirm` si besoin.
#[allow(dead_code)] // utilisée par les plans 2 à 5
pub async fn dispatch_tool_call(app: &AppHandle, call: ToolCall) -> ToolOutcome {
    let broker: Arc<ToolBroker> = app.state::<AppState>().broker.clone();
    let outcome = broker.call(call).await;
    if let ToolOutcome::NeedsConfirmation(p) = &outcome {
        let _ = windows::open_panel(app);
        let _ = app.emit("oniris://confirm", p);
    }
    outcome
}

/// Approuver ou annuler une action : seulement depuis le panneau, et seulement s'il est ouvert (visible).
fn approval_gate(label: &str, panel_open: bool) -> Result<(), String> {
    if label == "panel" && panel_open {
        Ok(())
    } else {
        Err("action refusée : seul le panneau ouvert peut approuver ou annuler".into())
    }
}

#[tauri::command]
pub async fn tool_resolve(
    webview: Webview,
    state: State<'_, AppState>,
    action_id: i64,
    approved: bool,
) -> Result<ToolOutcome, String> {
    approval_gate(webview.label(), state.panel_open.load(Ordering::SeqCst))?;
    let broker = state.broker.clone();
    Ok(broker.resolve(action_id, approved).await)
}

#[tauri::command]
pub async fn tool_undo(webview: Webview, state: State<'_, AppState>, action_id: i64) -> Result<(), String> {
    approval_gate(webview.label(), state.panel_open.load(Ordering::SeqCst))?;
    let broker = state.broker.clone();
    broker.undo(action_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn journal_list(state: State<AppState>, limit: u32) -> Vec<JournalEntry> {
    state.broker.journal(limit)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn approbation_reservee_au_panneau_ouvert() {
        assert!(approval_gate("panel", true).is_ok());
        assert!(approval_gate("panel", false).is_err());
        assert!(approval_gate("pill", true).is_err());
        assert!(approval_gate("pill", false).is_err());
    }
}
