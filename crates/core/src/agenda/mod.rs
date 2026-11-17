//! Agenda local : table `events` (migration 3) et outils du courtier (`tools`).
//! `between` réunit locaux, agendas Google et calendriers .ics, et fusionne les doublons.
pub mod calendars;
pub mod ics;
pub mod tools;

use crate::CoreError;
use chrono::{Datelike, Duration, Local, TimeZone};
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source { Local, Google, Ics }

/// Calendrier d'origine d'un événement (plusieurs après fusion des doublons).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventSource {
    pub calendar: String,
    /// Adresse du compte Google.
    pub account: Option<String>,
    pub color: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    /// Identifiant de la table `events` ; 0 pour un événement externe (ses ids sont du texte : voir `key`).
    pub id: i64,
    /// Clé unique côté interface : `l:<id>` (local) ou `g:<compte>:<agenda>:<id>` (Google).
    pub key: String,
    pub source: Source,
    /// Lecture seule : ni déplaçable ni supprimable (tout événement Google).
    pub readonly: bool,
    pub title: String,
    pub start_ms: i64,
    pub end_ms: Option<i64>,
    pub all_day: bool,
    pub color: Option<String>,
    pub notes: Option<String>,
    /// Nom du calendrier (externes seulement).
    #[serde(default)]
    pub calendar: Option<String>,
    /// Calendriers d'origine (externes, ou doublons fusionnés).
    #[serde(default)]
    pub sources: Vec<EventSource>,
    /// iCalUID : aide à reconnaître un doublon, jamais envoyé à l'interface.
    #[serde(skip)]
    pub uid: Option<String>,
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
    let id: i64 = r.get(0)?;
    Ok(Event {
        id, key: format!("l:{id}"), source: Source::Local, readonly: false, title: r.get(1)?, start_ms: r.get(2)?, end_ms: r.get(3)?,
        all_day: r.get::<_, i64>(4)? != 0, color: r.get(5)?, notes: r.get(6)?, calendar: None, sources: vec![], uid: None,
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

/// Événements locaux puis externes (Google, couleur du compte ; .ics, couleur du calendrier) des calendriers visibles
/// qui chevauchent [from_ms, to_ms[, triés par heure, doublons fusionnés (`merge_duplicates`).
/// La fin est exclue : finir pile à `from_ms` ne compte pas ;
/// un événement sans fin (ou de durée nulle) est un instant, compté si `start_ms >= from_ms`.
pub fn between(c: &Connection, from_ms: i64, to_ms: i64) -> Result<Vec<Event>, CoreError> {
    let mut st = c.prepare(&format!(
        "SELECT {COLS} FROM events
         WHERE deleted = 0 AND start_ms < ?2 AND (
             (end_ms > start_ms AND end_ms > ?1) OR ((end_ms IS NULL OR end_ms = start_ms) AND start_ms >= ?1))
         ORDER BY start_ms, id"
    ))?;
    let mut all: Vec<Event> = st.query_map([from_ms, to_ms], from_row)?.collect::<Result<_, _>>()?;
    let states = calendars::states(c)?;
    // Sans ligne `calendars` (avant la première relève de la liste) : affiché.
    let visible = |k: Option<i64>| k.is_none_or(|k| states.get(&k).is_none_or(|s| s.0));
    // Les externes ont toujours une fin ; un instant (fin = début) suit la même règle que ci-dessus.
    let mut st = c.prepare(
        "SELECT e.account_id, e.calendar_id, e.id, e.title, e.start_ms, e.end_ms, e.all_day, a.color, k.id, k.name, a.label, e.ical_uid
         FROM external_events e JOIN accounts a ON a.id = e.account_id
         LEFT JOIN calendars k ON k.kind = 'google' AND k.account_id = e.account_id AND k.remote_id = e.calendar_id
         WHERE e.start_ms < ?2 AND ((e.end_ms > e.start_ms AND e.end_ms > ?1) OR (e.end_ms = e.start_ms AND e.start_ms >= ?1))
         ORDER BY e.start_ms, e.account_id, e.calendar_id, e.id",
    )?;
    let ext = st.query_map([from_ms, to_ms], |r| {
        let (acct, cal, id): (i64, String, String) = (r.get(0)?, r.get(1)?, r.get(2)?);
        let color: Option<String> = r.get(7)?;
        let name: String = r.get::<_, Option<String>>(9)?.unwrap_or_else(|| cal.clone());
        let e = Event {
            id: 0, key: format!("g:{acct}:{cal}:{id}"), source: Source::Google, readonly: true,
            title: r.get(3)?, start_ms: r.get(4)?, end_ms: Some(r.get(5)?), all_day: r.get::<_, i64>(6)? != 0,
            color: color.clone(), notes: None, calendar: Some(name.clone()),
            sources: vec![EventSource { calendar: name, account: r.get(10)?, color }], uid: r.get(11)?,
        };
        Ok((r.get::<_, Option<i64>>(8)?, e))
    })?;
    for e in ext {
        let (k, e) = e?;
        if visible(k) {
            all.push(e);
        }
    }
    let mut st = c.prepare(
        "SELECT i.rowid, i.calendar_id, k.name, k.color, i.uid, i.title, i.start_ms, i.end_ms, i.all_day, i.notes
         FROM ics_events i JOIN calendars k ON k.id = i.calendar_id
         WHERE i.start_ms < ?2 AND ((i.end_ms > i.start_ms AND i.end_ms > ?1) OR (i.end_ms = i.start_ms AND i.start_ms >= ?1))
         ORDER BY i.start_ms, i.calendar_id, i.rowid",
    )?;
    let ics = st.query_map([from_ms, to_ms], |r| {
        let (row, cal, name, color): (i64, i64, String, Option<String>) = (r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?);
        Ok((cal, Event {
            id: 0, key: format!("c:{cal}:{row}"), source: Source::Ics, readonly: true, uid: r.get(4)?,
            title: r.get(5)?, start_ms: r.get(6)?, end_ms: Some(r.get(7)?), all_day: r.get::<_, i64>(8)? != 0,
            color: color.clone(), notes: r.get(9)?, calendar: Some(name.clone()), sources: vec![EventSource { calendar: name, account: None, color }],
        }))
    })?;
    for e in ics {
        let (k, e) = e?;
        if visible(Some(k)) {
            all.push(e);
        }
    }
    all.sort_by_key(|e| e.start_ms); // stable : locaux avant externes à heure égale
    Ok(merge_duplicates(all))
}

/// Titre comparé : casse et espaces ignorés.
fn norm(t: &str) -> String { t.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase() }

/// Même événement venu de plusieurs calendriers : même titre (normalisé), même début et même fin (journée entière :
/// mêmes jours, déjà ramenés à minuit local), et même iCalUID quand les deux en ont un. Deux événements locaux ne
/// sont jamais fusionnés. Le premier (local d'abord) est gardé ; il reçoit les sources des autres.
pub fn merge_duplicates(events: Vec<Event>) -> Vec<Event> {
    let mut out: Vec<(String, Event)> = Vec::with_capacity(events.len());
    for e in events {
        let t = norm(&e.title);
        let end = |x: &Event| x.end_ms.unwrap_or(x.start_ms);
        let same = out.iter_mut().find(|(ot, o)| {
            *ot == t && o.start_ms == e.start_ms && end(o) == end(&e) && o.all_day == e.all_day
                && !(o.source == Source::Local && e.source == Source::Local)
                && match (&o.uid, &e.uid) { (Some(a), Some(b)) => a == b, _ => true }
        });
        match same {
            Some((_, o)) => {
                for s in e.sources {
                    if !o.sources.contains(&s) {
                        o.sources.push(s);
                    }
                }
            }
            None => out.push((t, e)),
        }
    }
    out.into_iter().map(|(_, e)| e).collect()
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
    use crate::test_util::db;
    use crate::{migrate, CORE_MIGRATIONS};
    use chrono::NaiveDate;

    fn ms(y: i32, m: u32, d: u32, h: u32) -> i64 {
        let n = NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(h, 0, 0).unwrap();
        Local.from_local_datetime(&n).earliest().unwrap().timestamp_millis()
    }
    fn ev(title: &str, start_ms: i64, end_ms: Option<i64>) -> NewEvent {
        NewEvent { title: title.into(), start_ms, end_ms, ..Default::default() }
    }

    #[test]
    fn migration_3_cree_la_table_et_reste_idempotente() {
        let mut c = db();
        let v = migrate(&mut c, CORE_MIGRATIONS).unwrap();
        assert_eq!(v, CORE_MIGRATIONS.len() as u32);
        let n: i64 = c.query_row("SELECT count(*) FROM events", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
        assert!(c.execute("INSERT INTO events(title, start_ms, created_ms) VALUES ('a', 'x', 0)", []).is_err());
    }

    #[test]
    fn between_renvoie_l_evenement_qui_chevauche_le_debut() {
        let c = db();
        let id = insert(&c, &ev("nuit", ms(2026, 11, 11, 22), Some(ms(2026, 11, 12, 2)))).unwrap();
        insert(&c, &ev("avant", ms(2026, 11, 11, 8), Some(ms(2026, 11, 11, 9)))).unwrap();
        insert(&c, &ev("après", ms(2026, 11, 13, 8), None)).unwrap();
        let got = between(&c, ms(2026, 11, 12, 0), ms(2026, 11, 13, 0)).unwrap();
        assert_eq!(got.iter().map(|e| e.id).collect::<Vec<_>>(), vec![id]);
    }

    #[test]
    fn fin_exclusive_un_evenement_qui_finit_a_minuit_n_est_pas_le_jour_suivant() {
        let c = db();
        let soir = insert(&c, &ev("soir", ms(2026, 11, 11, 22), Some(ms(2026, 11, 12, 0)))).unwrap();
        let jour = insert(&c, &NewEvent { all_day: true, ..ev("jour", ms(2026, 11, 11, 0), Some(ms(2026, 11, 12, 0))) }).unwrap();
        let instant = insert(&c, &ev("instant", ms(2026, 11, 12, 0), None)).unwrap();
        let ids = |d: u32| between(&c, ms(2026, 11, d, 0), ms(2026, 11, d + 1, 0)).unwrap().iter().map(|e| e.id).collect::<Vec<_>>();
        assert_eq!(ids(11), vec![jour, soir]);
        assert_eq!(ids(12), vec![instant]);
    }

    #[test]
    fn supprime_exclu_et_restaurable() {
        let c = db();
        let id = insert(&c, &ev("x", ms(2026, 11, 12, 10), None)).unwrap();
        set_deleted(&c, id, true).unwrap();
        assert!(get(&c, id).unwrap().is_none());
        assert!(between(&c, 0, i64::MAX).unwrap().is_empty());
        set_deleted(&c, id, false).unwrap();
        assert_eq!(get(&c, id).unwrap().unwrap().title, "x");
    }

    #[test]
    fn jours_avec_evenements() {
        let c = db();
        insert(&c, &ev("a", ms(2026, 11, 12, 10), None)).unwrap();
        // Sur trois jours, finit à minuit pile : 20, 21, 22 seulement.
        insert(&c, &ev("b", ms(2026, 11, 20, 18), Some(ms(2026, 11, 23, 0)))).unwrap();
        // Chevauche la fin d'octobre : seul le 1er novembre compte.
        insert(&c, &ev("c", ms(2026, 10, 31, 22), Some(ms(2026, 11, 1, 3)))).unwrap();
        insert(&c, &ev("hors", ms(2026, 12, 2, 10), None)).unwrap();
        let days = days_with_events(&c, ms(2026, 11, 1, 0), ms(2026, 12, 1, 0)).unwrap();
        assert_eq!(days, vec![1, 12, 20, 21, 22]);
    }

    #[test]
    fn fusion_triee_avec_les_externes_en_lecture_seule() {
        let c = db();
        let acct = crate::accounts::add(&c, crate::oauth::Provider::Google, "moi@gmail.com", 1).unwrap();
        let color: String = c.query_row("SELECT color FROM accounts", [], |r| r.get(0)).unwrap();
        let loc = insert(&c, &ev("local", ms(2026, 11, 12, 10), Some(ms(2026, 11, 12, 11)))).unwrap();
        for (id, h) in [("b", 9), ("a", 14)] {
            c.execute("INSERT INTO external_events VALUES (?1, ?2, 'cal', 't', ?3, ?4, 0, NULL, NULL)", params![acct, id, ms(2026, 11, 12, h), ms(2026, 11, 12, h + 1)]).unwrap();
        }
        // Hors fenêtre, et un compte supprimé (pas de jointure) : ignorés.
        c.execute("INSERT INTO external_events VALUES (?1, 'z', 'cal', 't', ?2, ?3, 0, NULL, NULL)", params![acct, ms(2026, 11, 20, 9), ms(2026, 11, 20, 10)]).unwrap();
        c.execute("INSERT INTO external_events VALUES (99, 'o', 'cal', 't', ?1, ?2, 0, NULL, NULL)", params![ms(2026, 11, 12, 12), ms(2026, 11, 12, 13)]).unwrap();
        let got = between(&c, ms(2026, 11, 12, 0), ms(2026, 11, 13, 0)).unwrap();
        assert_eq!(got.iter().map(|e| e.key.clone()).collect::<Vec<_>>(), [format!("g:{acct}:cal:b"), format!("l:{loc}"), format!("g:{acct}:cal:a")]);
        assert_eq!(got.iter().map(|e| (e.source, e.readonly)).collect::<Vec<_>>(), [(Source::Google, true), (Source::Local, false), (Source::Google, true)]);
        assert_eq!(got[0].color.as_deref(), Some(color.as_str()), "couleur du compte");
        assert_eq!(days_with_events(&c, ms(2026, 11, 1, 0), ms(2026, 12, 1, 0)).unwrap(), vec![12, 20], "les points du calendrier incluent les externes");
    }

    #[test]
    fn armistice_de_cinq_comptes_affiche_une_fois_avec_ses_sources() {
        let c = db();
        let day = (ms(2026, 11, 11, 0), ms(2026, 11, 12, 0));
        let hol = "fr.french#holiday@group.v.calendar.google.com";
        let mut accts = vec![];
        for i in 1..=5 {
            let a = crate::accounts::add(&c, crate::oauth::Provider::Google, &format!("c{i}@gmail.com"), 1).unwrap();
            // Choix explicite pour tous : la fusion à l'affichage suffit même sans le masquage par défaut.
            let id = c.query_row("INSERT INTO calendars(kind, account_id, remote_id, name, hidden) VALUES ('google', ?1, ?2, 'Jours fériés', 0) RETURNING id", params![a, hol], |r| r.get::<_, i64>(0)).unwrap();
            let title = if i == 3 { "  armistice " } else { "Armistice" };
            c.execute("INSERT INTO external_events VALUES (?1, 'h', ?2, ?3, ?4, ?5, 1, NULL, 'uid-armistice')", params![a, hol, title, day.0, day.1]).unwrap();
            accts.push((a, id));
        }
        // Différents : autre iCalUID, autre heure, autre titre.
        let a0 = accts[0].0;
        c.execute("INSERT INTO external_events VALUES (?1, 'x', ?2, 'Armistice', ?3, ?4, 1, NULL, 'autre')", params![a0, hol, day.0, day.1]).unwrap();
        c.execute("INSERT INTO external_events VALUES (?1, 'y', 'perso', 'Armistice', ?2, ?3, 0, NULL, NULL)", params![a0, ms(2026, 11, 11, 10), ms(2026, 11, 11, 11)]).unwrap();
        c.execute("INSERT INTO external_events VALUES (?1, 'z', 'perso', 'Cérémonie', ?2, ?3, 1, NULL, NULL)", params![a0, day.0, day.1]).unwrap();
        let got = between(&c, day.0, day.1).unwrap();
        assert_eq!(got.len(), 4, "{:?}", got.iter().map(|e| &e.title).collect::<Vec<_>>());
        let a = got.iter().find(|e| e.uid.as_deref() == Some("uid-armistice")).unwrap();
        assert_eq!(a.sources.len(), 5);
        assert_eq!(a.sources.iter().map(|s| s.account.clone().unwrap()).collect::<Vec<_>>(), (1..=5).map(|i| format!("c{i}@gmail.com")).collect::<Vec<_>>());
        assert_eq!(a.calendar.as_deref(), Some("Jours fériés"));
        // Masqué : plus affiché du tout pour ce compte ; les autres restent.
        calendars::set_visible(&c, accts[0].1, false).unwrap();
        let got = between(&c, day.0, day.1).unwrap();
        assert_eq!(got.iter().find(|e| e.uid.as_deref() == Some("uid-armistice")).unwrap().sources.len(), 4);
        assert!(got.iter().all(|e| e.uid.as_deref() != Some("autre")));
    }

    #[test]
    fn fusion_local_et_ics_sans_fusionner_deux_locaux() {
        let c = db();
        let (s, e) = (ms(2026, 11, 12, 10), ms(2026, 11, 12, 11));
        let l1 = insert(&c, &ev("Dentiste", s, Some(e))).unwrap();
        insert(&c, &ev("Dentiste", s, Some(e))).unwrap();
        let ics = |t: &str, start: i64| ics::IcsEvent { uid: Some("u".into()), title: t.into(), start_ms: start, end_ms: e, all_day: false, location: None, notes: None };
        let k = calendars::add_local(&c, calendars::Kind::File, "Cabinet", None, &[ics("dentiste", s), ics("Dentiste", s + 60_000)], 0).unwrap();
        let got = between(&c, ms(2026, 11, 12, 0), ms(2026, 11, 13, 0)).unwrap();
        assert_eq!(got.iter().map(|e| (e.source, e.start_ms)).collect::<Vec<_>>(), [(Source::Local, s), (Source::Local, s), (Source::Ics, s + 60_000)]);
        assert_eq!(got[0].id, l1);
        assert_eq!(got[0].sources.iter().map(|x| x.calendar.as_str()).collect::<Vec<_>>(), ["Cabinet"], "le local garde sa suppression et montre la source");
        assert_eq!(got[2].key.split(':').nth(1), Some(k.to_string().as_str()));
        calendars::set_visible(&c, k, false).unwrap();
        assert_eq!(between(&c, ms(2026, 11, 12, 0), ms(2026, 11, 13, 0)).unwrap().len(), 2);
    }
}
