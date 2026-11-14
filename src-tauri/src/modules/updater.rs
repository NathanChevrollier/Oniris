//! Mise à jour : vérifie `latest.json` (45 s après le démarrage, toutes les 2 h, au réveil de veille et à
//! l'ouverture du panneau si la dernière vérification a plus d'1 h ; jamais en plein écran), télécharge et
//! vérifie la signature en fond dans `%LOCALAPPDATA%\Oniris\updates\` (`.part`, renommé en `.exe` une fois
//! vérifié), et lance l'installateur seulement au calme : avec relance sur « Redémarrer », sans à la fermeture.
//! Chaque changement d'état part dans `oniris://update` (même forme que `update_status`).
use crate::state::AppState;
use async_trait::async_trait;
use oniris_core::update::{can_install, is_newer, slept, Calm, Schedule};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime};
use tauri::{AppHandle, Emitter, Listener, Manager};
use tauri_plugin_updater::UpdaterExt;
use tokio::time::Instant;

/// Pas de la boucle : relit le réglage `auto_update`, l'échéance et l'horloge murale (réveil).
const TICK: Duration = Duration::from_secs(60);
/// Relecture du calme pendant qu'un « Redémarrer » attend.
const CALM_POLL: Duration = Duration::from_secs(2);
/// « Installation… » reste visible un instant avant que l'installateur ne ferme l'app.
const INSTALL_NOTICE: Duration = Duration::from_millis(1500);
/// Notes de la version téléchargée, relues au premier lancement de cette version pour l'annoncer.
const NOTES_FILE: &str = "notes.json";
const LAUNCH_FAILED: &str = "L'installateur n'a pas pu être lancé.";

/// Progression d'un téléchargement : octets reçus, taille totale si connue.
pub type Progress<'a> = &'a (dyn Fn(u64, Option<u64>) + Sync);

#[async_trait]
pub trait Source: Send + Sync {
    /// Dernière version publiée (`None` : rien de plus récent).
    async fn latest(&self) -> Result<Option<Remote>, String>;
    /// Écrit dans `to` l'installateur de `r`, seulement si sa signature est valide.
    async fn download(&self, r: &Remote, to: &Path, progress: Progress<'_>) -> Result<(), String>;
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Remote { pub version: String, pub notes: String }

pub struct Pending { pub version: String, pub notes: String, pub installer: PathBuf }

/// État affiché par la pilule et les Réglages (champ `state`).
#[derive(Clone, Serialize, Debug, PartialEq, Default)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Phase {
    #[default]
    Idle,
    Checking,
    Downloading { version: String, percent: Option<u8> },
    Ready { version: String, notes: String },
    /// « Redémarrer » attend le calme : `reason` vaut `game` (plein écran) ou `voice`.
    Waiting { version: String, reason: &'static str },
    Installing { version: String },
    /// `version` : échec de téléchargement ou d'installation de cette version ; `None` : vérification.
    Error { message: String, version: Option<String> },
}

#[derive(Serialize, Clone, Debug)]
pub struct UpdateStatus {
    current: String,
    pending: Option<(String, String)>,
    checking: bool,
    last_error: Option<String>,
    /// Version installée depuis le lancement précédent, avec ses notes (annonce « Oniris est passé en X »).
    updated: Option<Remote>,
    #[serde(flatten)]
    phase: Phase,
}

#[derive(Default)]
struct St { pending: Option<Pending>, checking: bool, last_error: Option<String>, phase: Phase, updated: Option<Remote> }

/// Fin de vérification quel que soit le chemin de sortie (panique ou future abandonnée comprises).
struct CheckGuard<'a>(&'a Mutex<St>);
impl Drop for CheckGuard<'_> {
    fn drop(&mut self) { self.0.lock().checking = false }
}

type Launch = Box<dyn Fn(&Path, bool) -> bool + Send + Sync>;

pub struct Updater {
    current: String,
    dir: PathBuf,
    source: Arc<dyn Source>,
    /// Lance l'installateur (`relaunch` : l'app redémarre après) ; faux si le lancement a échoué.
    launch: Launch,
    emit: Box<dyn Fn(&UpdateStatus) + Send + Sync>,
    calm: Box<dyn Fn() -> Calm + Send + Sync>,
    enabled: Box<dyn Fn() -> bool + Send + Sync>,
    st: Mutex<St>,
    schedule: Mutex<Schedule>,
}

fn installer_name(version: &str) -> String { format!("Oniris-{version}-setup.exe") }

fn percent(done: u64, total: Option<u64>) -> Option<u8> {
    total.filter(|t| *t > 0).map(|t| (done.saturating_mul(100) / t).min(100) as u8)
}

impl Updater {
    pub fn new(
        current: String,
        dir: PathBuf,
        source: Arc<dyn Source>,
        launch: Launch,
        emit: Box<dyn Fn(&UpdateStatus) + Send + Sync>,
        calm: Box<dyn Fn() -> Calm + Send + Sync>,
        enabled: Box<dyn Fn() -> bool + Send + Sync>,
    ) -> Self {
        let schedule = Mutex::new(Schedule::new(Instant::now()));
        Self { current, dir, source, launch, emit, calm, enabled, st: Mutex::default(), schedule }
    }

    pub fn status(&self) -> UpdateStatus {
        let s = self.st.lock();
        UpdateStatus {
            current: self.current.clone(),
            pending: s.pending.as_ref().map(|p| (p.version.clone(), p.notes.clone())),
            checking: s.checking,
            last_error: s.last_error.clone(),
            updated: s.updated.clone(),
            phase: s.phase.clone(),
        }
    }

    /// Change d'état et prévient les fenêtres ; rien n'est émis si l'état ne change pas.
    fn set(&self, phase: Phase) {
        {
            let mut s = self.st.lock();
            if s.phase == phase {
                return;
            }
            s.phase = phase;
        }
        (self.emit)(&self.status());
    }

    /// Au démarrage : relève l'annonce de la version tout juste installée (une seule fois : le fichier est
    /// supprimé), puis supprime installateurs et `.part` laissés par un lancement précédent.
    // shortcut: un installateur plus récent resté sur disque n'est pas repris (signature non revérifiée), il est retéléchargé ; upgrade : revérifier sa signature avec la clé publique pour le reprendre.
    pub fn cleanup(&self) {
        let notes = self.dir.join(NOTES_FILE);
        if let Some(r) = std::fs::read(&notes).ok().and_then(|b| serde_json::from_slice::<Remote>(&b).ok()) {
            if r.version == self.current {
                self.st.lock().updated = Some(r);
            }
        }
        let _ = std::fs::remove_file(&notes);
        let Ok(dir) = std::fs::read_dir(&self.dir) else { return };
        for e in dir.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with("Oniris-") && (name.ends_with("-setup.exe") || name.ends_with("-setup.part")) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }

    /// Boucle de fond : un pas par minute ; l'horloge murale révèle les sorties de veille.
    pub async fn run(self: Arc<Self>) {
        loop {
            let before = SystemTime::now();
            tokio::time::sleep(TICK).await;
            self.tick(SystemTime::now().duration_since(before).unwrap_or_default()).await;
        }
    }

    /// Un pas de la boucle : vérifie si `Schedule` le dit (jamais si `auto_update` est faux ni en plein écran).
    pub async fn tick(&self, wall_elapsed: Duration) {
        let now = Instant::now();
        if slept(wall_elapsed, TICK) {
            self.schedule.lock().woke(now);
        }
        let due = self.schedule.lock().due(now, (self.enabled)());
        if due && !(self.calm)().fullscreen {
            self.check().await;
        }
    }

    /// Ouverture du panneau : vérifie si la dernière vérification date de plus d'1 h.
    pub async fn panel_opened(&self) {
        let stale = self.schedule.lock().stale(Instant::now(), (self.enabled)());
        if stale && !(self.calm)().fullscreen {
            self.check().await;
        }
    }

    /// Un cycle (aussi « Vérifier maintenant », même réglage coupé) ; le suivant est dans 2 h, échec compris.
    pub async fn check(&self) {
        {
            let mut s = self.st.lock();
            if s.checking {
                return;
            }
            s.checking = true;
        }
        let _guard = CheckGuard(&self.st);
        self.schedule.lock().done(Instant::now());
        if matches!(self.st.lock().phase, Phase::Idle | Phase::Error { .. }) {
            self.set(Phase::Checking);
        }
        let res = self.cycle().await;
        let failed_version = match &self.st.lock().phase {
            Phase::Downloading { version, .. } => Some(version.clone()),
            _ => None,
        };
        let err = res.err().map(|e| {
            tracing::warn!("mise à jour : {e}");
            let msg = if failed_version.is_some() { "Le téléchargement a échoué (connexion coupée ou fichier invalide)." } else { "GitHub ne répond pas (connexion coupée ?)." };
            msg.to_string()
        });
        let phase = {
            let mut s = self.st.lock();
            s.last_error = err.clone();
            match (&s.phase, &s.pending, err) {
                // « Redémarrer » en cours : son état (attente, installation) reste affiché.
                (Phase::Waiting { .. } | Phase::Installing { .. }, _, _) => None,
                (_, Some(p), _) => Some(Phase::Ready { version: p.version.clone(), notes: p.notes.clone() }),
                (_, None, Some(message)) => Some(Phase::Error { message, version: failed_version }),
                (_, None, None) => Some(Phase::Idle),
            }
        };
        if let Some(phase) = phase {
            self.set(phase);
        }
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
        let downloading = |percent| Phase::Downloading { version: r.version.clone(), percent };
        self.set(downloading(None));
        let progress = |done, total| self.set(downloading(percent(done, total)));
        // Seul un fichier complet et vérifié devient un `.exe` installable.
        let done = match self.source.download(&r, &part, &progress).await {
            Ok(()) => std::fs::rename(&part, &exe).map_err(|e| format!("renommage : {e}")),
            Err(e) => Err(e),
        };
        if done.is_err() {
            let _ = std::fs::remove_file(&part);
        }
        done?;
        if let Err(e) = std::fs::write(self.dir.join(NOTES_FILE), serde_json::to_vec(&r).unwrap_or_default()) {
            tracing::warn!("mise à jour : notes non enregistrées : {e}");
        }
        let old = self.st.lock().pending.replace(Pending { version: r.version.clone(), notes: r.notes.clone(), installer: exe.clone() });
        if let Some(old) = old.filter(|o| o.installer != exe) {
            let _ = std::fs::remove_file(old.installer);
        }
        Ok(())
    }

    /// « Redémarrer » : attend le calme, puis lance l'installateur avec relance. Vrai si l'app doit quitter.
    pub async fn restart(&self) -> bool {
        loop {
            let Some(version) = self.st.lock().pending.as_ref().map(|p| p.version.clone()) else { return false };
            let c = (self.calm)();
            if can_install(c) {
                self.set(Phase::Installing { version });
                break;
            }
            self.set(Phase::Waiting { version, reason: if c.fullscreen { "game" } else { "voice" } });
            tokio::time::sleep(CALM_POLL).await;
        }
        tokio::time::sleep(INSTALL_NOTICE).await;
        // Retirée avant de lancer : la fermeture qui suit ne relance pas l'installateur.
        let Some(p) = self.st.lock().pending.take() else { return false };
        if (self.launch)(&p.installer, true) {
            return true;
        }
        let version = p.version.clone();
        {
            let mut s = self.st.lock();
            // Une version plus récente a pu arriver pendant le lancement : elle garde la place, l'ancienne part.
            if s.pending.is_none() {
                s.pending = Some(p);
            } else {
                let _ = std::fs::remove_file(&p.installer);
            }
            s.last_error = Some(LAUNCH_FAILED.into());
        }
        self.set(Phase::Error { message: LAUNCH_FAILED.into(), version: Some(version) });
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

    async fn download(&self, r: &Remote, to: &Path, progress: Progress<'_>) -> Result<(), String> {
        let u = self.last.lock().clone().filter(|u| u.version == r.version).ok_or("mise à jour inconnue")?;
        let mut done = 0u64;
        let bytes = u.download(|n, total| { done += n as u64; progress(done, total) }, || {}).await.map_err(|e| e.to_string())?;
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
        Box::new(move |st: &UpdateStatus| {
            if let Err(e) = emit.emit("oniris://update", st) {
                tracing::warn!("événement update : {e}");
            }
        }),
        Box::new(|| Calm { voice_busy: crate::modules::voice::pipeline::voice_busy(), fullscreen: crate::fullscreen::active() }),
        Box::new(move || settings.state::<AppState>().settings.read().auto_update),
    ));
    u.cleanup();
    if UPDATER.set(u.clone()).is_ok() {
        tauri::async_runtime::spawn(u.run());
        // Ouverture du panneau (émise par `windows::open_panel`) : vérification si la dernière est ancienne.
        app.listen_any("oniris://panel-state", |e| {
            let open = serde_json::from_str::<serde_json::Value>(e.payload()).ok().and_then(|v| v["open"].as_bool());
            if let (Some(true), Some(u)) = (open, UPDATER.get()) {
                let u = u.clone();
                tauri::async_runtime::spawn(async move { u.panel_opened().await });
            }
        });
    }
}

/// « Nouveautés » : la page de la version installée sur GitHub (version issue du paquet, pas d'une entrée).
#[tauri::command]
pub fn update_whats_new(app: AppHandle) {
    crate::launch::open_url(&format!("https://github.com/NathanChevrollier/Oniris/releases/tag/v{}", app.package_info().version));
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
    struct FakeSource { remote: Mutex<Option<Remote>>, fail_latest: AtomicBool, panic_latest: AtomicBool, fail_download: AtomicBool, latest_calls: AtomicUsize, downloads: AtomicUsize }

    #[async_trait]
    impl Source for FakeSource {
        async fn latest(&self) -> Result<Option<Remote>, String> {
            self.latest_calls.fetch_add(1, Ordering::SeqCst);
            if self.panic_latest.swap(false, Ordering::SeqCst) {
                panic!("source en panique");
            }
            if self.fail_latest.load(Ordering::SeqCst) {
                return Err("hors ligne".into());
            }
            Ok(self.remote.lock().clone())
        }
        async fn download(&self, r: &Remote, to: &Path, progress: Progress<'_>) -> Result<(), String> {
            self.downloads.fetch_add(1, Ordering::SeqCst);
            std::fs::write(to, b"MZ partiel").unwrap();
            progress(50, Some(200));
            progress(51, Some(200)); // même pourcentage : pas de nouvel événement
            if self.fail_download.load(Ordering::SeqCst) {
                return Err("coupure".into());
            }
            progress(200, Some(200));
            std::fs::write(to, format!("MZ {}", r.version)).unwrap();
            Ok(())
        }
    }

    type Launched = Arc<Mutex<Vec<(PathBuf, bool)>>>;
    struct Fx { u: Arc<Updater>, src: Arc<FakeSource>, launched: Launched, events: Arc<Mutex<Vec<Phase>>>, busy: Arc<AtomicBool>, full: Arc<AtomicBool>, launch_ok: Arc<AtomicBool>, dir: tempfile::TempDir }

    fn fx_in(dir: tempfile::TempDir, current: &str, remote: Option<&str>, enabled: bool) -> Fx {
        let src = Arc::new(FakeSource::default());
        *src.remote.lock() = remote.map(|v| Remote { version: v.into(), notes: format!("notes {v}") });
        let launched: Launched = Arc::default();
        let events: Arc<Mutex<Vec<Phase>>> = Arc::default();
        let (busy, full) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
        let launch_ok = Arc::new(AtomicBool::new(true));
        let (l, e, b, f, ok) = (launched.clone(), events.clone(), busy.clone(), full.clone(), launch_ok.clone());
        let u = Arc::new(Updater::new(
            current.into(),
            dir.path().join("updates"),
            src.clone(),
            Box::new(move |p: &Path, relaunch| { l.lock().push((p.to_path_buf(), relaunch)); ok.load(Ordering::SeqCst) }),
            Box::new(move |s: &UpdateStatus| e.lock().push(s.phase.clone())),
            Box::new(move || Calm { voice_busy: b.load(Ordering::SeqCst), fullscreen: f.load(Ordering::SeqCst) }),
            Box::new(move || enabled),
        ));
        Fx { u, src, launched, events, busy, full, launch_ok, dir }
    }

    fn fx(remote: Option<&str>, enabled: bool) -> Fx { fx_in(tempfile::tempdir().unwrap(), "1.2.0", remote, enabled) }

    fn files(f: &Fx) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(f.dir.path().join("updates")).map(|d| d.map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
        v.sort();
        v
    }

    fn pending(f: &Fx) -> Option<String> { f.u.status().pending.map(|(v, _)| v) }
    fn readies(f: &Fx) -> Vec<String> { f.events.lock().iter().filter_map(|p| match p { Phase::Ready { version, .. } => Some(version.clone()), _ => None }).collect() }
    fn calls(f: &Fx) -> usize { f.src.latest_calls.load(Ordering::SeqCst) }
    fn ready(v: &str) -> Phase { Phase::Ready { version: v.into(), notes: format!("notes {v}") } }

    #[tokio::test(start_paused = true)]
    async fn downloads_only_newer_and_emits_once() {
        let f = fx(Some("1.2.1"), true);
        f.u.check().await;
        assert_eq!(pending(&f).as_deref(), Some("1.2.1"));
        assert_eq!(f.u.status().pending.unwrap().1, "notes 1.2.1");
        assert_eq!(files(&f), ["Oniris-1.2.1-setup.exe", "notes.json"]);
        f.u.check().await;
        assert_eq!(f.src.downloads.load(Ordering::SeqCst), 1);
        assert_eq!(readies(&f), ["1.2.1"]);
        // Version égale : rien.
        let g = fx(Some("1.2.0"), true);
        g.u.check().await;
        assert_eq!(g.src.downloads.load(Ordering::SeqCst), 0);
        assert!(pending(&g).is_none());
        assert_eq!(*g.events.lock(), [Phase::Checking, Phase::Idle]);
    }

    #[tokio::test(start_paused = true)]
    async fn states_while_downloading() {
        let f = fx(Some("1.2.1"), true);
        f.u.check().await;
        let dl = |percent| Phase::Downloading { version: "1.2.1".into(), percent };
        assert_eq!(*f.events.lock(), [Phase::Checking, dl(None), dl(Some(25)), dl(Some(100)), ready("1.2.1")]);
        let j = serde_json::to_value(f.u.status()).unwrap();
        assert_eq!((j["state"].as_str(), j["version"].as_str(), j["pending"][0].as_str()), (Some("ready"), Some("1.2.1"), Some("1.2.1")));
    }

    #[tokio::test(start_paused = true)]
    async fn partial_download_is_never_installed() {
        let f = fx(Some("1.2.1"), true);
        f.src.fail_download.store(true, Ordering::SeqCst);
        f.u.check().await;
        assert!(files(&f).is_empty(), "{:?}", files(&f));
        assert!(pending(&f).is_none());
        assert!(f.u.status().last_error.is_some());
        assert!(matches!(f.events.lock().last(), Some(Phase::Error { version: Some(v), .. }) if v == "1.2.1"));
        f.u.install_on_exit();
        assert!(f.launched.lock().is_empty());
        assert!(readies(&f).is_empty());
        // « Réessayer » : la vérification suivante repart et réussit.
        f.src.fail_download.store(false, Ordering::SeqCst);
        f.u.check().await;
        assert_eq!(f.events.lock().last(), Some(&ready("1.2.1")));
        assert!(f.u.status().last_error.is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn check_error_is_readable_and_keeps_ready_update() {
        let f = fx(Some("1.2.1"), true);
        f.src.fail_latest.store(true, Ordering::SeqCst);
        f.u.check().await;
        assert!(matches!(f.events.lock().last(), Some(Phase::Error { version: None, message }) if message.contains("GitHub")));
        f.src.fail_latest.store(false, Ordering::SeqCst);
        f.u.check().await;
        f.src.fail_latest.store(true, Ordering::SeqCst);
        f.u.check().await;
        assert_eq!(f.events.lock().last(), Some(&ready("1.2.1")), "une vérification ratée n'efface pas la mise à jour prête");
    }

    #[tokio::test(start_paused = true)]
    async fn newer_download_replaces_pending() {
        let f = fx(Some("1.2.1"), true);
        f.u.check().await;
        *f.src.remote.lock() = Some(Remote { version: "1.2.2".into(), notes: "notes 1.2.2".into() });
        f.u.check().await;
        assert_eq!(pending(&f).as_deref(), Some("1.2.2"));
        assert_eq!(files(&f), ["Oniris-1.2.2-setup.exe", "notes.json"]);
        assert_eq!(readies(&f), ["1.2.1", "1.2.2"]);
    }

    #[tokio::test(start_paused = true)]
    async fn restart_waits_for_calm_and_says_why() {
        let f = fx(Some("1.2.1"), true);
        f.u.check().await;
        f.busy.store(true, Ordering::SeqCst);
        let u = f.u.clone();
        let task = tokio::spawn(async move { u.restart().await });
        tokio::time::sleep(Duration::from_secs(3)).await;
        f.full.store(true, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_secs(3)).await;
        assert!(f.launched.lock().is_empty());
        f.busy.store(false, Ordering::SeqCst);
        f.full.store(false, Ordering::SeqCst);
        assert!(task.await.unwrap());
        let exe = f.dir.path().join("updates").join("Oniris-1.2.1-setup.exe");
        assert_eq!(*f.launched.lock(), [(exe, true)]);
        let wait = |reason| Phase::Waiting { version: "1.2.1".into(), reason };
        assert!(f.events.lock().ends_with(&[ready("1.2.1"), wait("voice"), wait("game"), Phase::Installing { version: "1.2.1".into() }]), "{:?}", f.events.lock());
        // L'app quitte ensuite : la fermeture ne relance pas l'installateur.
        f.u.install_on_exit();
        assert_eq!(f.launched.lock().len(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn restart_keeps_pending_when_launch_fails() {
        let f = fx(Some("1.2.1"), true);
        f.u.check().await;
        f.launch_ok.store(false, Ordering::SeqCst);
        assert!(!f.u.restart().await, "pas de sortie de l'app");
        assert_eq!(f.launched.lock().len(), 1);
        assert_eq!(pending(&f).as_deref(), Some("1.2.1"));
        assert_eq!(files(&f), ["Oniris-1.2.1-setup.exe", "notes.json"]);
        assert!(matches!(f.events.lock().last(), Some(Phase::Error { version: Some(v), .. }) if v == "1.2.1"));
        // « Réessayer » relance l'installation.
        f.launch_ok.store(true, Ordering::SeqCst);
        assert!(f.u.restart().await);
    }

    #[tokio::test(start_paused = true)]
    async fn panicking_check_does_not_block_later_checks() {
        let f = fx(Some("1.2.1"), true);
        f.src.panic_latest.store(true, Ordering::SeqCst);
        let u = f.u.clone();
        assert!(tokio::spawn(async move { u.check().await }).await.is_err());
        assert!(!f.u.status().checking);
        f.u.check().await;
        assert_eq!(calls(&f), 2);
        assert_eq!(pending(&f).as_deref(), Some("1.2.1"));
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
        f.u.panel_opened().await;
        f.u.tick(Duration::from_secs(8 * 3600)).await;
        assert_eq!(calls(&f), 0);
        f.u.check().await;
        assert_eq!(calls(&f), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn startup_check_then_every_two_hours() {
        let f = fx(Some("1.2.0"), true);
        f.src.fail_latest.store(true, Ordering::SeqCst);
        tokio::spawn(f.u.clone().run());
        tokio::time::sleep(Duration::from_secs(30)).await;
        assert_eq!(calls(&f), 0, "pas tout de suite au démarrage");
        tokio::time::sleep(Duration::from_secs(31)).await;
        assert_eq!(calls(&f), 1, "première vérification dans la minute");
        tokio::time::sleep(Duration::from_secs(2 * 3600 - 120)).await;
        assert_eq!(calls(&f), 1, "pas de boucle serrée après un échec");
        tokio::time::sleep(Duration::from_secs(120)).await;
        assert_eq!(calls(&f), 2, "nouvel essai à +2 h");
    }

    #[tokio::test(start_paused = true)]
    async fn panel_opening_checks_only_when_last_check_is_old() {
        let f = fx(Some("1.2.0"), true);
        f.u.panel_opened().await;
        assert_eq!(calls(&f), 1, "aucune vérification encore : on vérifie");
        tokio::time::sleep(Duration::from_secs(1800)).await;
        f.u.panel_opened().await;
        assert_eq!(calls(&f), 1, "vérifié il y a 30 min");
        tokio::time::sleep(Duration::from_secs(1800)).await;
        f.u.panel_opened().await;
        assert_eq!(calls(&f), 2);
        // Plein écran : rien, même si c'est ancien.
        tokio::time::sleep(Duration::from_secs(3600)).await;
        f.full.store(true, Ordering::SeqCst);
        f.u.panel_opened().await;
        f.u.tick(TICK).await;
        assert_eq!(calls(&f), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn wake_from_sleep_checks_soon_without_bursts() {
        let f = fx(Some("1.2.0"), true);
        tokio::time::sleep(Duration::from_secs(60)).await;
        f.u.tick(TICK).await;
        assert_eq!(calls(&f), 1);
        // Veille de 8 h : l'horloge murale saute ; vérification 30 s après le réveil (laisser revenir le réseau).
        tokio::time::sleep(Duration::from_secs(1200)).await;
        f.u.tick(Duration::from_secs(8 * 3600)).await;
        assert_eq!(calls(&f), 1);
        tokio::time::sleep(Duration::from_secs(30)).await;
        f.u.tick(TICK).await;
        assert_eq!(calls(&f), 2);
        // Nouveau réveil 2 min plus tard : pas avant l'intervalle minimal de 10 min.
        tokio::time::sleep(Duration::from_secs(120)).await;
        f.u.tick(Duration::from_secs(3600)).await;
        tokio::time::sleep(Duration::from_secs(60)).await;
        f.u.tick(TICK).await;
        assert_eq!(calls(&f), 2);
        tokio::time::sleep(Duration::from_secs(420)).await;
        f.u.tick(TICK).await;
        assert_eq!(calls(&f), 3);
    }

    #[tokio::test(start_paused = true)]
    async fn startup_removes_installers_and_announces_new_version_once() {
        // Lancement en 1.2.0 : la 1.2.1 est téléchargée, ses notes sont gardées.
        let f = fx(Some("1.2.1"), true);
        f.u.check().await;
        let d = f.dir.path().join("updates");
        for n in ["Oniris-1.1.0-setup.exe", "Oniris-1.3.0-setup.part", "autre.txt"] {
            std::fs::write(d.join(n), b"x").unwrap();
        }
        // Lancement suivant, en 1.2.1 (l'installateur a tourné).
        let g = fx_in(f.dir, "1.2.1", None, true);
        g.u.cleanup();
        assert_eq!(g.u.status().updated, Some(Remote { version: "1.2.1".into(), notes: "notes 1.2.1".into() }));
        assert_eq!(files(&g), ["autre.txt"]);
        // Et le suivant : plus d'annonce.
        let h = fx_in(g.dir, "1.2.1", None, true);
        h.u.cleanup();
        assert!(h.u.status().updated.is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn no_announcement_when_notes_are_for_another_version() {
        let f = fx(Some("1.2.2"), true);
        f.u.check().await;
        let g = fx_in(f.dir, "1.2.1", None, true);
        g.u.cleanup();
        assert!(g.u.status().updated.is_none());
    }
}
