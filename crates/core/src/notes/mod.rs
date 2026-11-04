//! Notes rapides : table `notes` (migration 5), index plein texte `notes_fts` (FTS5, contenu externe) et outils (`tools`).
//! Les notes supprimées restent dans l'index ; la jointure sur `deleted = 0` les écarte.
pub mod tools;

use crate::CoreError;
use chrono::Local;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub id: i64,
    pub body: String,
    pub pinned: bool,
    pub created_ms: i64,
    pub updated_ms: i64,
}

const COLS: &str = "n.id, n.body, n.pinned, n.created_ms, n.updated_ms";
const ORDER: &str = "ORDER BY n.pinned DESC, n.updated_ms DESC, n.id DESC";

fn from_row(r: &Row) -> rusqlite::Result<Note> {
    Ok(Note { id: r.get(0)?, body: r.get(1)?, pinned: r.get::<_, i64>(2)? != 0, created_ms: r.get(3)?, updated_ms: r.get(4)? })
}
fn now() -> i64 { Local::now().timestamp_millis() }

pub fn insert(c: &Connection, body: &str) -> Result<i64, CoreError> {
    let t = now();
    c.execute("INSERT INTO notes(body, created_ms, updated_ms) VALUES (?1, ?2, ?2)", params![body, t])?;
    Ok(c.last_insert_rowid())
}

pub fn update(c: &Connection, id: i64, body: &str) -> Result<(), CoreError> {
    c.execute("UPDATE notes SET body = ?2, updated_ms = ?3 WHERE id = ?1", params![id, body, now()])?;
    Ok(())
}

pub fn set_deleted(c: &Connection, id: i64, deleted: bool) -> Result<(), CoreError> {
    c.execute("UPDATE notes SET deleted = ?2 WHERE id = ?1", params![id, deleted])?;
    Ok(())
}

pub fn set_pinned(c: &Connection, id: i64, pinned: bool) -> Result<(), CoreError> {
    c.execute("UPDATE notes SET pinned = ?2 WHERE id = ?1", params![id, pinned])?;
    Ok(())
}

/// Une note non supprimée.
pub fn get(c: &Connection, id: i64) -> Result<Option<Note>, CoreError> {
    let mut st = c.prepare(&format!("SELECT {COLS} FROM notes n WHERE n.id = ?1 AND n.deleted = 0"))?;
    let mut rows = st.query_map([id], from_row)?;
    Ok(rows.next().transpose()?)
}

/// Notes non supprimées : épinglées d'abord, puis par modification récente.
pub fn recent(c: &Connection, limit: usize) -> Result<Vec<Note>, CoreError> {
    let mut st = c.prepare(&format!("SELECT {COLS} FROM notes n WHERE n.deleted = 0 {ORDER} LIMIT ?1"))?;
    let rows = st.query_map([limit as i64], from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Requête FTS5 sûre : chaque mot devient `"mot"*` (préfixe, guillemets doublés), le tout en ET.
/// `None` si rien ne reste après nettoyage.
fn match_query(query: &str) -> Option<String> {
    let tokens: Vec<String> = query.split_whitespace().map(|t| format!("\"{}\"*", t.replace('"', "\"\""))).collect();
    (!tokens.is_empty()).then(|| tokens.join(" "))
}

/// Recherche par préfixe sur chaque mot (insensible à la casse et aux accents). Requête vide : `recent`.
pub fn search(c: &Connection, query: &str, limit: usize) -> Result<Vec<Note>, CoreError> {
    let Some(q) = match_query(query) else { return recent(c, limit) };
    let mut st = c.prepare(&format!(
        "SELECT {COLS} FROM notes_fts JOIN notes n ON n.id = notes_fts.rowid
         WHERE notes_fts MATCH ?1 AND n.deleted = 0 {ORDER} LIMIT ?2"
    ))?;
    let rows = st.query_map(params![q, limit as i64], from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::db;

    fn found(c: &Connection, q: &str) -> Vec<String> { search(c, q, 50).unwrap().into_iter().map(|n| n.body).collect() }

    #[test]
    fn recherche_insensible_aux_accents_et_a_la_casse() {
        let c = db();
        insert(&c, "Réunion client").unwrap();
        insert(&c, "Courses").unwrap();
        assert_eq!(found(&c, "reunion"), vec!["Réunion client"]);
        assert_eq!(found(&c, "RÉUNION"), vec!["Réunion client"]);
    }

    #[test]
    fn recherche_par_prefixe_et_et_entre_mots() {
        let c = db();
        insert(&c, "Réunion client").unwrap();
        insert(&c, "client suivant").unwrap();
        assert_eq!(found(&c, "cli").len(), 2);
        assert_eq!(found(&c, "reun cli"), vec!["Réunion client"]);
    }

    #[test]
    fn note_supprimee_absente_puis_restauree() {
        let c = db();
        let id = insert(&c, "secret").unwrap();
        set_deleted(&c, id, true).unwrap();
        assert!(found(&c, "secret").is_empty());
        assert!(recent(&c, 50).unwrap().is_empty());
        set_deleted(&c, id, false).unwrap();
        assert_eq!(found(&c, "secret"), vec!["secret"]);
    }

    #[test]
    fn modification_reindexee() {
        let c = db();
        let id = insert(&c, "alpha").unwrap();
        update(&c, id, "bravo").unwrap();
        assert!(found(&c, "alpha").is_empty());
        assert_eq!(found(&c, "bravo"), vec!["bravo"]);
    }

    #[test]
    fn caracteres_speciaux_sans_erreur() {
        let c = db();
        insert(&c, "près du port: \"quai\" 🙂 NEAR").unwrap();
        for q in ["\"", "*", "-", "NEAR", "(", ")", "a AND", "\"\"", "- *", "🙂", "près", "col:on", "NEAR(", "OR", "^x"] {
            search(&c, q, 50).unwrap_or_else(|e| panic!("{q:?} : {e}"));
        }
        assert_eq!(found(&c, "\"quai\"").len(), 1);
        assert_eq!(found(&c, "pres").len(), 1);
        assert_eq!(found(&c, "near").len(), 1);
    }

    #[test]
    fn requete_vide_renvoie_recent_epinglees_d_abord() {
        let c = db();
        let a = insert(&c, "a").unwrap();
        insert(&c, "b").unwrap();
        set_pinned(&c, a, true).unwrap();
        let bodies = |q| search(&c, q, 50).unwrap().into_iter().map(|n| n.body).collect::<Vec<_>>();
        assert_eq!(bodies(""), vec!["a", "b"]);
        assert_eq!(bodies("  \t "), vec!["a", "b"]);
        assert_eq!(recent(&c, 1).unwrap().len(), 1);
    }
}
