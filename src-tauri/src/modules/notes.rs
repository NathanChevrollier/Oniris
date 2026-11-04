//! Commandes Tauri des notes. Lecture directe de la base ; toute modification passe par le courtier.
use crate::commands::dispatch_as_user;
use crate::state::AppState;
use oniris_core::notes::{self, Note};
use oniris_core::tools::ToolOutcome;
use serde_json::json;
use tauri::{AppHandle, State};

const LIMIT: usize = 50;

/// Requête vide : notes récentes ; sinon recherche par préfixe.
#[tauri::command(async)]
pub fn notes_list(state: State<AppState>, query: String) -> Result<Vec<Note>, String> {
    notes::search(&state.db.lock(), &query, LIMIT).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn notes_create(app: AppHandle, text: String) -> ToolOutcome {
    dispatch_as_user(&app, "notes.create", json!({ "text": text })).await
}

#[tauri::command]
pub async fn notes_pin(app: AppHandle, id: i64, pinned: bool) -> ToolOutcome {
    dispatch_as_user(&app, "notes.pin", json!({ "id": id, "pinned": pinned })).await
}

#[tauri::command]
pub async fn notes_delete(app: AppHandle, id: i64) -> ToolOutcome {
    dispatch_as_user(&app, "notes.delete", json!({ "id": id })).await
}

#[tauri::command]
pub async fn notes_to_reminder(app: AppHandle, id: i64) -> ToolOutcome {
    dispatch_as_user(&app, "notes.to_reminder", json!({ "id": id })).await
}
