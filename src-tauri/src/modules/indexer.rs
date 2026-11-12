//! Indexeur de fichiers : parcours basse priorité, observation du disque, instantané.
//! Journal : seulement des compteurs et l'indice de la racine, jamais un chemin (contenu utilisateur),
//! ni le texte des erreurs de `notify` (il contient des chemins) : on ne journalise que leur `kind`.
use crate::state::AppState;
use ignore::WalkBuilder;
use notify::{EventKind, RecursiveMode, Watcher};
use oniris_core::search::{norm_key, under_any, FileIndex};
use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use std::collections::HashSet;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use windows::core::HSTRING;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Storage::FileSystem::{
    CreateFileW, GetFileInformationByHandleEx, FileAttributeTagInfo, FILE_ATTRIBUTE_TAG_INFO, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN};

/// Dossiers jamais indexés (casse ignorée).
const SKIP_NAMES: [&str; 10] = [
    "node_modules", ".git", "target", "dist", "build", "appdata", "__pycache__", ".venv", "$recycle.bin",
    "system volume information",
];
const REPARSE_POINT: u32 = 0x400;
/// HIDDEN | SYSTEM | OFFLINE | RECALL_ON_OPEN | RECALL_ON_DATA_ACCESS (vérifié par un test).
const SKIP_ATTRS: u32 = 0x2 | 0x4 | 0x1000 | 0x4_0000 | 0x40_0000;
/// Étiquettes de reparse à ne pas suivre : jonctions et liens symboliques. Les autres (OneDrive, etc.) sont gardées.
pub const TAG_MOUNT_POINT: u32 = 0xA000_0003;
pub const TAG_SYMLINK: u32 = 0xA000_000C;
const DEBOUNCE: Duration = Duration::from_millis(500);
/// Délai maximal avant de vider un lot, même si les événements continuent d'arriver.
const MAX_WAIT: Duration = Duration::from_secs(2);
const EMIT_EVERY: Duration = Duration::from_millis(500);
const SNAPSHOT_EVERY: Duration = Duration::from_secs(600);
/// Deux parcours complets au plus toutes les 10 minutes.
const REWALK_MIN: Duration = Duration::from_secs(600);
/// Réconciliation périodique : `notify` ne signale jamais de dépassement sous Windows, un événement perdu
/// (file pleine, tampon du système débordé) laisserait l'index faux jusqu'au prochain lancement.
const RECONCILE: Duration = Duration::from_secs(6 * 3600);
/// Chemins retenus au-delà desquels on programme un nouveau parcours complet.
const MAX_BATCH: usize = 200;
const QUEUE: usize = 4096;

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
    pub index: Arc<RwLock<FileIndex>>,
    status: Arc<Mutex<IndexStatus>>,
    /// Premier parcours complet fini (les suivants ne remettent pas l'index en « indexation »).
    first_done: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    #[allow(dead_code)] // lu par les tests
    walks: Arc<AtomicU64>,
}

impl Drop for Indexer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// Exclusions par nom (dossiers seulement), par attributs Windows et par étiquette de reparse
/// (`reparse_tag`, 0 si l'entrée n'est pas un point d'analyse) : jonctions et liens symboliques seulement.
pub fn should_skip(name: &str, attrs: u32, reparse_tag: u32, is_dir: bool) -> bool {
    attrs & SKIP_ATTRS != 0
        || (attrs & REPARSE_POINT != 0 && matches!(reparse_tag, TAG_MOUNT_POINT | TAG_SYMLINK))
        || (is_dir && SKIP_NAMES.iter().any(|n| n.eq_ignore_ascii_case(name)))
}

/// Étiquette du point d'analyse, lue sans le suivre. Échec : on la traite en lien symbolique (non suivi).
fn reparse_tag(path: &Path) -> u32 {
    unsafe {
        let Ok(h) = CreateFileW(
            &HSTRING::from(path.as_os_str()),
            0, // aucun droit d'accès : seuls les attributs nous intéressent
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
            None,
        ) else {
            return TAG_SYMLINK;
        };
        let mut info = FILE_ATTRIBUTE_TAG_INFO::default();
        let ok = GetFileInformationByHandleEx(
            h,
            FileAttributeTagInfo,
            &mut info as *mut _ as *mut _,
            std::mem::size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        );
        let _ = CloseHandle(h);
        if ok.is_ok() { info.ReparseTag } else { TAG_SYMLINK }
    }
}

/// `should_skip` avec l'étiquette lue seulement pour les points d'analyse (les autres entrées n'ont aucun appel en plus).
fn skip_entry(name: &str, attrs: u32, path: &Path, is_dir: bool) -> bool {
    let tag = if attrs & REPARSE_POINT != 0 { reparse_tag(path) } else { 0 };
    should_skip(name, attrs, tag, is_dir)
}

/// `search` (ou le dossier utilisateur s'il est vide) + `projects`, sans les racines contenues dans une autre.
pub fn effective_roots(search: &[String], projects: &[String], home: Option<PathBuf>) -> Vec<PathBuf> {
    let mut all: Vec<PathBuf> = search.iter().map(PathBuf::from).collect();
    if all.is_empty() {
        all.extend(home);
    }
    all.extend(projects.iter().map(PathBuf::from));
    let keys: Vec<String> = all.iter().map(|p| norm_key(&p.to_string_lossy())).collect();
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

/// Un parcours complet est-il permis ? (demandé, et pas de parcours depuis `REWALK_MIN`)
fn rewalk_due(wanted: bool, since_last_walk: Duration) -> bool {
    wanted && since_last_walk >= REWALK_MIN
}

/// Parcourt `root` et envoie chaque entrée à `sink` (la racine elle-même exclue). Renvoie le nombre d'erreurs d'accès.
/// `git_ignore(true)` : lire les `.gitignore` (petits) coûte peu et élague les dossiers de build des dépôts.
fn walk(root: &Path, stop: &AtomicBool, sink: &mut dyn FnMut(String, bool)) -> u64 {
    let walker = WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .git_global(false)
        .ignore(false)
        .follow_links(false)
        .filter_entry(|e| {
            let is_dir = e.file_type().is_some_and(|t| t.is_dir());
            // Les attributs viennent de l'énumération (gratuit sous Windows).
            let attrs = e.metadata().map_or(0, |m| m.file_attributes());
            !skip_entry(&e.file_name().to_string_lossy(), attrs, e.path(), is_dir)
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

/// Filtre à la réception : un événement n'est gardé que si son dossier parent est une racine ou déjà dans l'index
/// (ce qui écarte d'un coup tout sous-arbre caché, système, point d'analyse ou exclu par nom), et si le chemin lui-même
/// n'est pas exclu. Un chemin disparu (suppression) reste gardé : le retirer de l'index est sans danger.
fn keep(path: &Path, roots: &HashSet<String>, dirs: &HashSet<String>) -> bool {
    let Some(name) = path.file_name().map(|n| n.to_string_lossy()) else { return false };
    let Some(parent) = path.parent() else { return false };
    if name.starts_with('.') {
        return false; // fichiers « point » : masqués comme au parcours (`hidden(true)`)
    }
    let pk = norm_key(&parent.to_string_lossy());
    if !(roots.contains(&pk) || dirs.contains(&pk)) {
        return false;
    }
    match std::fs::symlink_metadata(path) {
        Ok(m) => !skip_entry(&name, m.file_attributes(), path, m.is_dir()),
        Err(_) => true,
    }
}

/// Entrées à (ré)insérer pour un lot : chaque chemin existant, plus le sous-arbre des dossiers, sans doublon
/// (un chemin dont un ancêtre est déjà parcouru dans le lot est sauté).
fn build_adds(paths: &HashSet<PathBuf>, stop: &AtomicBool) -> Vec<(String, bool)> {
    let mut sorted: Vec<&PathBuf> = paths.iter().collect();
    sorted.sort_by_key(|p| p.as_os_str().len());
    let (mut adds, mut walked): (Vec<(String, bool)>, Vec<String>) = (Vec::new(), Vec::new());
    for p in sorted {
        let k = norm_key(&p.to_string_lossy());
        if walked.iter().any(|d| k.len() > d.len() && k.starts_with(d.as_str()) && k.as_bytes()[d.len()] == b'\\') {
            continue;
        }
        let Ok(m) = std::fs::symlink_metadata(p) else { continue };
        adds.push((p.to_string_lossy().into_owned(), m.is_dir()));
        if m.is_dir() {
            walked.push(k);
            walk(p, stop, &mut |q, d| adds.push((q, d)));
        }
    }
    adds
}

struct Shared {
    index: Arc<RwLock<FileIndex>>,
    status: Arc<Mutex<IndexStatus>>,
    first_done: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    walks: Arc<AtomicU64>,
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
            tracing::warn!("indexeur : écriture de l'instantané : {:?}", e.kind());
        }
    }

    /// Parcours complet dans un index neuf, échangé à la fin ; puis instantané. Renvoie les dossiers indexés (clés `norm_key`).
    fn full_walk(&self, roots: &[PathBuf]) -> HashSet<String> {
        self.walks.fetch_add(1, Ordering::SeqCst);
        self.publish(IndexStatus::Indexing { done: 0 }, true);
        let (mut fresh, mut dirs) = (FileIndex::new(), HashSet::new());
        for (i, root) in roots.iter().enumerate() {
            let errors = walk(root, &self.stop, &mut |p, d| {
                if d {
                    dirs.insert(norm_key(&p));
                }
                fresh.insert(&p, d);
                if fresh.len().is_multiple_of(4096) {
                    self.publish(IndexStatus::Indexing { done: fresh.len() as u64 }, false);
                }
            });
            if errors > 0 {
                tracing::warn!("indexeur : racine {i} : {errors} erreurs d'accès ignorées");
            }
            if self.stop.load(Ordering::Relaxed) {
                return dirs;
            }
        }
        let count = fresh.len() as u64;
        // L'ancien index est libéré hors du verrou (sa libération est longue, des lecteurs attendent).
        let old = std::mem::replace(&mut *self.index.write(), fresh);
        drop(old);
        tracing::info!("indexeur : parcours terminé, {count} entrées");
        self.write_snapshot();
        self.publish(IndexStatus::Ready { count }, true);
        // Après `Ready` : qui voit le premier parcours fini voit aussi l'état `Ready`.
        self.first_done.store(true, Ordering::SeqCst);
        dirs
    }

    /// Applique un lot : tout est calculé avant le verrou ; sous le verrou, un seul `retain` puis les ajouts.
    fn apply(&self, paths: &HashSet<PathBuf>, dirs: &mut HashSet<String>) {
        let adds = build_adds(paths, &self.stop);
        let prefixes: HashSet<String> = paths.iter().map(|p| norm_key(&p.to_string_lossy())).collect();
        let new_dirs: Vec<String> = adds.iter().filter(|(_, d)| *d).map(|(p, _)| norm_key(p)).collect();
        let count = {
            let mut ix = self.index.write();
            ix.remove_prefixes(&prefixes);
            for (p, d) in &adds {
                ix.insert(p, *d);
            }
            ix.len() as u64
        };
        dirs.retain(|d| !under_any(d, &prefixes));
        dirs.extend(new_dirs);
        self.publish(IndexStatus::Ready { count }, false);
    }

    fn run(&self, roots: &[PathBuf]) {
        // Abaisse la priorité CPU et E/S du fil : l'indexation ne doit jamais gêner l'utilisateur.
        if unsafe { SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN) }.is_err() {
            tracing::warn!("indexeur : priorité basse indisponible");
        }
        // File bornée : si elle se remplit, on note le débordement et un parcours (limité en fréquence) rattrape.
        let (tx, rx) = mpsc::sync_channel(QUEUE);
        let overflow = Arc::new(AtomicBool::new(false));
        let of = overflow.clone();
        // Le watcher part avant le parcours : les événements survenus pendant le parcours restent en file
        // et sont filtrés ensuite contre l'index complet.
        let watcher = notify::recommended_watcher(move |r| {
            if tx.try_send(r).is_err() {
                of.store(true, Ordering::SeqCst);
            }
        });
        let mut watcher = match watcher {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!("indexeur : observation impossible : {:?}", e.kind);
                self.full_walk(roots);
                return; // index sans mise à jour continue, mieux que rien
            }
        };
        for (i, r) in roots.iter().enumerate() {
            if let Err(e) = watcher.watch(r, RecursiveMode::Recursive) {
                tracing::warn!("indexeur : racine {i} non observée : {:?}", e.kind);
            }
        }
        let root_keys: HashSet<String> = roots.iter().map(|r| norm_key(&r.to_string_lossy())).collect();
        let mut dirs = self.full_walk(roots);
        let mut last_walk = Instant::now();
        let (mut pending, mut first_event, mut last_event) = (HashSet::<PathBuf>::new(), None::<Instant>, Instant::now());
        let mut wanted = false;
        let (mut dirty, mut last_snap) = (false, Instant::now());
        while !self.stop.load(Ordering::Relaxed) {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(Ok(ev)) => {
                    if ev.need_rescan() {
                        wanted = true;
                    } else if matches!(ev.kind, EventKind::Create(_) | EventKind::Remove(_) | EventKind::Modify(notify::event::ModifyKind::Name(_))) {
                        let before = pending.len();
                        pending.extend(ev.paths.into_iter().filter(|p| keep(p, &root_keys, &dirs)));
                        if pending.len() > before {
                            first_event.get_or_insert_with(Instant::now);
                            last_event = Instant::now();
                        }
                    }
                }
                Ok(Err(e)) => {
                    tracing::warn!("indexeur : erreur du watcher : {:?}", e.kind);
                    wanted = true; // on ne sait plus ce qui a changé
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            wanted |= overflow.swap(false, Ordering::SeqCst) || last_walk.elapsed() >= RECONCILE;
            let flush = first_event.is_some_and(|f| last_event.elapsed() >= DEBOUNCE || f.elapsed() >= MAX_WAIT);
            if rewalk_due(wanted, last_walk.elapsed()) {
                dirs = self.full_walk(roots);
                (wanted, dirty, last_walk, last_snap) = (false, false, Instant::now(), Instant::now());
                (pending, first_event) = (HashSet::new(), None);
            } else if flush {
                wanted |= pending.len() > MAX_BATCH;
                self.apply(&pending, &mut dirs);
                (pending, first_event, dirty) = (HashSet::new(), None, true);
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
            first_done: Arc::new(AtomicBool::new(false)),
            stop: Arc::new(AtomicBool::new(false)),
            walks: Arc::new(AtomicU64::new(0)),
            emit,
            throttle: Mutex::new(Throttle::new()),
            snapshot,
        });
        let me = Self {
            index: shared.index.clone(),
            status: shared.status.clone(),
            first_done: shared.first_done.clone(),
            stop: shared.stop.clone(),
            walks: shared.walks.clone(),
        };
        if let Err(e) = std::thread::Builder::new().name("oniris-indexer".into()).spawn(move || shared.run(&roots)) {
            tracing::error!("indexeur : fil impossible : {e}");
        }
        me
    }

    /// Dernier état connu.
    pub fn status(&self) -> IndexStatus {
        *self.status.lock()
    }

    pub fn first_walk_done(&self) -> bool {
        self.first_done.load(Ordering::SeqCst)
    }

    #[cfg(test)]
    fn walks(&self) -> u64 {
        self.walks.load(Ordering::SeqCst)
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
    use std::collections::HashSet;
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
    fn should_skip_noms_attributs_et_etiquettes() {
        for n in ["node_modules", ".git", "target", "Node_Modules", "$RECYCLE.BIN", "dist", "build", "AppData"] {
            assert!(should_skip(n, 0, 0, true), "{n}");
        }
        assert!(!should_skip("target", 0, 0, false), "un fichier nommé target reste indexé");
        assert!(!should_skip("src", FILE_ATTRIBUTE_DIRECTORY.0, 0, true));
        for a in [
            FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_SYSTEM, FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS,
            FILE_ATTRIBUTE_RECALL_ON_OPEN, FILE_ATTRIBUTE_OFFLINE,
        ] {
            assert!(should_skip("ok.txt", a.0, 0, false), "{a:?}");
            assert!(should_skip("ok", a.0 | FILE_ATTRIBUTE_DIRECTORY.0, 0, true), "{a:?}");
        }
        assert!(!should_skip("ok.txt", FILE_ATTRIBUTE_ARCHIVE.0, 0, false));
        // Point d'analyse : seulement jonction et lien symbolique ; étiquette cloud (OneDrive) gardée.
        let rp = FILE_ATTRIBUTE_REPARSE_POINT.0;
        const CLOUD: u32 = 0x9000_001A;
        assert!(should_skip("j", rp | FILE_ATTRIBUTE_DIRECTORY.0, TAG_MOUNT_POINT, true));
        assert!(should_skip("l", rp, TAG_SYMLINK, false));
        assert!(!should_skip("od", rp | FILE_ATTRIBUTE_DIRECTORY.0, CLOUD, true));
        assert!(!should_skip("od.txt", rp, CLOUD, false));
        assert!(should_skip("od.txt", rp | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS.0, CLOUD, false), "fichier en ligne seulement");
        assert!(should_skip("od.txt", rp | FILE_ATTRIBUTE_OFFLINE.0, CLOUD, false));
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
        assert_eq!(reparse_tag(&link), TAG_MOUNT_POINT);
        assert_eq!(reparse_tag(d.path()), 0);
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

    #[test]
    fn nouvelle_demande_de_parcours_limitee_en_frequence() {
        assert!(!rewalk_due(false, Duration::from_secs(9999)));
        assert!(!rewalk_due(true, Duration::from_secs(599)));
        assert!(rewalk_due(true, Duration::from_secs(600)));
    }

    #[test]
    fn lot_sans_doublon_dossier_et_enfants() {
        let d = tempfile::tempdir().unwrap();
        touch(&d.path().join("dos/a.txt"));
        touch(&d.path().join("dos/sous/b.txt"));
        touch(&d.path().join("autre.txt"));
        let paths: HashSet<PathBuf> =
            ["dos", "dos/a.txt", "dos/sous", "dos/sous/b.txt", "autre.txt"].iter().map(|p| d.path().join(p)).collect();
        let adds = build_adds(&paths, &AtomicBool::new(false));
        let keys: HashSet<_> = adds.iter().map(|(p, _)| p.clone()).collect();
        assert_eq!(adds.len(), 5, "{adds:?}");
        assert_eq!(keys.len(), 5);
    }

    #[test]
    fn evenement_sous_dossier_cache_jamais_insere() {
        let d = tempfile::tempdir().unwrap();
        touch(&d.path().join("cache/avant.txt"));
        unsafe { SetFileAttributesW(&HSTRING::from(d.path().join("cache").as_os_str()), FILE_ATTRIBUTE_HIDDEN).unwrap() };
        let (_s, sp) = snap();
        let ix = Indexer::spawn(vec![d.path().to_path_buf()], sp, noop());
        wait_ready(&ix);
        std::thread::sleep(Duration::from_millis(300));
        touch(&d.path().join("cache/profond/secret.txt"));
        touch(&d.path().join("cache/direct.txt"));
        touch(&d.path().join("visible.txt"));
        let t = Instant::now();
        while !has(&ix, "visible.txt") {
            assert!(t.elapsed() < Duration::from_secs(2), "témoin non vu : le watcher ne tourne pas");
            std::thread::sleep(Duration::from_millis(20));
        }
        std::thread::sleep(Duration::from_millis(800));
        for n in ["secret.txt", "direct.txt", "avant.txt", "profond"] {
            assert!(!has(&ix, n), "{n}");
        }
    }

    #[test]
    fn rafale_sous_node_modules_sans_nouveau_parcours() {
        let d = tempfile::tempdir().unwrap();
        touch(&d.path().join("node_modules/x/seed.js"));
        touch(&d.path().join("a.txt"));
        let (_s, sp) = snap();
        let ix = Indexer::spawn(vec![d.path().to_path_buf()], sp, noop());
        wait_ready(&ix);
        std::thread::sleep(Duration::from_millis(300));
        for i in 0..5000 {
            std::fs::write(d.path().join("node_modules/x").join(format!("f{i}.js")), "x").unwrap();
        }
        touch(&d.path().join("temoin.txt"));
        let t = Instant::now();
        while !has(&ix, "temoin.txt") {
            assert!(t.elapsed() < Duration::from_secs(3), "témoin non vu");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(ix.walks(), 1, "aucun nouveau parcours");
        assert_eq!(ix.index.read().len(), 2, "a.txt + temoin.txt");
    }

    #[test]
    fn evenements_continus_vides_en_moins_de_deux_secondes() {
        let d = tempfile::tempdir().unwrap();
        touch(&d.path().join("base.txt"));
        let (_s, sp) = snap();
        let ix = Indexer::spawn(vec![d.path().to_path_buf()], sp, noop());
        wait_ready(&ix);
        std::thread::sleep(Duration::from_millis(300));
        let t = Instant::now();
        let mut seen_at = None;
        // Un événement toutes les 100 ms : le trailing debounce de 500 ms ne se déclencherait jamais seul.
        for i in 0..40 {
            std::fs::write(d.path().join(format!("f{i}.txt")), "x").unwrap();
            std::thread::sleep(Duration::from_millis(100));
            if seen_at.is_none() && has(&ix, "f0.txt") {
                seen_at = Some(t.elapsed());
            }
        }
        let s = seen_at.expect("jamais vidé");
        assert!(s < Duration::from_millis(2600), "vidé après {s:?}");
    }
}
