//! Import de favoris : export Nexus_Dashboard, signets Chromium (Chrome, Edge, Brave) et Firefox.
//! Les lecteurs sont purs (texte ou copie temporaire en entrée) ; `import` valide chaque adresse via `add`.
use crate::launch::safe_url;
use crate::CoreError;
use rusqlite::Connection;
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct NewFavorite {
    pub title: String,
    pub url: String,
    pub category: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct ImportReport {
    pub added: u32,
    pub skipped_dup: u32,
    pub skipped_unsafe: u32,
}

/// Export Nexus (`DashboardTransfer`) : adresses des widgets « link » (une) et « bookmarks » (liste).
/// Catégorie = celle du widget dans Nexus, sinon le nom du dashboard.
pub fn from_nexus(json: &str) -> Result<Vec<NewFavorite>, CoreError> {
    use serde_json::Value;
    let v: Value = serde_json::from_str(json).map_err(|_| CoreError::Invalid("fichier JSON invalide".into()))?;
    let (Some(widgets), Some(dash)) = (v["widgets"].as_array(), v["dashboard"]["name"].as_str()) else {
        return Err(CoreError::Invalid("ce fichier n'est pas un export Nexus".into()));
    };
    let mut out = Vec::new();
    for w in widgets {
        let cat = w["categoryIndex"].as_u64().and_then(|i| v["categories"].get(i as usize)?["name"].as_str()).unwrap_or(dash);
        let o = &w["options"];
        let mut push = |t: &Value, u: &Value| {
            if let Some(u) = u.as_str() {
                out.push(NewFavorite { title: t.as_str().unwrap_or("").into(), url: u.into(), category: Some(cat.into()) });
            }
        };
        match w["type"].as_str() {
            Some("link") => push(&o["label"], &o["url"]),
            Some("bookmarks") => o["bookmarks"].as_array().into_iter().flatten().for_each(|b| push(&b["title"], &b["url"])),
            _ => {}
        }
    }
    Ok(out)
}

/// Fichier `Bookmarks` de Chromium : barre, autres et synchronisés ; catégorie = chemin des dossiers.
pub fn from_chromium(json: &str) -> Result<Vec<NewFavorite>, CoreError> {
    use serde_json::Value;
    fn walk(children: &Value, path: &[&str], out: &mut Vec<NewFavorite>) {
        for n in children.as_array().into_iter().flatten() {
            match n["type"].as_str() {
                Some("url") => {
                    if let Some(u) = n["url"].as_str() {
                        let category = (!path.is_empty()).then(|| path.join(" / "));
                        out.push(NewFavorite { title: n["name"].as_str().unwrap_or("").into(), url: u.into(), category });
                    }
                }
                Some("folder") => {
                    let mut p = path.to_vec();
                    p.push(n["name"].as_str().unwrap_or("").trim());
                    walk(&n["children"], &p, out);
                }
                _ => {}
            }
        }
    }
    let v: Value = serde_json::from_str(json).map_err(|_| CoreError::Invalid("fichier de signets invalide".into()))?;
    if !v["roots"].is_object() {
        return Err(CoreError::Invalid("ce fichier n'est pas un fichier de signets Chromium".into()));
    }
    let mut out = Vec::new();
    for key in ["bookmark_bar", "other", "synced"] {
        walk(&v["roots"][key]["children"], &[], &mut out);
    }
    Ok(out)
}

/// `places.sqlite` de Firefox, **copie** temporaire (Firefox verrouille l'original), ouverte en lecture seule.
pub fn from_firefox(places_copy: &Path) -> Result<Vec<NewFavorite>, CoreError> {
    use std::collections::HashMap;
    const ROOTS: [&str; 5] = ["menu________", "toolbar_____", "unfiled_____", "mobile______", "root________"];
    let c = Connection::open_with_flags(places_copy, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    // id -> (parent, titre, guid) des dossiers ; le chemin se remonte jusqu'à une racine.
    let mut folders: HashMap<i64, (i64, String, String)> = HashMap::new();
    let mut st = c.prepare("SELECT id, parent, COALESCE(title, ''), guid FROM moz_bookmarks WHERE type = 2")?;
    for r in st.query_map([], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?, r.get(3)?))))? {
        let (id, f) = r?;
        folders.insert(id, f);
    }
    let mut st = c.prepare(
        "SELECT COALESCE(b.title, p.title), p.url, b.parent FROM moz_bookmarks b JOIN moz_places p ON p.id = b.fk
         WHERE b.type = 1 AND p.url NOT LIKE 'place:%' ORDER BY b.id",
    )?;
    let mut out = Vec::new();
    for r in st.query_map([], |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)))? {
        let (title, url, mut at) = r?;
        let (mut path, mut tagged) = (Vec::new(), false);
        for _ in 0..32 {
            let Some((parent, name, guid)) = folders.get(&at) else { break };
            if guid == "tags________" { tagged = true; break; }
            if ROOTS.contains(&guid.as_str()) { break; }
            path.push(name.as_str());
            at = *parent;
        }
        if tagged { continue; } // un tag n'est pas un dossier : le même signet est déjà listé ailleurs
        path.reverse();
        out.push(NewFavorite { title: title.unwrap_or_default(), url, category: (!path.is_empty()).then(|| path.join(" / ")) });
    }
    Ok(out)
}

/// Copie `places.sqlite` (et son `-wal`) dans un dossier temporaire, lit, puis supprime la copie.
pub fn from_firefox_profile(places: &Path) -> Result<Vec<NewFavorite>, CoreError> {
    let err = |e: std::io::Error| CoreError::Invalid(format!("lecture des signets Firefox impossible : {e}"));
    if std::fs::metadata(places).map_err(err)?.len() > 200 * 1024 * 1024 {
        return Err(CoreError::Invalid("base Firefox trop volumineuse".into()));
    }
    let dir = tempfile::tempdir().map_err(err)?;
    let copy = dir.path().join("places.sqlite");
    std::fs::copy(places, &copy).map_err(err)?;
    let mut wal = places.as_os_str().to_owned();
    wal.push("-wal");
    if Path::new(&wal).exists() {
        std::fs::copy(&wal, dir.path().join("places.sqlite-wal")).map_err(err)?;
    }
    from_firefox(&copy) // `dir` est supprimé en quittant la fonction
}

/// Ajoute chaque entrée en une transaction. Le journal ne reçoit que des nombres.
pub fn import(c: &Connection, items: &[NewFavorite], now_ms: i64) -> Result<ImportReport, CoreError> {
    let tx = c.unchecked_transaction()?;
    let mut r = ImportReport::default();
    for it in items {
        if safe_url(&it.url).is_none() {
            r.skipped_unsafe += 1;
            continue;
        }
        let title: String = it.title.chars().take(80).collect();
        match super::add(&tx, &title, &it.url, it.category.as_deref(), now_ms) {
            Ok(_) => r.added += 1,
            Err(CoreError::Invalid(_)) => r.skipped_dup += 1,
            Err(e) => return Err(e),
        }
    }
    tx.commit()?;
    tracing::info!(added = r.added, dup = r.skipped_dup, unsafe_ = r.skipped_unsafe, "favoris : import terminé");
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::favorites::list;
    use crate::test_util::db;

    const NEXUS: &str = include_str!("../../tests/fixtures/favorites/nexus.json");
    const CHROMIUM: &str = include_str!("../../tests/fixtures/favorites/chromium_bookmarks.json");

    fn nf(t: &str, u: &str, c: Option<&str>) -> NewFavorite {
        NewFavorite { title: t.into(), url: u.into(), category: c.map(Into::into) }
    }

    #[test]
    fn nexus_liens_et_signets_avec_categories() {
        let got = from_nexus(NEXUS).unwrap();
        assert_eq!(got, [
            nf("Portainer", "https://portainer.example.com/", Some("Outils")),
            nf("Sans catégorie", "https://free.example.com/", Some("Maison")),
            nf("Jellyfin", "https://jellyfin.example.com/", Some("Médias")),
            nf("Piégé", "javascript:alert(1)", Some("Médias")),
            nf("Vb", "vbscript:msgbox(1)", Some("Médias")),
            nf("Data", "data:text/html,<script>1</script>", Some("Médias")),
            nf("Fichier", "file:///c:/windows/system32/calc.exe", Some("Médias")),
        ]);
        assert!(from_nexus("pas du json").is_err());
        assert!(from_nexus(r#"{"foo":1}"#).is_err());
    }

    #[test]
    fn chromium_dossiers_imbriques_sans_le_nom_de_la_racine() {
        let got = from_chromium(CHROMIUM).unwrap();
        assert_eq!(got, [
            nf("Racine barre", "https://bar.example.com/", None),
            nf("MDN", "https://developer.mozilla.org/", Some("Dev")),
            nf("Docs", "https://doc.rust-lang.org/", Some("Dev / Rust")),
            nf("Bookmarklet", "javascript:void(0)", None),
            nf("Autre", "https://other.example.com/", None),
        ]);
        assert!(from_chromium("[]").is_err());
    }

    fn firefox_db(path: &Path) {
        let c = Connection::open(path).unwrap();
        c.execute_batch(
            "CREATE TABLE moz_places (id INTEGER PRIMARY KEY, url TEXT, title TEXT);
             CREATE TABLE moz_bookmarks (id INTEGER PRIMARY KEY, type INTEGER, fk INTEGER, parent INTEGER, title TEXT, guid TEXT);
             INSERT INTO moz_places VALUES (1,'https://ff.example.com/','FF'),(2,'place:type=6&sort=14','Historique'),
               (3,'javascript:alert(2)','Piégé'),(4,'https://deep.example.com/','Deep');
             INSERT INTO moz_bookmarks VALUES (1,2,NULL,0,'','root________'),(2,2,NULL,1,'toolbar','toolbar_____'),
               (3,2,NULL,2,'Lecture','guidfolder001'),(4,2,NULL,1,'tags','tags________'),(5,2,NULL,4,'rust','tagfolder001'),(6,2,NULL,3,'Profond','guidfolder002'),
               (10,1,1,2,'FF','g10'),(11,1,2,2,'Historique','g11'),(12,1,3,3,'Piégé','g12'),(13,1,4,3,NULL,'g13'),
               (14,3,NULL,2,'---','g14'),(15,1,1,5,'FF tag','g15'),(16,1,4,6,'Dans profond','g16');",
        ).unwrap();
    }

    #[test]
    fn firefox_copie_temporaire_lecture_seule_et_original_intact() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("places.sqlite");
        firefox_db(&src);
        let got = from_firefox_profile(&src).unwrap();
        assert_eq!(got, [
            nf("FF", "https://ff.example.com/", None),
            nf("Piégé", "javascript:alert(2)", Some("Lecture")),
            nf("Deep", "https://deep.example.com/", Some("Lecture")),
            nf("Dans profond", "https://deep.example.com/", Some("Lecture / Profond")),
        ]);
        assert!(src.exists());
        assert!(from_firefox(&dir.path().join("absent.sqlite")).is_err());
    }

    #[test]
    fn titres_tronques_a_80_caracteres() {
        let c = db();
        import(&c, &[nf(&"é".repeat(200), "https://long.example.com/", None)], 1).unwrap();
        assert_eq!(list(&c).unwrap()[0].title.chars().count(), 80);
    }

    #[test]
    fn import_compte_ajouts_doublons_et_liens_dangereux() {
        let c = db();
        let items = from_nexus(NEXUS).unwrap();
        let r = import(&c, &items, 5).unwrap();
        assert_eq!(r, ImportReport { added: 3, skipped_dup: 0, skipped_unsafe: 4 });
        let again = import(&c, &items, 6).unwrap();
        assert_eq!(again, ImportReport { added: 0, skipped_dup: 3, skipped_unsafe: 4 });
        let l = list(&c).unwrap();
        assert_eq!(l.len(), 3);
        assert!(l.iter().any(|f| f.category.as_deref() == Some("Outils") && f.title == "Portainer"));
    }
}
