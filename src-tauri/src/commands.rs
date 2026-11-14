use crate::hotkey::HotkeyConfig;
use crate::state::AppState;
use crate::{fullscreen, windows};
use oniris_core::tools::{JournalEntry, Origin, PendingAction, Provenance, ToolBroker, ToolCall, ToolOutcome};
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
pub fn pill_resize(app: AppHandle, width: f64, asleep: Option<bool>) -> tauri::Result<()> {
    windows::place_pill(&app, width, asleep.unwrap_or(false))
}

#[tauri::command(async)]
pub fn settings_get(state: State<AppState>) -> Settings {
    state.settings.read().clone()
}

#[tauri::command(async)]
pub fn settings_save(app: AppHandle, settings: Settings) -> Result<(), String> {
    apply_settings(&app, settings)
}

/// Seul chemin d'écriture des réglages (panneau, zone de notification) : valide, enregistre, applique,
/// puis émet `oniris://settings`. Verrouille la base : jamais sur le fil principal.
pub fn apply_settings(app: &AppHandle, mut settings: Settings) -> Result<(), String> {
    let state = app.state::<AppState>();
    settings.weather_city = settings.weather_city.trim().to_string();
    settings.normalize_roots();
    for s in &mut settings.sites {
        s.name = s.name.trim().to_string();
        s.url = s.url.trim().to_string();
    }
    settings.validate()?;
    save_settings(&state.db.lock(), &settings).map_err(|e| e.to_string())?;
    let cfg = HotkeyConfig::of(&settings);
    *state.hotkey.write() = cfg;
    let hide = settings.hide_pill_in_fullscreen;
    let changed = state.hide_pill_in_fullscreen.swap(hide, Ordering::SeqCst) != hide;
    let old = std::mem::replace(&mut *state.settings.write(), settings.clone());
    crate::tray::sync_autostart(app, settings.autostart);
    if changed {
        fullscreen::reapply(app);
    }
    if old.sites != settings.sites {
        crate::modules::sites::wake(); // sonde tout de suite les nouveaux sites, ou se rendort si la liste est vide
        state.events.changed(oniris_core::events::Topic::Sites);
    }
    if old.weather_city != settings.weather_city {
        crate::modules::weather::city_changed(app, &settings.weather_city);
    }
    if old.projects_roots != settings.projects_roots {
        crate::modules::projects::refresh(app); // carte Projets remplie dès la fin de l'accueil ou des Paramètres
    }
    app.emit("oniris://settings", settings).map_err(|e| e.to_string())
}

/// Point d'entrée des futurs appelants (IA, MCP, UI) : ouvre le panneau et émet `oniris://confirm` si besoin.
pub async fn dispatch_tool_call(app: &AppHandle, call: ToolCall) -> ToolOutcome {
    let broker: Arc<ToolBroker> = app.state::<AppState>().broker.clone();
    let outcome = broker.call(call).await;
    if let ToolOutcome::NeedsConfirmation(p) = &outcome {
        show_confirm(app, p);
    }
    outcome
}

/// Action en attente : ouvre le panneau et lui envoie `oniris://confirm`.
pub fn show_confirm(app: &AppHandle, p: &PendingAction) {
    let _ = windows::open_panel(app);
    let _ = app.emit_to("panel", "oniris://confirm", p);
}

/// Appel d'outil demandé par l'utilisateur depuis l'interface (origine et provenance « User »).
pub async fn dispatch_as_user(app: &AppHandle, tool: &str, args: serde_json::Value) -> ToolOutcome {
    dispatch_tool_call(app, ToolCall { tool: tool.into(), args, provenance: Provenance::User, origin: Origin::User }).await
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

#[tauri::command(async)]
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
