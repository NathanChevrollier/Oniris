//! Inventaire des applications du menu Démarrer : raccourcis `.lnk`, puis applications du Store et de Windows
//! (`Get-StartApps`, comme la recherche de Windows), lancées par `shell:AppsFolder\<AppID>`.
pub mod tools;

use crate::search::{Item, ItemKind};
use parking_lot::RwLock;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

pub const MAX_AGE_MS: i64 = 6 * 3600 * 1000;
const EXCLUDED: [&str; 6] = ["uninstall", "désinstaller", "désinstallation", "help", "aide", "readme"];
const MAX_DEPTH: usize = 8;

fn excluded(title: &str) -> bool {
    let t = title.to_lowercase();
    let words: Vec<&str> = t.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    words.iter().any(|w| EXCLUDED.contains(w)) || words.windows(2).any(|w| w == ["read", "me"])
}

/// Raccourcis lancés par Windows : `.lnk` et `.url` (jeux Steam et Epic).
fn is_shortcut(p: &Path) -> bool {
    p.extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk") || x.eq_ignore_ascii_case("url"))
}

fn walk(dir: &Path, depth: usize, max_depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let Ok(ft) = e.file_type() else { continue };
        let p = e.path();
        if ft.is_dir() {
            if depth < max_depth {
                walk(&p, depth + 1, max_depth, out);
            }
        } else if ft.is_file() && is_shortcut(&p) {
            out.push(p);
        }
    }
}

/// Raccourcis de `dirs` : récursif pour le menu Démarrer, à plat (`recursive` faux) pour le Bureau.
/// En cas de doublon de titre, le premier dossier gagne ; sans outils de désinstallation/aide ; triés par titre.
pub fn scan(dirs: &[(PathBuf, bool)]) -> Vec<Item> {
    let mut files = vec![];
    for (d, recursive) in dirs {
        let mut f = vec![];
        walk(d, 0, if *recursive { MAX_DEPTH } else { 0 }, &mut f);
        f.sort();
        files.extend(f);
    }
    let mut seen = HashSet::new();
    let mut items: Vec<Item> = files
        .into_iter()
        .filter_map(|p| {
            // « Rocket League®.url » : les marques déposées gênent la recherche et l'affichage.
            let title = p.file_stem()?.to_string_lossy().replace(['®', '™', '©'], "").trim().to_string();
            (!excluded(&title) && seen.insert(title.to_lowercase()))
                .then(|| Item { kind: ItemKind::App, key: p.to_string_lossy().into_owned(), title, subtitle: "Application".into() })
        })
        .collect();
    items.sort_by_key(|i| i.title.to_lowercase());
    items
}

/// Préfixe des clés d'applications lancées par leur AppID.
pub const APPS_FOLDER: &str = r"shell:AppsFolder\";

/// Ajoute à `items` les applications de `Get-StartApps | ConvertTo-Json` absentes des raccourcis (même titre) ; triées par titre.
pub fn merge_start_apps(mut items: Vec<Item>, json: &str) -> Vec<Item> {
    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    enum OneOrMany { Many(Vec<StartApp>), One(StartApp) }
    #[derive(serde::Deserialize)]
    struct StartApp {
        #[serde(rename = "Name")]
        name: String,
        #[serde(rename = "AppID")]
        app_id: String,
    }
    let apps = match serde_json::from_str::<OneOrMany>(json.trim_start_matches('\u{feff}')) {
        Ok(OneOrMany::Many(v)) => v,
        Ok(OneOrMany::One(a)) => vec![a],
        Err(_) => return items,
    };
    let mut seen: HashSet<String> = items.iter().map(|i| i.title.to_lowercase()).collect();
    for a in apps {
        let (title, id) = (a.name.trim().to_string(), a.app_id.trim());
        // L'AppID (parfois steam://…) passe en argument séparé à explorer.exe via shell:AppsFolder : ni guillemet ni contrôle.
        let id_ok = !id.is_empty() && id.len() <= 512 && !id.contains('"') && !id.chars().any(char::is_control);
        if title.is_empty() || !id_ok || excluded(&title) || !seen.insert(title.to_lowercase()) {
            continue;
        }
        items.push(Item { kind: ItemKind::App, key: format!("{APPS_FOLDER}{id}"), title, subtitle: "Application".into() });
    }
    items.sort_by_key(|i| i.title.to_lowercase());
    items
}

/// Inventaire partagé et date du dernier balayage (0 = jamais).
#[derive(Default)]
pub struct Inventory {
    /// Partagé par `Arc` : la recherche le reprend à chaque frappe sans copier les items.
    pub items: RwLock<Arc<Vec<Item>>>,
    scanned_ms: AtomicI64,
}

impl Inventory {
    pub fn set(&self, items: Vec<Item>, now_ms: i64) {
        *self.items.write() = Arc::new(items);
        self.scanned_ms.store(now_ms, Ordering::SeqCst);
    }
    pub fn stale(&self, now_ms: i64) -> bool {
        now_ms - self.scanned_ms.load(Ordering::SeqCst) > MAX_AGE_MS
    }
    pub fn contains_key(&self, key: &str) -> bool {
        self.items.read().iter().any(|i| i.key == key)
    }

    /// Clé de l'application désignée par `q` : sa clé exacte, son nom (casse et ponctuation ignorées), sinon le
    /// nom le plus court qui le contient (« rocket » -> « Rocket League »). IRIS ne connaît que les noms.
    pub fn resolve(&self, q: &str) -> Option<(String, String)> {
        let items = self.items.read();
        if let Some(i) = items.iter().find(|i| i.key == q) {
            return Some((i.key.clone(), i.title.clone()));
        }
        let norm = |s: &str| s.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ");
        let q = norm(q);
        if q.is_empty() {
            return None;
        }
        let pick = |i: &Item| (i.key.clone(), i.title.clone());
        items.iter().find(|i| norm(&i.title) == q).map(pick)
            .or_else(|| items.iter().filter(|i| norm(&i.title).contains(&q)).min_by_key(|i| i.title.len()).map(pick))
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
        let titles: Vec<_> = scan(&[(a, true), (b, true)]).into_iter().map(|i| i.title).collect();
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
        let items = scan(&[(user.clone(), true), (common, true)]);
        assert_eq!(items.len(), 1);
        assert!(items[0].key.starts_with(&*user.to_string_lossy()));
    }

    #[test]
    fn applications_du_store_ajoutees_sans_doublon() {
        let lnk = vec![Item { kind: ItemKind::App, key: r"C:\m\Firefox.lnk".into(), title: "Firefox".into(), subtitle: "Application".into() }];
        let json = r#"[{"Name":"Calculatrice","AppID":"Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"},
            {"Name":"firefox","AppID":"308046B0AF4A39CB"},{"Name":"Désinstaller X","AppID":"a!b"},
            {"Name":"Piège","AppID":"x\" & calc"},{"Name":"","AppID":"vide"}]"#;
        let items = merge_start_apps(lnk.clone(), json);
        let t: Vec<_> = items.iter().map(|i| (i.title.as_str(), i.key.as_str())).collect();
        assert_eq!(t, [("Calculatrice", r"shell:AppsFolder\Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"), ("Firefox", r"C:\m\Firefox.lnk")]);
        let one = merge_start_apps(vec![], r#"{"Name":"Photos","AppID":"Microsoft.Windows.Photos_8wekyb3d8bbwe!App"}"#);
        assert_eq!(one.len(), 1);
        assert_eq!(merge_start_apps(lnk.clone(), "pas du json"), lnk);
    }

    #[test]
    fn bureau_a_plat_avec_raccourcis_url() {
        let t = tempfile::tempdir().unwrap();
        let desk = t.path().join("Bureau");
        std::fs::create_dir_all(desk.join("dossier")).unwrap();
        for n in ["Rocket League®.url", "GTA V.lnk", "notes.txt", "dossier/Caché.lnk"] {
            std::fs::write(desk.join(n), "").unwrap();
        }
        let titles: Vec<_> = scan(&[(desk, false)]).into_iter().map(|i| i.title).collect();
        assert_eq!(titles, ["GTA V", "Rocket League"]);
    }

    #[test]
    fn resolution_par_nom() {
        let inv = Inventory::default();
        let it = |k: &str, t: &str| Item { kind: ItemKind::App, key: k.into(), title: t.into(), subtitle: String::new() };
        inv.set(vec![it("k1", "Rocket League"), it("k2", "Rocket League Tracker"), it("k3", "Discord"), it("k4", "Discord Canary")], 1);
        let key = |q: &str| inv.resolve(q).map(|(k, _)| k);
        assert_eq!(key("k3").as_deref(), Some("k3"));
        assert_eq!(key("rocket league").as_deref(), Some("k1"));
        assert_eq!(key("ROCKET").as_deref(), Some("k1"), "le plus court gagne");
        assert_eq!(key("discord").as_deref(), Some("k3"));
        assert_eq!(key("tracker").as_deref(), Some("k2"));
        assert!(key("steam").is_none() && key(" ").is_none());
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
