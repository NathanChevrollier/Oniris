//! Agendas Google en lecture seule : relève incrémentale (`syncToken` dans `kv`, clé `gcal_sync/<compte>/<agenda>`)
//! vers la table `external_events` (migration 15), et outil `calendar.list`. Journal : ids et nombres seulement.
pub mod google;

use crate::accounts::TokenCache;
use crate::agenda::{between, Event};
use crate::kv;
use crate::mail::sync::SyncError;
use crate::oauth::OAuthError;
use crate::tools::helpers::{failed, low, parse, spec};
use crate::tools::{ConfirmationText, Permission, Tool, ToolError, ToolResult, ToolSpec};
use crate::Db;
use async_trait::async_trait;
use google::{Client, EventsPage, GcalError, Query};
use rusqlite::{params, OptionalExtension};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

const DAY_MS: i64 = 86_400_000;
/// Fenêtre relevée : de aujourd'hui − 7 jours à + 60 jours.
const PAST_MS: i64 = 7 * DAY_MS;
const FUTURE_MS: i64 = 60 * DAY_MS;
/// Au-delà de cet âge depuis la dernière lecture COMPLÈTE (clé `gcal_full`), la fenêtre est relue pour qu'elle glisse
/// (nouveaux jours, récurrences). Le syncToken, lui, est réécrit à chaque relève : son âge ne dit rien.
const TOKEN_MAX_AGE_MS: i64 = DAY_MS;

/// Début de la fenêtre : minuit local du jour d'il y a 7 jours.
fn window_start(now: i64) -> i64 { crate::time_util::local_day_bounds(now - PAST_MS).0 }

fn map(e: GcalError) -> SyncError {
    match e {
        GcalError::Network => SyncError::Network,
        GcalError::Unauthorized => SyncError::Unauthorized,
        e => SyncError::Other(e.to_string()),
    }
}

fn sync_key(account_id: i64, cal: &str) -> String { format!("gcal_sync/{account_id}/{cal}") }
/// Date de la dernière lecture complète de la fenêtre.
fn full_key(account_id: i64, cal: &str) -> String { format!("gcal_full/{account_id}/{cal}") }

/// Relève les agendas cochés d'un compte ; `true` si des lignes ont changé. Le jeton refusé (401) est oublié.
pub async fn run(db: &Db, client: &Client, tokens: &TokenCache, account_id: i64, now: i64) -> Result<bool, SyncError> {
    let token = tokens.access_token(account_id).await.map_err(|e| match e {
        OAuthError::Revoked => SyncError::Revoked,
        OAuthError::Network => SyncError::Network,
        OAuthError::Other(m) => SyncError::Other(m),
    })?;
    let r = sync(db, client, &token, account_id, now).await;
    match &r {
        Ok(changed) => tracing::info!("agenda Google : compte {account_id} : {}", if *changed { "mis à jour" } else { "inchangé" }),
        Err(e) => tracing::warn!("agenda Google : compte {account_id} : {e}"),
    }
    if r == Err(SyncError::Unauthorized) {
        tokens.forget(account_id);
    }
    r
}

/// Page suivante depuis le syncToken, sinon (ou après un 410) la fenêtre complète. `None` : agenda disparu (404).
async fn fetch(client: &Client, token: &str, cal: &str, stored: Option<String>, now: i64) -> Result<Option<(EventsPage, bool)>, GcalError> {
    let missing = |e: GcalError| if e == GcalError::Status(404) { Ok(None) } else { Err(e) };
    if let Some(t) = &stored {
        match client.events(token, cal, Query::Sync(t)).await {
            Ok(p) if !p.gone => return Ok(Some((p, false))),
            Ok(_) => tracing::info!("agenda Google : syncToken périmé, resynchronisation complète"),
            Err(e) => return missing(e),
        }
    }
    match client.events(token, cal, Query::Window { time_min_ms: window_start(now), time_max_ms: now + FUTURE_MS }).await {
        // 410 sans syncToken : réponse anormale, on ne vide pas l'agenda.
        Ok(p) if p.gone => Err(GcalError::Status(410)),
        Ok(p) => Ok(Some((p, true))),
        Err(e) => missing(e),
    }
}

async fn sync(db: &Db, client: &Client, token: &str, account_id: i64, now: i64) -> Result<bool, SyncError> {
    let cals = client.list_calendars(token).await.map_err(map)?;
    let mut changed = false;
    for cal in &cals {
        let key = sync_key(account_id, cal);
        let fkey = full_key(account_id, cal);
        let (stored, full_at) = {
            let c = db.lock();
            let e = |e: crate::CoreError| SyncError::Other(e.to_string());
            (kv::get(&c, &key).map_err(e)?.map(|(v, _)| v), kv::get(&c, &fkey).map_err(e)?.map(|(_, at)| at))
        };
        // Dernière lecture complète absente ou vieille de plus de 24 h : on relit la fenêtre.
        let stored = stored.filter(|_| full_at.is_some_and(|at| now - at <= TOKEN_MAX_AGE_MS));
        let Some((page, full)) = fetch(client, token, cal, stored, now).await.map_err(map)? else {
            // Agenda disparu (404) : lignes et jeton retirés.
            let c = db.lock();
            let tx = c.unchecked_transaction()?;
            if !account_exists(&tx, account_id)? {
                return Ok(false);
            }
            changed |= tx.execute("DELETE FROM external_events WHERE account_id = ?1 AND calendar_id = ?2", params![account_id, cal])? > 0;
            for k in [&key, &fkey] {
                kv::delete(&tx, k).map_err(|e| SyncError::Other(e.to_string()))?;
            }
            tx.commit()?;
            continue;
        };
        let c = db.lock();
        let tx = c.unchecked_transaction()?;
        if !account_exists(&tx, account_id)? {
            return Ok(false); // compte supprimé pendant la relève : rien à réécrire
        }
        let mut n = 0;
        if full {
            n += tx.execute("DELETE FROM external_events WHERE account_id = ?1 AND calendar_id = ?2", params![account_id, cal])?;
        }
        for id in &page.cancelled {
            n += tx.execute("DELETE FROM external_events WHERE account_id = ?1 AND calendar_id = ?2 AND id = ?3", params![account_id, cal, id])?;
        }
        for e in &page.items {
            n += tx.execute(
                "INSERT INTO external_events(account_id, id, calendar_id, title, start_ms, end_ms, all_day, location) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(account_id, calendar_id, id) DO UPDATE SET title = excluded.title, start_ms = excluded.start_ms,
                 end_ms = excluded.end_ms, all_day = excluded.all_day, location = excluded.location",
                params![account_id, e.id, cal, e.title, e.start_ms, e.end_ms, e.all_day, e.location],
            )?;
        }
        match &page.next_sync_token {
            Some(t) => kv::set(&tx, &key, t, now),
            None => kv::delete(&tx, &key),
        }
        .and_then(|()| if full { kv::set(&tx, &fkey, "1", now) } else { Ok(()) })
        .map_err(|e| SyncError::Other(e.to_string()))?;
        tx.commit()?;
        changed |= n > 0;
    }
    Ok(forget_old(db, account_id, &cals, now)? | changed)
}

fn account_exists(c: &rusqlite::Connection, account_id: i64) -> rusqlite::Result<bool> {
    Ok(c.query_row("SELECT 1 FROM accounts WHERE id = ?1", [account_id], |_| Ok(())).optional()?.is_some())
}

/// Retire les agendas décochés (lignes et syncToken) et les événements finis depuis plus de 7 jours.
fn forget_old(db: &Db, account_id: i64, kept: &[String], now: i64) -> rusqlite::Result<bool> {
    let c = db.lock();
    let tx = c.unchecked_transaction()?;
    let known: Vec<String> = tx.prepare("SELECT DISTINCT calendar_id FROM external_events WHERE account_id = ?1")?
        .query_map([account_id], |r| r.get(0))?.collect::<Result<_, _>>()?;
    let mut n = 0;
    for cal in known.iter().filter(|k| !kept.contains(k)) {
        n += tx.execute("DELETE FROM external_events WHERE account_id = ?1 AND calendar_id = ?2", params![account_id, cal])?;
        tx.execute("DELETE FROM kv WHERE key IN (?1, ?2)", [sync_key(account_id, cal), full_key(account_id, cal)])?;
    }
    // Jetons orphelins : un agenda décoché alors qu'il était vide n'a pas de ligne.
    for (prefix, key_of) in [(format!("gcal_sync/{account_id}/"), sync_key as fn(i64, &str) -> String), (format!("gcal_full/{account_id}/"), full_key)] {
        let keys: Vec<String> = tx.prepare("SELECT key FROM kv WHERE substr(key, 1, ?2) = ?1")?
            .query_map(params![prefix, prefix.len() as i64], |r| r.get(0))?.collect::<Result<_, _>>()?;
        for k in keys.iter().filter(|k| !kept.iter().any(|c| key_of(account_id, c) == **k)) {
            tx.execute("DELETE FROM kv WHERE key = ?1", [k])?;
        }
    }
    n += tx.execute("DELETE FROM external_events WHERE account_id = ?1 AND end_ms < ?2", params![account_id, window_start(now)])?;
    tx.commit()?;
    Ok(n > 0)
}

/// `calendar.list { from_ms, to_ms }` : événements locaux et Google qui chevauchent [from_ms, to_ms[ (lecture seule, libre).
pub struct ListEvents { pub db: Db }

#[derive(Deserialize)]
struct ListArgs { from_ms: i64, to_ms: i64 }

#[async_trait]
impl Tool for ListEvents {
    fn spec(&self) -> ToolSpec {
        // Titres des agendas Google : le journal ne garde que le nombre d'éléments.
        ToolSpec { sensitive_output: true, ..spec("calendar.list", "Liste les événements locaux et des agendas Google qui chevauchent [from_ms, to_ms[ (lecture seule).",
            json!({"type":"object","required":["from_ms","to_ms"],"properties":{"from_ms":{"type":"integer"},"to_ms":{"type":"integer"}}}),
            Permission::Free) }
    }
    fn describe(&self, _: &Value) -> ConfirmationText { low("Lire les agendas") }
    async fn run(&self, args: &Value) -> Result<ToolResult, ToolError> {
        let a: ListArgs = parse(args)?;
        let list: Vec<Event> = between(&self.db.lock(), a.from_ms, a.to_ms).map_err(failed)?;
        Ok(ToolResult { output: json!(list), undo: None })
    }
}

pub fn all(db: &Db) -> Vec<Arc<dyn Tool>> { vec![Arc::new(ListEvents { db: db.clone() })] }

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::{self, Configs};
    use crate::events::NoEvents;
    use crate::oauth::{self, Provider, ProviderConfig, Tokens};
    use crate::secrets::{account_key, MemorySecrets, SecretStore};
    use crate::tools::{Origin, Provenance, ToolBroker, ToolCall, ToolOutcome};
    use chrono::{Local, NaiveDate, TimeZone};
    use wiremock::matchers::{header, method, path, query_param, query_param_is_missing};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    macro_rules! fixture { ($n:literal) => { include_str!(concat!("../../tests/fixtures/gcal/", $n)) } }
    const NOW: i64 = 1_794_000_000_000;
    const EVENTS: &str = "/calendar/v3/calendars/moi@gmail.com/events";
    const TEAM: &str = "/calendar/v3/calendars/team@group.calendar.google.com/events";

    fn body(s: &str) -> ResponseTemplate { ResponseTemplate::new(200).set_body_raw(s.to_string(), "application/json") }
    fn local_midnight(y: i32, m: u32, d: u32) -> i64 {
        Local.from_local_datetime(&NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(0, 0, 0).unwrap()).earliest().unwrap().timestamp_millis()
    }

    struct Env { db: Db, client: Client, tokens: TokenCache, id: i64 }

    fn env(server: &MockServer, with_token: bool) -> Env {
        let db: Db = Arc::new(parking_lot::Mutex::new(crate::test_util::db()));
        let id = accounts::add(&db.lock(), Provider::Google, "moi@gmail.com", 1).unwrap();
        let secrets = Arc::new(MemorySecrets::default());
        secrets.set(&account_key(id), "r").unwrap();
        let url = format!("{}/token", server.uri());
        let configs: Configs = Arc::new(move |_| Some(ProviderConfig { token_url: url.clone(), ..oauth::providers::google("g", Some("s".into())) }));
        let tokens = TokenCache::new(db.clone(), secrets, Arc::new(NoEvents), reqwest::Client::new(), configs);
        if with_token {
            tokens.insert(id, Tokens { access: "tok".into(), refresh: None, expires_at_ms: crate::time_util::now_ms() + 3_600_000 }).unwrap();
        }
        Env { db, client: Client { http: reqwest::Client::new(), base: server.uri() }, tokens, id }
    }

    impl Env {
        async fn run(&self) -> Result<bool, SyncError> { self.run_at(NOW).await }
        async fn run_at(&self, now: i64) -> Result<bool, SyncError> { run(&self.db, &self.client, &self.tokens, self.id, now).await }
        /// (calendar, id, start, end, all_day) triés.
        fn rows(&self) -> Vec<(String, String, i64, i64, bool)> {
            self.db.lock().prepare("SELECT calendar_id, id, start_ms, end_ms, all_day FROM external_events ORDER BY calendar_id, id").unwrap()
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get::<_, i64>(4)? != 0))).unwrap().map(Result::unwrap).collect()
        }
        fn token(&self, cal: &str) -> Option<String> { kv::get(&self.db.lock(), &sync_key(self.id, cal)).unwrap().map(|v| v.0) }
    }

    async fn calendars(server: &MockServer) {
        Mock::given(method("GET")).and(path("/calendar/v3/users/me/calendarList")).and(header("authorization", "Bearer tok"))
            .respond_with(body(fixture!("calendar_list.json"))).mount(server).await;
    }

    #[tokio::test]
    async fn complete_puis_incrementale_puis_410_resynchronise() {
        let server = MockServer::start().await;
        calendars(&server).await;
        // Complète : deux pages, un annulé retiré, jour entier, fuseau sans décalage.
        Mock::given(method("GET")).and(path(EVENTS)).and(query_param("singleEvents", "true")).and(query_param_is_missing("syncToken")).and(query_param_is_missing("pageToken"))
            .respond_with(body(fixture!("events_primary_p1.json"))).up_to_n_times(1).mount(&server).await;
        Mock::given(method("GET")).and(path(EVENTS)).and(query_param("pageToken", "p2")).and(query_param("timeMin", google::rfc3339(window_start(NOW))))
            .respond_with(body(fixture!("events_primary_p2.json"))).up_to_n_times(1).mount(&server).await;
        Mock::given(method("GET")).and(path(TEAM)).and(query_param_is_missing("syncToken")).respond_with(body(fixture!("events_team.json"))).up_to_n_times(1).mount(&server).await;
        let e = env(&server, true);
        assert!(e.run().await.unwrap());
        let d = |y, m, dd| local_midnight(y, m, dd);
        let ny = chrono_tz::America::New_York.with_ymd_and_hms(2026, 11, 13, 9, 0, 0).unwrap().timestamp_millis();
        let paris = |h| chrono_tz::Europe::Paris.with_ymd_and_hms(2026, 11, 12, h, 0, 0).unwrap().timestamp_millis();
        let rows = e.rows();
        assert_eq!(rows.len(), 4, "e4 annulé et agenda décoché absents : {rows:?}");
        assert_eq!(rows[0], ("moi@gmail.com".into(), "e1".into(), paris(10), paris(10) + 90 * 60_000, false), "décalage +01:00 converti tel quel");
        assert_eq!(rows[1], ("moi@gmail.com".into(), "e2".into(), d(2026, 11, 14), d(2026, 11, 16), true), "journée entière : minuit local, fin exclue");
        assert_eq!(rows[2], ("moi@gmail.com".into(), "e3".into(), ny, ny + 3_600_000, false), "fuseau de l'événement");
        assert_eq!(rows[3].1, "t1");
        assert_eq!((e.token("moi@gmail.com").as_deref(), e.token("team@group.calendar.google.com").as_deref()), (Some("S1"), Some("T1")));

        // Incrémentale : e1 déplacé, e3 annulé, rien pour l'équipe.
        Mock::given(method("GET")).and(path(EVENTS)).and(query_param("syncToken", "S1")).respond_with(body(fixture!("events_primary_sync.json"))).up_to_n_times(1).expect(1).mount(&server).await;
        Mock::given(method("GET")).and(path(TEAM)).and(query_param("syncToken", "T1")).respond_with(body(fixture!("events_empty_sync.json"))).up_to_n_times(1).expect(1).mount(&server).await;
        assert!(e.run().await.unwrap());
        let rows = e.rows();
        assert_eq!(rows.iter().map(|r| r.1.as_str()).collect::<Vec<_>>(), ["e1", "e2", "t1"]);
        assert_eq!(rows[0].2, paris(15));
        assert_eq!(e.token("moi@gmail.com").as_deref(), Some("S2"));
        assert_eq!(e.token("team@group.calendar.google.com").as_deref(), Some("T2"));

        // 410 : resynchronisation complète de cet agenda, sans doublon ; l'ancien contenu disparaît.
        Mock::given(method("GET")).and(path(EVENTS)).and(query_param("syncToken", "S2")).respond_with(ResponseTemplate::new(410)).up_to_n_times(1).expect(1).mount(&server).await;
        Mock::given(method("GET")).and(path(EVENTS)).and(query_param_is_missing("syncToken")).respond_with(body(fixture!("events_primary_resync.json"))).up_to_n_times(1).expect(1).mount(&server).await;
        Mock::given(method("GET")).and(path(TEAM)).and(query_param("syncToken", "T2")).respond_with(body(fixture!("events_empty_sync.json"))).mount(&server).await;
        assert!(e.run().await.unwrap());
        assert_eq!(e.rows().iter().map(|r| r.1.as_str()).collect::<Vec<_>>(), ["n1", "t1"]);
        assert_eq!(e.token("moi@gmail.com").as_deref(), Some("S3"));
    }

    #[tokio::test]
    async fn agenda_decoche_et_vieux_evenements_retires() {
        let server = MockServer::start().await;
        calendars(&server).await;
        Mock::given(method("GET")).and(path(EVENTS)).respond_with(body(fixture!("events_empty_sync.json"))).mount(&server).await;
        Mock::given(method("GET")).and(path(TEAM)).respond_with(body(fixture!("events_empty_sync.json"))).mount(&server).await;
        let e = env(&server, true);
        let now = NOW;
        {
            let c = e.db.lock();
            let ins = |cal: &str, id: &str, end: i64| c.execute("INSERT INTO external_events VALUES (?1, ?2, ?3, 't', ?4, ?4, 0, NULL)", params![e.id, id, cal, end]).unwrap();
            ins("ancien@x", "a", now);
            ins("moi@gmail.com", "vieux", now - 8 * DAY_MS);
            ins("moi@gmail.com", "recent", now - DAY_MS);
            kv::set(&c, &sync_key(e.id, "ancien@x"), "z", 1).unwrap();
            kv::set(&c, &sync_key(e.id, "moi@gmail.com"), "x", now).unwrap(); // incrémentale : rien n'est remplacé
            kv::set(&c, &full_key(e.id, "moi@gmail.com"), "1", now).unwrap();
            kv::set(&c, &sync_key(e.id, "vide@x"), "z", now).unwrap(); // décoché alors qu'il était vide : aucune ligne
            kv::set(&c, &full_key(e.id, "vide@x"), "1", now).unwrap();
        }
        assert!(e.run().await.unwrap());
        assert_eq!(e.rows().iter().map(|r| r.1.as_str()).collect::<Vec<_>>(), ["recent"]);
        assert_eq!(e.token("ancien@x"), None);
        assert_eq!(e.token("vide@x"), None, "jeton orphelin retiré");
        assert_eq!(kv::get(&e.db.lock(), &full_key(e.id, "vide@x")).unwrap(), None);
        assert_eq!(e.token("moi@gmail.com").as_deref(), Some("T2"), "jeton d'un agenda coché gardé");
    }

    #[tokio::test]
    async fn jeton_vieux_de_plus_de_24_h_relit_la_fenetre() {
        let server = MockServer::start().await;
        calendars(&server).await;
        // Pas de syncToken attendu : relecture complète, l'ancien contenu est remplacé.
        Mock::given(method("GET")).and(path(EVENTS)).and(query_param_is_missing("syncToken")).and(query_param("timeMax", google::rfc3339(NOW + FUTURE_MS)))
            .respond_with(body(fixture!("events_primary_resync.json"))).up_to_n_times(1).expect(1).mount(&server).await;
        Mock::given(method("GET")).and(path(TEAM)).and(query_param("syncToken", "T0")).respond_with(body(fixture!("events_empty_sync.json"))).expect(1).mount(&server).await;
        let e = env(&server, true);
        {
            let c = e.db.lock();
            c.execute("INSERT INTO external_events VALUES (?1, 'vieux', 'moi@gmail.com', 't', ?2, ?2, 0, NULL)", params![e.id, NOW]).unwrap();
            kv::set(&c, &sync_key(e.id, "moi@gmail.com"), "S0", NOW).unwrap();
            kv::set(&c, &full_key(e.id, "moi@gmail.com"), "1", NOW - DAY_MS - 1).unwrap();
            kv::set(&c, &sync_key(e.id, "team@group.calendar.google.com"), "T0", NOW).unwrap();
            kv::set(&c, &full_key(e.id, "team@group.calendar.google.com"), "1", NOW - DAY_MS + 1).unwrap(); // encore frais
        }
        assert!(e.run().await.unwrap());
        assert_eq!(e.rows().iter().map(|r| r.1.as_str()).collect::<Vec<_>>(), ["n1"]);
        assert_eq!(e.token("moi@gmail.com").as_deref(), Some("S3"));
    }

    #[tokio::test]
    async fn la_fenetre_est_relue_apres_24_h_malgre_les_releves_incrementales() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/calendar/v3/users/me/calendarList")).respond_with(body(r#"{"items":[{"id":"moi@gmail.com","selected":true}]}"#)).mount(&server).await;
        // Pas de syncToken : lecture complète ; avec S1 : incrémentale. Les deux renvoient le même jeton S1.
        Mock::given(method("GET")).and(path(EVENTS)).and(query_param_is_missing("syncToken")).respond_with(body(fixture!("events_primary_p2.json"))).mount(&server).await;
        Mock::given(method("GET")).and(path(EVENTS)).and(query_param("syncToken", "S1")).respond_with(body(fixture!("events_primary_p2.json"))).mount(&server).await;
        let e = env(&server, true);
        let h = 9 * 3_600_000;
        let mut seen = vec![];
        for i in 0..4 {
            e.run_at(NOW + i * h).await.unwrap(); // 0, 9, 18, 27 h : la fenêtre est relue à 27 h seulement
            let reqs = server.received_requests().await.unwrap();
            seen.push(reqs.iter().filter(|r| r.url.path() == EVENTS && r.url.query().unwrap_or("").contains("timeMin")).count());
        }
        assert_eq!(seen, [1, 1, 1, 2], "une lecture complète au départ, puis une seule quand 24 h sont dépassées");
    }

    #[tokio::test]
    async fn gone_sur_la_fenetre_est_une_erreur_et_garde_les_lignes() {
        let server = MockServer::start().await;
        calendars(&server).await;
        Mock::given(method("GET")).and(path(EVENTS)).respond_with(ResponseTemplate::new(410)).mount(&server).await;
        let e = env(&server, true);
        e.db.lock().execute("INSERT INTO external_events VALUES (?1, 'x', 'moi@gmail.com', 't', ?2, ?2, 0, NULL)", params![e.id, NOW]).unwrap();
        assert!(matches!(e.run().await, Err(SyncError::Other(m)) if m.contains("410")));
        assert_eq!(e.rows().len(), 1);
    }

    #[tokio::test]
    async fn agenda_disparu_404_est_nettoye_et_evenement_illisible_retire() {
        let server = MockServer::start().await;
        calendars(&server).await;
        // L'agenda de l'équipe a disparu ; sur le principal, e1 n'a plus d'heure lisible, e2 ne bouge pas.
        Mock::given(method("GET")).and(path(TEAM)).respond_with(ResponseTemplate::new(404)).expect(1).mount(&server).await;
        Mock::given(method("GET")).and(path(EVENTS)).and(query_param("syncToken", "S1")).respond_with(body(fixture!("events_unreadable_sync.json"))).expect(1).mount(&server).await;
        let e = env(&server, true);
        {
            let c = e.db.lock();
            for (cal, id) in [("moi@gmail.com", "e1"), ("moi@gmail.com", "e2"), ("team@group.calendar.google.com", "t1")] {
                c.execute("INSERT INTO external_events VALUES (?1, ?2, ?3, 't', ?4, ?4, 0, NULL)", params![e.id, id, cal, NOW]).unwrap();
            }
            for (cal, t) in [("moi@gmail.com", "S1"), ("team@group.calendar.google.com", "T1")] {
                kv::set(&c, &sync_key(e.id, cal), t, NOW).unwrap();
                kv::set(&c, &full_key(e.id, cal), "1", NOW).unwrap();
            }
        }
        assert!(e.run().await.unwrap());
        assert_eq!(e.rows().iter().map(|r| r.1.as_str()).collect::<Vec<_>>(), ["e2"]);
        assert_eq!(e.token("team@group.calendar.google.com"), None);
        assert_eq!(kv::get(&e.db.lock(), &full_key(e.id, "team@group.calendar.google.com")).unwrap(), None, "date de lecture complète retirée aussi");
        assert_eq!(e.token("moi@gmail.com").as_deref(), Some("S2"));
    }

    #[tokio::test]
    async fn erreurs_et_revocation_sans_ecriture() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/token")).respond_with(ResponseTemplate::new(400).set_body_raw(r#"{"error":"invalid_grant"}"#, "application/json")).mount(&server).await;
        let e = env(&server, false);
        assert_eq!(e.run().await, Err(SyncError::Revoked));
        assert_eq!(accounts::get(&e.db.lock(), e.id).unwrap().unwrap().status, accounts::Status::Reauth);
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/calendar/v3/users/me/calendarList")).respond_with(ResponseTemplate::new(503)).mount(&server).await;
        let e = env(&server, true);
        assert_eq!(e.run().await, Err(SyncError::Network));
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/calendar/v3/users/me/calendarList")).respond_with(ResponseTemplate::new(403)).mount(&server).await;
        let e = env(&server, true);
        assert!(matches!(e.run().await, Err(SyncError::Other(m)) if m.contains("403")));
        assert!(e.rows().is_empty());
    }

    #[tokio::test]
    async fn outil_list_fusionne_et_refuse_de_deplacer_ou_supprimer_un_externe() {
        let db: Db = Arc::new(parking_lot::Mutex::new(crate::test_util::db()));
        let acct = accounts::add(&db.lock(), Provider::Google, "moi@gmail.com", 1).unwrap();
        crate::agenda::insert(&db.lock(), &crate::agenda::NewEvent { title: "local".into(), start_ms: 2000, ..Default::default() }).unwrap();
        db.lock().execute("INSERT INTO external_events VALUES (?1, 'g', 'c', 'ext', 1000, 3000, 0, NULL)", [acct]).unwrap();
        let ev: Arc<dyn crate::events::Events> = Arc::new(NoEvents);
        let mut tools = all(&db);
        tools.extend(crate::agenda::tools::all(&db, &ev));
        let broker = ToolBroker::new(db.clone(), Arc::new(|| 0), tools).unwrap();
        let call = |tool: &str, args: Value| ToolCall { tool: tool.into(), args, provenance: Provenance::User, origin: Origin::User };
        let ToolOutcome::Done { output, .. } = broker.call(call("calendar.list", json!({"from_ms": 0, "to_ms": 10_000}))).await else { panic!() };
        let l = output.as_array().unwrap();
        assert_eq!(l.iter().map(|e| (e["source"].as_str().unwrap(), e["readonly"].as_bool().unwrap())).collect::<Vec<_>>(), [("google", true), ("local", false)], "tri par heure");
        let key = l[0]["key"].clone();
        for (tool, args) in [
            ("agenda.move_event", json!({"id": 0, "start_ms": 5})), ("agenda.move_event", json!({"id": key, "start_ms": 5})),
            ("agenda.delete_event", json!({"id": 0})), ("agenda.delete_event", json!({"id": key})),
        ] {
            let mut out = broker.call(call(tool, args.clone())).await;
            if let ToolOutcome::NeedsConfirmation(p) = out {
                out = broker.resolve(p.action_id, true).await; // la suppression passe par l'accord, puis échoue
            }
            assert!(matches!(out, ToolOutcome::Failed { .. }), "{tool} {args}");
        }
        let n: i64 = db.lock().query_row("SELECT start_ms FROM external_events", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1000);
        // Journal : aucun titre, seulement le nombre d'éléments (calendar.list et agenda.list).
        for tool in ["calendar.list", "agenda.list"] {
            assert!(matches!(broker.call(call(tool, json!({"from_ms": 0, "to_ms": 10_000}))).await, ToolOutcome::Done { .. }));
            let logged: String = db.lock().query_row("SELECT result FROM actions WHERE tool = ?1 ORDER BY id DESC LIMIT 1", [tool], |r| r.get(0)).unwrap();
            assert!(!logged.contains("ext") && !logged.contains("local"), "{tool} : {logged}");
            assert!(logged.contains("redacted"), "{tool} : {logged}");
        }
    }
}
