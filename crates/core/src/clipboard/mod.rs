//! Historique du presse-papiers : table `clipboard` (migration 7), filtre (`filter`) et outil (`tools`).
//! Une copie identique (même empreinte) remonte l'entrée existante. Jamais de contenu dans le journal.
pub mod filter;
pub mod tools;

pub use filter::{should_record, truncate_text, ClipFlags, MAX_TEXT_BYTES};

use crate::CoreError;
use rusqlite::functions::FunctionFlags;
use rusqlite::{params, Connection, Row};
use serde::Serialize;
use sha2::{Digest, Sha256};

/// Entrée listée (sans le PNG complet, seulement sa miniature).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ClipEntry {
    pub id: i64,
    /// "text" ou "image".
    pub kind: String,
    pub text: Option<String>,
    pub truncated: bool,
    pub thumb: Option<Vec<u8>>,
    pub pinned: bool,
    pub created_ms: i64,
}

/// Contenu à remettre dans le presse-papiers.
#[derive(Debug, Clone, PartialEq)]
pub enum ClipContent { Text(String), Image(Vec<u8>) }

const DAY_MS: i64 = 86_400_000;

fn from_row(r: &Row) -> rusqlite::Result<ClipEntry> {
    Ok(ClipEntry {
        id: r.get(0)?, kind: r.get(1)?, text: r.get(2)?, truncated: r.get::<_, i64>(3)? != 0,
        thumb: r.get(4)?, pinned: r.get::<_, i64>(5)? != 0, created_ms: r.get(6)?,
    })
}

/// Même empreinte : l'entrée existante remonte en tête (`created_ms`), son épinglage est gardé.
#[allow(clippy::too_many_arguments)]
fn upsert(c: &Connection, kind: &str, text: Option<&str>, truncated: bool, png: Option<&[u8]>, thumb: Option<&[u8]>, hash: &str, now: i64) -> Result<i64, CoreError> {
    Ok(c.query_row(
        "INSERT INTO clipboard(kind, text, truncated, png, thumb, hash, created_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(hash) DO UPDATE SET created_ms = excluded.created_ms RETURNING id",
        params![kind, text, truncated, png, thumb, hash, now],
        |r| r.get(0),
    )?)
}

fn hash(kind: &str, content: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(kind.as_bytes());
    h.update([0]);
    h.update(content);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Enregistre un texte (tronqué à `MAX_TEXT_BYTES`) ; renvoie l'id de l'entrée créée ou remontée.
pub fn record_text(c: &Connection, text: &str, now: i64) -> Result<i64, CoreError> {
    let (text, truncated) = truncate_text(text);
    upsert(c, "text", Some(&text), truncated, None, None, &hash("text", text.as_bytes()), now)
}

/// Enregistre une image (PNG et miniature PNG) ; renvoie l'id de l'entrée créée ou remontée.
pub fn record_image(c: &Connection, png: &[u8], thumb: &[u8], now: i64) -> Result<i64, CoreError> {
    upsert(c, "image", None, false, Some(png), Some(thumb), &hash("image", png), now)
}

/// Supprime les entrées non épinglées plus vieilles que `retention_days`, puis les plus anciennes au-delà de
/// `max_items` (épinglées non comptées). Renvoie le nombre de lignes supprimées.
pub fn purge(c: &Connection, now: i64, retention_days: u16, max_items: u16) -> Result<usize, CoreError> {
    let old = c.execute("DELETE FROM clipboard WHERE pinned = 0 AND created_ms < ?1", [now - i64::from(retention_days) * DAY_MS])?;
    let over = c.execute(
        "DELETE FROM clipboard WHERE pinned = 0 AND id NOT IN
         (SELECT id FROM clipboard WHERE pinned = 0 ORDER BY created_ms DESC, id DESC LIMIT ?1)",
        [i64::from(max_items)],
    )?;
    Ok(old + over)
}

/// Minuscules sans accents (« Été » -> « ete ») ; couvre le latin courant, pas toute la table Unicode.
fn fold(s: &str) -> String {
    s.to_lowercase().chars().fold(String::with_capacity(s.len()), |mut o, ch| {
        match ch {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => o.push('a'),
            'ç' => o.push('c'),
            'è' | 'é' | 'ê' | 'ë' => o.push('e'),
            'ì' | 'í' | 'î' | 'ï' => o.push('i'),
            'ñ' => o.push('n'),
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' => o.push('o'),
            'ù' | 'ú' | 'û' | 'ü' => o.push('u'),
            'ý' | 'ÿ' => o.push('y'),
            'œ' => o.push_str("oe"),
            'æ' => o.push_str("ae"),
            _ => o.push(ch),
        }
        o
    })
}

/// Requête vide : entrées récentes (textes et images) ; sinon textes contenant la requête. Épinglées d'abord.
pub fn search(c: &Connection, query: &str, limit: usize) -> Result<Vec<ClipEntry>, CoreError> {
    let q = query.trim();
    // LIKE ne replie que la casse ASCII : on compare des textes repliés (casse + accents) via une fonction SQL.
    c.create_scalar_function("fold", 1, FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC, |ctx| Ok(ctx.get::<Option<String>>(0)?.map(|t| fold(&t))))?; // images : texte NULL
    let pattern = fold(q);
    let filter = if q.is_empty() { "" } else { "WHERE kind = 'text' AND instr(fold(text), ?2) > 0" };
    let mut st = c.prepare(&format!(
        "SELECT id, kind, text, truncated, thumb, pinned, created_ms FROM clipboard {filter}
         ORDER BY pinned DESC, created_ms DESC, id DESC LIMIT ?1"
    ))?;
    let rows = if q.is_empty() { st.query_map(params![limit as i64], from_row)? } else { st.query_map(params![limit as i64, pattern], from_row)? };
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn set_pinned(c: &Connection, id: i64, pinned: bool) -> Result<(), CoreError> {
    c.execute("UPDATE clipboard SET pinned = ?2 WHERE id = ?1", params![id, pinned])?;
    Ok(())
}

/// Suppression définitive (pas de corbeille : le contenu ne doit pas survivre).
pub fn delete(c: &Connection, id: i64) -> Result<(), CoreError> {
    c.execute("DELETE FROM clipboard WHERE id = ?1", [id])?;
    Ok(())
}

pub fn get(c: &Connection, id: i64) -> Result<Option<ClipContent>, CoreError> {
    let mut st = c.prepare("SELECT kind, text, png FROM clipboard WHERE id = ?1")?;
    let mut rows = st.query_map([id], |r| {
        let kind: String = r.get(0)?;
        Ok(if kind == "image" { ClipContent::Image(r.get(2)?) } else { ClipContent::Text(r.get(1)?) })
    })?;
    Ok(rows.next().transpose()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::db;

    fn count(c: &Connection) -> i64 { c.query_row("SELECT count(*) FROM clipboard", [], |r| r.get(0)).unwrap() }
    fn texts(c: &Connection, q: &str) -> Vec<String> { search(c, q, 50).unwrap().into_iter().filter_map(|e| e.text).collect() }

    #[test]
    fn copie_en_double_une_seule_ligne_remontee_et_epinglage_garde() {
        let c = db();
        let a = record_text(&c, "bonjour", 1).unwrap();
        record_text(&c, "autre", 2).unwrap();
        set_pinned(&c, a, true).unwrap();
        assert_eq!(record_text(&c, "bonjour", 3).unwrap(), a);
        assert_eq!(count(&c), 2);
        let e = &search(&c, "", 50).unwrap()[0];
        assert_eq!((e.id, e.created_ms, e.pinned), (a, 3, true));
        let i = record_image(&c, b"png", b"th", 4).unwrap();
        assert_eq!(record_image(&c, b"png", b"th", 5).unwrap(), i);
        assert_eq!(count(&c), 3);
        // Même octets, autre type : deux entrées distinctes.
        assert_ne!(record_text(&c, "png", 6).unwrap(), i);
    }

    #[test]
    fn texte_enorme_tronque_et_marque() {
        let c = db();
        record_text(&c, &"a".repeat(10 * 1024 * 1024), 1).unwrap();
        let e = &search(&c, "", 1).unwrap()[0];
        assert!(e.truncated);
        assert_eq!(e.text.as_ref().unwrap().len(), MAX_TEXT_BYTES);
    }

    #[test]
    fn purge_respecte_epinglees_duree_et_nombre() {
        let c = db();
        let now = 100 * DAY_MS;
        let old_pinned = record_text(&c, "vieux épinglé", now - 30 * DAY_MS).unwrap();
        set_pinned(&c, old_pinned, true).unwrap();
        record_text(&c, "vieux", now - 8 * DAY_MS).unwrap();
        for i in 0..25 { record_text(&c, &format!("n{i}"), now - 25 + i).unwrap(); }
        assert_eq!(purge(&c, now, 7, 20).unwrap(), 6, "le vieux + les 5 plus anciens au-delà de 20");
        let left = texts(&c, "");
        assert_eq!(left.len(), 21);
        assert_eq!(left[0], "vieux épinglé");
        assert!(left.contains(&"n24".to_string()) && left.contains(&"n5".to_string()));
        assert!(!left.contains(&"n4".to_string()) && !left.contains(&"vieux".to_string()));
    }

    #[test]
    fn recherche_insensible_a_la_casse_textes_seulement() {
        let c = db();
        record_text(&c, "Mot de PASSE oublié", 1).unwrap();
        record_text(&c, "100% sûr", 2).unwrap();
        record_text(&c, "autre", 3).unwrap();
        record_image(&c, b"p", b"t", 4).unwrap();
        assert_eq!(texts(&c, "passe"), vec!["Mot de PASSE oublié"]);
        assert_eq!(texts(&c, "%"), vec!["100% sûr"], "les jokers LIKE sont échappés");
        assert!(search(&c, "_", 50).unwrap().is_empty());
        assert_eq!(search(&c, "", 50).unwrap().len(), 4);
        assert_eq!(search(&c, "", 50).unwrap()[0].kind, "image");
    }

    #[test]
    fn recherche_insensible_aux_accents_et_a_la_casse_unicode() {
        let c = db();
        for (i, t) in ["été", "ÉTÉ", "Ete", "hiver"].iter().enumerate() { record_text(&c, t, i as i64).unwrap(); }
        for q in ["ete", "Été", "ÉTÉ"] { assert_eq!(texts(&c, q).len(), 3, "{q}"); }
    }

    #[test]
    fn get_et_delete() {
        let c = db();
        let t = record_text(&c, "abc", 1).unwrap();
        let i = record_image(&c, b"png", b"th", 2).unwrap();
        assert_eq!(get(&c, t).unwrap(), Some(ClipContent::Text("abc".into())));
        assert_eq!(get(&c, i).unwrap(), Some(ClipContent::Image(b"png".to_vec())));
        let img = search(&c, "", 50).unwrap().into_iter().find(|e| e.id == i).unwrap();
        assert_eq!(img.thumb.as_deref(), Some(&b"th"[..]));
        delete(&c, t).unwrap();
        assert_eq!(get(&c, t).unwrap(), None);
        assert_eq!(count(&c), 1);
    }
}
