//! Inventaire des applications du menu Démarrer : balayage en tâche de fond, jamais sur le chemin d'ouverture.
use crate::state::AppState;
use oniris_core::apps;
use oniris_core::time_util::now_ms;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Manager};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::UI::Shell::{FOLDERID_CommonPrograms, FOLDERID_Programs, SHGetKnownFolderPath, KF_FLAG_DEFAULT};

static SCANNING: AtomicBool = AtomicBool::new(false);

/// Libère `SCANNING` même si le balayage panique.
struct ScanGuard;
impl Drop for ScanGuard {
    fn drop(&mut self) {
        SCANNING.store(false, Ordering::SeqCst);
    }
}

/// Menus Démarrer (utilisateur d'abord : il gagne les doublons) via les dossiers connus, pas de chemin en dur.
fn start_menu_dirs() -> Vec<PathBuf> {
    [&FOLDERID_Programs, &FOLDERID_CommonPrograms]
        .into_iter()
        .filter_map(|id| unsafe {
            let p = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
            let s = p.to_string().ok();
            CoTaskMemFree(Some(p.0 as _));
            s.map(PathBuf::from)
        })
        .collect()
}

/// Applications du menu Démarrer vues par Windows (Store compris), en JSON ; vide si PowerShell échoue.
fn start_apps_json() -> String {
    use std::os::windows::process::CommandExt;
    let out = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command", "[Console]::OutputEncoding=[Text.Encoding]::UTF8; Get-StartApps | Select-Object Name,AppID | ConvertTo-Json -Compress"])
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        _ => {
            tracing::warn!("applications : Get-StartApps indisponible");
            String::new()
        }
    }
}

/// Lance un balayage sur un fil dédié ; ignoré si un autre est en cours.
pub fn refresh(app: &AppHandle) {
    if SCANNING.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    let spawned = std::thread::Builder::new().name("oniris-apps".into()).spawn(move || {
        let _guard = ScanGuard;
        let items = apps::merge_start_apps(apps::scan(&start_menu_dirs()), &start_apps_json());
        tracing::info!("applications : {} entrées", items.len());
        app.state::<AppState>().apps.set(items, now_ms());
    });
    if let Err(e) = spawned {
        SCANNING.store(false, Ordering::SeqCst);
        tracing::error!("applications : fil impossible : {e}");
    }
}

/// Au démarrage.
pub fn start(app: &AppHandle) {
    refresh(app);
}

/// Appelé par `windows::open_panel` : rebalaie si l'inventaire a plus de 6 h. Ne bloque pas.
pub fn on_panel_open(app: &AppHandle) {
    if app.state::<AppState>().apps.stale(now_ms()) {
        refresh(app);
    }
}
