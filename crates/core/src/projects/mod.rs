//! Inventaire des projets : détection dans les racines, plan, git et dernière session Claude.
//! Journal : jamais de chemin, de titre ni de branche (contenu utilisateur) ; des comptes seulement.
pub mod claude;
pub mod git;
pub mod plan;
pub mod resume;
pub mod tools;

use crate::search::{Item, ItemKind};
use claude::SessionInfo;
use plan::PlanProgress;
use rusqlite::{params, Connection};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Project {
    pub path: String,
    pub name: String,
    pub last_activity_ms: i64,
    pub branch: Option<String>,
    pub dirty: u32,
    /// Commits par jour sur 14 jours (13 = aujourd'hui).
    pub activity_14d: [u8; 14],
    pub plan: Option<PlanProgress>,
    pub last_session: Option<SessionInfo>,
    pub pinned: bool,
    pub hidden: bool,
}

/// Dernier inventaire calculé (`at_ms` = 0 : jamais).
#[derive(Default)]
pub struct ProjectsCache {
    pub list: Vec<Project>,
    pub at_ms: i64,
    /// Entrées de recherche des projets visibles.
    pub items: Arc<Vec<Item>>,
}

const MARKERS: [&str; 6] = [".git", "package.json", "Cargo.toml", "project.godot", "pyproject.toml", "go.mod"];

pub(crate) fn mtime_ms(p: &Path) -> Option<i64> {
    let d = std::fs::metadata(p).ok()?.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(d.as_millis() as i64)
}

fn is_project(dir: &Path) -> bool {
    let Ok(rd) = std::fs::read_dir(dir) else { return false };
    rd.flatten().any(|e| {
        let n = e.file_name().to_string_lossy().to_lowercase();
        n.ends_with(".sln") || MARKERS.iter().any(|m| m.eq_ignore_ascii_case(&n))
    })
}

/// Sous-dossiers directs des racines qui portent un marqueur de projet ; dossiers cachés et `AppData` exclus.
pub fn detect(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for root in roots {
        let Ok(rd) = std::fs::read_dir(root) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let p = e.path();
            if name.starts_with('.') || name.eq_ignore_ascii_case("AppData") || !p.is_dir() || !is_project(&p) {
                continue;
            }
            if seen.insert(p.to_string_lossy().to_lowercase()) {
                out.push(p);
            }
        }
    }
    out
}

/// Partie « fichiers » d'un projet (sans git ni préférences). `claude_base` : `~/.claude/projects`.
pub fn inspect(path: &Path, claude_base: Option<&Path>) -> Project {
    let plan = plan::read(path);
    let session = claude_base.and_then(|b| claude::latest(b, path));
    let last_activity_ms = [
        mtime_ms(path),
        mtime_ms(&path.join(".git").join("index")),
        mtime_ms(&path.join(".git").join("HEAD")),
        plan.as_ref().map(|(_, ms)| *ms),
        session.as_ref().map(|s| s.updated_ms),
    ]
    .into_iter()
    .flatten()
    .max()
    .unwrap_or(0);
    Project {
        path: path.to_string_lossy().into_owned(),
        name: path.file_name().map_or_else(|| path.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned()),
        last_activity_ms,
        branch: None,
        dirty: 0,
        activity_14d: [0; 14],
        plan: plan.map(|(p, _)| p),
        last_session: session,
        pinned: false,
        hidden: false,
    }
}

/// Épinglés d'abord, puis activité récente.
pub fn sort(list: &mut [Project]) {
    list.sort_by(|a, b| b.pinned.cmp(&a.pinned).then(b.last_activity_ms.cmp(&a.last_activity_ms)).then_with(|| a.name.cmp(&b.name)));
}

/// Entrées de recherche des projets visibles : sous-titre = étape du plan en cours, sinon titre de la dernière session.
pub fn items(list: &[Project]) -> Vec<Item> {
    list.iter()
        .filter(|p| !p.hidden)
        .map(|p| Item {
            kind: ItemKind::Project,
            key: p.path.clone(),
            title: p.name.clone(),
            subtitle: p.plan.as_ref().and_then(|x| x.current.clone()).or_else(|| p.last_session.as_ref().and_then(|s| s.title.clone())).unwrap_or_default(),
        })
        .collect()
}

/// Projet visible dont le nom est le plus proche de `name` : égalité d'abord, sinon préfixe (casse ignorée).
pub fn find<'a>(list: &'a [Project], name: &str) -> Option<&'a Project> {
    let n = name.trim().to_lowercase();
    if n.is_empty() {
        return None;
    }
    let mut visible: Vec<&Project> = list.iter().filter(|p| !p.hidden).collect();
    visible.sort_by(|a, b| b.pinned.cmp(&a.pinned).then(b.last_activity_ms.cmp(&a.last_activity_ms)));
    visible.iter().find(|p| p.name.to_lowercase() == n).or_else(|| visible.iter().find(|p| p.name.to_lowercase().starts_with(&n))).copied()
}

fn ago(now_ms: i64, then_ms: i64) -> String {
    let min = (now_ms - then_ms).max(0) / 60_000;
    match min {
        0 => "à l'instant".into(),
        1..=59 => format!("il y a {min} min"),
        60..=1439 => format!("il y a {} h", min / 60),
        1440..=2879 => "hier".into(),
        _ => format!("il y a {} j", min / 1440),
    }
}

/// Résumé « où j'en suis » en français, pour IRIS.
pub fn status_text(p: &Project, now_ms: i64) -> String {
    let mut parts = Vec::new();
    if let Some(pl) = &p.plan {
        parts.push(match &pl.current {
            Some(c) => format!("plan {}/{} cases, en cours : {c}", pl.done, pl.total),
            None => format!("plan {}/{} cases", pl.done, pl.total),
        });
    }
    if let Some(s) = &p.last_session {
        let when = ago(now_ms, s.updated_ms);
        parts.push(match &s.title {
            Some(t) => format!("dernière session {when} : « {t} »"),
            None => format!("dernière session {when}"),
        });
    }
    if let Some(b) = &p.branch {
        parts.push(match p.dirty {
            0 => format!("branche {b}, aucune modification"),
            1 => format!("branche {b}, 1 fichier modifié"),
            n => format!("branche {b}, {n} fichiers modifiés"),
        });
    }
    parts.push(format!("dernière activité {}", ago(now_ms, p.last_activity_ms)));
    format!("{} — {}", p.name, parts.join(" ; "))
}

/// Préférences `(épinglé, caché)` par chemin.
pub fn load_prefs(conn: &Connection) -> HashMap<String, (bool, bool)> {
    let load = || -> rusqlite::Result<HashMap<String, (bool, bool)>> {
        let mut st = conn.prepare("SELECT path, pinned, hidden FROM project_prefs")?;
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, (r.get::<_, i64>(1)? != 0, r.get::<_, i64>(2)? != 0))))?;
        rows.collect()
    };
    load().inspect_err(|e| tracing::warn!("projets : préférences illisibles : {e}")).unwrap_or_default()
}

pub fn apply_prefs(list: &mut [Project], prefs: &HashMap<String, (bool, bool)>) {
    for p in list {
        (p.pinned, p.hidden) = prefs.get(&p.path).copied().unwrap_or_default();
    }
}

pub fn set_pinned(conn: &Connection, path: &str, v: bool) -> rusqlite::Result<()> {
    conn.execute("INSERT INTO project_prefs(path, pinned) VALUES(?1, ?2) ON CONFLICT(path) DO UPDATE SET pinned = ?2", params![path, v])?;
    Ok(())
}

pub fn set_hidden(conn: &Connection, path: &str, v: bool) -> rusqlite::Result<()> {
    conn.execute("INSERT INTO project_prefs(path, hidden) VALUES(?1, ?2) ON CONFLICT(path) DO UPDATE SET hidden = ?2", params![path, v])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(p: &Path) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, "x").unwrap();
    }

    #[test]
    fn detection_marqueurs_exclusions_et_enfants_directs() {
        let t = tempfile::tempdir().unwrap();
        let r = t.path();
        std::fs::create_dir_all(r.join("a/.git")).unwrap(); // dossier .git
        touch(&r.join("wt/.git")); // worktree : .git est un fichier
        for (d, f) in [("node", "package.json"), ("rust", "Cargo.toml"), ("sln", "App.sln"), ("godot", "project.godot"), ("py", "pyproject.toml"), ("go", "go.mod")] {
            touch(&r.join(d).join(f));
        }
        touch(&r.join("rien/readme.md"));
        touch(&r.join(".cache/package.json")); // caché
        touch(&r.join("AppData/package.json"));
        touch(&r.join("a/profond/package.json")); // petit-enfant : non
        touch(&r.join("fichier.json"));
        let mut names: Vec<String> = detect(&[r.to_path_buf(), r.to_path_buf()]).iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        names.sort();
        assert_eq!(names, ["a", "go", "godot", "node", "py", "rust", "sln", "wt"], "doublons de racines écartés");
        assert!(detect(&[r.join("absent")]).is_empty());
    }

    #[test]
    fn inspection_plan_session_et_derniere_activite() {
        let t = tempfile::tempdir().unwrap();
        let proj = t.path().join("Mon Projet");
        touch(&proj.join("package.json"));
        touch(&proj.join("docs/superpowers/plans/p.md"));
        std::fs::write(proj.join("docs/superpowers/plans/p.md"), "### Task 1 : A\n- [x] a\n- [ ] b\n").unwrap();
        let base = t.path().join("claude");
        let dir = base.join(claude::encode(&proj.to_string_lossy()));
        std::fs::create_dir_all(&dir).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(30));
        std::fs::write(dir.join("s.jsonl"), "{\"type\":\"summary\",\"summary\":\"Titre\"}\n").unwrap();
        let p = inspect(&proj, Some(&base));
        assert_eq!(p.name, "Mon Projet");
        assert_eq!(p.plan.as_ref().map(|x| (x.done, x.total)), Some((1, 2)));
        let s = p.last_session.unwrap();
        assert_eq!(s.title.as_deref(), Some("Titre"));
        assert!(p.last_activity_ms >= s.updated_ms && p.last_activity_ms > 0);
        assert!(inspect(&proj, None).last_session.is_none());
    }

    fn proj(name: &str, act: i64) -> Project {
        Project { path: format!(r"C:\p\{name}"), name: name.into(), last_activity_ms: act, branch: None, dirty: 0, activity_14d: [0; 14], plan: None, last_session: None, pinned: false, hidden: false }
    }

    #[test]
    fn tri_recherche_et_nom() {
        let mut l = vec![proj("Alpha", 1), proj("MyEmpire", 5), proj("MyHome", 9), proj("Zed", 3)];
        l[3].pinned = true;
        l[2].hidden = true;
        sort(&mut l);
        assert_eq!(l.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Zed", "MyHome", "MyEmpire", "Alpha"]);
        assert_eq!(find(&l, "myem").unwrap().name, "MyEmpire");
        assert_eq!(find(&l, "ALPHA").unwrap().name, "Alpha");
        assert!(find(&l, "myhome").is_none(), "caché");
        assert!(find(&l, "").is_none() && find(&l, "zzz").is_none());
        l[0].plan = Some(PlanProgress { file: "p.md".into(), done: 1, total: 2, current: Some("Task 2 : X".into()) });
        let it = items(&l);
        assert_eq!(it.len(), 3);
        assert_eq!((it[0].kind, it[0].subtitle.as_str()), (ItemKind::Project, "Task 2 : X"));
    }

    #[test]
    fn texte_de_statut_en_francais() {
        let now = 10 * 86_400_000;
        let mut p = proj("MyEmpire", now - 86_400_000);
        p.plan = Some(PlanProgress { file: "p.md".into(), done: 7, total: 12, current: Some("Task 8 : Pathfinding".into()) });
        p.last_session = Some(SessionInfo { id: None, updated_ms: now - 86_400_000 * 3, title: Some("Heuristique".into()) });
        p.branch = Some("feat/x".into());
        p.dirty = 4;
        assert_eq!(
            status_text(&p, now),
            "MyEmpire — plan 7/12 cases, en cours : Task 8 : Pathfinding ; dernière session il y a 3 j : « Heuristique » ; branche feat/x, 4 fichiers modifiés ; dernière activité hier"
        );
        assert_eq!(status_text(&proj("Vide", now - 5 * 60_000), now), "Vide — dernière activité il y a 5 min");
    }

    #[test]
    fn preferences_epingle_et_cache() {
        let c = crate::test_util::db();
        set_pinned(&c, r"C:\p\A", true).unwrap();
        set_hidden(&c, r"C:\p\A", true).unwrap();
        set_pinned(&c, r"C:\p\B", true).unwrap();
        set_pinned(&c, r"C:\p\B", false).unwrap();
        let mut l = vec![proj("A", 1), proj("B", 1), proj("C", 1)];
        apply_prefs(&mut l, &load_prefs(&c));
        assert_eq!(l.iter().map(|p| (p.pinned, p.hidden)).collect::<Vec<_>>(), [(true, true), (false, false), (false, false)]);
    }
}
