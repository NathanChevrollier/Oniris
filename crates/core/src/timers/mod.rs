//! Minuteurs et sessions focus/pause : table `timers` (migration 6) et outils du courtier (`tools`).
pub mod tools;

use crate::CoreError;
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Timer {
    pub id: i64,
    pub label: String,
    /// "timer", "focus" ou "break".
    pub kind: String,
    pub duration_ms: i64,
    pub ends_ms: i64,
    pub focus_cycle: Option<i64>,
}

const COLS: &str = "id, label, kind, duration_ms, ends_ms, focus_cycle";

fn from_row(r: &Row) -> rusqlite::Result<Timer> {
    Ok(Timer { id: r.get(0)?, label: r.get(1)?, kind: r.get(2)?, duration_ms: r.get(3)?, ends_ms: r.get(4)?, focus_cycle: r.get(5)? })
}

pub fn start(c: &Connection, label: &str, kind: &str, duration_ms: i64, now: i64) -> Result<Timer, CoreError> {
    c.execute(
        "INSERT INTO timers(label, kind, duration_ms, ends_ms, state, created_ms) VALUES (?1, ?2, ?3, ?4, 'running', ?5)",
        params![label, kind, duration_ms, now + duration_ms, now],
    )?;
    Ok(Timer { id: c.last_insert_rowid(), label: label.into(), kind: kind.into(), duration_ms, ends_ms: now + duration_ms, focus_cycle: None })
}

/// Annule un minuteur en cours ; faux s'il n'y en a pas (inconnu, déjà fini ou annulé).
pub fn cancel(c: &Connection, id: i64) -> Result<bool, CoreError> {
    Ok(c.execute("UPDATE timers SET state = 'cancelled' WHERE id = ?1 AND state = 'running'", [id])? > 0)
}

/// Minuteurs en cours, le plus proche de la fin d'abord.
pub fn running(c: &Connection) -> Result<Vec<Timer>, CoreError> {
    let mut st = c.prepare(&format!("SELECT {COLS} FROM timers WHERE state = 'running' ORDER BY ends_ms, id"))?;
    let rows = st.query_map([], from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Passe à `done` et renvoie les minuteurs échus, en une seule instruction (donc atomique) :
/// deux appels ne renvoient jamais le même minuteur.
pub fn take_finished(c: &Connection, now: i64) -> Result<Vec<Timer>, CoreError> {
    let mut st = c.prepare(&format!(
        "UPDATE timers SET state = 'done' WHERE state = 'running' AND ends_ms <= ?1 RETURNING {COLS}"
    ))?;
    let mut out = st.query_map([now], from_row)?.collect::<Result<Vec<_>, _>>()?;
    out.sort_by_key(|t| (t.ends_ms, t.id));
    Ok(out)
}

/// Prochaine fin d'un minuteur en cours.
pub fn next_end(c: &Connection) -> Result<Option<i64>, CoreError> {
    Ok(c.query_row("SELECT min(ends_ms) FROM timers WHERE state = 'running'", [], |r| r.get(0))?)
}

/// Suite d'un minuteur terminé : un focus enchaîne sur une pause ; une pause ou un minuteur simple, rien.
pub fn after_finish(c: &Connection, done: &Timer, break_ms: i64, now: i64) -> Result<Option<Timer>, CoreError> {
    if done.kind != "focus" { return Ok(None); }
    let b = start(c, "Pause", "break", break_ms, now)?;
    c.execute("UPDATE timers SET focus_cycle = ?2 WHERE id = ?1", params![b.id, done.focus_cycle])?;
    Ok(Some(Timer { focus_cycle: done.focus_cycle, ..b }))
}

/// Cherche une durée dans le texte : millisecondes et plage d'octets occupée.
/// Reconnaît « 25 minutes », « 25 min », « 1h30 », « 1 h 30 », « 90 s », « 2 heures ».
pub(crate) fn find_duration(text: &str) -> Option<(u64, std::ops::Range<usize>)> {
    let b = text.as_bytes();
    let digits = |from: usize| from + b[from..].iter().take_while(|c| c.is_ascii_digit()).count();
    let spaces = |from: usize| from + b[from..].iter().take_while(|c| **c == b' ').count();
    let word = |from: usize| {
        let end = from + text[from..].chars().take_while(|c| c.is_alphabetic()).map(char::len_utf8).sum::<usize>();
        (text[from..end].to_lowercase(), end)
    };
    let mut i = 0;
    while i < b.len() {
        if !b[i].is_ascii_digit() || (i > 0 && b[i - 1].is_ascii_alphanumeric()) { i += 1; continue; }
        let d_end = digits(i);
        let Ok(n) = text[i..d_end].parse::<u64>() else { i = d_end; continue };
        let (unit, w_end) = word(spaces(d_end));
        let (secs, end) = match unit.as_str() {
            "h" | "heure" | "heures" => {
                // « 1h30 », « 1 h 30 » : les minutes suivent, « min » facultatif.
                let m0 = spaces(w_end);
                let m_end = digits(m0);
                if m_end > m0 && m_end - m0 <= 2 {
                    let mins: u64 = text[m0..m_end].parse().unwrap_or(0);
                    let (u2, e2) = word(spaces(m_end));
                    (n * 3600 + mins * 60, if matches!(u2.as_str(), "m" | "min" | "mins" | "minute" | "minutes" | "mn") { e2 } else { m_end })
                } else { (n * 3600, w_end) }
            }
            "m" | "min" | "mins" | "minute" | "minutes" | "mn" => (n * 60, w_end),
            "s" | "sec" | "secs" | "seconde" | "secondes" => (n, w_end),
            _ => { i = d_end; continue }
        };
        return (secs > 0).then_some((secs * 1000, i..end));
    }
    None
}

/// Durée en millisecondes, ou `None` si le texte n'en contient pas.
pub fn parse_duration(text: &str) -> Option<u64> { find_duration(text).map(|(ms, _)| ms) }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{migrate, open_db, CORE_MIGRATIONS};

    pub(crate) fn db() -> (tempfile::TempDir, Connection) {
        let dir = tempfile::tempdir().unwrap();
        let mut c = open_db(&dir.path().join("t.db")).unwrap();
        migrate(&mut c, CORE_MIGRATIONS).unwrap();
        (dir, c)
    }

    #[test]
    fn parse_duration_formats() {
        let min = |m: u64| Some(m * 60_000);
        assert_eq!(parse_duration("25 minutes"), min(25));
        assert_eq!(parse_duration("25 min"), min(25));
        assert_eq!(parse_duration("10 min"), min(10));
        assert_eq!(parse_duration("1h30"), min(90));
        assert_eq!(parse_duration("1 h 30"), min(90));
        assert_eq!(parse_duration("90 s"), Some(90_000));
        assert_eq!(parse_duration("2 heures"), min(120));
        assert_eq!(parse_duration("minuteur 25 minutes thé"), min(25));
        assert_eq!(parse_duration("2h"), min(120));
        assert_eq!(parse_duration("bientôt"), None);
        assert_eq!(parse_duration("0 min"), None);
        assert_eq!(parse_duration("5 minuteur"), None);
    }

    #[test]
    fn take_finished_une_seule_fois() {
        let (_d, c) = db();
        let a = start(&c, "a", "timer", 1000, 0).unwrap();
        start(&c, "futur", "timer", 9000, 0).unwrap();
        let got = take_finished(&c, 2000).unwrap();
        assert_eq!(got.iter().map(|t| t.id).collect::<Vec<_>>(), vec![a.id]);
        assert!(take_finished(&c, 2000).unwrap().is_empty());
        assert!(take_finished(&c, 3000).unwrap().is_empty());
        assert_eq!(running(&c).unwrap().len(), 1);
    }

    #[test]
    fn minuteur_ecoule_pendant_l_absence_renvoye_une_seule_fois_au_demarrage() {
        let (_d, c) = db();
        start(&c, "thé", "timer", 60_000, 0).unwrap();
        let got = take_finished(&c, 3_600_000).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].label, "thé");
        assert!(take_finished(&c, 3_600_001).unwrap().is_empty());
    }

    #[test]
    fn focus_termine_demarre_une_pause_pause_terminee_rien() {
        let (_d, c) = db();
        start(&c, "Focus", "focus", 1000, 0).unwrap();
        let done = take_finished(&c, 1000).unwrap();
        let b = after_finish(&c, &done[0], 300_000, 1000).unwrap().expect("pause");
        assert_eq!((b.kind.as_str(), b.ends_ms), ("break", 301_000));
        assert_eq!(running(&c).unwrap().len(), 1);
        let done = take_finished(&c, 301_000).unwrap();
        assert_eq!(done[0].kind, "break");
        assert!(after_finish(&c, &done[0], 300_000, 301_000).unwrap().is_none());
        assert!(running(&c).unwrap().is_empty());
        let t = start(&c, "x", "timer", 1, 0).unwrap();
        assert!(after_finish(&c, &t, 1, 0).unwrap().is_none());
    }

    #[test]
    fn next_end_et_cancel() {
        let (_d, c) = db();
        assert_eq!(next_end(&c).unwrap(), None);
        let a = start(&c, "a", "timer", 500, 0).unwrap();
        start(&c, "b", "timer", 900, 0).unwrap();
        assert_eq!(next_end(&c).unwrap(), Some(500));
        assert!(cancel(&c, a.id).unwrap());
        assert!(!cancel(&c, a.id).unwrap());
        assert_eq!(next_end(&c).unwrap(), Some(900));
        assert!(c.execute("INSERT INTO timers(label, kind, duration_ms, ends_ms, state, created_ms) VALUES ('a','x',1,1,'running',0)", []).is_err());
    }
}
