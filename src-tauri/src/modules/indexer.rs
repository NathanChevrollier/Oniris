//! Indexeur de fichiers : parcours basse priorité, observation du disque, instantané.
//! Journal : seulement des compteurs et l'indice de la racine, jamais un chemin (contenu utilisateur).
use crate::state::AppState;
use ignore::WalkBuilder;
use notify::{EventKind, RecursiveMode, Watcher};
use oniris_core::search::FileIndex;
use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use std::collections::HashSet;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN};

/// Dossiers jamais indexés (casse ignorée).
const SKIP_NAMES: [&str; 7] =
    ["node_modules", ".git", "target", "__pycache__", ".venv", "$recycle.bin", "system volume information"];
/// HIDDEN | SYSTEM | REPARSE_POINT | OFFLINE | RECALL_ON_OPEN | RECALL_ON_DATA_ACCESS (vérifié par un test).
const SKIP_ATTRS: u32 = 0x2 | 0x4 | 0x400 | 0x1000 | 0x4_0000 | 0x40_0000;
const DEBOUNCE: Duration = Duration::from_millis(500);
const EMIT_EVERY: Duration = Duration::from_millis(500);
const SNAPSHOT_EVERY: Duration = Duration::from_secs(600);
/// Au-delà, un nouveau parcours complet coûte moins cher que des `remove_prefix` en série.
const MAX_BATCH: usize = 200;

/// État émis vers le panneau (`oniris://index`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "lowercase")]
pub enum IndexStatus {
    Indexing { done: u64 },
    Ready { count: u64 },
}

/// Appelé à chaque changement d'état (au plus deux fois par seconde).
pub type Emit = Arc<dyn Fn(IndexStatus) + Send + Sync>;

/// Index vivant des fichiers du disque : l'instantané le rend utilisable dès le lancement, un parcours complet
/// en tâche de fond le remplace, puis le watcher le tient à jour. S'arrête quand on le lâche.
pub struct Indexer {
    /// Lisible pendant tout le parcours ; remplacé d'un bloc à la fin (verrou d'écriture bref).
    #[allow(dead_code)] // lu par la recherche (tâche 4)
    pub index: Arc<RwLock<FileIndex>>,
    status: Arc<Mutex<IndexStatus>>,
    stop: Arc<AtomicBool>,
}

impl Drop for Indexer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// Exclusions par nom (dossiers seulement) et par attributs Windows (fichiers et dossiers).
pub fn should_skip(name: &str, attrs: u32, is_dir: bool) -> bool {
    attrs & SKIP_ATTRS != 0 || (is_dir && SKIP_NAMES.iter().any(|n| n.eq_ignore_ascii_case(name)))
}

fn norm(p: &str) -> String {
    p.trim_end_matches(['\\', '/']).replace('/', "\\").to_lowercase()
}

/// `search` (ou le dossier utilisateur s'il est vide) + `projects`, sans les racines contenues dans une autre.
pub fn effective_roots(search: &[String], projects: &[String], home: Option<PathBuf>) -> Vec<PathBuf> {
    let mut all: Vec<PathBuf> = search.iter().map(PathBuf::from).collect();
    if all.is_empty() {
        all.extend(home);
    }
    all.extend(projects.iter().map(PathBuf::from));
    let keys: Vec<String> = all.iter().map(|p| norm(&p.to_string_lossy())).collect();
    let inside = |a: &str, b: &str| a.len() > b.len() && a.starts_with(b) && a.as_bytes()[b.len()] == b'\\';
    let mut out: Vec<PathBuf> = Vec::new();
    for (i, p) in all.iter().enumerate() {
        let dup = keys[..i].contains(&keys[i]);
        if !dup && !keys.iter().any(|k| inside(&keys[i], k)) {
            out.push(p.clone());
        }
    }
    out
}

/// Au plus un passage toutes les 500 ms.
pub struct Throttle(Option<Instant>);
impl Throttle {
    pub fn new() -> Self {
        Self(None)
    }
    pub fn allow(&mut self, now: Instant) -> bool {
        if self.0.is_none_or(|t| now.duration_since(t) >= EMIT_EVERY) {
            self.0 = Some(now);
            return true;
        }
        false
    }
}

fn attrs_of(path: &Path) -> u32 {
    // symlink_metadata : une jonction ou un lien n'est jamais suivi.
    std::fs::symlink_metadata(path).map_or(0, |m| m.file_attributes())
}

/// Parcourt `root` et envoie chaque entrée à `sink` (la racine elle-même exclue). Renvoie le nombre d'erreurs d'accès.
fn walk(root: &Path, stop: &AtomicBool, sink: &mut dyn FnMut(String, bool)) -> u64 {
    let walker = WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .git_global(false)
        .ignore(false)
        .follow_links(false)
        .filter_entry(|e| {
            let is_dir = e.file_type().is_some_and(|t| t.is_dir());
            !should_skip(&e.file_name().to_string_lossy(), attrs_of(e.path()), is_dir)
        })
        .build();
    let (mut errors, mut n) = (0u64, 0u32);
    for r in walker {
        n += 1;
        if n % 1024 == 0 && stop.load(Ordering::Relaxed) {
            break;
        }
        match r {
            Ok(e) if e.depth() > 0 => sink(e.path().to_string_lossy().into_owned(), e.file_type().is_some_and(|t| t.is_dir())),
            Ok(_) => {}
            Err(_) => errors += 1,
        }
    }
    errors
}

/// Événement ignoré : hors des racines, ou sous un dossier exclu / point-caché, ou exclu lui-même.
fn path_skipped(path: &Path, roots: &[PathBuf]) -> bool {
    let Some(rel) = roots.iter().find_map(|r| path.strip_prefix(r).ok()) else { return true };
    let names: Vec<_> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    let Some((last, parents)) = names.split_last() else { return true };
    parents.iter().chain(std::iter::once(last)).any(|n| n.starts_with('.') || SKIP_NAMES.iter().any(|s| s.eq_ignore_ascii_case(n)))
        || should_skip(last, attrs_of(path), path.is_dir())
}

struct Shared {
    index: Arc<RwLock<FileIndex>>,
    status: Arc<Mutex<IndexStatus>>,
    stop: Arc<AtomicBool>,
    emit: Emit,
    throttle: Mutex<Throttle>,
    snapshot: PathBuf,
}

impl Shared {
    /// `force` : attend son tour plutôt que de perdre l'état (changements d'état rares).
    fn publish(&self, st: IndexStatus, force: bool) {
        *self.status.lock() = st;
        loop {
            if self.throttle.lock().allow(Instant::now()) {
                (self.emit)(st);
                return;
            }
            if !force || self.stop.load(Ordering::Relaxed) {
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    /// Instantané atomique : fichier temporaire puis renommage.
    fn write_snapshot(&self) {
        let bytes = match self.index.read().snapshot() {
            Ok(b) => b,
            Err(e) => return tracing::warn!("indexeur : instantané impossible : {e}"),
        };
        let tmp = self.snapshot.with_extension("bin.tmp");
        let res = (|| {
            if let Some(dir) = self.snapshot.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(&tmp, bytes)?;
            std::fs::rename(&tmp, &self.snapshot)
        })();
        if let Err(e) = res {
            tracing::warn!("indexeur : écriture de l'instantané : {e}");
        }
    }

    /// Parcours complet dans un index neuf, échangé à la fin ; puis instantané.
    fn full_walk(&self, roots: &[PathBuf]) {
        self.publish(IndexStatus::Indexing { done: 0 }, true);
        let mut fresh = FileIndex::new();
        for (i, root) in roots.iter().enumerate() {
            let errors = walk(root, &self.stop, &mut |p, d| {
                fresh.insert(&p, d);
                if fresh.len().is_multiple_of(4096) {
                    self.publish(IndexStatus::Indexing { done: fresh.len() as u64 }, false);
                }
            });
            if errors > 0 {
                tracing::warn!("indexeur : racine {i} : {errors} erreurs d'accès ignorées");
            }
            if self.stop.load(Ordering::Relaxed) {
                return;
            }
        }
        let count = fresh.len() as u64;
        *self.index.write() = fresh;
        tracing::info!("indexeur : parcours terminé, {count} entrées");
        self.write_snapshot();
        self.publish(IndexStatus::Ready { count }, true);
    }

    /// Applique un lot d'événements : chaque chemin est retiré puis, s'il existe encore, réinséré (avec son sous-arbre).
    fn apply(&self, paths: HashSet<PathBuf>, roots: &[PathBuf]) {
        let mut adds: Vec<(String, bool)> = Vec::new();
        for p in &paths {
            if path_skipped(p, roots) || !p.exists() {
                continue;
            }
            let is_dir = p.is_dir();
            adds.push((p.to_string_lossy().into_owned(), is_dir));
            if is_dir {
                walk(p, &self.stop, &mut |q, d| adds.push((q, d)));
            }
        }
        let mut ix = self.index.write();
        for p in &paths {
            ix.remove_prefix(&p.to_string_lossy());
        }
        for (p, d) in &adds {
            ix.insert(p, *d);
        }
        let count = ix.len() as u64;
        drop(ix);
        self.publish(IndexStatus::Ready { count }, false);
    }

    fn run(&self, roots: &[PathBuf]) {
        // Abaisse la priorité CPU et E/S du fil : l'indexation ne doit jamais gêner l'utilisateur.
        if unsafe { SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN) }.is_err() {
            tracing::warn!("indexeur : priorité basse indisponible");
        }
        // Le watcher part avant le parcours : un changement pendant le parcours n'est pas perdu.
        let (tx, rx) = mpsc::channel();
        let mut watcher = match notify::recommended_watcher(move |r| {
            let _ = tx.send(r);
        }) {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!("indexeur : observation impossible : {e}");
                return self.full_walk(roots); // index sans mise à jour continue, mieux que rien
            }
        };
        for (i, r) in roots.iter().enumerate() {
            if let Err(e) = watcher.watch(r, RecursiveMode::Recursive) {
                tracing::warn!("indexeur : racine {i} non observée : {e}");
            }
        }
        self.full_walk(roots);
        let (mut pending, mut last_event, mut rescan) = (HashSet::<PathBuf>::new(), Instant::now(), false);
        let (mut dirty, mut last_snap) = (false, Instant::now());
        while !self.stop.load(Ordering::Relaxed) {
            let r = rx.recv_timeout(Duration::from_millis(100));
            match r {
                Ok(Ok(ev)) => {
                    if ev.need_rescan() {
                        rescan = true;
                    } else if matches!(ev.kind, EventKind::Create(_) | EventKind::Remove(_) | EventKind::Modify(notify::event::ModifyKind::Name(_))) {
                        pending.extend(ev.paths);
                    } else {
                        continue;
                    }
                    last_event = Instant::now();
                }
                Ok(Err(_)) => {
                    rescan = true; // dépassement ou erreur du watcher : on ne sait plus ce qui a changé
                    last_event = Instant::now();
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            if (rescan || !pending.is_empty()) && last_event.elapsed() >= DEBOUNCE {
                if rescan || pending.len() > MAX_BATCH {
                    self.full_walk(roots);
                    (dirty, last_snap) = (false, Instant::now());
                } else {
                    self.apply(std::mem::take(&mut pending), roots);
                    dirty = true;
                }
                (pending, rescan) = (HashSet::new(), false);
            }
            if dirty && last_snap.elapsed() >= SNAPSHOT_EVERY {
                self.write_snapshot();
                (dirty, last_snap) = (false, Instant::now());
            }
        }
    }
}

impl Indexer {
    /// Restaure l'instantané tout de suite (un instantané corrompu est ignoré et journalisé), puis lance le
    /// fil d'indexation basse priorité sur `roots`. `emit` reçoit l'état, au plus deux fois par seconde.
    pub fn spawn(roots: Vec<PathBuf>, snapshot: PathBuf, emit: Emit) -> Self {
        let mut restored = FileIndex::new();
        if let Ok(bytes) = std::fs::read(&snapshot) {
            match FileIndex::restore(&bytes) {
                Ok(ix) => restored = ix,
                Err(e) => tracing::warn!("indexeur : instantané ignoré : {e}"),
            }
        }
        let shared = Arc::new(Shared {
            index: Arc::new(RwLock::new(restored)),
            status: Arc::new(Mutex::new(IndexStatus::Indexing { done: 0 })),
            stop: Arc::new(AtomicBool::new(false)),
            emit,
            throttle: Mutex::new(Throttle::new()),
            snapshot,
        });
        let me = Self { index: shared.index.clone(), status: shared.status.clone(), stop: shared.stop.clone() };
        if let Err(e) = std::thread::Builder::new().name("oniris-indexer".into()).spawn(move || shared.run(&roots)) {
            tracing::error!("indexeur : fil impossible : {e}");
        }
        me
    }

    /// Dernier état connu.
    pub fn status(&self) -> IndexStatus {
        *self.status.lock()
    }
}

/// À appeler dans `setup`, état géré, hors du fil principal (la restauration lit l'instantané).
/// Racines : `search_roots` (dossier utilisateur si vide) + `projects_roots`.
pub fn start(app: &AppHandle) {
    let st = app.state::<AppState>();
    let (search, projects) = {
        let s = st.settings.read();
        (s.search_roots.clone(), s.projects_roots.clone())
    };
    let roots = effective_roots(&search, &projects, dirs::home_dir());
    let Some(appdata) = std::env::var_os("APPDATA") else { return tracing::error!("indexeur : APPDATA absente") };
    let handle = app.clone();
    let emit: Emit = Arc::new(move |s| {
        if let Err(e) = handle.emit_to("panel", "oniris://index", s) {
            tracing::warn!("événement index : {e}");
        }
    });
    let ix = Indexer::spawn(roots, Path::new(&appdata).join("Oniris").join("index.bin"), emit);
    let _ = st.indexer.set(ix);
}

/// État de l'indexation (`Indexing` tant que l'indexeur n'est pas lancé).
#[tauri::command]
pub async fn index_status(state: tauri::State<'_, AppState>) -> Result<IndexStatus, String> {
    Ok(state.indexer.get().map_or(IndexStatus::Indexing { done: 0 }, Indexer::status))
}

#[cfg(test)]
mod tests {
    use super::*;
    use oniris_core::search::{query, UsageMap};
    use std::os::windows::process::CommandExt;
    use std::path::Path;
    use std::time::Duration;
    use windows::core::HSTRING;
    use windows::Win32::Storage::FileSystem::*;

    fn noop() -> Emit { Arc::new(|_| {}) }
    fn has(ix: &Indexer, name: &str) -> bool {
        query(&ix.index.read(), &[], &UsageMap::default(), name, 50).iter().any(|h| h.item.key.ends_with(name))
    }
    fn wait_ready(ix: &Indexer) {
        let t = Instant::now();
        while !matches!(ix.status(), IndexStatus::Ready { .. }) {
            assert!(t.elapsed() < Duration::from_secs(10), "index jamais prêt");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    fn touch(p: &Path) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, "x").unwrap();
    }
    /// Instantané hors de l'arborescence parcourue.
    fn snap() -> (tempfile::TempDir, PathBuf) { let d = tempfile::tempdir().unwrap(); let p = d.path().join("snap").join("index.bin"); (d, p) }

    #[test]
    fn should_skip_noms_et_chaque_attribut() {
        for n in ["node_modules", ".git", "target", "Node_Modules", "$RECYCLE.BIN"] {
            assert!(should_skip(n, 0, true), "{n}");
        }
        assert!(!should_skip("target", 0, false), "un fichier nommé target reste indexé");
        assert!(!should_skip("src", FILE_ATTRIBUTE_DIRECTORY.0, true));
        for a in [
            FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_SYSTEM, FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS,
            FILE_ATTRIBUTE_RECALL_ON_OPEN, FILE_ATTRIBUTE_OFFLINE, FILE_ATTRIBUTE_REPARSE_POINT,
        ] {
            assert!(should_skip("ok.txt", a.0, false), "{a:?}");
            assert!(should_skip("ok", a.0 | FILE_ATTRIBUTE_DIRECTORY.0, true), "{a:?}");
        }
        assert!(!should_skip("ok.txt", FILE_ATTRIBUTE_ARCHIVE.0, false));
    }

    #[test]
    fn racines_effectives_dedoublonnees() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        let home = Some(PathBuf::from("C:\\Users\\u"));
        // Liste vide : dossier utilisateur ; projet imbriqué retiré.
        assert_eq!(effective_roots(&[], &s(&["C:\\Users\\u\\Projets", "D:\\Code"]), home.clone()), vec![PathBuf::from("C:\\Users\\u"), PathBuf::from("D:\\Code")]);
        // Racine explicite : le dossier utilisateur n'est pas ajouté ; casse et séparateur final ignorés.
        assert_eq!(effective_roots(&s(&["D:\\Code\\"]), &s(&["d:\\code\\app", "d:\\CODE"]), home.clone()), vec![PathBuf::from("D:\\Code\\")]);
        // Préfixe de nom qui n'est pas un dossier parent.
        assert_eq!(effective_roots(&s(&["D:\\Code"]), &s(&["D:\\Code2"]), home).len(), 2);
        assert!(effective_roots(&[], &[], None).is_empty());
    }

    #[test]
    fn limiteur_deux_par_seconde() {
        let mut t = Throttle::new();
        let t0 = Instant::now();
        assert!(t.allow(t0));
        assert!(!t.allow(t0 + Duration::from_millis(100)));
        assert!(!t.allow(t0 + Duration::from_millis(499)));
        assert!(t.allow(t0 + Duration::from_millis(500)));
    }

    #[test]
    fn parcours_exclut_dossiers_caches_et_reparse() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        for f in ["a.txt", "src/main.rs", "node_modules/x.js", ".git/config", "target/o.rs", "hidden.txt", ".dot.txt", "sys/ok.txt"] {
            touch(&r.join(f));
        }
        unsafe { SetFileAttributesW(&HSTRING::from(r.join("hidden.txt").as_os_str()), FILE_ATTRIBUTE_HIDDEN).unwrap() };
        let (_s, sp) = snap();
        let ix = Indexer::spawn(vec![r.to_path_buf()], sp.clone(), noop());
        wait_ready(&ix);
        // a.txt, src, main.rs, sys, ok.txt
        assert_eq!(ix.index.read().len(), 5);
        assert!(has(&ix, "a.txt") && has(&ix, "main.rs") && has(&ix, "ok.txt"));
        for n in ["x.js", "config", "o.rs", "hidden.txt", ".dot.txt"] {
            assert!(!has(&ix, n), "{n}");
        }
    }

    #[test]
    fn jonction_non_suivie() {
        let d = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        touch(&out.path().join("dehors.txt"));
        touch(&d.path().join("a.txt"));
        let link = d.path().join("lien");
        // `mklink /J` ne demande pas de privilège ; fenêtre masquée.
        let ok = std::process::Command::new("cmd")
            .arg("/C").arg("mklink").arg("/J").arg(&link).arg(out.path())
            .creation_flags(0x0800_0000)
            .output().map(|o| o.status.success()).unwrap_or(false);
        assert!(ok, "mklink /J a échoué");
        let (_s, sp) = snap();
        let ix = Indexer::spawn(vec![d.path().to_path_buf()], sp.clone(), noop());
        wait_ready(&ix);
        assert_eq!(ix.index.read().len(), 1, "jonction ni indexée ni suivie");
        assert!(has(&ix, "a.txt") && !has(&ix, "dehors.txt"));
        std::fs::remove_dir(&link).unwrap();
    }

    #[tokio::test]
    async fn creation_puis_suppression_observees_en_moins_d_une_seconde() {
        let d = tempfile::tempdir().unwrap();
        touch(&d.path().join("base.txt"));
        let (_s, sp) = snap();
        let ix = Indexer::spawn(vec![d.path().to_path_buf()], sp.clone(), noop());
        wait_ready(&ix);
        tokio::time::sleep(Duration::from_millis(300)).await; // laisse le watcher s'armer
        let f = d.path().join("nouveau.txt");
        std::fs::write(&f, "x").unwrap();
        let t = Instant::now();
        while !has(&ix, "nouveau.txt") {
            assert!(t.elapsed() < Duration::from_secs(1), "création non vue");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        std::fs::remove_file(&f).unwrap();
        let t = Instant::now();
        while has(&ix, "nouveau.txt") {
            assert!(t.elapsed() < Duration::from_secs(1), "suppression non vue");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(has(&ix, "base.txt"));
    }

    #[test]
    fn instantane_recharge_au_demarrage_puis_ecrit_apres_parcours() {
        let (_s, sp) = snap();
        let root = tempfile::tempdir().unwrap();
        touch(&root.path().join("reel.txt"));
        let mut old = FileIndex::new();
        old.insert("Z:\\fantome\\ancien.txt", false);
        std::fs::create_dir_all(sp.parent().unwrap()).unwrap();
        std::fs::write(&sp, old.snapshot().unwrap()).unwrap();
        let ix = Indexer::spawn(vec![root.path().to_path_buf()], sp.clone(), noop());
        assert!(has(&ix, "ancien.txt"), "instantané utilisable tout de suite");
        wait_ready(&ix);
        assert!(!has(&ix, "ancien.txt") && has(&ix, "reel.txt"), "remplacé à la fin du parcours");
        let t = Instant::now();
        while FileIndex::restore(&std::fs::read(&sp).unwrap()).map_or(true, |i| i.len() != 1) {
            assert!(t.elapsed() < Duration::from_secs(5), "instantané non réécrit");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!sp.with_extension("bin.tmp").exists());
    }

    #[test]
    fn instantane_corrompu_ignore() {
        let (_s, sp) = snap();
        let root = tempfile::tempdir().unwrap();
        touch(&root.path().join("reel.txt"));
        std::fs::create_dir_all(sp.parent().unwrap()).unwrap();
        std::fs::write(&sp, [9u8, 9, 9, 9]).unwrap();
        let ix = Indexer::spawn(vec![root.path().to_path_buf()], sp, noop());
        wait_ready(&ix);
        assert!(has(&ix, "reel.txt"));
    }
}
