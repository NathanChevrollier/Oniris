//! Installation des fichiers de la voix, sur clic seulement. Journal : clés du catalogue et erreurs, jamais de chemin.
use crate::state::AppState;
use oniris_core::voice::assets::{self, Asset, VoiceStatus};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

/// `%LOCALAPPDATA%\Oniris\voice` (sous-dossiers `whisper`, `piper`, `models`).
pub fn voice_dir() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(std::env::temp_dir).join("Oniris").join("voice")
}

/// Une seule installation à la fois.
static INSTALLING: AtomicBool = AtomicBool::new(false);

struct Guard;
impl Guard {
    fn take() -> Option<Self> { (!INSTALLING.swap(true, Ordering::SeqCst)).then_some(Self) }
}
impl Drop for Guard {
    fn drop(&mut self) { INSTALLING.store(false, Ordering::SeqCst); }
}

/// Événement `oniris://voice-install` : progression `{key, done, total}`, `{error}` ou `{finished}`.
#[derive(Serialize, Clone, Default, Debug, PartialEq)]
pub struct InstallEvent {
    #[serde(skip_serializing_if = "Option::is_none")] pub key: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")] pub done: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")] pub total: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")] pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")] pub finished: Option<bool>,
}

/// Télécharge ce qui manque, dans l'ordre ; s'arrête à la première erreur. Progression à 10 évé/s au plus.
async fn run(catalog: &[Asset], dir: &Path, stt: &str, http: &reqwest::Client, emit: &(dyn Fn(InstallEvent) + Sync)) -> Result<(), String> {
    let status = assets::status_in(catalog, dir, stt);
    for a in assets::needed_in(catalog, stt).into_iter().filter(|a| status.missing.iter().any(|k| k == a.key)) {
        let mut last = Instant::now() - Duration::from_secs(1);
        let r = assets::download(http, a, dir, |done, total| {
            if last.elapsed() >= Duration::from_millis(100) || total == Some(done) {
                last = Instant::now();
                emit(InstallEvent { key: Some(a.key), done: Some(done), total, ..Default::default() });
            }
        })
        .await;
        if let Err(e) = r {
            tracing::warn!("oniris: installation de la voix échouée ({}, {e:?})", a.key);
            return Err(e.to_string());
        }
    }
    Ok(())
}

/// État de l'installation : simples contrôles de fichiers, hors fil principal, sans base.
#[tauri::command]
pub async fn voice_status(app: AppHandle) -> Result<VoiceStatus, String> {
    let stt = app.state::<AppState>().settings.read().voice_stt_model.clone();
    tauri::async_runtime::spawn_blocking(move || assets::status(&voice_dir(), &stt)).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn voice_install(app: AppHandle) -> Result<(), String> {
    let _guard = Guard::take().ok_or("Une installation est déjà en cours.")?;
    let stt = app.state::<AppState>().settings.read().voice_stt_model.clone();
    let emit = |ev: InstallEvent| { let _ = app.emit("oniris://voice-install", ev); };
    let r = match assets::http_client() {
        Ok(http) => run(assets::CATALOG, &voice_dir(), &stt, &http, &emit).await,
        Err(_) => Err(assets::AssetError::Network.to_string()),
    };
    match r {
        Ok(()) => { emit(InstallEvent { finished: Some(true), ..Default::default() }); Ok(()) }
        Err(e) => { emit(InstallEvent { error: Some(e.clone()), ..Default::default() }); Err(e) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn une_seule_installation_a_la_fois() {
        let g = Guard::take().expect("libre");
        assert!(Guard::take().is_none());
        drop(g);
        assert!(Guard::take().is_some());
    }

    #[test]
    fn evenement_serialise_sans_champs_vides() {
        let j = serde_json::to_string(&InstallEvent { key: Some("a"), done: Some(1), total: None, ..Default::default() }).unwrap();
        assert_eq!(j, r#"{"key":"a","done":1}"#);
    }

    #[tokio::test]
    async fn run_ignore_ce_qui_est_deja_installe() {
        let d = tempfile::tempdir().unwrap();
        // tout installé : aucune requête (le client https seul refuserait de toute façon l'http).
        for a in assets::CATALOG {
            match a.kind {
                assets::AssetKind::File { dest } => { std::fs::create_dir_all(d.path().join(dest).parent().unwrap()).unwrap(); std::fs::write(d.path().join(dest), b"x").unwrap(); }
                assets::AssetKind::Zip { dest_dir, .. } => {
                    std::fs::create_dir_all(d.path().join(dest_dir)).unwrap();
                    for exe in ["whisper-server.exe", "piper.exe"] { std::fs::write(d.path().join(dest_dir).join(exe), b"x").unwrap(); }
                }
            }
        }
        let n = std::sync::atomic::AtomicU32::new(0);
        let emit = |_: InstallEvent| { n.fetch_add(1, Ordering::SeqCst); };
        run(assets::CATALOG, d.path(), "small", &assets::http_client().unwrap(), &emit).await.unwrap();
        assert_eq!(n.load(Ordering::SeqCst), 0);
    }
}
