//! Contrôles rapides du PC (son, Bluetooth, alimentation) : appels Windows réels, jamais lancés par les tests (R8).
use super::audio;
use oniris_core::general::pc::{AppAudio, PcControl, Power, Volume};
use parking_lot::Mutex;
use serde::Serialize;
use std::path::Path;

pub struct Win32Pc;

fn system32(exe: &str) -> std::path::PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
    Path::new(&root).join("System32").join(exe)
}

fn bluetooth_radio() -> Result<Option<windows::Devices::Radios::Radio>, String> {
    use windows::Devices::Radios::{Radio, RadioKind};
    let radios = Radio::GetRadiosAsync().and_then(|op| op.join()).map_err(|e| format!("Bluetooth : {}", e.message()))?;
    Ok(radios.into_iter().find(|r| r.Kind().is_ok_and(|k| k == RadioKind::Bluetooth)))
}

const BT_DENIED: &str = "Windows refuse l'accès au Bluetooth (Paramètres > Confidentialité > Radios).";

impl PcControl for Win32Pc {
    fn volume(&self) -> Result<Volume, String> { audio::volume() }
    fn set_volume(&self, percent: u8) -> Result<(), String> { audio::set_volume(percent) }
    fn set_mute(&self, muted: bool) -> Result<(), String> { audio::set_mute(muted) }
    fn apps(&self) -> Result<Vec<AppAudio>, String> { Ok(audio::apps()?.into_iter().map(|(a, _)| a).collect()) }
    fn set_app(&self, key: &str, percent: Option<u8>, muted: Option<bool>) -> Result<(), String> { audio::set_app(key, percent, muted) }
    fn bluetooth(&self) -> Result<Option<bool>, String> {
        use windows::Devices::Radios::RadioState;
        Ok(bluetooth_radio()?.map(|r| r.State().is_ok_and(|s| s == RadioState::On)))
    }
    fn set_bluetooth(&self, on: bool) -> Result<(), String> {
        use windows::Devices::Radios::{Radio, RadioAccessStatus, RadioState};
        let allowed = |s: windows::core::Result<RadioAccessStatus>| match s {
            Ok(RadioAccessStatus::Allowed) => Ok(()),
            _ => Err(BT_DENIED.to_string()),
        };
        allowed(Radio::RequestAccessAsync().and_then(|op| op.join()))?;
        let radio = bluetooth_radio()?.ok_or("Aucun adaptateur Bluetooth sur ce PC.")?;
        allowed(radio.SetStateAsync(if on { RadioState::On } else { RadioState::Off }).and_then(|op| op.join()))
    }
    fn task_manager(&self) -> Result<(), String> {
        // ShellExecute : Windows gère lui-même l'élévation que demande taskmgr.
        crate::launch::open_path(&system32("Taskmgr.exe"));
        Ok(())
    }
    fn power(&self, action: Power) -> Result<(), String> {
        let shutdown = |flag: &str| {
            crate::launch::spawn_hidden(&system32("shutdown.exe").to_string_lossy(), &[flag.into(), "/t".into(), "0".into()], None, &[])
                .map(drop)
                .map_err(|e| format!("arrêt impossible : {e}"))
        };
        match action {
            // SAFETY: appels Win32 sans argument pointeur.
            Power::Lock => unsafe { windows::Win32::System::Shutdown::LockWorkStation() }.map_err(|e| e.message()),
            Power::Sleep => {
                // Veille (pas veille prolongée), réveils programmés permis.
                let ok = unsafe { windows::Win32::System::Power::SetSuspendState(false, false, false) };
                if ok { Ok(()) } else { Err("Windows a refusé la mise en veille".into()) }
            }
            Power::Restart => shutdown("/r"),
            Power::Shutdown => shutdown("/s"),
        }
    }
}

/// Son de la sortie par défaut ; `volume` absent : `volume_error` dit pourquoi (affiché au survol).
#[derive(Serialize)]
pub struct Sound {
    volume: Option<Volume>,
    volume_error: Option<String>,
    output: Option<String>,
}

/// État relu à l'ouverture du panneau ; `None` : indisponible (pas de sortie audio, pas d'adaptateur Bluetooth).
#[derive(Serialize)]
pub struct PcState {
    #[serde(flatten)]
    sound: Sound,
    bluetooth: Option<bool>,
}

static LAST_ERR: Mutex<Option<String>> = Mutex::new(None);

/// Journalise une erreur du son (une fois tant qu'elle ne change pas) et la rend telle quelle.
fn logged<T>(r: Result<T, String>) -> Result<T, String> {
    audio::warn_changed(&LAST_ERR, &r);
    r
}

#[tauri::command(async)]
pub fn pc_sound() -> Sound {
    match logged(Win32Pc.volume()) {
        Ok(v) => Sound { volume: Some(v), volume_error: None, output: audio::output_name() },
        Err(e) => Sound { volume: None, volume_error: Some(e), output: None },
    }
}

#[tauri::command(async)]
pub fn pc_state() -> PcState {
    PcState { sound: pc_sound(), bluetooth: Win32Pc.bluetooth().ok().flatten() }
}

/// Régler un niveau rétablit le son, comme le curseur de Windows.
#[tauri::command(async)]
pub fn pc_set_volume(percent: u8) -> Result<(), String> {
    logged(Win32Pc.set_volume(percent).and_then(|_| Win32Pc.set_mute(false)))
}

#[tauri::command(async)]
pub fn pc_set_mute(muted: bool) -> Result<(), String> {
    logged(Win32Pc.set_mute(muted))
}

/// Ligne du mélangeur : l'appli et son icône (PNG en data URI, absente pour les sons système).
#[derive(Serialize)]
pub struct AppRow {
    #[serde(flatten)]
    app: AppAudio,
    icon: Option<String>,
}

#[tauri::command(async)]
pub fn pc_apps() -> Result<Vec<AppRow>, String> {
    let apps = logged(audio::apps())?;
    Ok(apps.into_iter().map(|(app, path)| AppRow { icon: path.and_then(|p| super::icons::exe_icon(&p)), app }).collect())
}

#[tauri::command(async)]
pub fn pc_outputs() -> Result<Vec<audio::Output>, String> {
    logged(audio::outputs())
}

#[tauri::command(async)]
pub fn pc_set_output(id: String, percent: Option<u8>, muted: Option<bool>) -> Result<(), String> {
    logged(audio::set_output(&id, percent, muted.or(percent.map(|_| false))))
}

/// Régler un niveau rétablit le son de l'appli, comme le mélangeur de Windows.
#[tauri::command(async)]
pub fn pc_set_app(key: String, percent: Option<u8>, muted: Option<bool>) -> Result<(), String> {
    logged(audio::set_app(&key, percent, muted.or(percent.map(|_| false))))
}

#[tauri::command(async)]
pub fn pc_set_bluetooth(on: bool) -> Result<(), String> {
    Win32Pc.set_bluetooth(on)
}

#[tauri::command(async)]
pub fn pc_task_manager() -> Result<(), String> {
    Win32Pc.task_manager()
}

#[tauri::command(async)]
pub fn pc_windows_settings() {
    crate::launch::open_url("ms-settings:");
}

/// L'interface demande une confirmation avant veille, redémarrage et extinction.
#[tauri::command(async)]
pub fn pc_power(action: Power) -> Result<(), String> {
    Win32Pc.power(action)
}

