//! Projets : inventaire recalculé en tâche de fond (disque, puis git à 4 de front), jamais sur le chemin d'ouverture.
//! Journal : des comptes seulement (chemins, titres de session et branches sont du contenu utilisateur).
use crate::launch::spawn_hidden;
use crate::state::AppState;
use oniris_core::events::Topic;
use oniris_core::projects::{self, git, Project};
use oniris_core::time_util::now_ms;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Manager, State};

const STALE_MS: i64 = 2 * 60 * 1000;
const GIT_TIMEOUT: Duration = Duration::from_secs(3);
const GIT_PARALLEL: usize = 4;

static REFRESHING: AtomicBool = AtomicBool::new(false);

/// Libère `REFRESHING` même si le calcul panique.
struct Guard;
impl Drop for Guard {
    fn drop(&mut self) {
        REFRESHING.store(false, Ordering::SeqCst);
    }
}

/// Sortie standard d'un `git -C <path> <args>` ; `None` si échec, code de sortie non nul ou délai de 3 s dépassé
/// (le processus est alors tué). `GIT_OPTIONAL_LOCKS=0` : git ne prend jamais le verrou de l'index.
fn run_git(path: &Path, args: &[&str]) -> Option<String> {
    let mut full: Vec<String> = ["-c", "core.fsmonitor=false", "-C"].iter().map(|s| s.to_string()).collect();
    full.push(path.to_string_lossy().into_owned());
    full.extend(args.iter().map(|s| s.to_string()));
    let mut child = spawn_hidden("git", &full, None, &[("GIT_OPTIONAL_LOCKS", "0")]).ok()?;
    let mut out = child.stdout.take()?;
    let (tx, rx) = mpsc::channel();
    // Lecteur dédié : une sortie volumineuse ne bloque pas git sur un tube plein.
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = out.read_to_end(&mut buf);
        let _ = tx.send(buf);
    });
    match rx.recv_timeout(GIT_TIMEOUT) {
        Ok(buf) => child.wait().ok().filter(|s| s.success()).map(|_| String::from_utf8_lossy(&buf).into_owned()),
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            None
        }
    }
}

/// Branche, fichiers modifiés et activité 14 jours d'un dépôt ; champs vides si ce n'est pas un dépôt ou en cas d'erreur.
fn git_info(path: &Path) -> (Option<String>, u32, [u8; 14]) {
    if !path.join(".git").exists() {
        return (None, 0, [0; 14]);
    }
    let st = run_git(path, &["status", "--porcelain=v2", "--branch"]).map(|o| git::parse_status(&o)).unwrap_or_default();
    // Dépôt sans commit : `log` échoue, donc aucune activité.
    let log = run_git(path, &["log", "--since=14.days", "--format=%ct"]).unwrap_or_default();
    let times: Vec<i64> = log.lines().filter_map(|l| l.trim().parse().ok()).collect();
    (st.branch, st.dirty, git::bucket_activity(&times, now_ms() / 1000))
}

/// Racines des projets : `projects_roots`, à défaut les racines de recherche (dossier utilisateur si vides).
fn roots(app: &AppHandle) -> Vec<PathBuf> {
    let s = app.state::<AppState>().settings.read().clone();
    if s.projects_roots.is_empty() {
        crate::modules::indexer::effective_roots(&s.search_roots, &[], dirs::home_dir())
    } else {
        s.projects_roots.iter().map(PathBuf::from).collect()
    }
}

/// Range `list` dans le cache (préférences relues pour ne pas écraser un épinglage fait pendant le calcul).
fn store(st: &AppState, mut list: Vec<Project>) {
    projects::apply_prefs(&mut list, &projects::load_prefs(&st.db.lock()));
    projects::sort(&mut list);
    let items = Arc::new(projects::items(&list));
    *st.projects.write() = projects::ProjectsCache { list, at_ms: now_ms(), items };
}

fn compute(app: &AppHandle) {
    let claude = dirs::home_dir().map(|h| h.join(".claude").join("projects"));
    let paths = projects::detect(&roots(app));
    let mut list: Vec<Project> = paths.iter().map(|p| projects::inspect(p, claude.as_deref())).collect();
    // Git à 4 de front : chaque fil prend le projet suivant.
    let next = AtomicUsize::new(0);
    let infos = Mutex::new(vec![None; list.len()]);
    std::thread::scope(|s| {
        for _ in 0..GIT_PARALLEL.min(list.len()) {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                let Some(p) = paths.get(i) else { break };
                let info = git_info(p);
                infos.lock().unwrap()[i] = Some(info);
            });
        }
    });
    for (p, info) in list.iter_mut().zip(infos.into_inner().unwrap()) {
        if let Some((branch, dirty, act)) = info {
            (p.branch, p.dirty, p.activity_14d) = (branch, dirty, act);
        }
    }
    tracing::info!("projets : {} détectés", list.len());
    store(&app.state::<AppState>(), list);
    app.state::<AppState>().events.changed(Topic::Projects);
}

/// Lance un recalcul sur un fil dédié ; ignoré si un autre est en cours.
pub fn refresh(app: &AppHandle) {
    if REFRESHING.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    let spawned = std::thread::Builder::new().name("oniris-projects".into()).spawn(move || {
        let _guard = Guard;
        compute(&app);
    });
    if let Err(e) = spawned {
        REFRESHING.store(false, Ordering::SeqCst);
        tracing::error!("projets : fil impossible : {e}");
    }
}

/// Appelé par `windows::open_panel` : recalcule si l'inventaire a plus de 2 min. Ne bloque pas.
pub fn on_panel_open(app: &AppHandle) {
    if now_ms() - app.state::<AppState>().projects.read().at_ms > STALE_MS {
        refresh(app);
    }
}

#[tauri::command]
pub async fn projects_list(state: State<'_, AppState>, include_hidden: Option<bool>) -> Result<Vec<Project>, String> {
    let all = include_hidden.unwrap_or(false);
    Ok(state.projects.read().list.iter().filter(|p| all || !p.hidden).cloned().collect())
}

/// Écrit la préférence puis met le cache à jour sans recalcul.
fn set_pref(state: &AppState, path: &str, apply: impl Fn(&mut Project), save: impl Fn(&rusqlite::Connection) -> rusqlite::Result<()>) -> Result<(), String> {
    if !state.projects.read().list.iter().any(|p| p.path == path) {
        return Err("projet inconnu".into());
    }
    save(&state.db.lock()).map_err(|e| e.to_string())?;
    {
        let mut c = state.projects.write();
        if let Some(p) = c.list.iter_mut().find(|p| p.path == path) {
            apply(p);
        }
        projects::sort(&mut c.list);
        c.items = Arc::new(projects::items(&c.list));
    }
    state.events.changed(Topic::Projects);
    Ok(())
}

#[tauri::command]
pub async fn projects_pin(state: State<'_, AppState>, path: String, pinned: bool) -> Result<(), String> {
    set_pref(&state, &path, |p| p.pinned = pinned, |c| projects::set_pinned(c, &path, pinned))
}

#[tauri::command]
pub async fn projects_hide(state: State<'_, AppState>, path: String, hidden: bool) -> Result<(), String> {
    set_pref(&state, &path, |p| p.hidden = hidden, |c| projects::set_hidden(c, &path, hidden))
}

/// Enregistre les dossiers de projets (validés comme tout réglage) puis relance l'inventaire.
/// shortcut : l'indexeur de fichiers n'est pas redémarrable à chaud (`OnceLock`) ; les nouvelles racines n'y
/// entrent qu'au prochain lancement, la liste des projets, elle, est à jour tout de suite.
#[tauri::command]
pub async fn projects_set_roots(app: AppHandle, roots: Vec<String>) -> Result<(), String> {
    let mut s = app.state::<AppState>().settings.read().clone();
    s.projects_roots = roots;
    crate::commands::apply_settings(&app, s)?;
    refresh(&app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// Dépôt temporaire créé par le test : jamais de signature (`commit.gpgsign=false`), identité locale.
    fn git_ok(dir: &Path, args: &[&str]) {
        let mut full: Vec<String> = vec!["-c".into(), "user.name=t".into(), "-c".into(), "user.email=t@t".into(), "-c".into(), "commit.gpgsign=false".into(), "-C".into(), dir.to_string_lossy().into_owned()];
        full.extend(args.iter().map(|s| s.to_string()));
        let mut c = spawn_hidden("git", &full, None, &[]).unwrap();
        assert!(c.wait().unwrap().success(), "git {args:?}");
    }

    #[test]
    fn depot_temporaire_branche_modifications_et_activite() {
        if Command::new("git").arg("--version").output().is_err() {
            return; // git absent de la machine : rien à vérifier
        }
        let t = tempfile::tempdir().unwrap();
        assert_eq!(git_info(t.path()), (None, 0, [0; 14]), "pas un dépôt");
        git_ok(t.path(), &["init", "-q", "-b", "main"]);
        let (b, d, a) = git_info(t.path());
        assert_eq!((b.as_deref(), d, a), (Some("main"), 0, [0; 14]), "aucun commit");
        std::fs::write(t.path().join("a.txt"), "x").unwrap();
        git_ok(t.path(), &["add", "a.txt"]);
        git_ok(t.path(), &["commit", "-q", "-m", "un"]);
        std::fs::write(t.path().join("a.txt"), "y").unwrap();
        std::fs::write(t.path().join("b.txt"), "z").unwrap();
        let (b, d, a) = git_info(t.path());
        assert_eq!((b.as_deref(), d, a[13]), (Some("main"), 2, 1));
        assert!(run_git(&t.path().join("absent"), &["status"]).is_none());
    }
}
