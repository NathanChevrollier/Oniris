//! Calendriers (migration 20) : agendas Google (un par compte), fichiers .ics importés et abonnements en ligne.
//! Visibilité : le choix de l'utilisateur (`hidden`) sinon le défaut — coché chez Google, et un agenda partagé par
//! plusieurs comptes (jours fériés…) affiché une seule fois. Les calendriers masqués ne sont plus relevés.
use super::ics::IcsEvent;
use crate::CoreError;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind { Google, File, Url }

impl Kind {
    fn of(s: &str) -> Kind { match s { "google" => Kind::Google, "url" => Kind::Url, _ => Kind::File } }
    fn as_str(self) -> &'static str { match self { Kind::Google => "google", Kind::File => "file", Kind::Url => "url" } }
}

/// Agenda Google tel que listé par `calendarList`.
#[derive(Debug, Clone, PartialEq)]
pub struct GoogleCal { pub id: String, pub name: String, pub color: Option<String>, pub primary: bool, pub selected: bool }

/// Ce que voit l'interface.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Calendar {
    pub id: i64,
    pub kind: Kind,
    pub account_id: Option<i64>,
    /// Adresse du compte Google.
    pub account: Option<String>,
    pub name: String,
    pub color: Option<String>,
    pub primary: bool,
    pub visible: bool,
    /// Masqué par défaut : le même agenda est déjà affiché depuis un autre compte.
    pub duplicate: bool,
    pub url: Option<String>,
    pub fetched_ms: Option<i64>,
    pub error: Option<String>,
    pub events: i64,
}

/// Couleurs des calendriers importés, attribuées à tour de rôle.
const PALETTE: [&str; 6] = ["#30d158", "#bf5af2", "#ff9f0a", "#64d2ff", "#ff375f", "#ffd60a"];

/// id, type, id Google, coché chez Google, choix explicite.
type StateRow = (i64, String, Option<String>, bool, Option<bool>);

/// (visible, en double) de chaque calendrier.
pub fn states(c: &Connection) -> Result<HashMap<i64, (bool, bool)>, CoreError> {
    let mut st = c.prepare("SELECT id, kind, remote_id, selected, hidden FROM calendars ORDER BY id")?;
    let rows: Vec<StateRow> =
        st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get::<_, i64>(3)? != 0, r.get::<_, Option<i64>>(4)?.map(|h| h != 0))))?.collect::<Result<_, _>>()?;
    // Un agenda Google affiché explicitement compte avant les choix par défaut, quel que soit le compte.
    let mut shown: HashSet<&str> = rows.iter().filter(|r| r.1 == "google" && r.4 == Some(false)).filter_map(|r| r.2.as_deref()).collect();
    let mut out = HashMap::new();
    for (id, kind, remote, selected, hidden) in &rows {
        let state = match (hidden, kind.as_str()) {
            (Some(h), _) => (!h, false),
            (None, "google") => {
                let dup = remote.as_deref().is_some_and(|r| shown.contains(r));
                let visible = *selected && !dup;
                if visible {
                    shown.extend(remote.as_deref());
                }
                (visible, *selected && dup)
            }
            (None, _) => (true, false),
        };
        out.insert(*id, state);
    }
    Ok(out)
}

/// Enregistre la liste des agendas d'un compte (nom, couleur, principal, coché) et retire ceux qui ont disparu.
/// Renvoie les ids d'agenda à relever : les visibles. Compte supprimé entre-temps : rien.
pub fn store_google(c: &Connection, account_id: i64, cals: &[GoogleCal]) -> Result<Vec<String>, CoreError> {
    let tx = c.unchecked_transaction()?;
    if tx.query_row("SELECT 1 FROM accounts WHERE id = ?1", [account_id], |_| Ok(())).optional()?.is_none() {
        return Ok(vec![]);
    }
    for g in cals {
        tx.execute(
            "INSERT INTO calendars(kind, account_id, remote_id, name, color, is_primary, selected) VALUES ('google', ?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(account_id, remote_id) WHERE kind = 'google' DO UPDATE SET name = excluded.name, color = excluded.color,
             is_primary = excluded.is_primary, selected = excluded.selected",
            params![account_id, g.id, g.name, g.color, g.primary, g.selected],
        )?;
    }
    let known: Vec<(i64, String)> = tx.prepare("SELECT id, remote_id FROM calendars WHERE kind = 'google' AND account_id = ?1 ORDER BY id")?
        .query_map([account_id], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
    for (id, remote) in &known {
        if !cals.iter().any(|g| &g.id == remote) {
            tx.execute("DELETE FROM calendars WHERE id = ?1", [id])?;
        }
    }
    let states = states(&tx)?;
    tx.commit()?;
    Ok(known.into_iter().filter(|(id, r)| cals.iter().any(|g| &g.id == r) && states.get(id).is_some_and(|s| s.0)).map(|(_, r)| r).collect())
}

pub fn list(c: &Connection) -> Result<Vec<Calendar>, CoreError> {
    let states = states(c)?;
    let mut st = c.prepare(
        "SELECT k.id, k.kind, k.account_id, a.label, k.name, k.color, k.is_primary, k.url, k.fetched_ms, k.fetch_error,
            CASE WHEN k.kind = 'google' THEN (SELECT count(*) FROM external_events e WHERE e.account_id = k.account_id AND e.calendar_id = k.remote_id)
                 ELSE (SELECT count(*) FROM ics_events i WHERE i.calendar_id = k.id) END
         FROM calendars k LEFT JOIN accounts a ON a.id = k.account_id
         WHERE k.kind != 'google' OR a.id IS NOT NULL
         ORDER BY k.kind != 'google', k.account_id, k.is_primary DESC, k.name COLLATE NOCASE, k.id",
    )?;
    let rows = st.query_map([], |r| {
        let id: i64 = r.get(0)?;
        let (visible, duplicate) = states.get(&id).copied().unwrap_or((true, false));
        Ok(Calendar {
            id, kind: Kind::of(&r.get::<_, String>(1)?), account_id: r.get(2)?, account: r.get(3)?, name: r.get(4)?, color: r.get(5)?,
            primary: r.get::<_, i64>(6)? != 0, visible, duplicate, url: r.get(7)?, fetched_ms: r.get(8)?, error: r.get(9)?, events: r.get(10)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Choix explicite ; renvoie le type du calendrier (un agenda Google réaffiché doit être relevé).
pub fn set_visible(c: &Connection, id: i64, visible: bool) -> Result<Kind, CoreError> {
    let kind: Option<String> = c.query_row("SELECT kind FROM calendars WHERE id = ?1", [id], |r| r.get(0)).optional()?;
    let kind = kind.ok_or_else(|| CoreError::NotFound("calendrier introuvable".into()))?;
    c.execute("UPDATE calendars SET hidden = ?2 WHERE id = ?1", params![id, !visible])?;
    Ok(Kind::of(&kind))
}

fn insert_events(c: &Connection, id: i64, events: &[IcsEvent]) -> Result<(), CoreError> {
    let mut st = c.prepare("INSERT INTO ics_events(calendar_id, uid, title, start_ms, end_ms, all_day, location, notes) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)")?;
    for e in events {
        st.execute(params![id, e.uid, e.title, e.start_ms, e.end_ms, e.all_day, e.location, e.notes])?;
    }
    Ok(())
}

/// Nouveau calendrier local (fichier importé ou abonnement) avec ses événements.
pub fn add_local(c: &Connection, kind: Kind, name: &str, url: Option<&str>, events: &[IcsEvent], now: i64) -> Result<i64, CoreError> {
    if kind == Kind::Google {
        return Err(CoreError::Invalid("calendrier local attendu".into()));
    }
    if let Some(u) = url {
        if c.query_row("SELECT 1 FROM calendars WHERE kind = 'url' AND url = ?1", [u], |_| Ok(())).optional()?.is_some() {
            return Err(CoreError::Invalid("tu es déjà abonné à ce calendrier".into()));
        }
    }
    let tx = c.unchecked_transaction()?;
    let n: i64 = tx.query_row("SELECT count(*) FROM calendars WHERE kind != 'google'", [], |r| r.get(0))?;
    tx.execute(
        "INSERT INTO calendars(kind, url, name, color, fetched_ms) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![kind.as_str(), url, name, PALETTE[n as usize % PALETTE.len()], now],
    )?;
    let id = tx.last_insert_rowid();
    insert_events(&tx, id, events)?;
    tx.commit()?;
    Ok(id)
}

/// Résultat d'une relève d'abonnement : événements remplacés, ou erreur notée (les anciens restent).
pub fn store_fetch(c: &Connection, id: i64, result: Result<&[IcsEvent], &str>, now: i64) -> Result<(), CoreError> {
    let tx = c.unchecked_transaction()?;
    match result {
        Ok(events) => {
            tx.execute("DELETE FROM ics_events WHERE calendar_id = ?1", [id])?;
            insert_events(&tx, id, events)?;
            tx.execute("UPDATE calendars SET fetched_ms = ?2, fetch_error = NULL WHERE id = ?1", params![id, now])?;
        }
        Err(msg) => {
            tx.execute("UPDATE calendars SET fetched_ms = ?2, fetch_error = ?3 WHERE id = ?1", params![id, now, msg])?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// Supprime un calendrier importé ou un abonnement (jamais un agenda Google : on le masque).
pub fn delete(c: &Connection, id: i64) -> Result<(), CoreError> {
    let tx = c.unchecked_transaction()?;
    tx.execute("DELETE FROM ics_events WHERE calendar_id = ?1 AND calendar_id IN (SELECT id FROM calendars WHERE kind != 'google')", [id])?;
    let n = tx.execute("DELETE FROM calendars WHERE id = ?1 AND kind != 'google'", [id])?;
    tx.commit()?;
    if n == 0 {
        return Err(CoreError::NotFound("calendrier introuvable".into()));
    }
    Ok(())
}

/// Abonnements visibles relevés il y a plus de `max_age_ms` : (id, url).
pub fn stale_subscriptions(c: &Connection, now: i64, max_age_ms: i64) -> Result<Vec<(i64, String)>, CoreError> {
    let states = states(c)?;
    let rows: Vec<(i64, String)> = c.prepare("SELECT id, url FROM calendars WHERE kind = 'url' AND url IS NOT NULL AND (fetched_ms IS NULL OR fetched_ms <= ?1)")?
        .query_map([now - max_age_ms], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
    Ok(rows.into_iter().filter(|(id, _)| states.get(id).is_some_and(|s| s.0)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::db;

    fn g(id: &str, selected: bool) -> GoogleCal { GoogleCal { id: id.into(), name: id.into(), color: Some("#123456".into()), primary: false, selected } }
    const HOL: &str = "fr.french#holiday@group.v.calendar.google.com";

    #[test]
    fn jours_feries_en_double_affiches_une_fois_et_choix_explicites() {
        let c = db();
        let ids: Vec<i64> = (1..=3).map(|i| crate::accounts::add(&c, crate::oauth::Provider::Google, &format!("c{i}@gmail.com"), 1).unwrap()).collect();
        assert_eq!(store_google(&c, ids[0], &[g("a@gmail.com", true), g(HOL, true)]).unwrap(), ["a@gmail.com", HOL]);
        assert_eq!(store_google(&c, ids[1], &[g("b@gmail.com", true), g(HOL, true), g("off", false)]).unwrap(), ["b@gmail.com"], "doublon et décoché non relevés");
        assert_eq!(store_google(&c, ids[2], &[g(HOL, true)]).unwrap(), Vec::<String>::new());
        let l = list(&c).unwrap();
        let hol: Vec<&Calendar> = l.iter().filter(|k| k.name == HOL).collect();
        assert_eq!(hol.iter().map(|k| (k.visible, k.duplicate)).collect::<Vec<_>>(), [(true, false), (false, true), (false, true)]);
        assert_eq!(hol[0].account.as_deref(), Some("c1@gmail.com"));
        // Masqué dans le premier compte : le suivant prend le relais ; affiché explicitement ailleurs : celui-là seul.
        set_visible(&c, hol[0].id, false).unwrap();
        assert_eq!(list(&c).unwrap().iter().filter(|k| k.name == HOL).map(|k| k.visible).collect::<Vec<_>>(), [false, true, false]);
        set_visible(&c, hol[2].id, true).unwrap();
        assert_eq!(list(&c).unwrap().iter().filter(|k| k.name == HOL).map(|k| k.visible).collect::<Vec<_>>(), [false, false, true]);
        // Décoché chez Google mais coché ici : relevé. Disparu de la liste Google : retiré.
        let off = list(&c).unwrap().into_iter().find(|k| k.name == "off").unwrap();
        set_visible(&c, off.id, true).unwrap();
        assert_eq!(store_google(&c, ids[1], &[g("b@gmail.com", true), g("off", false)]).unwrap(), ["b@gmail.com", "off"]);
        assert_eq!(list(&c).unwrap().iter().filter(|k| k.account_id == Some(ids[1])).count(), 2);
        assert!(matches!(set_visible(&c, 999, true), Err(CoreError::NotFound(_))));
    }

    #[test]
    fn calendrier_local_relève_et_suppression() {
        let c = db();
        let ev = |t: &str| IcsEvent { uid: None, title: t.into(), start_ms: 1, end_ms: 2, all_day: false, location: None, notes: None };
        let f = add_local(&c, Kind::File, "Club", None, &[ev("a"), ev("b")], 10).unwrap();
        let u = add_local(&c, Kind::Url, "École", Some("https://x.test/a.ics"), &[], 10).unwrap();
        assert!(add_local(&c, Kind::Url, "École", Some("https://x.test/a.ics"), &[], 10).is_err(), "même abonnement deux fois");
        let l = list(&c).unwrap();
        assert_eq!(l.iter().map(|k| (k.id, k.events, k.visible)).collect::<Vec<_>>(), [(f, 2, true), (u, 0, true)]);
        assert_ne!(l[0].color, l[1].color);
        assert_eq!(stale_subscriptions(&c, 10 + 5, 6).unwrap(), Vec::<(i64, String)>::new());
        assert_eq!(stale_subscriptions(&c, 10 + 6, 6).unwrap(), [(u, "https://x.test/a.ics".to_string())]);
        store_fetch(&c, u, Ok(&[ev("c")]), 20).unwrap();
        store_fetch(&c, u, Err("téléchargement impossible"), 30).unwrap();
        let k = list(&c).unwrap().into_iter().find(|k| k.id == u).unwrap();
        assert_eq!((k.events, k.fetched_ms, k.error.as_deref()), (1, Some(30), Some("téléchargement impossible")), "l'échec garde les événements");
        set_visible(&c, u, false).unwrap();
        assert!(stale_subscriptions(&c, 1_000, 6).unwrap().is_empty(), "masqué : plus relevé");
        delete(&c, f).unwrap();
        assert!(delete(&c, f).is_err());
        let n: i64 = c.query_row("SELECT count(*) FROM ics_events WHERE calendar_id = ?1", [f], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }
}
