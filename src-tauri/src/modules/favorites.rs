//! Commandes Tauri des favoris. Lecture, réordonnancement et renommage : base seule ; ajout, suppression
//! et ouverture passent par le courtier. Journal : jamais d'URL ni de titre.
use crate::commands::dispatch_as_user;
use crate::state::AppState;
use oniris_core::events::Topic;
use oniris_core::favorites::{self, Favorite};
use oniris_core::tools::ToolOutcome;
use serde_json::json;
use tauri::{AppHandle, Manager, State};

/// Reconstruit le cache de la recherche (appelé sur `Topic::Favorites`, jamais en tenant le verrou de la base).
pub fn refresh(app: &AppHandle) {
    if let Some(st) = app.try_state::<AppState>() {
        let items = favorites::items(&st.db.lock());
        *st.favorites.write() = std::sync::Arc::new(items);
    }
}

#[tauri::command]
pub async fn favorites_list(state: State<'_, AppState>) -> Result<Vec<Favorite>, String> {
    favorites::list(&state.db.lock()).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn favorites_reorder(state: State<'_, AppState>, ids: Vec<i64>) -> Result<(), String> {
    favorites::reorder(&state.db.lock(), &ids).map_err(|e| e.to_string())?;
    state.events.changed(Topic::Favorites);
    Ok(())
}

#[tauri::command]
pub async fn favorites_rename(state: State<'_, AppState>, id: i64, title: String) -> Result<(), String> {
    favorites::rename(&state.db.lock(), id, &title).map_err(|e| e.to_string())?;
    state.events.changed(Topic::Favorites);
    Ok(())
}

#[tauri::command]
pub async fn favorites_add(app: AppHandle, url: String, title: Option<String>) -> ToolOutcome {
    dispatch_as_user(&app, "favorites.add", json!({ "url": url, "title": title })).await
}

#[tauri::command]
pub async fn favorites_delete(app: AppHandle, id: i64) -> ToolOutcome {
    dispatch_as_user(&app, "favorites.delete", json!({ "id": id })).await
}

/// Ouvre le favori, puis ferme le panneau si c'est fait.
#[tauri::command]
pub async fn favorites_open(app: AppHandle, id: i64) -> ToolOutcome {
    let out = dispatch_as_user(&app, "favorites.open", json!({ "id": id })).await;
    if matches!(out, ToolOutcome::Done { .. }) {
        if let Err(e) = crate::windows::close_panel(&app) {
            tracing::warn!("favoris : fermeture du panneau impossible : {e}");
        }
    }
    out
}
