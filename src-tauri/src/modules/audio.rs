//! Core Audio (volume maître, mélangeur par application) sur un fil dédié, en MTA initialisé une fois pour toutes.
//! Avant, chaque appel initialisait puis rendait COM sur le worker tokio de la commande : l'état COM de ces fils
//! partagés dépend de ce qui y a tourné avant (cpal y laisse un STA à vie, WinRT un MTA implicite), et toute
//! erreur était avalée. Ici l'appartement est connu, les appels sont sérialisés et chaque échec remonte.
//! Jamais lancé par les tests (R8). Journal : codes d'erreur seulement, ni nom d'appli ni nom de périphérique.
use oniris_core::general::pc::{AppAudio, Volume};
use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::sync::mpsc;
use std::time::Duration;
use windows::core::{Interface, Result as WinResult, PWSTR};
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{
    eConsole, eRender, AudioSessionStateActive, AudioSessionStateExpired, IAudioSessionControl2, IAudioSessionManager2,
    IMMDeviceEnumerator, ISimpleAudioVolume, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};

type Job = Box<dyn FnOnce() + Send>;
static TX: Mutex<Option<mpsc::Sender<Job>>> = Mutex::new(None);

fn sender() -> Result<mpsc::Sender<Job>, String> {
    let mut g = TX.lock();
    if let Some(t) = g.as_ref() {
        return Ok(t.clone());
    }
    let (tx, rx) = mpsc::channel::<Job>();
    std::thread::Builder::new()
        .name("oniris-audio".into())
        .spawn(move || {
            // SAFETY: fil possédé, en MTA pour toute sa vie (il vit autant que le processus).
            let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            if hr.is_err() {
                tracing::warn!("son : initialisation COM refusée ({:#010x})", hr.0);
            }
            for job in rx {
                if std::panic::catch_unwind(std::panic::AssertUnwindSafe(job)).is_err() {
                    tracing::error!("son : appel Core Audio en panique");
                }
            }
        })
        .map_err(|e| format!("son : fil audio impossible : {e}"))?;
    *g = Some(tx.clone());
    Ok(tx)
}

/// Exécute `f` sur le fil audio ; 5 s au plus (service audio de Windows bloqué).
fn run<T: Send + 'static>(f: impl FnOnce() -> WinResult<T> + Send + 'static) -> Result<T, String> {
    let (rtx, rrx) = mpsc::sync_channel(1);
    let job: Job = Box::new(move || {
        let _ = rtx.send(f());
    });
    if sender()?.send(job).is_err() {
        *TX.lock() = None; // relancé au prochain appel
        return Err("son : fil audio arrêté".into());
    }
    match rrx.recv_timeout(Duration::from_secs(5)) {
        Ok(r) => r.map_err(|e| format!("son : {} ({:#010x})", e.message().trim(), e.code().0)),
        Err(_) => Err("son : le service audio de Windows ne répond pas".into()),
    }
}

/// Écrit l'erreur au journal seulement quand elle change (le panneau relit l'état toutes les quelques secondes).
pub fn warn_changed(slot: &Mutex<Option<String>>, r: &Result<impl Sized, String>) {
    let now = r.as_ref().err().cloned();
    let mut last = slot.lock();
    if *last != now {
        if let Some(e) = &now {
            tracing::warn!("{e}");
        }
        *last = now;
    }
}

unsafe fn enumerator() -> WinResult<IMMDeviceEnumerator> {
    CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
}

/// Volume de la sortie par défaut (rôle « console », celle du curseur de Windows).
unsafe fn endpoint() -> WinResult<IAudioEndpointVolume> {
    enumerator()?.GetDefaultAudioEndpoint(eRender, eConsole)?.Activate(CLSCTX_ALL, None)
}

pub fn volume() -> Result<Volume, String> {
    // SAFETY: appels COM sur le fil audio (MTA) ; les interfaces meurent dans la fermeture.
    run(|| unsafe {
        let v = endpoint()?;
        Ok(Volume { percent: (v.GetMasterVolumeLevelScalar()? * 100.0).round() as u8, muted: v.GetMute()?.as_bool() })
    })
}

pub fn set_volume(percent: u8) -> Result<(), String> {
    // SAFETY: idem ; pas de GUID de contexte.
    run(move || unsafe { endpoint()?.SetMasterVolumeLevelScalar(f32::from(percent.min(100)) / 100.0, std::ptr::null()) })
}

pub fn set_mute(muted: bool) -> Result<(), String> {
    // SAFETY: idem.
    run(move || unsafe { endpoint()?.SetMute(muted, std::ptr::null()) })
}

/// Nom de la sortie par défaut (« Haut-parleurs (Arctis Nova Pro Wireless) »), pour l'infobulle.
pub fn output_name() -> Option<String> {
    use cpal::traits::{DeviceTrait, HostTrait};
    // cpal tente un STA sur ce fil, refusé (RPC_E_CHANGED_MODE) et toléré : il reste en MTA.
    run(|| Ok(cpal::default_host().default_output_device().and_then(|d| d.description().ok()).map(|d| d.name().to_string()))).ok().flatten()
}

/// Une sortie audio active et son volume (avec SteelSeries Sonar, la sortie par défaut est virtuelle :
/// régler aussi la sortie physique, le casque, est alors utile).
#[derive(serde::Serialize, Clone, Debug)]
pub struct Output {
    id: String,
    name: String,
    percent: u8,
    muted: bool,
    default: bool,
}

unsafe fn endpoint_of(id: &str) -> WinResult<IAudioEndpointVolume> {
    enumerator()?.GetDevice(&windows::core::HSTRING::from(id))?.Activate(CLSCTX_ALL, None)
}

pub fn outputs() -> Result<Vec<Output>, String> {
    use cpal::traits::{DeviceTrait, HostTrait};
    // SAFETY: appels COM sur le fil audio ; les interfaces meurent dans la fermeture.
    run(|| unsafe {
        let host = cpal::default_host();
        let def = host.default_output_device().and_then(|d| d.id().ok()).map(|i| i.id().to_string());
        let mut out = vec![];
        for d in host.output_devices().map_err(|e| windows::core::Error::new(windows::Win32::Foundation::E_FAIL, e.to_string()))? {
            let (Ok(id), Ok(desc)) = (d.id(), d.description()) else { continue };
            let id = id.id().to_string();
            let Ok(v) = endpoint_of(&id) else { continue };
            out.push(Output {
                percent: (v.GetMasterVolumeLevelScalar()? * 100.0).round() as u8,
                muted: v.GetMute()?.as_bool(),
                default: def.as_deref() == Some(id.as_str()),
                name: desc.name().to_string(),
                id,
            });
        }
        Ok(out)
    })
}

pub fn set_output(id: &str, percent: Option<u8>, muted: Option<bool>) -> Result<(), String> {
    let id = id.to_string();
    // SAFETY: idem ; pas de GUID de contexte.
    run(move || unsafe {
        let v = endpoint_of(&id)?;
        if let Some(p) = percent {
            v.SetMasterVolumeLevelScalar(f32::from(p.min(100)) / 100.0, std::ptr::null())?;
        }
        if let Some(m) = muted {
            v.SetMute(m, std::ptr::null())?;
        }
        Ok(())
    })
}

struct Session {
    key: String,
    name: String,
    path: Option<String>,
    vol: ISimpleAudioVolume,
    active: bool,
}

/// Chemin de l'exe d'un processus, sans droits élevés ; `None` s'il est protégé ou terminé.
fn exe_path(pid: u32) -> Option<String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
    // SAFETY: handle fermé avant de sortir ; le tampon vit pendant l'appel.
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(h);
        ok.then(|| String::from_utf16_lossy(&buf[..len as usize]))
    }
}

/// « discord.exe » → (« discord », « Discord »).
fn names_of(path: &str) -> (String, String) {
    let stem = std::path::Path::new(path).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let mut c = stem.chars();
    // shortcut: nom = exe capitalisé (« Msedge »), passer à la description de version de l'exe si ça gêne.
    let name = c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default();
    (stem.to_lowercase(), name)
}

/// Sessions audio ouvertes sur toutes les sorties actives (Discord joue souvent sur la sortie « Chat »).
unsafe fn sessions() -> WinResult<Vec<Session>> {
    use windows::Win32::Foundation::S_OK;
    let devs = enumerator()?.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?;
    let mut out = vec![];
    for i in 0..devs.GetCount()? {
        let Ok(mgr) = devs.Item(i).and_then(|d| d.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None)) else { continue };
        let en = mgr.GetSessionEnumerator()?;
        for j in 0..en.GetCount()? {
            let ctl = en.GetSession(j)?;
            let state = ctl.GetState()?;
            if state == AudioSessionStateExpired {
                continue;
            }
            let c2: IAudioSessionControl2 = ctl.cast()?;
            let (key, name, path) = if c2.IsSystemSoundsSession() == S_OK {
                ("system".to_string(), "Sons système".to_string(), None)
            } else {
                let Some(path) = c2.GetProcessId().ok().and_then(exe_path) else { continue };
                let (k, n) = names_of(&path);
                if k == "audiodg" {
                    continue; // moteur audio de Windows (effets, Sonar) : pas une application
                }
                (k, n, Some(path))
            };
            out.push(Session { key, name, path, vol: ctl.cast()?, active: state == AudioSessionStateActive });
        }
    }
    Ok(out)
}

/// Une ligne par application (toutes ses sessions réunies), avec le chemin de son exe pour l'icône.
pub fn apps() -> Result<Vec<(AppAudio, Option<String>)>, String> {
    // SAFETY: appels COM sur le fil audio ; les sessions meurent dans la fermeture.
    run(|| unsafe {
        let mut by: BTreeMap<String, (AppAudio, Option<String>)> = BTreeMap::new();
        for s in sessions()? {
            let (percent, muted) = ((s.vol.GetMasterVolume()? * 100.0).round() as u8, s.vol.GetMute()?.as_bool());
            let e = by.entry(s.key.clone()).or_insert_with(|| {
                (AppAudio { key: s.key, name: s.name, percent, muted, active: false }, s.path)
            });
            e.0.muted &= muted;
            e.0.active |= s.active;
        }
        let mut v: Vec<_> = by.into_values().collect();
        // Celles qui jouent d'abord, les sons système en dernier.
        v.sort_by_key(|(a, _)| (!a.active, a.key == "system", a.name.to_lowercase()));
        Ok(v)
    })
}

pub fn set_app(key: &str, percent: Option<u8>, muted: Option<bool>) -> Result<(), String> {
    let key = key.to_string();
    // SAFETY: idem ; pas de GUID de contexte.
    let n = run(move || unsafe {
        let mut n = 0;
        for s in sessions()?.into_iter().filter(|s| s.key == key) {
            if let Some(p) = percent {
                s.vol.SetMasterVolume(f32::from(p.min(100)) / 100.0, std::ptr::null())?;
            }
            if let Some(m) = muted {
                s.vol.SetMute(m, std::ptr::null())?;
            }
            n += 1;
        }
        Ok(n)
    })?;
    if n == 0 { Err("cette application n'a plus de son ouvert".into()) } else { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::names_of;

    #[test]
    fn nom_d_appli_depuis_l_exe() {
        assert_eq!(names_of(r"C:\Users\x\AppData\Local\Discord\app-1.0\Discord.exe"), ("discord".into(), "Discord".into()));
        assert_eq!(names_of(r"C:\Program Files\Spotify\spotify.exe"), ("spotify".into(), "Spotify".into()));
    }
}
