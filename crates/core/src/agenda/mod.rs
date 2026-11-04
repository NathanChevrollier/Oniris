//! Agenda local : table `events` (migration 3) et outils du courtier (`tools`).
pub mod tools;

use crate::CoreError;
use chrono::{Datelike, Duration, Local, TimeZone};
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub id: i64,
    pub title: String,
    pub start_ms: i64,
    pub end_ms: Option<i64>,
    pub all_day: bool,
    pub color: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct NewEvent {
    pub title: String,
    pub start_ms: i64,
    pub end_ms: Option<i64>,
    pub all_day: bool,
    pub color: Option<String>,
    pub notes: Option<String>,
}

const COLS: &str = "id, title, start_ms, end_ms, all_day, color, notes";

fn from_row(r: &Row) -> rusqlite::Result<Event> {
    Ok(Event {
        id: r.get(0)?, title: r.get(1)?, start_ms: r.get(2)?, end_ms: r.get(3)?,
        all_day: r.get::<_, i64>(4)? != 0, color: r.get(5)?, notes: r.get(6)?,
    })
}

pub fn insert(c: &Connection, e: &NewEvent) -> Result<i64, CoreError> {
    c.execute(
        "INSERT INTO events(title, start_ms, end_ms, all_day, color, notes, created_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![e.title, e.start_ms, e.end_ms, e.all_day, e.color, e.notes, Local::now().timestamp_millis()],
    )?;
    Ok(c.last_insert_rowid())
}

pub fn update_times(c: &Connection, id: i64, start_ms: i64, end_ms: Option<i64>) -> Result<(), CoreError> {
    c.execute("UPDATE events SET start_ms = ?2, end_ms = ?3 WHERE id = ?1", params![id, start_ms, end_ms])?;
    Ok(())
}

pub fn set_deleted(c: &Connection, id: i64, deleted: bool) -> Result<(), CoreError> {
    c.execute("UPDATE events SET deleted = ?2 WHERE id = ?1", params![id, deleted])?;
    Ok(())
}

/// Un événement non supprimé.
pub fn get(c: &Connection, id: i64) -> Result<Option<Event>, CoreError> {
    let mut st = c.prepare(&format!("SELECT {COLS} FROM events WHERE id = ?1 AND deleted = 0"))?;
    let mut rows = st.query_map([id], from_row)?;
    Ok(rows.next().transpose()?)
}

/// Événements qui chevauchent [from_ms, to_ms[ ; sans fin, un événement est un instant.
pub fn between(c: &Connection, from_ms: i64, to_ms: i64) -> Result<Vec<Event>, CoreError> {
    let mut st = c.prepare(&format!(
        "SELECT {COLS} FROM events
         WHERE deleted = 0 AND start_ms < ?2 AND COALESCE(end_ms, start_ms) >= ?1
         ORDER BY start_ms, id"
    ))?;
    let rows = st.query_map([from_ms, to_ms], from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Jours (1..=31, heure locale) du mois [month_start_ms, month_end_ms[ qui portent au moins un événement.
pub fn days_with_events(c: &Connection, month_start_ms: i64, month_end_ms: i64) -> Result<Vec<u32>, CoreError> {
    let mut days = std::collections::BTreeSet::new();
    let local = |ms: i64| Local.timestamp_millis_opt(ms).earliest();
    let (Some(m0), Some(m1)) = (local(month_start_ms), local(month_end_ms)) else { return Ok(vec![]) };
    for e in between(c, month_start_ms, month_end_ms)? {
        let Some(s) = local(e.start_ms) else { continue };
        // Fin exclusive : un événement qui finit à minuit pile ne déborde pas sur le jour suivant.
        let end = e.end_ms.and_then(local).filter(|t| *t > s).map_or(s, |t| t - Duration::milliseconds(1));
        let mut d = s.date_naive().max(m0.date_naive());
        let last = end.date_naive().min((m1 - Duration::milliseconds(1)).date_naive());
        while d <= last {
            days.insert(d.day());
            let Some(next) = d.succ_opt() else { break };
            d = next;
        }
    }
    Ok(days.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{migrate, open_db, CORE_MIGRATIONS};
    use chrono::NaiveDate;

    pub(crate) fn db() -> (tempfile::TempDir, Connection) {
        let dir = tempfile::tempdir().unwrap();
        let mut c = open_db(&dir.path().join("t.db")).unwrap();
        migrate(&mut c, CORE_MIGRATIONS).unwrap();
        (dir, c)
    }
    fn ms(y: i32, m: u32, d: u32, h: u32) -> i64 {
        let n = NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(h, 0, 0).unwrap();
        Local.from_local_datetime(&n).earliest().unwrap().timestamp_millis()
    }
    fn ev(title: &str, start_ms: i64, end_ms: Option<i64>) -> NewEvent {
        NewEvent { title: title.into(), start_ms, end_ms, ..Default::default() }
    }

    #[test]
    fn migration_3_cree_la_table_et_reste_idempotente() {
        let (_d, mut c) = db();
        let v = migrate(&mut c, CORE_MIGRATIONS).unwrap();
        assert_eq!(v, CORE_MIGRATIONS.len() as u32);
        let n: i64 = c.query_row("SELECT count(*) FROM events", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
        assert!(c.execute("INSERT INTO events(title, start_ms, created_ms) VALUES ('a', 'x', 0)", []).is_err());
    }

    #[test]
    fn between_renvoie_l_evenement_qui_chevauche_le_debut() {
        let (_d, c) = db();
        let id = insert(&c, &ev("nuit", ms(2026, 11, 11, 22), Some(ms(2026, 11, 12, 2)))).unwrap();
        insert(&c, &ev("avant", ms(2026, 11, 11, 8), Some(ms(2026, 11, 11, 9)))).unwrap();
        insert(&c, &ev("après", ms(2026, 11, 13, 8), None)).unwrap();
        let got = between(&c, ms(2026, 11, 12, 0), ms(2026, 11, 13, 0)).unwrap();
        assert_eq!(got.iter().map(|e| e.id).collect::<Vec<_>>(), vec![id]);
    }

    #[test]
    fn supprime_exclu_et_restaurable() {
        let (_d, c) = db();
        let id = insert(&c, &ev("x", ms(2026, 11, 12, 10), None)).unwrap();
        set_deleted(&c, id, true).unwrap();
        assert!(get(&c, id).unwrap().is_none());
        assert!(between(&c, 0, i64::MAX).unwrap().is_empty());
        set_deleted(&c, id, false).unwrap();
        assert_eq!(get(&c, id).unwrap().unwrap().title, "x");
    }

    #[test]
    fn jours_avec_evenements() {
        let (_d, c) = db();
        insert(&c, &ev("a", ms(2026, 11, 12, 10), None)).unwrap();
        // Sur trois jours, finit à minuit pile : 20, 21, 22 seulement.
        insert(&c, &ev("b", ms(2026, 11, 20, 18), Some(ms(2026, 11, 23, 0)))).unwrap();
        // Chevauche la fin d'octobre : seul le 1er novembre compte.
        insert(&c, &ev("c", ms(2026, 10, 31, 22), Some(ms(2026, 11, 1, 3)))).unwrap();
        insert(&c, &ev("hors", ms(2026, 12, 2, 10), None)).unwrap();
        let days = days_with_events(&c, ms(2026, 11, 1, 0), ms(2026, 12, 1, 0)).unwrap();
        assert_eq!(days, vec![1, 12, 20, 21, 22]);
    }
}
