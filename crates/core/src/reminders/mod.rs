//! Rappels : table `reminders` (migration 4) et outils du courtier (`tools`).
pub mod tools;

use crate::time_util::local_day_bounds;
use crate::CoreError;
use chrono::Local;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reminder {
    pub id: i64,
    pub text: String,
    pub due_ms: Option<i64>,
    pub done: bool,
}

/// Au-delà de ce retard, la notification est « en retard » (l'ordonnanceur est plafonné à 60 s : en dessous, c'est normal).
const LATE_AFTER_MS: i64 = 60_000;

/// Vrai si le rappel a plus de 60 s de retard.
pub fn is_late(due_ms: i64, now_ms: i64) -> bool { now_ms - due_ms > LATE_AFTER_MS }

const COLS: &str = "id, text, due_ms, done";

fn from_row(r: &Row) -> rusqlite::Result<Reminder> {
    Ok(Reminder { id: r.get(0)?, text: r.get(1)?, due_ms: r.get(2)?, done: r.get::<_, i64>(3)? != 0 })
}

pub fn insert(c: &Connection, text: &str, due_ms: Option<i64>) -> Result<i64, CoreError> {
    c.execute(
        "INSERT INTO reminders(text, due_ms, created_ms) VALUES (?1, ?2, ?3)",
        params![text, due_ms, Local::now().timestamp_millis()],
    )?;
    Ok(c.last_insert_rowid())
}

/// Coche (heure du premier cochage gardée dans `done_ms`, migration 9) ou décoche (`done_ms` effacé).
pub fn set_done(c: &Connection, id: i64, done: bool) -> Result<(), CoreError> {
    c.execute(
        "UPDATE reminders SET done = ?2, done_ms = CASE WHEN ?2 THEN COALESCE(done_ms, ?3) END WHERE id = ?1",
        params![id, done, Local::now().timestamp_millis()],
    )?;
    Ok(())
}

pub fn set_deleted(c: &Connection, id: i64, deleted: bool) -> Result<(), CoreError> {
    c.execute("UPDATE reminders SET deleted = ?2 WHERE id = ?1", params![id, deleted])?;
    Ok(())
}

/// Un rappel non supprimé.
pub fn get(c: &Connection, id: i64) -> Result<Option<Reminder>, CoreError> {
    let mut st = c.prepare(&format!("SELECT {COLS} FROM reminders WHERE id = ?1 AND deleted = 0"))?;
    let mut rows = st.query_map([id], from_row)?;
    Ok(rows.next().transpose()?)
}

/// Rappels non faits et non supprimés, par échéance croissante (sans échéance en dernier).
pub fn list_open(c: &Connection) -> Result<Vec<Reminder>, CoreError> {
    let mut st = c.prepare(&format!(
        "SELECT {COLS} FROM reminders WHERE done = 0 AND deleted = 0 ORDER BY due_ms IS NULL, due_ms, id"
    ))?;
    let rows = st.query_map([], from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Liste du panneau : rappels ouverts, puis ceux cochés depuis le minuit local de `now_ms` (barrés jusqu'à minuit).
pub fn list_today(c: &Connection, now_ms: i64) -> Result<Vec<Reminder>, CoreError> {
    let (start, _) = local_day_bounds(now_ms);
    let mut st = c.prepare(&format!(
        "SELECT {COLS} FROM reminders WHERE deleted = 0 AND (done = 0 OR done_ms >= ?1)
         ORDER BY done, due_ms IS NULL, due_ms, done_ms, id"
    ))?;
    let rows = st.query_map([start], from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Prochaine échéance à notifier (ni fait, ni supprimé, ni déjà notifié).
pub fn next_due(c: &Connection) -> Result<Option<i64>, CoreError> {
    Ok(c.query_row(
        "SELECT min(due_ms) FROM reminders WHERE done = 0 AND deleted = 0 AND notified = 0 AND due_ms IS NOT NULL",
        [], |r| r.get(0),
    )?)
}

/// Marque `notified = 1` et renvoie les rappels échus en une seule instruction (donc atomique) :
/// deux appels ne renvoient jamais le même rappel.
pub fn take_due(c: &Connection, now_ms: i64) -> Result<Vec<Reminder>, CoreError> {
    let mut st = c.prepare(&format!(
        "UPDATE reminders SET notified = 1
         WHERE done = 0 AND deleted = 0 AND notified = 0 AND due_ms IS NOT NULL AND due_ms <= ?1
         RETURNING {COLS}"
    ))?;
    let mut out = st.query_map([now_ms], from_row)?.collect::<Result<Vec<_>, _>>()?;
    out.sort_by_key(|r| (r.due_ms, r.id));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::db;
    use rusqlite::Connection;

    fn add(c: &Connection, text: &str, due: Option<i64>) -> i64 { insert(c, text, due).unwrap() }

    #[test]
    fn take_due_ne_renvoie_un_rappel_qu_une_fois() {
        let c = db();
        let id = add(&c, "a", Some(1000));
        add(&c, "futur", Some(9000));
        let got = take_due(&c, 2000).unwrap();
        assert_eq!(got.iter().map(|r| r.id).collect::<Vec<_>>(), vec![id]);
        assert!(take_due(&c, 2000).unwrap().is_empty());
        assert!(take_due(&c, 3000).unwrap().is_empty());
    }

    #[test]
    fn rappel_echu_pendant_l_absence_est_en_retard_puis_jamais_renvoye() {
        let c = db();
        add(&c, "banque", Some(1_000));
        let now = 1_000 + 3_600_000; // une heure plus tard, au démarrage
        let got = take_due(&c, now).unwrap();
        assert_eq!(got.len(), 1);
        assert!(is_late(got[0].due_ms.unwrap(), now));
        assert!(take_due(&c, now + 1).unwrap().is_empty());
        assert!(!is_late(1_000, 1_000 + 60_000));
        assert!(is_late(1_000, 1_000 + 60_001));
    }

    #[test]
    fn next_due_ignore_faits_supprimes_et_notifies() {
        let c = db();
        assert_eq!(next_due(&c).unwrap(), None);
        let fait = add(&c, "fait", Some(10));
        let supp = add(&c, "supp", Some(20));
        let notif = add(&c, "notif", Some(30));
        add(&c, "sans", None);
        add(&c, "ok", Some(500));
        set_done(&c, fait, true).unwrap();
        set_deleted(&c, supp, true).unwrap();
        take_due(&c, 30).unwrap(); // notifie « notif » (les autres sont exclus)
        assert_eq!(c.query_row("SELECT notified FROM reminders WHERE id = ?1", [notif], |r| r.get::<_, i64>(0)).unwrap(), 1);
        assert_eq!(next_due(&c).unwrap(), Some(500));
    }

    #[test]
    fn list_open_trie_par_echeance_sans_echeance_en_dernier() {
        let c = db();
        let sans = add(&c, "sans", None);
        let tard = add(&c, "tard", Some(900));
        let tot = add(&c, "tôt", Some(100));
        let fait = add(&c, "fait", Some(50));
        set_done(&c, fait, true).unwrap();
        let ids: Vec<_> = list_open(&c).unwrap().iter().map(|r| r.id).collect();
        assert_eq!(ids, vec![tot, tard, sans]);
    }

    #[test]
    fn list_today_ouverts_puis_faits_du_jour() {
        let c = db();
        let now = Local::now().timestamp_millis();
        let (start, _) = local_day_bounds(now);
        let fait = add(&c, "fait", Some(10));
        let hier = add(&c, "fait hier", Some(20));
        let ouvert = add(&c, "ouvert", None);
        let tot = add(&c, "tôt", Some(100));
        set_done(&c, fait, true).unwrap();
        let done_ms: i64 = c.query_row("SELECT done_ms FROM reminders WHERE id = ?1", [fait], |r| r.get(0)).unwrap();
        assert!((now - done_ms).abs() < 60_000);
        set_done(&c, hier, true).unwrap();
        c.execute("UPDATE reminders SET done_ms = ?2 WHERE id = ?1", params![hier, start - 1]).unwrap();
        let got = list_today(&c, now).unwrap();
        assert_eq!(got.iter().map(|r| r.id).collect::<Vec<_>>(), vec![tot, ouvert, fait]);
        assert!(got[2].done);
        // Décocher efface l'heure : le rappel redevient ouvert.
        set_done(&c, fait, false).unwrap();
        assert_eq!(c.query_row("SELECT done_ms FROM reminders WHERE id = ?1", [fait], |r| r.get::<_, Option<i64>>(0)).unwrap(), None);
        assert_eq!(list_today(&c, now).unwrap().iter().map(|r| r.id).collect::<Vec<_>>(), vec![fait, tot, ouvert]);
    }

    #[test]
    fn recocher_garde_l_heure_du_premier_cochage() {
        let c = db();
        let id = add(&c, "x", None);
        set_done(&c, id, true).unwrap();
        c.execute("UPDATE reminders SET done_ms = 42 WHERE id = ?1", [id]).unwrap();
        set_done(&c, id, true).unwrap();
        assert_eq!(c.query_row("SELECT done_ms FROM reminders WHERE id = ?1", [id], |r| r.get::<_, i64>(0)).unwrap(), 42);
    }

    #[test]
    fn migration_4_cree_la_table() {
        let c = db();
        assert!(c.execute("INSERT INTO reminders(text, due_ms, created_ms) VALUES ('a', 'x', 0)", []).is_err());
    }
}
