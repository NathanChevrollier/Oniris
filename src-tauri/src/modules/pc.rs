//! Contrôles rapides du PC (son, Bluetooth, alimentation) : appels Windows réels, jamais lancés par les tests (R8).
use oniris_core::general::pc::{PcControl, Power, Volume};
use serde::Serialize;
use std::path::Path;

pub struct Win32Pc;

fn system32(exe: &str) -> std::path::PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
    Path::new(&root).join("System32").join(exe)
}

/// COM initialisé (puis rendu) le temps de `f` ; les objets COM doivent mourir dans `f`.
fn with_com<T>(f: impl FnOnce() -> windows::core::Result<T>) -> Result<T, String> {
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};
    // SAFETY: l'initialisation est rendue sur le même fil, seulement si elle a réussi.
    unsafe {
        let init = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
        let r = f();
        if init {
            CoUninitialize();
        }
        r.map_err(|e| format!("son : {}", e.message()))
    }
}

/// Volume du périphérique de sortie par défaut (Core Audio).
fn endpoint() -> windows::core::Result<windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume> {
    use windows::Win32::Media::Audio::{eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_ALL};
    // SAFETY: appels COM sur un fil initialisé par `with_com`.
    unsafe {
        let e: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        e.GetDefaultAudioEndpoint(eRender, eConsole)?.Activate(CLSCTX_ALL, None)
    }
}

fn bluetooth_radio() -> Result<Option<windows::Devices::Radios::Radio>, String> {
    use windows::Devices::Radios::{Radio, RadioKind};
    let radios = Radio::GetRadiosAsync().and_then(|op| op.join()).map_err(|e| format!("Bluetooth : {}", e.message()))?;
    Ok(radios.into_iter().find(|r| r.Kind().is_ok_and(|k| k == RadioKind::Bluetooth)))
}

const BT_DENIED: &str = "Windows refuse l'accès au Bluetooth (Paramètres > Confidentialité > Radios).";

impl PcControl for Win32Pc {
    fn volume(&self) -> Result<Volume, String> {
        with_com(|| {
            let v = endpoint()?;
            // SAFETY: interface valide pendant l'appel.
            unsafe { Ok(Volume { percent: (v.GetMasterVolumeLevelScalar()? * 100.0).round() as u8, muted: v.GetMute()?.as_bool() }) }
        })
    }
    fn set_volume(&self, percent: u8) -> Result<(), String> {
        // SAFETY: interface valide pendant l'appel ; pas de GUID de contexte.
        with_com(|| unsafe { endpoint()?.SetMasterVolumeLevelScalar(percent.min(100) as f32 / 100.0, std::ptr::null()) })
    }
    fn set_mute(&self, muted: bool) -> Result<(), String> {
        // SAFETY: idem.
        with_com(|| unsafe { endpoint()?.SetMute(muted, std::ptr::null()) })
    }
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

/// État relu à l'ouverture du panneau ; `None` : indisponible (pas de sortie audio, pas d'adaptateur Bluetooth).
#[derive(Serialize)]
pub struct PcState {
    volume: Option<Volume>,
    bluetooth: Option<bool>,
}

#[tauri::command(async)]
pub fn pc_state() -> PcState {
    PcState { volume: Win32Pc.volume().ok(), bluetooth: Win32Pc.bluetooth().ok().flatten() }
}

/// Régler un niveau rétablit le son, comme le curseur de Windows.
#[tauri::command(async)]
pub fn pc_set_volume(percent: u8) -> Result<(), String> {
    Win32Pc.set_volume(percent)?;
    Win32Pc.set_mute(false)
}

#[tauri::command(async)]
pub fn pc_set_mute(muted: bool) -> Result<(), String> {
    Win32Pc.set_mute(muted)
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
