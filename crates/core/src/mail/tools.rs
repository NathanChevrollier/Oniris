//! Outils mails : liste du cache, lecture (corps en mémoire seulement, LRU de 20), lu/non lu, archivage,
//! corbeille et envoi (à confirmer), ouverture dans Gmail et des liens. Journal : jamais d'adresse, de sujet ni de corps.
use super::gmail::{Client, GmailError};
use super::{html, mime};
use crate::accounts::{self, TokenCache};
use crate::events::{Events, Topic};
use crate::launch::{safe_url, Opener};
use crate::oauth::{OAuthError, Provider};
use crate::tools::helpers::{bad, failed, low, parse, spec, untrusted};
use crate::tools::{ConfirmationText, Permission, Risk, Tool, ToolError, ToolResult, ToolSpec};
use crate::Db;
use async_trait::async_trait;
use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::sync::Arc;

/// Corps lus gardés en mémoire (jamais en base).
pub const BODY_CACHE: usize = 20;
/// Lignes rendues par `mail.list`, au plus.
pub const LIST_MAX: u32 = 50;
/// Corps d'un mail envoyé, au plus (caractères).
pub const SEND_MAX: usize = 100_000;

/// Ligne du cache des en-têtes. Contenu utilisateur : jamais journalisé.
#[derive(Clone, Serialize)]
pub struct MailRow {
    pub account_id: i64,
    pub id: String,
    pub thread_id: String,
    pub from_name: String,
    pub from_addr: String,
    pub subject: String,
    pub snippet: String,
    pub date_ms: i64,
    pub unread: bool,
    pub bucket: String,
    pub summary: Option<String>,
    pub tag: Option<String>,
}

const COLS: &str = "account_id, id, thread_id, from_name, from_addr, subject, snippet, date_ms, unread, bucket, summary, tag";

fn from_row(r: &rusqlite::Row) -> rusqlite::Result<MailRow> {
    Ok(MailRow {
        account_id: r.get(0)?, id: r.get(1)?, thread_id: r.get(2)?, from_name: r.get(3)?, from_addr: r.get(4)?,
        subject: r.get(5)?, snippet: r.get(6)?, date_ms: r.get(7)?, unread: r.get(8)?, bucket: r.get(9)?,
        summary: r.get(10)?, tag: r.get(11)?,
    })
}

/// Mails de la boîte de réception (archivés et supprimés exclus), du plus récent au plus ancien.
pub fn list(c: &Connection, account: Option<i64>, bucket: Option<&str>, limit: u32) -> rusqlite::Result<Vec<MailRow>> {
    let mut st = c.prepare(&format!(
        "SELECT {COLS} FROM mails WHERE labels LIKE '%\"INBOX\"%' AND (?1 IS NULL OR account_id = ?1) AND (?2 IS NULL OR bucket = ?2)
         ORDER BY date_ms DESC LIMIT ?3"
    ))?;
    let rows = st.query_map(params![account, bucket, limit.min(LIST_MAX)], from_row)?;
    rows.collect()
}

fn row(c: &Connection, account: i64, id: &str) -> Option<MailRow> {
    c.query_row(
        &format!("SELECT {COLS} FROM mails WHERE account_id = ?1 AND id = ?2"),
        params![account, id],
        from_row,
    )
    .optional()
    .ok()
    .flatten()
}

/// Reporte un changement de libellés dans le cache (si la ligne y est encore).
fn relabel(c: &Connection, account: i64, id: &str, add: &[&str], remove: &[&str]) -> rusqlite::Result<()> {
    let labels: Option<String> = c.query_row("SELECT labels FROM mails WHERE account_id = ?1 AND id = ?2", params![account, id], |r| r.get(0)).optional()?;
    let Some(labels) = labels else { return Ok(()) };
    let mut l: Vec<String> = serde_json::from_str(&labels).unwrap_or_default();
    l.retain(|x| !remove.contains(&x.as_str()));
    for a in add {
        if !l.iter().any(|x| x == a) {
            l.push(a.to_string());
        }
    }
    let unread = l.iter().any(|x| x == "UNREAD");
    c.execute(
        "UPDATE mails SET labels = ?3, unread = ?4 WHERE account_id = ?1 AND id = ?2",
        params![account, id, serde_json::to_string(&l).unwrap_or_else(|_| "[]".into()), unread],
    )?;
    Ok(())
}

/// État partagé des outils mails.
pub struct Ctx {
    pub db: Db,
    pub events: Arc<dyn Events>,
    pub tokens: Arc<TokenCache>,
    pub client: Client,
    pub opener: Arc<dyn Opener>,
    /// LRU `(compte, id) -> sortie de mail.read`, le plus récent devant.
    bodies: Mutex<VecDeque<((i64, String), Value)>>,
}

#[derive(Deserialize)]
struct MsgArgs { account: i64, id: String }
#[derive(Deserialize)]
struct ReadArgs { account: i64, id: String, read: bool }
#[derive(Deserialize)]
struct ListArgs { bucket: Option<String>, account: Option<i64>, limit: Option<u32> }
#[derive(Deserialize)]
struct SendArgs { account: i64, to: String, subject: String, body: String, reply_to_id: Option<String> }
#[derive(Deserialize)]
struct UrlArgs { url: String }

impl Ctx {
    /// Adresse du compte Google.
    fn address(&self, account: i64) -> Result<String, ToolError> {
        let a = accounts::get(&self.db.lock(), account).map_err(failed)?;
        match a {
            Some(a) if a.provider == Provider::Google => Ok(a.label),
            _ => Err(bad("compte Gmail introuvable")),
        }
    }

    async fn token(&self, account: i64) -> Result<String, ToolError> {
        self.address(account)?;
        self.tokens.access_token(account).await.map_err(|e| match e {
            OAuthError::Revoked => failed("compte à reconnecter"),
            OAuthError::Network => failed("réseau indisponible"),
            OAuthError::Other(m) => failed(m),
        })
    }

    fn gmail_err(&self, account: i64, e: GmailError) -> ToolError {
        if e == GmailError::Unauthorized {
            self.tokens.forget(account);
        }
        failed(e)
    }

    async fn modify(&self, account: i64, id: &str, add: &[&str], remove: &[&str]) -> Result<(), ToolError> {
        let token = self.token(account).await?;
        self.client.modify(&token, id, add, remove).await.map_err(|e| self.gmail_err(account, e))?;
        let r = relabel(&self.db.lock(), account, id, add, remove);
        r.map_err(failed)?;
        self.events.changed(Topic::Mail);
        Ok(())
    }

    async fn set_read(&self, account: i64, id: &str, read: bool) -> Result<(), ToolError> {
        if read { self.modify(account, id, &[], &["UNREAD"]).await } else { self.modify(account, id, &["UNREAD"], &[]).await }
    }

    fn cached(&self, key: &(i64, String)) -> Option<Value> {
        let mut b = self.bodies.lock();
        let i = b.iter().position(|(k, _)| k == key)?;
        let e = b.remove(i)?;
        let v = e.1.clone();
        b.push_front(e);
        Some(v)
    }

    fn remember(&self, key: (i64, String), v: Value) {
        let mut b = self.bodies.lock();
        b.retain(|(k, _)| *k != key);
        b.push_front((key, v));
        b.truncate(BODY_CACHE);
    }
}

#[derive(Clone, Copy)]
enum Kind { List, Read, MarkRead, Archive, Delete, Send, OpenWeb, OpenLink }

pub struct MailTool { kind: Kind, ctx: Arc<Ctx> }

fn obj(props: Value, required: &[&str]) -> Value { json!({"type": "object", "properties": props, "required": required}) }

#[async_trait]
impl Tool for MailTool {
    fn spec(&self) -> ToolSpec {
        let msg = || obj(json!({"account": {"type": "integer"}, "id": {"type": "string"}}), &["account", "id"]);
        let sensitive = |s: ToolSpec| ToolSpec { sensitive_output: true, ..s };
        let untrusted_sensitive = |s: ToolSpec| untrusted(ToolSpec { sensitive_output: true, ..s });
        match self.kind {
            Kind::List => untrusted_sensitive(spec("mail.list", "Mails récents de la boîte de réception (cache local) : expéditeur, sujet, extrait, tri.",
                obj(json!({"bucket": {"enum": ["todo", "read", "filtered"]}, "account": {"type": "integer"}, "limit": {"type": "integer", "maximum": LIST_MAX}}), &[]),
                Permission::Free)),
            Kind::Read => untrusted_sensitive(spec("mail.read", "Lit un mail (HTML nettoyé, texte, pièces jointes, liens) et le marque comme lu.", msg(), Permission::Free)),
            Kind::MarkRead => spec("mail.mark_read", "Marque un mail comme lu ou non lu.",
                obj(json!({"account": {"type": "integer"}, "id": {"type": "string"}, "read": {"type": "boolean"}}), &["account", "id", "read"]), Permission::Free),
            Kind::Archive => spec("mail.archive", "Archive un mail (le retire de la boîte de réception).", msg(), Permission::Free),
            Kind::Delete => spec("mail.delete", "Met un mail à la corbeille Gmail (confirmation requise).", msg(), Permission::Confirm),
            Kind::Send => sensitive(spec("mail.send", "Envoie un mail en texte brut, ou une réponse avec reply_to_id (confirmation requise).",
                obj(json!({"account": {"type": "integer"}, "to": {"type": "string"}, "subject": {"type": "string"}, "body": {"type": "string"}, "reply_to_id": {"type": "string"}}),
                    &["account", "to", "subject", "body"]),
                Permission::Confirm)),
            Kind::OpenWeb => spec("mail.open_web", "Ouvre le mail dans Gmail sur le navigateur.", msg(), Permission::Free),
            Kind::OpenLink => spec("mail.open_link", "Ouvre un lien d'un mail (http, https ou mailto).", obj(json!({"url": {"type": "string"}}), &["url"]), Permission::Free),
        }
    }

    // Ce texte va à l'écran de confirmation, jamais à tracing.
    fn describe(&self, a: &Value) -> ConfirmationText {
        let s = |k: &str| a[k].as_str().unwrap_or_default().to_string();
        match self.kind {
            Kind::Delete => {
                let r = a["account"].as_i64().and_then(|acc| row(&self.ctx.db.lock(), acc, &s("id")));
                let (from, subject) = r.map_or_else(
                    || ("expéditeur inconnu".to_string(), "(mail absent du cache)".to_string()),
                    |r| (format!("{} <{}>", r.from_name, r.from_addr), r.subject),
                );
                ConfirmationText { summary: format!("Mettre à la corbeille « {subject} »"), details: format!("De : {from}\nSujet : {subject}"), risk: Risk::Medium }
            }
            Kind::Send => {
                let from = a["account"].as_i64().and_then(|acc| self.ctx.address(acc).ok()).unwrap_or_else(|| "compte inconnu".into());
                let subject = send_subject(&s("subject"), a["reply_to_id"].is_string());
                let too_long = s("body").chars().count() > SEND_MAX;
                ConfirmationText {
                    summary: if too_long { "Texte trop long : l'envoi sera refusé".into() } else { format!("Envoyer un mail à {}", s("to")) },
                    details: format!("Compte : {from}\nÀ : {}\nSujet : {subject}\n\n{}", s("to"), s("body")),
                    risk: Risk::High,
                }
            }
            _ => low("Action sur un mail"),
        }
    }

    async fn run(&self, a: &Value) -> Result<ToolResult, ToolError> {
        let ctx = &self.ctx;
        let done = |output: Value, undo: Option<Value>| Ok(ToolResult { output, undo });
        match self.kind {
            Kind::List => {
                let ListArgs { bucket, account, limit } = parse(a)?;
                let rows = list(&ctx.db.lock(), account, bucket.as_deref(), limit.unwrap_or(LIST_MAX)).map_err(failed)?;
                done(serde_json::to_value(rows).map_err(failed)?, None)
            }
            Kind::Read => {
                let MsgArgs { account, id } = parse(a)?;
                ctx.address(account)?;
                let key = (account, id.clone());
                let out = match ctx.cached(&key) {
                    Some(v) => v,
                    None => {
                        let token = ctx.token(account).await?;
                        let m = ctx.client.get_full(&token, &id).await.map_err(|e| ctx.gmail_err(account, e))?;
                        let rendered = match (&m.html, &m.text) {
                            (Some(h), _) => html::sanitize(h),
                            (None, t) => html::text_to_html(t.as_deref().unwrap_or_default()),
                        };
                        let text = m.text.as_deref().map(|t| html::truncate(t, html::MAX_INPUT));
                        let v = json!({ "html": rendered.html, "text": text, "attachments": m.attachments, "links": rendered.links, "truncated": rendered.truncated });
                        ctx.remember(key, v.clone());
                        v
                    }
                };
                let unread = row(&ctx.db.lock(), account, &id).is_none_or(|r| r.unread);
                if !unread {
                    return done(out, None);
                }
                // Le mail reste lisible même si le marquage échoue.
                match ctx.set_read(account, &id, true).await {
                    Ok(()) => done(out, Some(json!({ "account": account, "id": id, "read": false }))),
                    Err(e) => {
                        tracing::warn!("mails : compte {account} : marquage lu impossible : {e}");
                        done(out, None)
                    }
                }
            }
            Kind::MarkRead => {
                let ReadArgs { account, id, read } = parse(a)?;
                ctx.set_read(account, &id, read).await?;
                done(json!({ "read": read }), Some(json!({ "account": account, "id": id, "read": !read })))
            }
            Kind::Archive => {
                let MsgArgs { account, id } = parse(a)?;
                ctx.modify(account, &id, &[], &["INBOX"]).await?;
                done(json!({ "archived": true }), Some(json!({ "account": account, "id": id })))
            }
            Kind::Delete => {
                let MsgArgs { account, id } = parse(a)?;
                let token = ctx.token(account).await?;
                ctx.client.trash(&token, &id).await.map_err(|e| ctx.gmail_err(account, e))?;
                let r = relabel(&ctx.db.lock(), account, &id, &["TRASH"], &["INBOX"]);
                r.map_err(failed)?;
                ctx.events.changed(Topic::Mail);
                done(json!({ "trashed": true }), Some(json!({ "account": account, "id": id })))
            }
            Kind::Send => {
                // Forme masquée du journal (après un redémarrage) : `body` n'est plus une chaîne, refusé ici.
                let SendArgs { account, to, subject, body, reply_to_id } = parse(a)?;
                if body.chars().count() > SEND_MAX {
                    return Err(bad("texte trop long"));
                }
                let from = ctx.address(account)?;
                // Validé avant toute requête (jeton compris).
                let subject = send_subject(&subject, reply_to_id.is_some());
                mime::reply(&from, &to, &subject, &body, None, None).map_err(bad)?;
                let token = ctx.token(account).await?;
                let (thread, raw) = match reply_to_id {
                    Some(rid) => {
                        let (thread, msg_id, refs) = ctx.client.reply_headers(&token, &rid).await.map_err(|e| ctx.gmail_err(account, e))?;
                        let raw = mime::reply(&from, &to, &subject, &body, msg_id.as_deref(), refs.as_deref()).map_err(bad)?;
                        (Some(thread), raw)
                    }
                    None => (None, mime::reply(&from, &to, &subject, &body, None, None).map_err(bad)?),
                };
                ctx.client.send(&token, &raw, thread.as_deref()).await.map_err(|e| ctx.gmail_err(account, e))?;
                tracing::info!("mails : compte {account} : mail envoyé");
                done(json!({ "sent": true }), None)
            }
            Kind::OpenWeb => {
                let MsgArgs { account, id } = parse(a)?;
                let addr = ctx.address(account)?;
                let thread = row(&ctx.db.lock(), account, &id).map_or(id, |r| r.thread_id);
                let url = gmail_web_url(&addr, &thread).ok_or_else(|| bad("mail introuvable"))?;
                ctx.opener.open_url(&url);
                done(json!({ "opened": true }), None)
            }
            Kind::OpenLink => {
                let UrlArgs { url } = parse(a)?;
                let url = safe_url(&url).ok_or_else(|| bad("lien refusé"))?;
                ctx.opener.open_url(&url);
                done(json!({ "opened": true }), None)
            }
        }
    }

    async fn undo(&self, u: &Value) -> Result<(), ToolError> {
        let ctx = &self.ctx;
        match self.kind {
            Kind::Read | Kind::MarkRead => {
                let ReadArgs { account, id, read } = parse(u)?;
                ctx.set_read(account, &id, read).await
            }
            Kind::Archive => {
                let MsgArgs { account, id } = parse(u)?;
                ctx.modify(account, &id, &["INBOX"], &[]).await
            }
            Kind::Delete => {
                let MsgArgs { account, id } = parse(u)?;
                let token = ctx.token(account).await?;
                ctx.client.untrash(&token, &id).await.map_err(|e| ctx.gmail_err(account, e))?;
                let r = relabel(&ctx.db.lock(), account, &id, &["INBOX"], &["TRASH"]);
                r.map_err(failed)?;
                ctx.events.changed(Topic::Mail);
                Ok(())
            }
            _ => Err(ToolError::NotUndoable),
        }
    }

    fn journal_args(&self, a: &Value) -> Value {
        if let Kind::Send = self.kind {
            // Ni destinataire, ni sujet, ni corps en base : seulement des tailles.
            let n = |k: &str| a.get(k).and_then(Value::as_str).map_or(0, |s| s.chars().count());
            return json!({ "account": a.get("account"), "reply_to_id": a.get("reply_to_id"), "subject_chars": n("subject"), "body_chars": n("body") });
        }
        a.clone()
    }
}

/// Sujet réellement envoyé (carte de confirmation comprise) : `Re: ` ajouté une fois pour une réponse.
fn send_subject(subject: &str, reply: bool) -> String {
    if reply { mime::reply_subject(subject) } else { subject.trim().to_string() }
}

/// `https://mail.google.com/mail/u/<adresse encodée>/#all/<fil>` ; fil non alphanumérique : `None`.
pub fn gmail_web_url(address: &str, thread: &str) -> Option<String> {
    if thread.is_empty() || !thread.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return None;
    }
    let addr: String = url::form_urlencoded::byte_serialize(address.as_bytes()).collect();
    safe_url(&format!("https://mail.google.com/mail/u/{addr}/#all/{thread}"))
}

pub fn all(db: &Db, events: &Arc<dyn Events>, tokens: &Arc<TokenCache>, client: Client, opener: &Arc<dyn Opener>) -> Vec<Arc<dyn Tool>> {
    let ctx = Arc::new(Ctx { db: db.clone(), events: events.clone(), tokens: tokens.clone(), client, opener: opener.clone(), bodies: Mutex::default() });
    [Kind::List, Kind::Read, Kind::MarkRead, Kind::Archive, Kind::Delete, Kind::Send, Kind::OpenWeb, Kind::OpenLink]
        .into_iter()
        .map(|kind| Arc::new(MailTool { kind, ctx: ctx.clone() }) as Arc<dyn Tool>)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::Configs;
    use crate::events::NoEvents;
    use crate::oauth::{self, ProviderConfig, Tokens};
    use crate::secrets::MemorySecrets;
    use crate::tools::{Origin, Provenance, ToolBroker, ToolCall, ToolOutcome};
    use std::path::Path;
    use wiremock::matchers::{any, body_json, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[derive(Default)]
    struct Fake(Mutex<Vec<String>>);
    impl Opener for Fake {
        fn open_path(&self, _: &Path) {}
        fn open_url(&self, u: &str) { self.0.lock().push(u.into()); }
        fn spawn_detached(&self, _: &str, _: &[String], _: Option<&Path>) -> std::io::Result<()> { Ok(()) }
    }

    struct Env { db: Db, broker: ToolBroker, tools: Vec<Arc<dyn Tool>>, opener: Arc<Fake>, account: i64 }

    fn env(server: &MockServer) -> Env {
        let db: Db = Arc::new(Mutex::new(crate::test_util::db()));
        let account = accounts::add(&db.lock(), Provider::Google, "moi@gmail.com", 1).unwrap();
        db.lock().execute(
            "INSERT INTO mails VALUES (?1, 'm1', 'th1', 'Jean Dupont', 'j@x.fr', 'Point budget', 'extrait', 5, 1, '[\"INBOX\",\"UNREAD\"]', 'todo', 'heuristic', NULL, NULL)",
            [account],
        ).unwrap();
        let url = format!("{}/token", server.uri());
        let configs: Configs = Arc::new(move |_| Some(ProviderConfig { token_url: url.clone(), ..oauth::providers::google("g", Some("s".into())) }));
        let events: Arc<dyn Events> = Arc::new(NoEvents);
        let tokens = Arc::new(TokenCache::new(db.clone(), Arc::new(MemorySecrets::default()), events.clone(), reqwest::Client::new(), configs));
        tokens.insert(account, Tokens { access: "tok".into(), refresh: None, expires_at_ms: crate::time_util::now_ms() + 3_600_000 }).unwrap();
        let fake = Arc::new(Fake::default());
        let opener: Arc<dyn Opener> = fake.clone();
        let tools = all(&db, &events, &tokens, Client { http: reqwest::Client::new(), base: server.uri() }, &opener);
        let broker = ToolBroker::new(db.clone(), Arc::new(|| 0), tools.clone()).unwrap();
        Env { db, broker, tools, opener: fake, account }
    }

    impl Env {
        async fn call(&self, tool: &str, args: Value) -> ToolOutcome {
            self.broker.call(ToolCall { tool: tool.into(), args, provenance: Provenance::User, origin: Origin::User }).await
        }
        fn inbox(&self) -> Vec<String> { list(&self.db.lock(), None, None, 50).unwrap().into_iter().map(|r| r.id).collect() }
        fn unread(&self) -> bool { row(&self.db.lock(), self.account, "m1").unwrap().unread }
        fn column(&self, col: &str, id: i64) -> String {
            self.db.lock().query_row(&format!("SELECT {col} FROM actions WHERE id = ?1"), [id], |r| r.get(0)).unwrap()
        }
    }

    fn ok() -> ResponseTemplate { ResponseTemplate::new(200).set_body_raw("{}", "application/json") }
    fn msg(p: &str) -> wiremock::matchers::PathExactMatcher { path(format!("/gmail/v1/users/me/messages/{p}")) }
    fn done(o: ToolOutcome) -> (i64, Value) {
        match o { ToolOutcome::Done { action_id, output } => (action_id, output), o => panic!("attendu Done : {o:?}") }
    }
    fn pending(o: ToolOutcome) -> crate::tools::PendingAction {
        match o { ToolOutcome::NeedsConfirmation(p) => p, o => panic!("attendu NeedsConfirmation : {o:?}") }
    }

    const BODY: &str = "Bonjour Jean,\nVoici le point — à demain.\nMoi";

    #[tokio::test] async fn envoi_confirme_carte_exacte_et_refus_sans_requete() {
        let server = MockServer::start().await;
        Mock::given(any()).respond_with(ok()).expect(0).mount(&server).await;
        let e = env(&server);
        let args = json!({"account": e.account, "to": "j@x.fr", "subject": "Réunion", "body": BODY});
        let p = pending(e.call("mail.send", args.clone()).await);
        assert_eq!(p.text.risk, Risk::High);
        assert_eq!(p.text.details, format!("Compte : moi@gmail.com\nÀ : j@x.fr\nSujet : Réunion\n\n{BODY}"), "texte intégral");
        let logged = e.column("args", p.action_id);
        assert!(!logged.contains("Bonjour") && !logged.contains("Réunion") && !logged.contains("j@x.fr") && logged.contains("body_chars"), "corps jamais en base : {logged}");
        assert_eq!(e.broker.resolve(p.action_id, false).await, ToolOutcome::Rejected { action_id: p.action_id });
        // Injection d'en-tête : refusée à l'exécution, avant toute requête.
        for (to, subject) in [("j@x.fr\r\nBcc: z@y.fr", "a"), ("j@x.fr", "a\r\nBcc: z@y.fr"), ("Jean <j@x.fr>", "a")] {
            let p = pending(e.call("mail.send", json!({"account": e.account, "to": to, "subject": subject, "body": "x"})).await);
            assert!(matches!(e.broker.resolve(p.action_id, true).await, ToolOutcome::Failed { .. }), "{to:?} {subject:?}");
        }
        // Après un redémarrage, le journal n'a que le masque : l'approbation échoue au lieu d'envoyer.
        let p = pending(e.call("mail.send", args).await);
        let b2 = ToolBroker::new(e.db.clone(), Arc::new(|| 0), e.tools.clone()).unwrap();
        assert!(matches!(b2.resolve(p.action_id, true).await, ToolOutcome::Failed { .. }));
        server.verify().await;
    }

    #[tokio::test] async fn envoi_approuve_en_reponse() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(msg("m1")).and(query_param("format", "metadata"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                r#"{"id":"m1","threadId":"th1","payload":{"headers":[{"name":"Message-ID","value":"<orig@x>"},{"name":"References","value":"<r0@x>"}]}}"#,
                "application/json",
            ))
            .expect(1).mount(&server).await;
        Mock::given(method("POST")).and(path("/gmail/v1/users/me/messages/send")).respond_with(ok()).expect(1).mount(&server).await;
        let e = env(&server);
        let p = pending(e.call("mail.send", json!({"account": e.account, "to": "j@x.fr", "subject": "Point budget", "body": BODY, "reply_to_id": "m1"})).await);
        assert!(p.text.details.contains("Sujet : Re: Point budget\n"), "{}", p.text.details);
        let (id, out) = done(e.broker.resolve(p.action_id, true).await);
        assert_eq!(out, json!({"sent": true}));
        assert!(e.column("result", id).contains("redacted"));
        assert!(!e.column("args", id).contains("Bonjour"));
        let sent = server.received_requests().await.unwrap().into_iter().find(|r| r.url.path().ends_with("/send")).unwrap();
        let v: Value = serde_json::from_slice(&sent.body).unwrap();
        assert_eq!(v["threadId"], "th1");
        let raw = String::from_utf8(mime::b64url(v["raw"].as_str().unwrap()).unwrap()).unwrap();
        assert!(raw.starts_with("From: moi@gmail.com\r\nTo: j@x.fr\r\nSubject: Re: Point budget\r\n"), "{raw}");
        assert!(raw.contains("In-Reply-To: <orig@x>\r\nReferences: <r0@x>\r\n <orig@x>\r\n"), "{raw}");
        server.verify().await;
    }

    #[tokio::test] async fn archive_puis_annulation() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(msg("m1/modify")).and(body_json(json!({"addLabelIds": [], "removeLabelIds": ["INBOX"]})))
            .respond_with(ok()).expect(1).mount(&server).await;
        Mock::given(method("POST")).and(msg("m1/modify")).and(body_json(json!({"addLabelIds": ["INBOX"], "removeLabelIds": []})))
            .respond_with(ok()).expect(1).mount(&server).await;
        let e = env(&server);
        let (id, _) = done(e.call("mail.archive", json!({"account": e.account, "id": "m1"})).await);
        assert!(e.inbox().is_empty(), "archivé : hors de la liste");
        e.broker.undo(id).await.unwrap();
        assert_eq!(e.inbox(), ["m1"]);
        server.verify().await;
    }

    #[tokio::test] async fn corbeille_confirmee_puis_restauree() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(msg("m1/trash")).respond_with(ok()).expect(1).mount(&server).await;
        Mock::given(method("POST")).and(msg("m1/untrash")).respond_with(ok()).expect(1).mount(&server).await;
        let e = env(&server);
        let p = pending(e.call("mail.delete", json!({"account": e.account, "id": "m1"})).await);
        assert!(p.text.details.contains("Jean Dupont <j@x.fr>") && p.text.details.contains("Point budget"), "{}", p.text.details);
        assert!(server.received_requests().await.unwrap().is_empty(), "rien avant confirmation");
        let (id, _) = done(e.broker.resolve(p.action_id, true).await);
        assert!(e.inbox().is_empty());
        e.broker.undo(id).await.unwrap();
        assert_eq!(e.inbox(), ["m1"]);
        server.verify().await;
    }

    #[tokio::test] async fn lecture_marque_lu_et_garde_le_corps_en_memoire() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(msg("m1")).and(query_param("format", "full"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(include_str!("../../tests/fixtures/gmail/full_18c1c.json"), "application/json"))
            .expect(1).mount(&server).await;
        Mock::given(method("POST")).and(msg("m1/modify")).and(body_json(json!({"addLabelIds": [], "removeLabelIds": ["UNREAD"]})))
            .respond_with(ok()).expect(1).mount(&server).await;
        Mock::given(method("POST")).and(msg("m1/modify")).and(body_json(json!({"addLabelIds": ["UNREAD"], "removeLabelIds": []})))
            .respond_with(ok()).expect(1).mount(&server).await;
        let e = env(&server);
        let a = json!({"account": e.account, "id": "m1"});
        let (id, out) = done(e.call("mail.read", a.clone()).await);
        assert!(out["html"].as_str().unwrap().contains("<b>compte rendu</b>"), "{out}");
        assert_eq!(out["attachments"][0]["filename"], "compte-rendu.pdf");
        assert!(!e.unread());
        assert!(!e.column("result", id).contains("compte rendu"), "corps jamais au journal");
        let (id2, again) = done(e.call("mail.read", a).await);
        assert_eq!(again, out, "seconde lecture depuis le cache mémoire");
        let last = &e.broker.journal(1)[0];
        assert!(last.id == id2 && !last.undoable, "déjà lu : rien à annuler");
        e.broker.undo(id).await.unwrap();
        assert!(e.unread());
        server.verify().await;
    }

    #[tokio::test] async fn lecture_tronquee_signalee_et_texte_borne() {
        use base64::Engine;
        let big = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode("a".repeat(html::MAX_INPUT + 10));
        let small = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode("court");
        let server = MockServer::start().await;
        for (id, data) in [("m1", big), ("m2", small)] {
            Mock::given(method("GET")).and(msg(id)).and(query_param("format", "full"))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": id, "threadId": "t", "payload": {"mimeType": "text/plain", "body": {"data": data}}})))
                .mount(&server).await;
        }
        Mock::given(method("POST")).respond_with(ok()).mount(&server).await;
        let e = env(&server);
        let (_, out) = done(e.call("mail.read", json!({"account": e.account, "id": "m1"})).await);
        assert_eq!(out["truncated"], true);
        assert_eq!(out["text"].as_str().unwrap().len(), html::MAX_INPUT, "texte borné comme le HTML");
        let (_, out) = done(e.call("mail.read", json!({"account": e.account, "id": "m2"})).await);
        assert_eq!((&out["truncated"], &out["text"]), (&json!(false), &json!("court")));
    }

    #[tokio::test] async fn compte_retire_ne_sert_plus_son_corps_en_cache() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(msg("m1")).and(query_param("format", "full"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(include_str!("../../tests/fixtures/gmail/full_18c1c.json"), "application/json"))
            .mount(&server).await;
        Mock::given(method("POST")).respond_with(ok()).mount(&server).await;
        let e = env(&server);
        let a = json!({"account": e.account, "id": "m1"});
        done(e.call("mail.read", a.clone()).await);
        accounts::remove(&e.db.lock(), &MemorySecrets::default(), e.account).unwrap();
        assert!(matches!(e.call("mail.read", a).await, ToolOutcome::Failed { .. }), "plus de corps pour un compte retiré");
    }

    #[tokio::test] async fn reponse_sans_message_id_garde_le_meme_sujet_que_la_carte() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(msg("m1")).and(query_param("format", "metadata"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(r#"{"id":"m1","threadId":"th1","payload":{"headers":[]}}"#, "application/json"))
            .mount(&server).await;
        Mock::given(method("POST")).and(path("/gmail/v1/users/me/messages/send")).respond_with(ok()).expect(1).mount(&server).await;
        let e = env(&server);
        let p = pending(e.call("mail.send", json!({"account": e.account, "to": "j@x.fr", "subject": " Point ", "body": "x", "reply_to_id": "m1"})).await);
        assert!(p.text.details.contains("Sujet : Re: Point\n"), "{}", p.text.details);
        done(e.broker.resolve(p.action_id, true).await);
        let sent = server.received_requests().await.unwrap().into_iter().find(|r| r.url.path().ends_with("/send")).unwrap();
        let v: Value = serde_json::from_slice(&sent.body).unwrap();
        let raw = String::from_utf8(mime::b64url(v["raw"].as_str().unwrap()).unwrap()).unwrap();
        assert!(raw.contains("Subject: Re: Point\r\n") && !raw.contains("In-Reply-To"), "{raw}");
    }

    #[tokio::test] async fn envoi_trop_long_annonce_le_refus() {
        let server = MockServer::start().await;
        let e = env(&server);
        let p = pending(e.call("mail.send", json!({"account": e.account, "to": "j@x.fr", "subject": "S", "body": "a".repeat(SEND_MAX + 1)})).await);
        assert_eq!(p.text.summary, "Texte trop long : l'envoi sera refusé");
        assert!(matches!(e.broker.resolve(p.action_id, true).await, ToolOutcome::Failed { .. }));
    }

    #[test] fn cache_lru_de_vingt() {
        let db: Db = Arc::new(Mutex::new(crate::test_util::db()));
        let ctx = Ctx {
            db: db.clone(), events: Arc::new(NoEvents),
            tokens: Arc::new(TokenCache::new(db, Arc::new(MemorySecrets::default()), Arc::new(NoEvents), reqwest::Client::new(), Arc::new(|_| None))),
            client: Client { http: reqwest::Client::new(), base: "http://127.0.0.1:9".into() }, opener: Arc::new(Fake::default()), bodies: Mutex::default(),
        };
        for i in 0..25 {
            ctx.remember((1, i.to_string()), json!(i));
        }
        assert_eq!(ctx.bodies.lock().len(), BODY_CACHE);
        assert!(ctx.cached(&(1, "4".into())).is_none());
        assert_eq!(ctx.cached(&(1, "5".into())), Some(json!(5)));
        ctx.remember((1, "x".into()), json!("x"));
        assert!(ctx.cached(&(1, "5".into())).is_some(), "relu : remis devant, pas évincé");
        assert!(ctx.cached(&(1, "6".into())).is_none());
    }

    #[tokio::test] async fn ouverture_dans_gmail_et_des_liens() {
        let server = MockServer::start().await;
        Mock::given(any()).respond_with(ok()).expect(0).mount(&server).await;
        let e = env(&server);
        done(e.call("mail.open_web", json!({"account": e.account, "id": "m1"})).await);
        assert!(matches!(e.call("mail.open_web", json!({"account": e.account, "id": "../x"})).await, ToolOutcome::Failed { .. }));
        assert!(matches!(e.call("mail.open_link", json!({"url": "javascript:alert(1)"})).await, ToolOutcome::Failed { .. }));
        done(e.call("mail.open_link", json!({"url": "https://exemple.fr/a"})).await);
        assert_eq!(*e.opener.0.lock(), ["https://mail.google.com/mail/u/moi%40gmail.com/#all/th1", "https://exemple.fr/a"]);
        pending(e.broker.call(ToolCall {
            tool: "mail.open_link".into(), args: json!({"url": "https://x.fr"}),
            provenance: Provenance::Untrusted { source: "mail:m1".into() }, origin: Origin::User,
        }).await);
        assert_eq!(e.opener.0.lock().len(), 2, "lien venu d'un contenu non fiable : confirmation d'abord");
        server.verify().await;
    }

    #[tokio::test] async fn liste_par_tri_et_compte() {
        let server = MockServer::start().await;
        let e = env(&server);
        let (_, out) = done(e.call("mail.list", json!({"bucket": "todo"})).await);
        assert_eq!(out[0]["subject"], "Point budget");
        let (_, out) = done(e.call("mail.list", json!({"bucket": "filtered"})).await);
        assert_eq!(out, json!([]));
        let (_, out) = done(e.call("mail.list", json!({"account": e.account + 1})).await);
        assert_eq!(out, json!([]));
        assert!(e.broker.journal(1)[0].result.as_ref().unwrap().get("redacted").is_some());
    }
}
