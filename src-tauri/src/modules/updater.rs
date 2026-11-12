//! Mise à jour : vérifie `latest.json` (3 min après le démarrage, puis toutes les 6 h), télécharge et vérifie la
//! signature en fond dans `%LOCALAPPDATA%\Oniris\updates\` (`.part`, renommé en `.exe` une fois vérifié), et
//! lance l'installateur seulement au calme : avec relance sur « Redémarrer », sans relance à la fermeture.
//! Échec (réseau, signature, installation) : une ligne `warn`, aucun toast, nouvel essai au cycle suivant.
use crate::state::AppState;
use async_trait::async_trait;
use oniris_core::update::{can_install, is_newer, Calm, Schedule};
use parking_lot::Mutex;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::UpdaterExt;
use tokio::time::Instant;

/// Pas de la boucle : relit le réglage `auto_update` et l'échéance.
const TICK: Duration = Duration::from_secs(60);
/// Relecture du calme pendant qu'un « Redémarrer » attend.
const CALM_POLL: Duration = Duration::from_secs(2);

#[async_trait]
pub trait Source: Send + Sync {
    /// Dernière version publiée (`None` : rien de plus récent).
    async fn latest(&self) -> Result<Option<Remote>, String>;
    /// Écrit dans `to` l'installateur de `r`, seulement si sa signature est valide.
    async fn download(&self, r: &Remote, to: &Path) -> Result<(), String>;
}

#[derive(Clone, Serialize, Debug)]
pub struct Remote { pub version: String, pub notes: String }

pub struct Pending { pub version: String, pub notes: String, pub installer: PathBuf }

#[derive(Serialize)]
pub struct UpdateStatus { current: String, pending: Option<(String, String)>, checking: bool, last_error: Option<String> }

#[derive(Default)]
struct St { pending: Option<Pending>, checking: bool, last_error: Option<String> }

type Launch = Box<dyn Fn(&Path, bool) -> bool + Send + Sync>;

pub struct Updater {
    current: String,
    dir: PathBuf,
    source: Arc<dyn Source>,
    /// Lance l'installateur (`relaunch` : l'app redémarre après) ; faux si le lancement a échoué.
    launch: Launch,
    on_ready: Box<dyn Fn(&Remote) + Send + Sync>,
    calm: Box<dyn Fn() -> Calm + Send + Sync>,
    enabled: Box<dyn Fn() -> bool + Send + Sync>,
    st: Mutex<St>,
    schedule: Mutex<Schedule>,
}

fn installer_name(version: &str) -> String { format!("Oniris-{version}-setup.exe") }

impl Updater {
    pub fn new(
        current: String,
        dir: PathBuf,
        source: Arc<dyn Source>,
        launch: Launch,
        on_ready: Box<dyn Fn(&Remote) + Send + Sync>,
        calm: Box<dyn Fn() -> Calm + Send + Sync>,
        enabled: Box<dyn Fn() -> bool + Send + Sync>,
    ) -> Self {
        let schedule = Mutex::new(Schedule::new(Instant::now()));
        Self { current, dir, source, launch, on_ready, calm, enabled, st: Mutex::default(), schedule }
    }

    pub fn status(&self) -> UpdateStatus {
        let s = self.st.lock();
        UpdateStatus {
            current: self.current.clone(),
            pending: s.pending.as_ref().map(|p| (p.version.clone(), p.notes.clone())),
            checking: s.checking,
            last_error: s.last_error.clone(),
        }
    }

    /// Au démarrage : supprime les installateurs déjà installés (version ≤ courante) et les `.part` orphelins.
    pub fn cleanup(&self) {
        let Ok(dir) = std::fs::read_dir(&self.dir) else { return };
        for e in dir.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Some(rest) = name.strip_prefix("Oniris-") else { continue };
            let stale = match (rest.strip_suffix("-setup.exe"), rest.ends_with("-setup.part")) {
                (Some(v), _) => !is_newer(&self.current, v),
                (None, part) => part,
            };
            if stale {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }

    /// Boucle de fond : vérifie quand `Schedule` le dit (jamais si `auto_update` est faux).
    pub async fn run(self: Arc<Self>) {
        loop {
            tokio::time::sleep(TICK).await;
            let due = self.schedule.lock().due(Instant::now(), (self.enabled)());
            if due {
                self.check().await;
            }
        }
    }

    /// Un cycle (aussi « Vérifier maintenant », même réglage coupé) ; le suivant est dans 6 h, échec compris.
    pub async fn check(&self) {
        {
            let mut s = self.st.lock();
            if s.checking {
                return;
            }
            s.checking = true;
        }
        self.schedule.lock().done(Instant::now());
        let res = self.cycle().await;
        if let Err(e) = &res {
            tracing::warn!("mise à jour : {e}");
        }
        let mut s = self.st.lock();
        s.checking = false;
        s.last_error = res.err();
    }

    async fn cycle(&self) -> Result<(), String> {
        let Some(r) = self.source.latest().await? else { return Ok(()) };
        // `is_newer` valide aussi la version (semver) avant qu'elle n'entre dans un nom de fichier.
        let held = self.st.lock().pending.as_ref().map(|p| p.version.clone());
        if !is_newer(&self.current, &r.version) || held.is_some_and(|h| !is_newer(&h, &r.version)) {
            return Ok(());
        }
        std::fs::create_dir_all(&self.dir).map_err(|e| format!("dossier des mises à jour : {e}"))?;
        let exe = self.dir.join(installer_name(&r.version));
        let part = exe.with_extension("part");
        // Seul un fichier complet et vérifié devient un `.exe` installable.
        let done = match self.source.download(&r, &part).await {
            Ok(()) => std::fs::rename(&part, &exe).map_err(|e| format!("renommage : {e}")),
            Err(e) => Err(e),
        };
        if done.is_err() {
            let _ = std::fs::remove_file(&part);
        }
        done?;
        let old = self.st.lock().pending.replace(Pending { version: r.version.clone(), notes: r.notes.clone(), installer: exe.clone() });
        if let Some(old) = old.filter(|o| o.installer != exe) {
            let _ = std::fs::remove_file(old.installer);
        }
        (self.on_ready)(&r);
        Ok(())
    }

    /// « Redémarrer » : attend le calme, puis lance l'installateur avec relance. Vrai si l'app doit quitter.
    pub async fn restart(&self) -> bool {
        while self.st.lock().pending.is_some() && !can_install((self.calm)()) {
            tokio::time::sleep(CALM_POLL).await;
        }
        // Retirée avant de lancer : la fermeture qui suit ne relance pas l'installateur.
        let Some(p) = self.st.lock().pending.take() else { return false };
        if (self.launch)(&p.installer, true) {
            return true;
        }
        self.st.lock().pending = Some(p);
        false
    }

    /// Fermeture : installe sans relance si une mise à jour est prête et que c'est le calme ; ne bloque pas.
    pub fn install_on_exit(&self) {
        if !can_install((self.calm)()) {
            return;
        }
        let p = self.st.lock().pending.take();
        if let Some(p) = p {
            (self.launch)(&p.installer, false);
        }
    }
}

// --- Branchement Tauri ---

static UPDATER: OnceLock<Arc<Updater>> = OnceLock::new();

/// `%LOCALAPPDATA%\Oniris\updates`.
fn updates_dir() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(std::env::temp_dir).join("Oniris").join("updates")
}

/// `latest.json` de GitHub via `tauri-plugin-updater` ; `download` du plugin vérifie la signature.
struct GithubSource { app: AppHandle, last: Mutex<Option<tauri_plugin_updater::Update>> }

#[async_trait]
impl Source for GithubSource {
    async fn latest(&self) -> Result<Option<Remote>, String> {
        let u = self.app.updater().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string())?;
        let r = u.as_ref().map(|u| Remote { version: u.version.clone(), notes: u.body.clone().unwrap_or_default() });
        *self.last.lock() = u;
        Ok(r)
    }

    async fn download(&self, r: &Remote, to: &Path) -> Result<(), String> {
        let u = self.last.lock().clone().filter(|u| u.version == r.version).ok_or("mise à jour inconnue")?;
        let bytes = u.download(|_, _| {}, || {}).await.map_err(|e| e.to_string())?;
        std::fs::write(to, bytes).map_err(|e| format!("écriture : {e}"))
    }
}

/// Arguments NSIS repris de `tauri-plugin-updater` 2.14.1 (`Update::updater_parameters`, `installMode: passive`,
/// sans argument d'app) : `/P /UPDATE`, plus `/R /ARGS` pour relancer. Lancé par `ShellExecuteW("open")` comme le plugin.
fn launch_installer(path: &Path, relaunch: bool) -> bool {
    use ::windows::core::{w, HSTRING};
    use ::windows::Win32::UI::Shell::ShellExecuteW;
    use ::windows::Win32::UI::WindowsAndMessaging::SW_SHOW;
    let args = if relaunch { "/P /UPDATE /R /ARGS" } else { "/P /UPDATE" };
    // SAFETY: chaînes larges terminées par zéro, vivantes pendant l'appel.
    let code = unsafe { ShellExecuteW(None, w!("open"), &HSTRING::from(path), &HSTRING::from(args), None, SW_SHOW) };
    let ok = code.0 as isize > 32;
    if !ok {
        tracing::warn!("mise à jour : lancement de l'installateur impossible (code {})", code.0 as isize);
    }
    ok
}

pub fn start(app: &AppHandle) {
    let (emit, settings) = (app.clone(), app.clone());
    let u = Arc::new(Updater::new(
        app.package_info().version.to_string(),
        updates_dir(),
        Arc::new(GithubSource { app: app.clone(), last: Mutex::default() }),
        Box::new(launch_installer),
        Box::new(move |r| {
            if let Err(e) = emit.emit("oniris://update", r) {
                tracing::warn!("événement update : {e}");
            }
        }),
        Box::new(|| Calm { voice_busy: crate::modules::voice::pipeline::voice_busy(), fullscreen: crate::fullscreen::active() }),
        Box::new(move || settings.state::<AppState>().settings.read().auto_update),
    ));
    u.cleanup();
    if UPDATER.set(u.clone()).is_ok() {
        tauri::async_runtime::spawn(u.run());
    }
}

/// Appelé dans `RunEvent::Exit` : rend la main tout de suite (l'installateur part détaché).
pub fn install_on_exit(_app: &AppHandle) {
    use ::windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_SHUTTINGDOWN};
    // SAFETY: lecture d'une métrique système, sans pointeur. Extinction de Windows : rien ne s'exécute.
    if unsafe { GetSystemMetrics(SM_SHUTTINGDOWN) } != 0 {
        return;
    }
    if let Some(u) = UPDATER.get() {
        u.install_on_exit();
    }
}

fn updater() -> Result<&'static Arc<Updater>, String> { UPDATER.get().ok_or_else(|| "mise à jour indisponible".into()) }

#[tauri::command]
pub fn update_status() -> Result<UpdateStatus, String> { Ok(updater()?.status()) }

/// « Vérifier maintenant » : marche même réglage coupé ; renvoie l'état après le cycle.
#[tauri::command]
pub async fn update_check_now() -> Result<UpdateStatus, String> {
    let u = updater()?;
    u.check().await;
    Ok(u.status())
}

/// « Redémarrer » : attend le calme, lance l'installateur avec relance, puis quitte l'app.
#[tauri::command]
pub async fn update_restart(app: AppHandle) -> Result<(), String> {
    if updater()?.restart().await {
        app.exit(0);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::Duration;

    #[derive(Default)]
    struct FakeSource { remote: Mutex<Option<Remote>>, fail_latest: AtomicBool, fail_download: AtomicBool, latest_calls: AtomicUsize, downloads: AtomicUsize }

    #[async_trait]
    impl Source for FakeSource {
        async fn latest(&self) -> Result<Option<Remote>, String> {
            self.latest_calls.fetch_add(1, Ordering::SeqCst);
            if self.fail_latest.load(Ordering::SeqCst) {
                return Err("hors ligne".into());
            }
            Ok(self.remote.lock().clone())
        }
        async fn download(&self, r: &Remote, to: &Path) -> Result<(), String> {
            self.downloads.fetch_add(1, Ordering::SeqCst);
            std::fs::write(to, b"MZ partiel").unwrap();
            if self.fail_download.load(Ordering::SeqCst) {
                return Err("coupure".into());
            }
            std::fs::write(to, format!("MZ {}", r.version)).unwrap();
            Ok(())
        }
    }

    type Launched = Arc<Mutex<Vec<(PathBuf, bool)>>>;
    struct Fx { u: Arc<Updater>, src: Arc<FakeSource>, launched: Launched, events: Arc<Mutex<Vec<String>>>, busy: Arc<AtomicBool>, full: Arc<AtomicBool>, dir: tempfile::TempDir }

    fn fx(remote: Option<&str>, enabled: bool) -> Fx {
        let dir = tempfile::tempdir().unwrap();
        let src = Arc::new(FakeSource::default());
        *src.remote.lock() = remote.map(|v| Remote { version: v.into(), notes: format!("notes {v}") });
        let launched: Launched = Arc::default();
        let events: Arc<Mutex<Vec<String>>> = Arc::default();
        let (busy, full) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
        let (l, e, b, f) = (launched.clone(), events.clone(), busy.clone(), full.clone());
        let u = Arc::new(Updater::new(
            "1.2.0".into(),
            dir.path().join("updates"),
            src.clone(),
            Box::new(move |p: &Path, relaunch| { l.lock().push((p.to_path_buf(), relaunch)); true }),
            Box::new(move |r: &Remote| e.lock().push(r.version.clone())),
            Box::new(move || Calm { voice_busy: b.load(Ordering::SeqCst), fullscreen: f.load(Ordering::SeqCst) }),
            Box::new(move || enabled),
        ));
        Fx { u, src, launched, events, busy, full, dir }
    }

    fn files(f: &Fx) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(f.dir.path().join("updates")).map(|d| d.map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
        v.sort();
        v
    }

    fn pending(f: &Fx) -> Option<String> { f.u.status().pending.map(|(v, _)| v) }

    #[tokio::test(start_paused = true)]
    async fn downloads_only_newer_and_emits_once() {
        let f = fx(Some("1.2.1"), true);
        f.u.check().await;
        assert_eq!(pending(&f).as_deref(), Some("1.2.1"));
        assert_eq!(f.u.status().pending.unwrap().1, "notes 1.2.1");
        assert_eq!(files(&f), ["Oniris-1.2.1-setup.exe"]);
        f.u.check().await;
        assert_eq!(f.src.downloads.load(Ordering::SeqCst), 1);
        assert_eq!(*f.events.lock(), ["1.2.1"]);
        // Version égale : rien.
        let g = fx(Some("1.2.0"), true);
        g.u.check().await;
        assert_eq!(g.src.downloads.load(Ordering::SeqCst), 0);
        assert!(pending(&g).is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn partial_download_is_never_installed() {
        let f = fx(Some("1.2.1"), true);
        f.src.fail_download.store(true, Ordering::SeqCst);
        f.u.check().await;
        assert!(files(&f).is_empty(), "{:?}", files(&f));
        assert!(pending(&f).is_none());
        assert!(f.u.status().last_error.is_some());
        f.u.install_on_exit();
        assert!(f.launched.lock().is_empty());
        assert!(f.events.lock().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn newer_download_replaces_pending() {
        let f = fx(Some("1.2.1"), true);
        f.u.check().await;
        *f.src.remote.lock() = Some(Remote { version: "1.2.2".into(), notes: "notes 1.2.2".into() });
        f.u.check().await;
        assert_eq!(pending(&f).as_deref(), Some("1.2.2"));
        assert_eq!(files(&f), ["Oniris-1.2.2-setup.exe"]);
        assert_eq!(*f.events.lock(), ["1.2.1", "1.2.2"]);
    }

    #[tokio::test(start_paused = true)]
    async fn restart_waits_for_calm() {
        let f = fx(Some("1.2.1"), true);
        f.u.check().await;
        f.busy.store(true, Ordering::SeqCst);
        let u = f.u.clone();
        let task = tokio::spawn(async move { u.restart().await });
        tokio::time::sleep(Duration::from_secs(3)).await;
        assert!(f.launched.lock().is_empty());
        f.busy.store(false, Ordering::SeqCst);
        assert!(task.await.unwrap());
        let exe = f.dir.path().join("updates").join("Oniris-1.2.1-setup.exe");
        assert_eq!(*f.launched.lock(), [(exe, true)]);
        // L'app quitte ensuite : la fermeture ne relance pas l'installateur.
        f.u.install_on_exit();
        assert_eq!(f.launched.lock().len(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn exit_installs_without_relaunch_only_when_calm() {
        let f = fx(Some("1.2.1"), true);
        f.u.check().await;
        f.full.store(true, Ordering::SeqCst);
        f.u.install_on_exit();
        assert!(f.launched.lock().is_empty());
        f.full.store(false, Ordering::SeqCst);
        f.u.install_on_exit();
        let exe = f.dir.path().join("updates").join("Oniris-1.2.1-setup.exe");
        assert_eq!(*f.launched.lock(), [(exe, false)]);
    }

    #[tokio::test(start_paused = true)]
    async fn disabled_setting_never_contacts_source() {
        let f = fx(Some("1.2.1"), false);
        tokio::spawn(f.u.clone().run());
        tokio::time::sleep(Duration::from_secs(7 * 3600)).await;
        assert_eq!(f.src.latest_calls.load(Ordering::SeqCst), 0);
        f.u.check().await;
        assert_eq!(f.src.latest_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn source_error_is_quiet() {
        let f = fx(Some("1.2.1"), true);
        f.src.fail_latest.store(true, Ordering::SeqCst);
        tokio::spawn(f.u.clone().run());
        tokio::time::sleep(Duration::from_secs(150)).await;
        assert_eq!(f.src.latest_calls.load(Ordering::SeqCst), 0, "pas avant 3 min");
        tokio::time::sleep(Duration::from_secs(60)).await;
        assert_eq!(f.src.latest_calls.load(Ordering::SeqCst), 1);
        assert_eq!(f.u.status().last_error.as_deref(), Some("hors ligne"));
        assert!(f.events.lock().is_empty());
        tokio::time::sleep(Duration::from_secs(6 * 3600 - 120)).await;
        assert_eq!(f.src.latest_calls.load(Ordering::SeqCst), 1, "pas de boucle serrée");
        tokio::time::sleep(Duration::from_secs(120)).await;
        assert_eq!(f.src.latest_calls.load(Ordering::SeqCst), 2, "nouvel essai à +6 h");
    }

    #[tokio::test(start_paused = true)]
    async fn startup_removes_installed_versions() {
        let f = fx(None, true);
        let d = f.dir.path().join("updates");
        std::fs::create_dir_all(&d).unwrap();
        for n in ["Oniris-1.1.0-setup.exe", "Oniris-1.2.0-setup.exe", "Oniris-1.2.1-setup.part", "Oniris-1.3.0-setup.exe", "autre.txt"] {
            std::fs::write(d.join(n), b"x").unwrap();
        }
        f.u.cleanup();
        assert_eq!(files(&f), ["Oniris-1.3.0-setup.exe", "autre.txt"]);
    }
}
