//! Musique via la Web API Spotify : playlists favorites et commandes de l'interface.
//! Aucune requête Spotify en plein écran ni panneau fermé, sauf demande explicite d'un outil.
use crate::state::AppState;
use oniris_core::music::tools::PlaylistsView;
use oniris_core::tools::ToolOutcome;
use serde_json::json;
use tauri::{AppHandle, Manager, State};

/// Ouverture du panneau : relève les playlists (au plus toutes les 10 minutes) dans une tâche, sans verrou ni attente.
pub fn on_panel_open(app: &AppHandle) {
    if crate::fullscreen::active() {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let web = app.state::<AppState>().spotify.clone();
        web.refresh_if_stale().await;
    });
}

/// Lit le cache (jamais le réseau) : favoris, toutes les playlists, compte connecté ou non.
#[tauri::command(async)]
pub fn music_playlists(state: State<'_, AppState>) -> PlaylistsView { state.spotify.view() }

#[tauri::command(async)]
pub fn music_set_favorites(state: State<'_, AppState>, ids: Vec<String>) -> Result<(), String> { state.spotify.set_favorites(&ids) }

#[tauri::command]
pub async fn music_devices(app: AppHandle) -> ToolOutcome { crate::commands::dispatch_as_user(&app, "music.devices", json!({})).await }

#[tauri::command]
pub async fn music_play_playlist(app: AppHandle, id: String) -> ToolOutcome {
    crate::commands::dispatch_as_user(&app, "music.play_playlist", json!({ "id": id })).await
}

#[tauri::command]
pub async fn music_play_search(app: AppHandle, query: String, kind: String) -> ToolOutcome {
    crate::commands::dispatch_as_user(&app, "music.play_search", json!({ "query": query, "kind": kind })).await
}

#[tauri::command]
pub async fn music_volume(app: AppHandle, percent: Option<i32>, delta: Option<i32>) -> ToolOutcome {
    crate::commands::dispatch_as_user(&app, "music.volume", json!({ "percent": percent, "delta": delta })).await
}

#[tauri::command]
pub async fn music_queue(app: AppHandle, query: String) -> ToolOutcome {
    crate::commands::dispatch_as_user(&app, "music.queue", json!({ "query": query })).await
}

#[tauri::command]
pub async fn music_transfer(app: AppHandle, device_id: String) -> ToolOutcome {
    crate::commands::dispatch_as_user(&app, "music.transfer", json!({ "device_id": device_id })).await
}

/// Lance l'appli Spotify du PC pour qu'elle se déclare comme appareil : version classique réduite si elle existe,
/// sinon par son protocole (version du Microsoft Store comprise).
pub fn spotify_launcher() -> std::sync::Arc<dyn Fn() + Send + Sync> {
    std::sync::Arc::new(|| {
        let classic = std::env::var_os("APPDATA").map(|d| std::path::PathBuf::from(d).join(r"Spotify\Spotify.exe"));
        let started = match classic.filter(|p| p.is_file()) {
            Some(exe) => crate::launch::spawn_detached(&exe.to_string_lossy(), &["--minimized".into()], None).is_ok(),
            None => crate::launch::spawn_detached("explorer.exe", &["spotify:".into()], None).is_ok(),
        };
        tracing::info!(started, "musique : Spotify fermé, lancement");
    })
}
