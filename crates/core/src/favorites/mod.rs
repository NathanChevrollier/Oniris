//! Favoris : table `favorites` (migration 11) et outils du courtier (`tools`).
pub mod tools;

use crate::launch::safe_url;
use crate::search::{Item, ItemKind};
use crate::CoreError;
use rusqlite::{params, Connection, Row};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Favorite {
    pub id: i64,
    pub title: String,
    pub url: String,
    pub category: Option<String>,
    pub position: i64,
    /// URL `data:image/png;base64,…` ou rien (initiales côté interface).
    pub icon: Option<String>,
}

const COLS: &str = "id, title, url, category, position, icon";

fn from_row(r: &Row) -> rusqlite::Result<Favorite> {
    Ok(Favorite { id: r.get(0)?, title: r.get(1)?, url: r.get(2)?, category: r.get(3)?, position: r.get(4)?, icon: r.get(5)? })
}

/// Domaine sans `www.` (l'URL elle-même pour un lien sans domaine, mailto).
pub fn host_of(url: &str) -> String {
    url::Url::parse(url).ok().and_then(|u| u.host_str().map(|h| h.strip_prefix("www.").unwrap_or(h).to_string())).unwrap_or_else(|| url.to_string())
}

/// Ajoute à la fin de sa catégorie. URL dangereuse ou déjà présente : `Invalid`. Titre vide : le domaine.
pub fn add(c: &Connection, title: &str, url: &str, category: Option<&str>, now_ms: i64) -> Result<i64, CoreError> {
    let url = safe_url(url).ok_or_else(|| CoreError::Invalid("adresse invalide".into()))?;
    let title = if title.trim().is_empty() { host_of(&url) } else { title.trim().to_string() };
    let category = category.map(str::trim).filter(|s| !s.is_empty());
    let dup: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM favorites WHERE url = ?1 AND deleted = 0)", [&url], |r| r.get(0))?;
    if dup {
        return Err(CoreError::Invalid("favori déjà présent".into()));
    }
    c.execute(
        "INSERT INTO favorites(title, url, category, position, created_ms)
         VALUES (?1, ?2, ?3, (SELECT COALESCE(MAX(position), -1) + 1 FROM favorites WHERE category IS ?3 AND deleted = 0), ?4)",
        params![title, url, category, now_ms],
    )?;
    Ok(c.last_insert_rowid())
}

/// Supprime ou restaure ; restaurer une URL entre-temps rajoutée échoue (index unique).
pub fn set_deleted(c: &Connection, id: i64, deleted: bool) -> Result<(), CoreError> {
    let n = c.execute("UPDATE favorites SET deleted = ?2 WHERE id = ?1", params![id, deleted]).map_err(|e| match e {
        rusqlite::Error::SqliteFailure(f, _) if f.code == rusqlite::ErrorCode::ConstraintViolation => CoreError::Invalid("favori déjà présent".into()),
        e => CoreError::Db(e),
    })?;
    if n == 0 {
        return Err(CoreError::NotFound("favori introuvable".into()));
    }
    Ok(())
}

/// La position de chaque id devient son rang dans `ids` (l'ordre relatif par catégorie suit).
/// `ids` doit être exactement l'ensemble des favoris non supprimés, sans doublon : sinon `Invalid`, rien n'est écrit.
pub fn reorder(c: &Connection, ids: &[i64]) -> Result<(), CoreError> {
    let tx = c.unchecked_transaction()?;
    let mut live: Vec<i64> = list(c)?.iter().map(|f| f.id).collect();
    let mut given = ids.to_vec();
    live.sort_unstable();
    given.sort_unstable();
    if live != given {
        return Err(CoreError::Invalid("ordre incomplet ou inconnu".into()));
    }
    for (i, id) in ids.iter().enumerate() {
        tx.execute("UPDATE favorites SET position = ?2 WHERE id = ?1 AND deleted = 0", params![id, i as i64])?;
    }
    Ok(tx.commit()?)
}

pub fn rename(c: &Connection, id: i64, title: &str) -> Result<(), CoreError> {
    let title = title.trim();
    if title.is_empty() {
        return Err(CoreError::Invalid("titre manquant".into()));
    }
    c.execute("UPDATE favorites SET title = ?2 WHERE id = ?1 AND deleted = 0", params![id, title])?;
    Ok(())
}

/// Un favori non supprimé.
pub fn get(c: &Connection, id: i64) -> Result<Option<Favorite>, CoreError> {
    let mut st = c.prepare(&format!("SELECT {COLS} FROM favorites WHERE id = ?1 AND deleted = 0"))?;
    let mut rows = st.query_map([id], from_row)?;
    Ok(rows.next().transpose()?)
}

/// Non supprimés : sans catégorie d'abord, puis par catégorie, puis par position.
pub fn list(c: &Connection) -> Result<Vec<Favorite>, CoreError> {
    let mut st = c.prepare(&format!("SELECT {COLS} FROM favorites WHERE deleted = 0 ORDER BY category IS NOT NULL, category, position, id"))?;
    let rows = st.query_map([], from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Entrées de la recherche universelle (clé = id, sous-titre = domaine).
pub fn items(c: &Connection) -> Vec<Item> {
    list(c)
        .inspect_err(|e| tracing::warn!("favoris : liste indisponible pour la recherche : {e}"))
        .unwrap_or_default()
        .into_iter()
        .map(|f| Item { kind: ItemKind::Favorite, key: f.id.to_string(), subtitle: host_of(&f.url), title: f.title })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::db;

    #[test]
    fn url_dangereuse_refusee_et_doublon_refuse() {
        let c = db();
        for u in ["javascript:alert(1)", "file:///c:/x", "https://u:p@evil.com", "pas une url"] {
            assert!(matches!(add(&c, "x", u, None, 1), Err(CoreError::Invalid(_))), "{u}");
        }
        add(&c, "Nexus", "https://nexus.example.com/", None, 1).unwrap();
        assert!(matches!(add(&c, "Autre", "https://nexus.example.com/", None, 2), Err(CoreError::Invalid(m)) if m.contains("déjà")));
        assert_eq!(list(&c).unwrap().len(), 1);
    }

    #[test]
    fn titre_par_defaut_le_domaine_sans_www() {
        let c = db();
        let id = add(&c, "  ", "https://www.example.com/a", None, 1).unwrap();
        assert_eq!(get(&c, id).unwrap().unwrap().title, "example.com");
    }

    #[test]
    fn reordonnancement_persiste_et_ajout_en_fin_de_categorie() {
        let c = db();
        let a = add(&c, "A", "https://a.com", Some("Outils"), 1).unwrap();
        let b = add(&c, "B", "https://b.com", Some("Outils"), 1).unwrap();
        let d = add(&c, "D", "https://d.com", Some("Outils"), 1).unwrap();
        let ids = |c: &Connection| list(c).unwrap().iter().map(|f| f.id).collect::<Vec<_>>();
        assert_eq!(ids(&c), [a, b, d]);
        reorder(&c, &[d, a, b]).unwrap();
        assert_eq!(ids(&c), [d, a, b]);
        let e = add(&c, "E", "https://e.com", Some("Outils"), 1).unwrap();
        assert_eq!(*ids(&c).last().unwrap(), e);
    }

    #[test]
    fn suppression_puis_restauration() {
        let c = db();
        let id = add(&c, "A", "https://a.com", None, 1).unwrap();
        set_deleted(&c, id, true).unwrap();
        assert!(get(&c, id).unwrap().is_none() && list(&c).unwrap().is_empty());
        set_deleted(&c, id, false).unwrap();
        assert_eq!(list(&c).unwrap().len(), 1);
        set_deleted(&c, id, true).unwrap();
        add(&c, "A bis", "https://a.com", None, 2).unwrap(); // l'URL supprimée est libre
        assert!(set_deleted(&c, id, false).is_err(), "restauration en doublon refusée");
    }

    #[test]
    fn reorder_strict_et_set_deleted_precis() {
        let c = db();
        let a = add(&c, "A", "https://a.com", None, 1).unwrap();
        let b = add(&c, "B", "https://b.com", None, 1).unwrap();
        for bad in [vec![a], vec![a, a], vec![a, b, 999], vec![b, 999]] {
            assert!(matches!(reorder(&c, &bad), Err(CoreError::Invalid(_))), "{bad:?}");
        }
        assert_eq!(list(&c).unwrap().iter().map(|f| f.id).collect::<Vec<_>>(), [a, b], "rien d'écrit");
        assert!(matches!(set_deleted(&c, 999, true), Err(CoreError::NotFound(_))));
        c.execute_batch("DROP TABLE favorites").unwrap();
        assert!(matches!(set_deleted(&c, a, true), Err(CoreError::Db(_))), "autre erreur : pas masquée");
    }

    #[test]
    fn renommer_et_entrees_de_recherche() {
        let c = db();
        let id = add(&c, "A", "https://www.a.com/x", None, 1).unwrap();
        rename(&c, id, " Alpha ").unwrap();
        assert!(rename(&c, id, "  ").is_err());
        let it = items(&c);
        assert_eq!((it[0].title.as_str(), it[0].subtitle.as_str()), ("Alpha", "a.com"));
        assert_eq!(it[0].key, id.to_string());
        assert_eq!(it[0].kind, ItemKind::Favorite);
    }
}
