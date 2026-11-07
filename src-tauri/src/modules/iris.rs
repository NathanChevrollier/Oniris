//! IRIS : état du cerveau local (Ollama) et téléchargement de son modèle. Le journal ne porte aucun texte utilisateur.
use crate::state::AppState;
use oniris_core::iris::{LocalStatus, Ollama};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, Manager};

/// Claude Code : la détection arrive avec la tâche 6 (`Found { path }` / `Missing`).
#[derive(Serialize, Clone, Copy)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ClaudeStatus { Unknown }

#[derive(Serialize)]
pub struct IrisStatus { local: LocalStatus, model: String, claude: ClaudeStatus }

#[derive(Serialize, Clone)]
struct PullEvent { done: Option<u64>, total: Option<u64>, error: Option<String> }

static PULLING: AtomicBool = AtomicBool::new(false);

fn model(app: &AppHandle) -> String { app.state::<AppState>().settings.read().iris_local_model.clone() }

#[tauri::command]
pub async fn iris_status(app: AppHandle) -> IrisStatus {
    let model = model(&app);
    IrisStatus { local: Ollama::new(&model).status(&model).await, model, claude: ClaudeStatus::Unknown }
}

/// Télécharge le modèle configuré ; progression sur `oniris://iris-pull`, puis `{done: total}` ou `{error}`.
#[tauri::command]
pub async fn iris_pull_model(app: AppHandle) -> Result<(), String> {
    if PULLING.swap(true, Ordering::SeqCst) {
        return Err("téléchargement déjà en cours".into());
    }
    let model = model(&app);
    let emit = |done, total, error| { let _ = app.emit("oniris://iris-pull", PullEvent { done, total, error }); };
    let mut last = (0, 0);
    let r = Ollama::new(&model).pull(&model, |d, t| { last = (d, t); emit(Some(d), Some(t), None); }).await;
    PULLING.store(false, Ordering::SeqCst);
    match r {
        Ok(()) => { emit(Some(last.1), Some(last.1), None); Ok(()) }
        Err(e) => { tracing::warn!("oniris: téléchargement du modèle local échoué ({e})"); emit(None, None, Some(e.to_string())); Err(e.to_string()) }
    }
}

/// Entrée en plein écran : libère la mémoire du modèle, hors du fil du détecteur.
pub fn unload_on_fullscreen(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let model = model(&app);
        Ollama::new(&model).unload(&model).await;
    });
}
