//! Avancement d'un plan d'implémentation (`docs/superpowers/plans/*.md`) : cases cochées et tâche en cours.
use serde::Serialize;
use std::io::Read;
use std::path::Path;

const MAX_BYTES: u64 = 1 << 20;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlanProgress {
    /// Nom du fichier (sans dossier).
    pub file: String,
    pub done: u32,
    pub total: u32,
    /// Titre (`Task N : …`) de la première tâche qui a encore une case non cochée.
    pub current: Option<String>,
}

pub fn parse(text: &str, file: &str) -> PlanProgress {
    let (mut done, mut total) = (0, 0);
    let (mut heading, mut current): (Option<&str>, Option<String>) = (None, None);
    for line in text.lines() {
        let t = line.trim_start();
        if let Some(h) = t.strip_prefix("### ") {
            let is_task = h.strip_prefix("Task ").is_some_and(|r| r.starts_with(|c: char| c.is_ascii_digit()));
            heading = is_task.then(|| h.trim());
        } else if t.starts_with("- [ ]") {
            total += 1;
            if current.is_none() {
                current = heading.map(str::to_string);
            }
        } else if t.starts_with("- [x]") || t.starts_with("- [X]") {
            total += 1;
            done += 1;
        }
    }
    PlanProgress { file: file.into(), done, total, current }
}

/// Plan le plus récemment modifié (hors journaux et feuilles de route) et son mtime en ms.
pub fn read(project: &Path) -> Option<(PlanProgress, i64)> {
    let dir = project.join("docs").join("superpowers").join("plans");
    let (path, ms) = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| {
            let n = e.file_name().to_string_lossy().to_lowercase();
            n.ends_with(".md") && !n.ends_with("-journal.md") && !n.contains("feuille-de-route")
        })
        .filter_map(|e| Some((e.path(), super::mtime_ms(&e.path())?)))
        .max_by_key(|(_, ms)| *ms)?;
    let mut buf = Vec::new();
    std::fs::File::open(&path).ok()?.take(MAX_BYTES).read_to_end(&mut buf).ok()?;
    let name = path.file_name()?.to_string_lossy().into_owned();
    Some((parse(&String::from_utf8_lossy(&buf), &name), ms))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 12 cases dont 7 cochées (indentées ou non, `x` ou `X`), tâche 3 en cours, une section hors tâches.
    const MODEL: &str = "# Plan\n\n### Task 1 : A\n- [x] a\n- [X] b\n  - [x] c\n### Task 2: B\n- [x] d\n- [x] e\n- [x] f\n- [x] g\n### Task 3 : C\n- [ ] h\n- [ ] i\n### Autre\n- [ ] j\n- [ ] k\n- [ ] l\n";

    #[test]
    fn sept_cases_sur_douze_et_tache_courante() {
        let p = parse(MODEL, "p.md");
        assert_eq!((p.done, p.total), (7, 12));
        assert_eq!(p.current.as_deref(), Some("Task 3 : C"));
    }

    #[test]
    fn tache_terminee_ignoree_et_separateur_libre() {
        let p = parse("### Task 1 : A\n- [x] a\n### Task 2: B\n- [ ] b\n", "p.md");
        assert_eq!(p.current.as_deref(), Some("Task 2: B"));
        assert_eq!(parse("- [x] a\n- [ ] b\n", "p.md").current, None, "case hors tâche");
    }

    #[test]
    fn plus_recent_hors_journal_et_feuille_de_route() {
        let t = tempfile::tempdir().unwrap();
        let d = t.path().join("docs/superpowers/plans");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("a-plan.md"), "### Task 1 : A\n- [ ] x\n").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(30));
        std::fs::write(d.join("b-journal.md"), "- [x] z\n").unwrap();
        std::fs::write(d.join("2026-feuille-de-route.md"), "- [x] z\n").unwrap();
        let (p, _) = read(t.path()).unwrap();
        assert_eq!(p.file, "a-plan.md");
        assert!(read(&t.path().join("absent")).is_none());
    }
}
