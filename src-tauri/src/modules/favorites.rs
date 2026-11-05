//! Commandes Tauri des favoris. Lecture, réordonnancement et renommage : base seule ; ajout, suppression
//! et ouverture passent par le courtier. Journal : jamais d'URL ni de titre.
use crate::commands::dispatch_as_user;
use crate::state::AppState;
use oniris_core::events::Topic;
use oniris_core::favorites::import::{self, ImportReport, NewFavorite};
use oniris_core::favorites::{self, Favorite};
use oniris_core::time_util::now_ms;
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

const MAX_IMPORT_BYTES: u64 = 10 * 1024 * 1024;

/// Dossiers de profils (`Default`, `Profile N`) d'un navigateur Chromium : fichiers `Bookmarks` existants.
fn chromium_files(user_data: std::path::PathBuf) -> Vec<std::path::PathBuf> {
    let Ok(dir) = std::fs::read_dir(user_data) else { return vec![] };
    dir.flatten()
        .filter(|e| e.file_name().to_str().is_some_and(|n| n == "Default" || n.starts_with("Profile ")))
        .map(|e| e.path().join("Bookmarks"))
        .filter(|p| p.is_file())
        .collect()
}

fn small_text(p: &std::path::Path) -> Result<String, String> {
    if std::fs::metadata(p).map_err(|_| "fichier illisible".to_string())?.len() > MAX_IMPORT_BYTES {
        return Err("fichier trop volumineux (10 Mo au maximum)".into());
    }
    std::fs::read_to_string(p).map_err(|_| "fichier illisible".to_string())
}

/// Signets d'un navigateur : `Err` si des fichiers existent mais qu'aucun n'a pu être lu.
type Source = (&'static str, Result<Vec<NewFavorite>, ()>);

/// Réunit les lectures d'un navigateur : au moins un fichier lu = succès.
fn gather(name: &'static str, reads: Vec<Result<Vec<NewFavorite>, ()>>) -> Option<Source> {
    if reads.is_empty() {
        return None;
    }
    let ok: Vec<_> = reads.iter().filter_map(|r| r.as_ref().ok()).flatten().cloned().collect();
    Some((name, if reads.iter().any(Result::is_ok) { Ok(ok) } else { Err(()) }))
}

/// Lit les signets des navigateurs installés (seulement quand l'utilisateur clique « Importer »).
fn read_browsers() -> Vec<Source> {
    let env = |k: &str| std::env::var_os(k).map(std::path::PathBuf::from);
    let mut out = Vec::new();
    if let Some(local) = env("LOCALAPPDATA") {
        for (name, rel) in [("Chrome", "Google/Chrome"), ("Edge", "Microsoft/Edge"), ("Brave", "BraveSoftware/Brave-Browser")] {
            let reads = chromium_files(local.join(rel).join("User Data"))
                .iter()
                .map(|p| small_text(p).and_then(|t| import::from_chromium(&t).map_err(|e| e.to_string())).map_err(|_| ()))
                .collect();
            out.extend(gather(name, reads));
        }
    }
    if let Some(roaming) = env("APPDATA") {
        let reads = std::fs::read_dir(roaming.join("Mozilla/Firefox/Profiles"))
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path().join("places.sqlite"))
            .filter(|p| p.is_file())
            .map(|p| import::from_firefox_profile(&p).map_err(|_| ()))
            .collect();
        out.extend(gather("Firefox", reads));
    }
    out
}

/// Un rapport par navigateur ; `None` = signets trouvés mais illisibles.
#[tauri::command]
pub async fn favorites_import_browsers(state: State<'_, AppState>) -> Result<Vec<(String, Option<ImportReport>)>, String> {
    let found = tauri::async_runtime::spawn_blocking(read_browsers).await.map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for (name, items) in found {
        let report = match items {
            Ok(items) => Some(import::import(&state.db.lock(), &items, now_ms()).map_err(|e| e.to_string())?),
            Err(()) => None,
        };
        out.push((name.to_string(), report));
    }
    tracing::info!(browsers = out.len(), "favoris : import des navigateurs");
    state.events.changed(Topic::Favorites);
    Ok(out)
}

/// Export JSON de Nexus_Dashboard choisi dans la boîte de dialogue (10 Mo au maximum).
#[tauri::command]
pub async fn favorites_import_file(state: State<'_, AppState>, path: String) -> Result<ImportReport, String> {
    let p = std::path::PathBuf::from(path);
    if p.extension().is_none_or(|e| !e.eq_ignore_ascii_case("json")) {
        return Err("choisis un fichier .json".into());
    }
    let text = tauri::async_runtime::spawn_blocking(move || small_text(&p)).await.map_err(|e| e.to_string())??;
    let items = import::from_nexus(&text).map_err(|e| e.to_string())?;
    let report = import::import(&state.db.lock(), &items, now_ms()).map_err(|e| e.to_string())?;
    state.events.changed(Topic::Favorites);
    Ok(report)
}
