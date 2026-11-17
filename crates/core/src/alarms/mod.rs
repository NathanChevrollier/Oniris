//! Alarmes à heure fixe, répétées ou non : table `alarms` (migration 19) et outils du courtier (`tools`).
//! La sonnerie et l'ordonnanceur vivent côté Tauri ; ici, analyse du texte et échéances.
pub mod tools;

use crate::time_util::now_ms;
use crate::when::{at, norm, parse_when_tz};
use crate::CoreError;
use chrono::{DateTime, Datelike, Duration, Local, TimeZone, Timelike};
use rusqlite::{params, Connection, Row};
use serde::Serialize;

/// Jours de répétition : bit 0 = lundi … bit 6 = dimanche ; 0 = une seule fois.
pub const EVERY_DAY: u8 = 0x7F;
pub const WEEKDAYS: u8 = 0x1F;
pub const WEEKEND: u8 = 0x60;
pub const SNOOZE_MIN: i64 = 5;
/// Au démarrage, une alarme passée de plus d'une minute est « manquée » (PC éteint) : signalée, jamais sonnée.
pub const STARTUP_GRACE_MS: i64 = 60_000;
/// En marche (réveil de veille), une alarme en retard de plus de 10 min est aussi manquée.
pub const LATE_GRACE_MS: i64 = 10 * 60_000;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Alarm {
    pub id: i64,
    pub label: String,
    pub hour: u32,
    pub minute: u32,
    pub days: u8,
    /// Prochaine sonnerie (échéance, pour une alarme renvoyée par `take_due`).
    pub next_ms: i64,
    /// « une fois », « tous les jours », « en semaine », « lun, mer »…
    pub repeat: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Spec<Tz: TimeZone> {
    pub label: String,
    pub hour: u32,
    pub minute: u32,
    pub days: u8,
    pub next: DateTime<Tz>,
}

const DAYS: [&str; 7] = ["lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche"];

pub fn repeat_label(days: u8) -> String {
    match days & EVERY_DAY {
        0 => "une fois".into(),
        EVERY_DAY => "tous les jours".into(),
        WEEKDAYS => "en semaine".into(),
        WEEKEND => "le week-end".into(),
        d => DAYS.iter().enumerate().filter(|(i, _)| d & (1 << i) != 0).map(|(_, n)| &n[..3]).collect::<Vec<_>>().join(", "),
    }
}

/// Répétition écrite : « tous les jours », « en semaine », « le week-end », « tous les lundis », « chaque mardi et jeudi ».
fn repeat_days(w: &[String]) -> u8 {
    let has = |s: &str| w.iter().any(|x| x == s);
    if has("ouvres") || has("ouvrables") || w.windows(2).any(|p| matches!(p[0].as_str(), "en" | "de") && p[1] == "semaine") {
        return WEEKDAYS;
    }
    let every = has("chaque") || has("tous");
    let mut m = 0;
    for x in w {
        match x.as_str() {
            "jours" | "jour" if every => m |= EVERY_DAY,
            "quotidien" | "quotidienne" | "quotidiennement" => m |= EVERY_DAY,
            "week-end" | "weekend" | "week-ends" | "weekends" => m |= WEEKEND,
            _ => {
                // Pluriel (« les lundis ») ou après « chaque » / « tous » : jour répété, sinon simple date.
                if let Some(i) = DAYS.iter().position(|d| *d == x.trim_end_matches('s')) {
                    if every || x.ends_with('s') {
                        m |= 1 << i;
                    }
                }
            }
        }
    }
    m
}

const NOT_LABEL: [&str; 20] = [
    "tous", "les", "chaque", "jour", "jours", "en", "de", "semaine", "week-end", "weekend", "quotidien", "ouvres", "et",
    "reveille-moi", "reveille", "moi", "alarme", "reveil", "une", "mets",
];

fn label_of(rest: &str, words: &[String]) -> String {
    let label = rest.split_whitespace()
        .filter(|x| { let n = norm(x); !NOT_LABEL.contains(&n.as_str()) && !DAYS.contains(&n.trim_end_matches('s')) })
        .collect::<Vec<_>>().join(" ");
    if !label.is_empty() { label } else if words.iter().any(|x| x.starts_with("reveil")) { "Réveil".into() } else { "Alarme".into() }
}

/// Prochain instant strictement après `now`, à h:m, un jour permis par `days` (tous si 0).
pub fn next_occurrence<Tz: TimeZone>(now: &DateTime<Tz>, hour: u32, minute: u32, days: u8) -> Option<DateTime<Tz>> {
    let tz = now.timezone();
    let today = now.date_naive();
    (0..8)
        .map(|k| today + Duration::days(k))
        .filter(|d| days == 0 || days & (1 << d.weekday().num_days_from_monday()) != 0)
        .filter_map(|d| at(&tz, d, (hour, minute)))
        .find(|t| t > now)
}

/// « réveille-moi à 7 h », « alarme à 14 h 30 demain », « tous les jours en semaine à 6h45 sport ».
/// Une heure est obligatoire ; heure passée sans date : demain.
pub fn parse_alarm_tz<Tz: TimeZone>(text: &str, now: DateTime<Tz>) -> Option<Spec<Tz>> {
    let words: Vec<String> = text.split_whitespace().map(norm).collect();
    let days = repeat_days(&words);
    let w = parse_when_tz(text, now.clone())?;
    if w.all_day {
        return None;
    }
    let (hour, minute) = (w.start.hour(), w.start.minute());
    let next = if days == 0 { w.start } else { next_occurrence(&now, hour, minute, days)? };
    Some(Spec { label: label_of(&w.rest, &words), hour, minute, days, next })
}

pub fn parse_alarm(text: &str) -> Option<Spec<Local>> { parse_alarm_tz(text, Local::now()) }

const COLS: &str = "id, label, hour, minute, days, next_ms";

fn from_row(r: &Row) -> rusqlite::Result<Alarm> {
    let days: u8 = r.get(4)?;
    Ok(Alarm { id: r.get(0)?, label: r.get(1)?, hour: r.get(2)?, minute: r.get(3)?, days, next_ms: r.get(5)?, repeat: repeat_label(days) })
}

pub fn add<Tz: TimeZone>(c: &Connection, s: &Spec<Tz>) -> Result<Alarm, CoreError> {
    let next_ms = s.next.timestamp_millis();
    c.execute(
        "INSERT INTO alarms(label, hour, minute, days, next_ms, created_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![s.label, s.hour, s.minute, s.days, next_ms, now_ms()],
    )?;
    Ok(Alarm { id: c.last_insert_rowid(), label: s.label.clone(), hour: s.hour, minute: s.minute, days: s.days, next_ms, repeat: repeat_label(s.days) })
}

/// Toutes les alarmes, la plus proche d'abord.
pub fn list(c: &Connection) -> Result<Vec<Alarm>, CoreError> {
    let mut st = c.prepare(&format!("SELECT {COLS} FROM alarms ORDER BY next_ms, id"))?;
    let rows = st.query_map([], from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Supprime une alarme ; faux si elle n'existe pas.
pub fn cancel(c: &Connection, id: i64) -> Result<bool, CoreError> {
    Ok(c.execute("DELETE FROM alarms WHERE id = ?1", [id])? > 0)
}

pub fn next_due(c: &Connection) -> Result<Option<i64>, CoreError> {
    Ok(c.query_row("SELECT min(next_ms) FROM alarms", [], |r| r.get(0))?)
}

/// Alarmes échues (avec leur échéance dans `next_ms`) : une alarme unique est supprimée, une répétée passe à
/// sa prochaine occurrence après `now` (les occurrences manquées sont sautées). Deux appels ne rendent jamais la même.
pub fn take_due_in<Tz: TimeZone>(c: &Connection, now: i64, tz: &Tz) -> Result<Vec<Alarm>, CoreError> {
    let tx = c.unchecked_transaction()?;
    let due = {
        let mut st = tx.prepare(&format!("SELECT {COLS} FROM alarms WHERE next_ms <= ?1 ORDER BY next_ms, id"))?;
        let rows = st.query_map([now], from_row)?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    let now_dt = tz.timestamp_millis_opt(now).single().ok_or_else(|| CoreError::Invalid("instant invalide".into()))?;
    for a in &due {
        match (a.days != 0).then(|| next_occurrence(&now_dt, a.hour, a.minute, a.days)).flatten() {
            Some(n) => tx.execute("UPDATE alarms SET next_ms = ?2 WHERE id = ?1", params![a.id, n.timestamp_millis()])?,
            None => tx.execute("DELETE FROM alarms WHERE id = ?1", [a.id])?,
        };
    }
    tx.commit()?;
    Ok(due)
}

pub fn take_due(c: &Connection, now: i64) -> Result<Vec<Alarm>, CoreError> { take_due_in(c, now, &Local) }

/// Sépare les alarmes à sonner de celles manquées (échéance trop ancienne) : `(à sonner, manquées)`.
pub fn split_late(due: Vec<Alarm>, now: i64, grace_ms: i64) -> (Vec<Alarm>, Vec<Alarm>) {
    due.into_iter().partition(|a| now - a.next_ms <= grace_ms)
}

/// « Répéter dans 5 min » : nouvelle alarme unique, même libellé.
pub fn snooze_in<Tz: TimeZone>(c: &Connection, label: &str, now: i64, tz: &Tz) -> Result<Alarm, CoreError> {
    let next = tz.timestamp_millis_opt(now + SNOOZE_MIN * 60_000).single().ok_or_else(|| CoreError::Invalid("instant invalide".into()))?;
    add(c, &Spec { label: label.into(), hour: next.hour(), minute: next.minute(), days: 0, next })
}

pub fn snooze(c: &Connection, label: &str, now: i64) -> Result<Alarm, CoreError> { snooze_in(c, label, now, &Local) }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::db;
    use chrono_tz::Europe::Paris;

    type P = DateTime<chrono_tz::Tz>;
    fn p(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> P { Paris.with_ymd_and_hms(y, mo, d, h, mi, 0).unwrap() }
    /// mardi 2026-11-03 14:20
    fn now() -> P { p(2026, 11, 3, 14, 20) }
    fn parse(t: &str) -> Option<Spec<chrono_tz::Tz>> { parse_alarm_tz(t, now()) }

    #[test]
    fn heures_simples_passees_et_lendemain() {
        let s = parse("réveille-moi à 7 h").unwrap();
        assert_eq!((s.next, s.days, s.label.as_str()), (p(2026, 11, 4, 7, 0), 0, "Réveil"));
        assert_eq!(parse("alarme à 15h").unwrap().next, p(2026, 11, 3, 15, 0));
        let s = parse("alarme à 14 h 30 demain").unwrap();
        assert_eq!((s.next, s.label.as_str()), (p(2026, 11, 4, 14, 30), "Alarme"));
        assert_eq!(parse("alarme 14h20").unwrap().next, p(2026, 11, 4, 14, 20), "l'instant présent est déjà passé");
        assert_eq!(parse("alarme à minuit").unwrap().next, p(2026, 11, 4, 0, 0));
        assert_eq!(parse("alarme demain à minuit").unwrap().next, p(2026, 11, 5, 0, 0));
        assert_eq!(parse("alarme 6h45 sport").unwrap().label, "sport");
    }

    #[test]
    fn repetitions() {
        let s = parse("tous les jours à 7h").unwrap();
        assert_eq!((s.days, s.next), (EVERY_DAY, p(2026, 11, 4, 7, 0)));
        let s = parse("en semaine à 15h").unwrap();
        assert_eq!((s.days, s.next), (WEEKDAYS, p(2026, 11, 3, 15, 0)));
        let s = parse("le week-end à 9h").unwrap();
        assert_eq!((s.days, s.next), (WEEKEND, p(2026, 11, 7, 9, 0)));
        let s = parse("tous les lundis et jeudis à 8h piscine").unwrap();
        assert_eq!((s.days, s.next, s.label.as_str()), (0b1001, p(2026, 11, 5, 8, 0), "piscine"));
        let s = parse("chaque mardi à 18h").unwrap();
        assert_eq!((s.days, s.next), (0b10, p(2026, 11, 3, 18, 0)), "aujourd'hui si l'heure n'est pas passée");
        assert_eq!(parse("chaque mardi à 9h").unwrap().next, p(2026, 11, 10, 9, 0));
        let s = parse("lundi à 7h").unwrap();
        assert_eq!((s.days, s.next), (0, p(2026, 11, 9, 7, 0)), "un jour au singulier : une seule fois");
        assert_eq!(parse("tous les jours à minuit").unwrap().next, p(2026, 11, 4, 0, 0));
        assert_eq!(repeat_label(0b101), "lun, mer");
        assert_eq!(repeat_label(WEEKDAYS), "en semaine");
    }

    #[test]
    fn sans_heure_rien() {
        for t in ["alarme", "alarme demain", "lundi", "tous les jours"] {
            assert!(parse(t).is_none(), "{t}");
        }
    }

    #[test]
    fn echeances_unique_supprimee_repetee_avancee() {
        let c = db();
        let one = add(&c, &parse("alarme à 15h").unwrap()).unwrap();
        let daily = add(&c, &parse("tous les jours à 16h").unwrap()).unwrap();
        assert_eq!(next_due(&c).unwrap(), Some(one.next_ms));
        let t = p(2026, 11, 3, 16, 0).timestamp_millis();
        let due = take_due_in(&c, t, &Paris).unwrap();
        assert_eq!(due.iter().map(|a| a.id).collect::<Vec<_>>(), vec![one.id, daily.id]);
        assert!(take_due_in(&c, t, &Paris).unwrap().is_empty());
        let left = list(&c).unwrap();
        assert_eq!((left.len(), left[0].next_ms), (1, p(2026, 11, 4, 16, 0).timestamp_millis()));
        // PC éteint trois jours : une seule sonnerie rendue, la suivante est après maintenant.
        let late = p(2026, 11, 7, 17, 0).timestamp_millis();
        let due = take_due_in(&c, late, &Paris).unwrap();
        assert_eq!(due.len(), 1);
        let (ring, missed) = split_late(due, late, STARTUP_GRACE_MS);
        assert!(ring.is_empty());
        assert_eq!(missed.len(), 1);
        assert_eq!(list(&c).unwrap()[0].next_ms, p(2026, 11, 8, 16, 0).timestamp_millis());
        assert!(cancel(&c, daily.id).unwrap());
        assert!(!cancel(&c, daily.id).unwrap());
        assert_eq!(next_due(&c).unwrap(), None);
    }

    #[test]
    fn repeter_dans_cinq_minutes() {
        let c = db();
        let a = snooze_in(&c, "Réveil", p(2026, 11, 3, 7, 0).timestamp_millis(), &Paris).unwrap();
        assert_eq!((a.hour, a.minute, a.days, a.next_ms), (7, 5, 0, p(2026, 11, 3, 7, 5).timestamp_millis()));
        let (ring, missed) = split_late(vec![a.clone()], a.next_ms + 1000, LATE_GRACE_MS);
        assert_eq!((ring.len(), missed.len()), (1, 0));
    }
}
