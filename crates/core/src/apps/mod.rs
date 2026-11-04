//! Inventaire des applications du menu Démarrer (raccourcis `.lnk`).
// shortcut: pas d'apps UWP, à ajouter via shell:AppsFolder si l'utilisateur le demande.
pub mod tools;

use crate::search::{Item, ItemKind};
use parking_lot::RwLock;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};

pub const MAX_AGE_MS: i64 = 6 * 3600 * 1000;
const EXCLUDED: [&str; 6] = ["uninstall", "désinstaller", "désinstallation", "help", "aide", "readme"];
const MAX_DEPTH: usize = 8;

fn excluded(title: &str) -> bool {
    let t = title.to_lowercase();
    let words: Vec<&str> = t.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    words.iter().any(|w| EXCLUDED.contains(w)) || words.windows(2).any(|w| w == ["read", "me"])
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let Ok(ft) = e.file_type() else { continue };
        let p = e.path();
        if ft.is_dir() {
            if depth < MAX_DEPTH {
                walk(&p, depth + 1, out);
            }
        } else if ft.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")) {
            out.push(p);
        }
    }
}

/// Raccourcis `.lnk` de `dirs` (récursif ; en cas de doublon de titre, le premier dossier gagne), sans doublon de titre ni outils de désinstallation/aide ; triés par titre.
pub fn scan(dirs: &[PathBuf]) -> Vec<Item> {
    let mut files = vec![];
    for d in dirs {
        let mut f = vec![];
        walk(d, 0, &mut f);
        f.sort();
        files.extend(f);
    }
    let mut seen = HashSet::new();
    let mut items: Vec<Item> = files
        .into_iter()
        .filter_map(|p| {
            let title = p.file_stem()?.to_string_lossy().into_owned();
            (!excluded(&title) && seen.insert(title.to_lowercase()))
                .then(|| Item { kind: ItemKind::App, key: p.to_string_lossy().into_owned(), title, subtitle: "Application".into() })
        })
        .collect();
    items.sort_by_key(|i| i.title.to_lowercase());
    items
}

/// Inventaire partagé et date du dernier balayage (0 = jamais).
#[derive(Default)]
pub struct Inventory {
    pub items: RwLock<Vec<Item>>,
    scanned_ms: AtomicI64,
}

impl Inventory {
    pub fn set(&self, items: Vec<Item>, now_ms: i64) {
        *self.items.write() = items;
        self.scanned_ms.store(now_ms, Ordering::SeqCst);
    }
    pub fn stale(&self, now_ms: i64) -> bool {
        now_ms - self.scanned_ms.load(Ordering::SeqCst) > MAX_AGE_MS
    }
    pub fn contains_key(&self, key: &str) -> bool {
        self.items.read().iter().any(|i| i.key == key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_deduplique_et_exclut() {
        let t = tempfile::tempdir().unwrap();
        let (a, b) = (t.path().join("a"), t.path().join("b").join("Outils"));
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        for (d, n) in [
            (&a, "Firefox.lnk"),
            (&b, "firefox.LNK"),
            (&a, "Code.lnk"),
            (&b, "Uninstall Foo.lnk"),
            (&a, "Désinstaller Bar.lnk"),
            (&a, "Help.lnk"),
            (&b, "Read me.lnk"),
            (&a, "Aide Baz.lnk"),
            (&a, "notes.txt"),
            (&a, "Tomb Raider.lnk"),
            (&b, "Helpdesk.lnk"),
            (&b, "Aide de Foo.lnk"),
        ] {
            std::fs::write(d.join(n), "").unwrap();
        }
        let titles: Vec<_> = scan(&[a, b]).into_iter().map(|i| i.title).collect();
        assert_eq!(titles, ["Code", "Firefox", "Helpdesk", "Tomb Raider"]);
    }

    #[test]
    fn le_premier_dossier_gagne_les_doublons() {
        let t = tempfile::tempdir().unwrap();
        let (user, common) = (t.path().join("user"), t.path().join("common"));
        std::fs::create_dir_all(&user).unwrap();
        std::fs::create_dir_all(&common).unwrap();
        std::fs::write(user.join("App.lnk"), "").unwrap();
        std::fs::write(common.join("App.lnk"), "").unwrap();
        let items = scan(&[user.clone(), common]);
        assert_eq!(items.len(), 1);
        assert!(items[0].key.starts_with(&*user.to_string_lossy()));
    }

    #[test]
    fn inventaire_perime_apres_6h() {
        let inv = Inventory::default();
        assert!(inv.stale(MAX_AGE_MS + 1));
        inv.set(vec![], 1_000);
        assert!(!inv.stale(1_000 + MAX_AGE_MS));
        assert!(inv.stale(1_001 + MAX_AGE_MS));
    }
}
