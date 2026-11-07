//! Synchronisation d'un compte Gmail vers le cache `mails` : complète (100 derniers mails de la boîte de réception)
//! sans `history_id`, sinon par l'historique. Un 404 d'historique repart d'une synchronisation complète.
use super::gmail::{Client, GmailError, MetaMsg};
use super::heuristic_bucket;
use crate::accounts::TokenCache;
use crate::oauth::OAuthError;
use crate::Db;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashSet;

/// Mails lus par une synchronisation complète.
pub const FULL_MAX: u32 = 100;
/// Mails gardés en cache par compte, les plus récents.
pub const CACHE_MAX: i64 = 500;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct SyncReport { pub added: usize, pub removed: usize, pub changed: usize }

impl SyncReport {
    pub fn any(&self) -> bool { self.added + self.removed + self.changed > 0 }
}

#[derive(Debug, PartialEq, thiserror::Error)]
pub enum SyncError {
    /// Le compte est passé à `reauth` : plus de relève jusqu'à la reconnexion.
    #[error("accès révoqué")]
    Revoked,
    #[error("réseau indisponible")]
    Network,
    #[error("jeton refusé")]
    Unauthorized,
    #[error("{0}")]
    Other(String),
}

impl From<GmailError> for SyncError {
    fn from(e: GmailError) -> Self {
        match e {
            GmailError::Network => SyncError::Network,
            GmailError::Unauthorized => SyncError::Unauthorized,
            e => SyncError::Other(e.to_string()),
        }
    }
}

impl From<rusqlite::Error> for SyncError {
    fn from(e: rusqlite::Error) -> Self { SyncError::Other(format!("base : {e}")) }
}

fn in_inbox(labels: &[String]) -> bool {
    let has = |l: &str| labels.iter().any(|x| x == l);
    has("INBOX") && !has("TRASH") && !has("SPAM")
}

/// Relève un compte. Le jeton refusé (401) est oublié : le prochain essai l'actualise.
pub async fn run(db: &Db, client: &Client, tokens: &TokenCache, account_id: i64, now: i64) -> Result<SyncReport, SyncError> {
    let token = tokens.access_token(account_id).await.map_err(|e| match e {
        OAuthError::Revoked => SyncError::Revoked,
        OAuthError::Network => SyncError::Network,
        OAuthError::Other(m) => SyncError::Other(m),
    })?;
    let r = sync(db, client, &token, account_id, now).await;
    match &r {
        Ok(rep) => tracing::info!("mails : compte {account_id} : +{} -{} ~{}", rep.added, rep.removed, rep.changed),
        Err(e) => tracing::warn!("mails : compte {account_id} : {e}"),
    }
    if r == Err(SyncError::Unauthorized) {
        tokens.forget(account_id);
    }
    r
}

async fn sync(db: &Db, client: &Client, token: &str, account_id: i64, now: i64) -> Result<SyncReport, SyncError> {
    let history_id: Option<String> = {
        let c = db.lock();
        c.query_row("SELECT history_id FROM mail_sync WHERE account_id = ?1", [account_id], |r| r.get(0)).optional()?.flatten()
    };
    let Some(start) = history_id else { return full(db, client, token, account_id, now).await };
    let delta = match client.history(token, &start).await {
        Ok(d) => d,
        Err(GmailError::HistoryGone) => {
            tracing::info!("mails : compte {account_id} : historique expiré, synchronisation complète");
            return full(db, client, token, account_id, now).await;
        }
        Err(e) => return Err(e.into()),
    };
    let (metas, gone) = fetch(client, token, &delta.added).await?;
    let c = db.lock();
    let tx = c.unchecked_transaction()?;
    let mut rep = SyncReport::default();
    if !account_exists(&tx, account_id)? {
        return Ok(rep); // compte supprimé pendant la relève : rien à réécrire
    }
    for m in &metas {
        if in_inbox(&m.labels) {
            upsert(&tx, account_id, m, &mut rep)?;
        } else {
            rep.removed += delete(&tx, account_id, &m.id)?;
        }
    }
    for id in delta.removed.iter().chain(&gone) {
        rep.removed += delete(&tx, account_id, id)?;
    }
    for (id, labels) in &delta.label_changed {
        rep.changed += tx.execute(
            "UPDATE mails SET unread = ?3, labels = ?4 WHERE account_id = ?1 AND id = ?2 AND labels <> ?4",
            params![account_id, id, labels.iter().any(|l| l == "UNREAD"), json(labels)],
        )?;
    }
    finish(&tx, account_id, Some(&delta.new_history_id), now, &mut rep)?;
    tx.commit()?;
    Ok(rep)
}

/// Les 100 derniers mails de la boîte de réception remplacent le cache du compte.
async fn full(db: &Db, client: &Client, token: &str, account_id: i64, now: i64) -> Result<SyncReport, SyncError> {
    // Lu AVANT la liste : rien ne se perd entre les deux, et un compte calme garde un point de reprise récent.
    let profile = client.profile_history_id(token).await?;
    let ids = client.list_inbox(token, FULL_MAX).await?;
    let (metas, _) = fetch(client, token, &ids).await?;
    // Profil sans `historyId` (inattendu) : repli sur le plus récent des mails lus.
    let history_id = Some(profile).filter(|h| !h.is_empty())
        .or_else(|| metas.iter().filter_map(|m| m.history_id.parse::<u64>().ok()).max().map(|h| h.to_string()));
    let c = db.lock();
    let tx = c.unchecked_transaction()?;
    let mut rep = SyncReport::default();
    if !account_exists(&tx, account_id)? {
        return Ok(rep); // compte supprimé pendant la relève : rien à réécrire
    }
    let keep: HashSet<&str> = metas.iter().filter(|m| in_inbox(&m.labels)).map(|m| m.id.as_str()).collect();
    let cached: Vec<String> =
        tx.prepare("SELECT id FROM mails WHERE account_id = ?1")?.query_map([account_id], |r| r.get(0))?.collect::<Result<_, _>>()?;
    for id in cached.iter().filter(|id| !keep.contains(id.as_str())) {
        rep.removed += delete(&tx, account_id, id)?;
    }
    for m in metas.iter().filter(|m| keep.contains(m.id.as_str())) {
        upsert(&tx, account_id, m, &mut rep)?;
    }
    finish(&tx, account_id, history_id.as_deref(), now, &mut rep)?;
    tx.commit()?;
    Ok(rep)
}

/// Métadonnées des ids ; ceux qui n'existent plus (404) sont rendus à part. Toute autre erreur annule la relève.
async fn fetch(client: &Client, token: &str, ids: &[String]) -> Result<(Vec<MetaMsg>, Vec<String>), SyncError> {
    let (mut metas, mut gone) = (Vec::new(), Vec::new());
    for (id, r) in ids.iter().zip(client.get_metadata_many(token, ids).await) {
        match r {
            Ok(m) => metas.push(m),
            Err(GmailError::NotFound) => gone.push(id.clone()),
            Err(e) => return Err(e.into()),
        }
    }
    Ok((metas, gone))
}

fn account_exists(c: &Connection, account_id: i64) -> rusqlite::Result<bool> {
    Ok(c.query_row("SELECT 1 FROM accounts WHERE id = ?1", [account_id], |_| Ok(())).optional()?.is_some())
}

fn json(labels: &[String]) -> String { serde_json::to_string(labels).unwrap_or_else(|_| "[]".into()) }

fn delete(c: &Connection, account_id: i64, id: &str) -> rusqlite::Result<usize> {
    c.execute("DELETE FROM mails WHERE account_id = ?1 AND id = ?2", params![account_id, id])
}

/// Ajoute ou met à jour un mail. Le tri d'IRIS (`bucket_src = 'iris'`), le résumé et l'étiquette sont gardés.
fn upsert(c: &Connection, account_id: i64, m: &MetaMsg, rep: &mut SyncReport) -> rusqlite::Result<()> {
    let labels = json(&m.labels);
    let unread = m.labels.iter().any(|l| l == "UNREAD");
    let old: Option<(bool, String)> = c
        .query_row("SELECT unread, labels FROM mails WHERE account_id = ?1 AND id = ?2", params![account_id, m.id], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    if old.as_ref() == Some(&(unread, labels.clone())) {
        return Ok(());
    }
    c.execute(
        "INSERT INTO mails(account_id, id, thread_id, from_name, from_addr, subject, snippet, date_ms, unread, labels, bucket, bucket_src)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'heuristic')
         ON CONFLICT(account_id, id) DO UPDATE SET thread_id = excluded.thread_id, from_name = excluded.from_name,
           from_addr = excluded.from_addr, subject = excluded.subject, snippet = excluded.snippet, date_ms = excluded.date_ms,
           unread = excluded.unread, labels = excluded.labels,
           bucket = CASE WHEN bucket_src = 'heuristic' THEN excluded.bucket ELSE bucket END",
        params![
            account_id, m.id, m.thread_id, m.from_name, m.from_addr, m.subject, m.snippet, m.date_ms, unread, labels,
            heuristic_bucket(&m.labels, m.list_unsubscribe).as_str()
        ],
    )?;
    if old.is_some() { rep.changed += 1 } else { rep.added += 1 }
    Ok(())
}

/// Borne le cache, note la réussite et le point de reprise.
fn finish(c: &Connection, account_id: i64, history_id: Option<&str>, now: i64, rep: &mut SyncReport) -> rusqlite::Result<()> {
    rep.removed += c.execute(
        "DELETE FROM mails WHERE account_id = ?1 AND id NOT IN
           (SELECT id FROM mails WHERE account_id = ?1 ORDER BY date_ms DESC, id DESC LIMIT ?2)",
        params![account_id, CACHE_MAX],
    )?;
    c.execute(
        "INSERT INTO mail_sync(account_id, history_id, last_ok_ms, backoff_until_ms) VALUES (?1, ?2, ?3, NULL)
         ON CONFLICT(account_id) DO UPDATE SET history_id = excluded.history_id, last_ok_ms = ?3, backoff_until_ms = NULL",
        params![account_id, history_id, now],
    )?;
    Ok(())
}

/// Note le délai d'attente après un échec (`None` : effacé).
pub fn set_backoff(c: &Connection, account_id: i64, until_ms: Option<i64>) -> rusqlite::Result<()> {
    c.execute(
        "INSERT INTO mail_sync(account_id, backoff_until_ms) SELECT ?1, ?2 WHERE EXISTS (SELECT 1 FROM accounts WHERE id = ?1)
         ON CONFLICT(account_id) DO UPDATE SET backoff_until_ms = ?2",
        params![account_id, until_ms],
    )?;
    Ok(())
}

/// Délai après `fails` échecs consécutifs : 1, 2, 4, 8 puis 15 minutes au plus.
pub fn backoff_ms(fails: u32) -> i64 {
    const MIN: i64 = 60_000;
    (MIN << fails.saturating_sub(1).min(4)).min(15 * MIN)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::{self, Configs, Status};
    use crate::events::NoEvents;
    use crate::oauth::{self, Provider, ProviderConfig, Tokens};
    use crate::secrets::{account_key, MemorySecrets, SecretStore};
    use std::sync::Arc;
    use wiremock::matchers::{method, path, query_param, query_param_is_missing};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    macro_rules! fixture { ($n:literal) => { include_str!(concat!("../../tests/fixtures/gmail/", $n)) } }

    fn body(s: &str) -> ResponseTemplate { ResponseTemplate::new(200).set_body_raw(s.to_string(), "application/json") }

    async fn meta(server: &MockServer, id: &str, json: &str) {
        Mock::given(method("GET")).and(path(format!("/gmail/v1/users/me/messages/{id}"))).and(query_param("format", "metadata"))
            .respond_with(body(json)).mount(server).await;
    }

    async fn inbox(server: &MockServer) {
        Mock::given(method("GET")).and(path("/gmail/v1/users/me/profile")).respond_with(body(fixture!("profile.json"))).mount(server).await;
        Mock::given(method("GET")).and(path("/gmail/v1/users/me/messages")).and(query_param("labelIds", "INBOX")).and(query_param("maxResults", "100"))
            .respond_with(body(fixture!("list_inbox.json"))).mount(server).await;
        meta(server, "18c1a", fixture!("meta_18c1a.json")).await;
        meta(server, "18c1b", fixture!("meta_18c1b.json")).await;
        meta(server, "18c1c", fixture!("meta_18c1c.json")).await;
    }

    async fn history(server: &MockServer, start: &str) {
        let h = "/gmail/v1/users/me/history";
        Mock::given(method("GET")).and(path(h)).and(query_param("startHistoryId", start)).and(query_param_is_missing("pageToken"))
            .respond_with(body(fixture!("history_page1.json"))).mount(server).await;
        Mock::given(method("GET")).and(path(h)).and(query_param("startHistoryId", start)).and(query_param("pageToken", "page2"))
            .respond_with(body(fixture!("history_page2.json"))).mount(server).await;
        meta(server, "18c2d", fixture!("meta_18c2d.json")).await;
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
        async fn run(&self) -> Result<SyncReport, SyncError> { run(&self.db, &self.client, &self.tokens, self.id, 42).await }
        fn ids(&self) -> Vec<String> {
            self.db.lock().prepare("SELECT id FROM mails ORDER BY date_ms DESC").unwrap().query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
        }
        fn history_id(&self) -> Option<String> {
            self.db.lock().query_row("SELECT history_id FROM mail_sync WHERE account_id = ?1", [self.id], |r| r.get(0)).optional().unwrap().flatten()
        }
        fn seed(&self, id: &str, date_ms: i64) {
            self.db.lock().execute(
                "INSERT INTO mails VALUES (?1, ?2, 't', 'n', 'a', 's', '', ?3, 0, '[\"INBOX\"]', 'read', 'heuristic', NULL, NULL)",
                params![self.id, id, date_ms],
            ).unwrap();
        }
    }

    async fn complete_puis_historique(server: &MockServer) -> Env {
        inbox(server).await;
        let e = env(server, true);
        assert_eq!(e.run().await.unwrap(), SyncReport { added: 3, removed: 0, changed: 0 });
        assert_eq!(e.ids(), ["18c1a", "18c1b", "18c1c"]);
        assert_eq!(e.history_id().as_deref(), Some("1500"), "historyId du profil");
        let row: (String, String, String, String, bool, String, String) = e.db.lock().query_row(
            "SELECT from_name, from_addr, subject, snippet, unread, bucket, bucket_src FROM mails WHERE id = '18c1a'", [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)),
        ).unwrap();
        assert_eq!(row, ("Dupont, Jean".into(), "j@x.fr".into(), "Réunion demain à 10 h".into(),
            "On se retrouve à l'accueil & on monte ensemble.".into(), true, "todo".into(), "heuristic".into()));
        let b: (String, String) = e.db.lock().query_row("SELECT from_name, bucket FROM mails WHERE id = '18c1b'", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(b, ("Boutique été".into(), "filtered".into()));
        // Historique sur deux pages : 18c2d arrive, 18c1a lu, 18c1b archivé ailleurs, 18c1c supprimé, 18c2e (envoyé) ignoré.
        history(server, "1500").await;
        assert_eq!(e.run().await.unwrap(), SyncReport { added: 1, removed: 2, changed: 1 });
        assert_eq!(e.ids(), ["18c2d", "18c1a"]);
        let unread: bool = e.db.lock().query_row("SELECT unread FROM mails WHERE id = '18c1a'", [], |r| r.get(0)).unwrap();
        assert!(!unread, "passé à lu");
        assert_eq!(e.history_id().as_deref(), Some("2000"));
        e
    }

    #[tokio::test] async fn complete_puis_par_l_historique() {
        let server = MockServer::start().await;
        complete_puis_historique(&server).await;
    }

    #[tokio::test] async fn historique_expire_resynchronise_sans_doublon() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/gmail/v1/users/me/history"))
            .respond_with(ResponseTemplate::new(404).set_body_raw(fixture!("history_404.json"), "application/json"))
            .up_to_n_times(1).expect(1).mount(&server).await;
        inbox(&server).await;
        let e = env(&server, true);
        e.seed("18c1a", 1);
        e.seed("ailleurs", 2); // archivé depuis un autre appareil
        set_history(&e, "7");
        // Le tri d'IRIS survit à la resynchronisation.
        e.db.lock().execute("UPDATE mails SET bucket = 'todo', bucket_src = 'iris', summary = 'r' WHERE id = '18c1a'", []).unwrap();
        let rep = e.run().await.unwrap();
        assert_eq!(rep, SyncReport { added: 2, removed: 1, changed: 1 });
        assert_eq!(e.ids(), ["18c1a", "18c1b", "18c1c"], "sans doublon, le mail archivé ailleurs a disparu");
        let kept: (String, String, Option<String>) = e.db.lock()
            .query_row("SELECT bucket, bucket_src, summary FROM mails WHERE id = '18c1a'", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
        assert_eq!(kept, ("todo".into(), "iris".into(), Some("r".into())));
        assert_eq!(e.history_id().as_deref(), Some("1500"), "historyId du profil, pas l'ancien");
        // Le tour suivant repart de l'historique, pas d'une nouvelle synchronisation complète.
        history(&server, "1500").await;
        assert_eq!(e.run().await.unwrap(), SyncReport { added: 1, removed: 2, changed: 1 });
        assert_eq!(e.history_id().as_deref(), Some("2000"));
        let full_lists = server.received_requests().await.unwrap().iter().filter(|r| r.url.path() == "/gmail/v1/users/me/messages").count();
        assert_eq!(full_lists, 1, "une seule synchronisation complète");
        server.verify().await;
    }

    #[tokio::test] async fn compte_calme_ne_boucle_pas_en_resynchronisation() {
        // Boîte vide : le profil donne quand même un point de reprise, le tour suivant passe par l'historique.
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/gmail/v1/users/me/profile")).respond_with(body(fixture!("profile.json"))).expect(1).mount(&server).await;
        Mock::given(method("GET")).and(path("/gmail/v1/users/me/messages")).respond_with(body("{\"resultSizeEstimate\": 0}")).expect(1).mount(&server).await;
        Mock::given(method("GET")).and(path("/gmail/v1/users/me/history")).and(query_param("startHistoryId", "1500"))
            .respond_with(body("{\"historyId\": \"\"}")).expect(1).mount(&server).await;
        let e = env(&server, true);
        assert_eq!(e.run().await.unwrap(), SyncReport::default());
        assert_eq!(e.history_id().as_deref(), Some("1500"));
        assert_eq!(e.run().await.unwrap(), SyncReport::default());
        assert_eq!(e.history_id().as_deref(), Some("1500"), "historyId vide : le départ est gardé");
        server.verify().await;
    }

    fn set_history(e: &Env, h: &str) {
        e.db.lock().execute("INSERT INTO mail_sync(account_id, history_id) VALUES (?1, ?2)", params![e.id, h]).unwrap();
    }

    #[tokio::test] async fn cache_borne_a_500() {
        let server = MockServer::start().await;
        history(&server, "1500").await;
        let e = env(&server, true);
        for i in 0..CACHE_MAX {
            e.seed(&format!("x{i:03}"), 1_000 + i);
        }
        set_history(&e, "1500");
        let rep = e.run().await.unwrap();
        assert_eq!((rep.added, rep.removed), (1, 1));
        let ids = e.ids();
        assert_eq!(ids.len() as i64, CACHE_MAX);
        assert_eq!(ids[0], "18c2d");
        assert!(!ids.contains(&"x000".to_string()), "le plus ancien sort");
    }

    #[tokio::test] async fn invalid_grant_passe_en_reauth_sans_essai() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({"error": "invalid_grant"})))
            .expect(1).mount(&server).await;
        Mock::given(path("/gmail/v1/users/me/messages")).respond_with(ResponseTemplate::new(500)).expect(0).mount(&server).await;
        let e = env(&server, false);
        assert_eq!(e.run().await, Err(SyncError::Revoked));
        assert_eq!(accounts::get(&e.db.lock(), e.id).unwrap().unwrap().status, Status::Reauth);
        server.verify().await;
    }

    #[tokio::test] async fn erreurs_reseau_et_401() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).respond_with(ResponseTemplate::new(503)).up_to_n_times(1).mount(&server).await;
        Mock::given(method("GET")).respond_with(ResponseTemplate::new(401)).mount(&server).await;
        let e = env(&server, true);
        assert_eq!(e.run().await, Err(SyncError::Network));
        assert_eq!(e.run().await, Err(SyncError::Unauthorized));
        assert!(e.ids().is_empty());
        assert_eq!(e.history_id(), None, "rien noté après un échec");
        // Jeton refusé : oublié, le prochain essai passe par l'actualisation.
        Mock::given(method("POST")).and(path("/token")).respond_with(ResponseTemplate::new(503)).expect(1).mount(&server).await;
        assert_eq!(e.run().await, Err(SyncError::Network));
        server.verify().await;
    }

    #[test] fn delai_croissant() {
        let min = |f| backoff_ms(f) / 60_000;
        assert_eq!([1, 2, 3, 4, 5, 6, 40].map(min), [1, 2, 4, 8, 15, 15, 15]);
        assert_eq!(min(2), 2, "deux erreurs réseau : 2 minutes");
        let c = crate::test_util::db();
        let id = accounts::add(&c, Provider::Google, "a", 1).unwrap();
        set_backoff(&c, id, Some(99)).unwrap();
        let until: Option<i64> = c.query_row("SELECT backoff_until_ms FROM mail_sync WHERE account_id = ?1", [id], |r| r.get(0)).unwrap();
        assert_eq!(until, Some(99));
        set_backoff(&c, id + 1, Some(99)).unwrap();
        let n: i64 = c.query_row("SELECT count(*) FROM mail_sync", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1, "pas de ligne pour un compte supprimé");
    }

    thread_local! {
        /// Journal capturé pour ce fil seulement (les autres tests tournent en parallèle).
        static CAPTURE: std::cell::RefCell<Vec<u8>> = const { std::cell::RefCell::new(Vec::new()) };
    }
    struct Buf;
    impl std::io::Write for Buf {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> { CAPTURE.with(|c| c.borrow_mut().extend_from_slice(b)); Ok(b.len()) }
        fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
    }

    #[test] fn le_journal_ne_contient_ni_adresse_ni_sujet() {
        // Abonné global : un abonné par fil (`with_default`) perd des événements quand d'autres tests tournent en même temps.
        let sub = tracing_subscriber::fmt().with_max_level(tracing::Level::TRACE).with_ansi(false).with_writer(|| Buf).finish();
        let _ = tracing::subscriber::set_global_default(sub);
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let server = MockServer::start().await;
            let e = complete_puis_historique(&server).await;
            // Et un échec : l'erreur se journalise aussi sans contenu.
            e.db.lock().execute("UPDATE mail_sync SET history_id = '1'", []).unwrap();
            Mock::given(path("/gmail/v1/users/me/history")).respond_with(ResponseTemplate::new(500)).mount(&server).await;
            assert_eq!(e.run().await, Err(SyncError::Network));
        });
        let log = CAPTURE.with(|c| String::from_utf8(c.borrow().clone()).unwrap());
        assert!(log.contains("mails : compte"), "la relève est journalisée : {log}");
        for secret in ["j@x.fr", "Dupont", "Réunion", "UsOpdW5pb24", "promo@boutique", "Boutique", "Soldes", "marie@", "Compte rendu", "colis", "moi@gmail", "accueil"] {
            assert!(!log.contains(secret), "« {secret} » dans le journal");
        }
    }

    #[test] fn jetons_et_secret_client_absents_du_journal_et_des_tables() {
        use crate::accounts::complete_connect;
        const AT: &str = "AT-SENTINEL-q81x";
        const RT: &str = "RT-SENTINEL-m47z";
        const CS: &str = "CS-SENTINEL-k20w";
        let sub = tracing_subscriber::fmt().with_max_level(tracing::Level::TRACE).with_ansi(false).with_writer(|| Buf).finish();
        let _ = tracing::subscriber::set_global_default(sub);
        CAPTURE.with(|c| c.borrow_mut().clear());
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let db: Db = Arc::new(parking_lot::Mutex::new(crate::test_util::db()));
        rt.block_on(async {
            let server = MockServer::start().await;
            Mock::given(method("POST")).and(path("/token"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"access_token": AT, "expires_in": 3600, "refresh_token": RT})))
                .mount(&server).await;
            Mock::given(method("GET")).and(path("/me"))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"email": "moi@gmail.com"})))
                .mount(&server).await;
            let token_url = format!("{}/token", server.uri());
            let cfg = ProviderConfig { token_url: token_url.clone(), ..oauth::providers::google("cid", Some(CS.into())) };
            let configs: Configs = Arc::new({ let cfg = cfg.clone(); move |_| Some(cfg.clone()) });
            let http = reqwest::Client::new();
            let tokens = TokenCache::new(db.clone(), Arc::new(MemorySecrets::default()), Arc::new(NoEvents), http.clone(), configs);
            let v = complete_connect(&db, &tokens, &NoEvents, &http, &cfg, "code", "ver", oauth::REDIRECT_URI, &format!("{}/me", server.uri())).await.unwrap();
            // Jeton d'accès périmé : l'actualisation repasse par le point d'accès avec le secret client.
            tokens.insert(v.id, Tokens { access: "vieux".into(), refresh: Some(RT.into()), expires_at_ms: 0 }).unwrap();
            assert_eq!(tokens.access_token(v.id).await.unwrap(), AT);
        });
        let log = CAPTURE.with(|c| String::from_utf8(c.borrow().clone()).unwrap());
        let c = db.lock();
        for t in ["accounts", "kv", "actions"] {
            let cols: Vec<String> = c.prepare(&format!("SELECT name FROM pragma_table_info('{t}')")).unwrap().query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
            let expr = cols.iter().map(|k| format!("coalesce(CAST({k} AS TEXT), '')")).collect::<Vec<_>>().join(" || ' ' || ");
            let all: Option<String> = c.query_row(&format!("SELECT group_concat({expr}, ' ') FROM {t}"), [], |r| r.get(0)).unwrap();
            let all = all.unwrap_or_default();
            for secret in [AT, RT, CS] {
                assert!(!all.contains(secret) && !log.contains(secret), "« {secret} » dans {t} ou le journal");
            }
        }
    }
}
