//! Client de l'API Gmail (REST v1). Erreurs sans URL ni contenu : seulement le statut HTTP.
use super::mime;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

pub const BASE: &str = "https://gmail.googleapis.com";
/// Requêtes de métadonnées en parallèle, au plus.
const CONCURRENCY: usize = 4;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum GmailError {
    /// 404 sur l'historique : `historyId` trop ancien, resynchronisation complète.
    #[error("historique expiré")]
    HistoryGone,
    #[error("message introuvable")]
    NotFound,
    /// 401 : jeton d'accès refusé.
    #[error("jeton refusé")]
    Unauthorized,
    /// Transport, 429 ou 5xx : réessayer plus tard.
    #[error("réseau indisponible")]
    Network,
    #[error("Gmail refuse (HTTP {0})")]
    Status(u16),
    #[error("réponse Gmail illisible")]
    Parse,
}

#[derive(Clone)]
pub struct Client {
    pub http: reqwest::Client,
    /// `https://gmail.googleapis.com` ; les tests le pointent sur un faux serveur.
    pub base: String,
}

/// En-têtes utiles d'un mail (`format=metadata`). Sujet et expéditeur : contenu utilisateur, jamais journalisés.
#[derive(Clone, PartialEq)]
pub struct MetaMsg {
    pub id: String,
    pub thread_id: String,
    pub labels: Vec<String>,
    pub snippet: String,
    pub history_id: String,
    pub date_ms: i64,
    pub from_name: String,
    pub from_addr: String,
    pub subject: String,
    pub list_unsubscribe: bool,
}

impl std::fmt::Debug for MetaMsg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MetaMsg").field("id", &self.id).field("labels", &self.labels).finish_non_exhaustive()
    }
}

/// Ce qui a changé depuis `start`, déjà résumé par message (le dernier état gagne).
#[derive(Debug, Default, PartialEq)]
pub struct HistoryDelta {
    /// Entrés dans la boîte de réception (ou à relire) : métadonnées à récupérer.
    pub added: Vec<String>,
    /// Sortis de la boîte de réception (archivés, à la corbeille, supprimés).
    pub removed: Vec<String>,
    /// Libellés changés (lu, non lu) : id et libellés courants.
    pub label_changed: Vec<(String, Vec<String>)>,
    pub new_history_id: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AttachmentMeta {
    pub filename: String,
    pub mime_type: String,
    pub size: u64,
    pub attachment_id: Option<String>,
}

/// Corps décodé (`format=full`) : premier `text/html` et premier `text/plain`, pièces jointes décrites.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct FullMsg {
    pub html: Option<String>,
    pub text: Option<String>,
    pub attachments: Vec<AttachmentMeta>,
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct Part {
    mime_type: String,
    filename: String,
    headers: Vec<Header>,
    body: Body,
    parts: Vec<Part>,
}

#[derive(Deserialize, Default)]
struct Header { name: String, value: String }

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct Body { size: u64, data: Option<String>, attachment_id: Option<String> }

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct RawMsg {
    id: String,
    thread_id: String,
    label_ids: Vec<String>,
    snippet: String,
    history_id: String,
    internal_date: String,
    payload: Part,
}

#[derive(PartialEq)]
enum Change { In, Out, Labels(Vec<String>) }

fn header<'a>(p: &'a Part, name: &str) -> Option<&'a str> {
    p.headers.iter().find(|h| h.name.eq_ignore_ascii_case(name)).map(|h| h.value.as_str())
}

fn charset(p: &Part) -> String {
    header(p, "Content-Type")
        .and_then(|ct| ct.split(';').find_map(|kv| kv.trim().strip_prefix("charset=").or_else(|| kv.trim().strip_prefix("CHARSET="))))
        .map(|c| c.trim_matches('"').to_string())
        .unwrap_or_default()
}

fn walk(p: &Part, out: &mut FullMsg) {
    if !p.filename.is_empty() {
        out.attachments.push(AttachmentMeta {
            filename: p.filename.clone(),
            mime_type: p.mime_type.clone(),
            size: p.body.size,
            attachment_id: p.body.attachment_id.clone(),
        });
        return;
    }
    let slot = match p.mime_type.to_ascii_lowercase().as_str() {
        "text/html" => &mut out.html,
        "text/plain" => &mut out.text,
        _ => {
            for c in &p.parts {
                walk(c, out);
            }
            return;
        }
    };
    if slot.is_none() {
        if let Some(bytes) = p.body.data.as_deref().and_then(mime::b64url) {
            *slot = Some(mime::to_text(&bytes, &charset(p)));
        }
    }
}

/// Les ids Gmail sont alphanumériques : rien d'autre n'entre dans le chemin.
fn msg_path(id: &str, suffix: &str) -> Result<String, GmailError> {
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(GmailError::NotFound);
    }
    Ok(format!("messages/{id}{suffix}"))
}

fn status_error(s: reqwest::StatusCode) -> GmailError {
    match s.as_u16() {
        401 => GmailError::Unauthorized,
        404 => GmailError::NotFound,
        429 | 500..=599 => GmailError::Network,
        n => GmailError::Status(n),
    }
}

impl Client {
    pub fn new(http: reqwest::Client) -> Self { Self { http, base: BASE.into() } }

    fn url(&self, path: &str) -> String { format!("{}/gmail/v1/users/me/{path}", self.base.trim_end_matches('/')) }

    async fn send_json(&self, req: reqwest::RequestBuilder) -> Result<Value, GmailError> {
        let resp = req.send().await.map_err(|_| GmailError::Network)?;
        if !resp.status().is_success() {
            return Err(status_error(resp.status()));
        }
        resp.json().await.map_err(|_| GmailError::Parse)
    }

    async fn get<T: serde::de::DeserializeOwned>(&self, token: &str, path: &str, query: &[(&str, &str)]) -> Result<T, GmailError> {
        let v = self.send_json(self.http.get(self.url(path)).bearer_auth(token).query(query)).await?;
        serde_json::from_value(v).map_err(|_| GmailError::Parse)
    }

    async fn post(&self, token: &str, path: &str, body: Value) -> Result<(), GmailError> {
        self.send_json(self.http.post(self.url(path)).bearer_auth(token).json(&body)).await.map(drop)
    }

    /// Ids des `max` derniers mails de la boîte de réception (100 au plus : une page).
    pub async fn list_inbox(&self, token: &str, max: u32) -> Result<Vec<String>, GmailError> {
        #[derive(Deserialize, Default)]
        #[serde(default)]
        struct List { messages: Vec<Id> }
        #[derive(Deserialize)]
        struct Id { id: String }
        let max = max.min(100).to_string();
        let l: List = self.get(token, "messages", &[("labelIds", "INBOX"), ("maxResults", &max)]).await?;
        Ok(l.messages.into_iter().map(|m| m.id).collect())
    }

    pub async fn get_metadata(&self, token: &str, id: &str) -> Result<MetaMsg, GmailError> {
        let q = [
            ("format", "metadata"),
            ("metadataHeaders", "From"),
            ("metadataHeaders", "Subject"),
            ("metadataHeaders", "Date"),
            ("metadataHeaders", "List-Unsubscribe"),
        ];
        let m: RawMsg = self.get(token, &msg_path(id, "")?, &q).await?;
        let (from_name, from_addr) = mime::parse_from(header(&m.payload, "From").unwrap_or(""));
        Ok(MetaMsg {
            subject: mime::decode_header(header(&m.payload, "Subject").unwrap_or("")).trim().to_string(),
            list_unsubscribe: header(&m.payload, "List-Unsubscribe").is_some(),
            snippet: mime::unescape_snippet(&m.snippet),
            date_ms: m.internal_date.parse().unwrap_or(0),
            id: m.id,
            thread_id: m.thread_id,
            labels: m.label_ids,
            history_id: m.history_id,
            from_name,
            from_addr,
        })
    }

    /// Métadonnées de plusieurs mails, `CONCURRENCY` requêtes à la fois ; résultats dans l'ordre des ids.
    pub async fn get_metadata_many(&self, token: &str, ids: &[String]) -> Vec<Result<MetaMsg, GmailError>> {
        let next = std::sync::atomic::AtomicUsize::new(0);
        let results = parking_lot::Mutex::new(HashMap::new());
        let worker = || async {
            loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let Some(id) = ids.get(i) else { break };
                let r = self.get_metadata(token, id).await;
                results.lock().insert(i, r);
            }
        };
        const _: () = assert!(CONCURRENCY == 4, "un worker par requête parallèle");
        tokio::join!(worker(), worker(), worker(), worker());
        let mut map = results.into_inner();
        (0..ids.len()).map(|i| map.remove(&i).unwrap_or(Err(GmailError::Network))).collect()
    }

    /// Changements depuis `start`, toutes pages lues. 404 : `HistoryGone`.
    pub async fn history(&self, token: &str, start: &str) -> Result<HistoryDelta, GmailError> {
        #[derive(Deserialize, Default)]
        #[serde(default, rename_all = "camelCase")]
        struct Page { history: Vec<Value>, next_page_token: Option<String>, history_id: String }
        // Dernier état de chaque message, dans l'ordre de première apparition.
        let mut order: Vec<String> = Vec::new();
        let mut state: HashMap<String, Change> = HashMap::new();
        let mut page_token: Option<String> = None;
        let new_history_id = loop {
            let mut q = vec![("startHistoryId", start.to_string())];
            if let Some(t) = &page_token {
                q.push(("pageToken", t.clone()));
            }
            let q: Vec<(&str, &str)> = q.iter().map(|(k, v)| (*k, v.as_str())).collect();
            let page: Page = self.get(token, "history", &q).await.map_err(|e| if e == GmailError::NotFound { GmailError::HistoryGone } else { e })?;
            for rec in &page.history {
                for (kind, list) in rec.as_object().into_iter().flatten() {
                    let Some(list) = list.as_array() else { continue };
                    for entry in list {
                        let msg = &entry["message"];
                        let Some(id) = msg["id"].as_str() else { continue };
                        let labels: Vec<String> = msg["labelIds"].as_array().into_iter().flatten().filter_map(|l| l.as_str().map(String::from)).collect();
                        let changed: Vec<&str> = entry["labelIds"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
                        let has = |l: &str| labels.iter().any(|x| x == l);
                        let in_box = has("INBOX") && !has("TRASH") && !has("SPAM");
                        let inbox = if in_box { Change::In } else { Change::Out };
                        let new = match kind.as_str() {
                            "messagesAdded" if in_box => Change::In,
                            "messagesDeleted" => Change::Out,
                            "labelsAdded" | "labelsRemoved" if changed.iter().any(|l| ["INBOX", "TRASH", "SPAM"].contains(l)) => inbox,
                            "labelsAdded" | "labelsRemoved" if changed.contains(&"UNREAD") => {
                                if in_box { Change::Labels(labels) } else { Change::Out }
                            }
                            _ => continue,
                        };
                        let slot = state.entry(id.to_string()).or_insert_with(|| { order.push(id.to_string()); Change::Labels(vec![]) });
                        // Après une entrée, un changement de libellés ne compte pas : le mail sera relu en entier.
                        if !(*slot == Change::In && matches!(new, Change::Labels(_))) {
                            *slot = new;
                        }
                    }
                }
            }
            match page.next_page_token {
                Some(t) if !t.is_empty() => page_token = Some(t),
                _ => break page.history_id,
            }
        };
        let mut d = HistoryDelta { new_history_id, ..Default::default() };
        for id in order {
            match state.remove(&id) {
                Some(Change::In) => d.added.push(id),
                Some(Change::Out) => d.removed.push(id),
                Some(Change::Labels(labels)) => d.label_changed.push((id, labels)),
                None => {}
            }
        }
        Ok(d)
    }

    pub async fn get_full(&self, token: &str, id: &str) -> Result<FullMsg, GmailError> {
        let m: RawMsg = self.get(token, &msg_path(id, "")?, &[("format", "full")]).await?;
        let mut out = FullMsg::default();
        walk(&m.payload, &mut out);
        Ok(out)
    }

    pub async fn modify(&self, token: &str, id: &str, add: &[&str], remove: &[&str]) -> Result<(), GmailError> {
        self.post(token, &msg_path(id, "/modify")?, json!({ "addLabelIds": add, "removeLabelIds": remove })).await
    }

    pub async fn trash(&self, token: &str, id: &str) -> Result<(), GmailError> {
        self.post(token, &msg_path(id, "/trash")?, json!({})).await
    }

    pub async fn untrash(&self, token: &str, id: &str) -> Result<(), GmailError> {
        self.post(token, &msg_path(id, "/untrash")?, json!({})).await
    }

    /// `raw_b64url` : message RFC 822 complet, encodé base64url.
    pub async fn send(&self, token: &str, raw_b64url: &str, thread_id: Option<&str>) -> Result<(), GmailError> {
        let mut body = json!({ "raw": raw_b64url });
        if let Some(t) = thread_id {
            body["threadId"] = json!(t);
        }
        self.post(token, "messages/send", body).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_json, header as has_header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn client(server: &MockServer) -> Client { Client { base: server.uri(), ..Client::new(reqwest::Client::new()) } }
    fn body(s: &str) -> ResponseTemplate { ResponseTemplate::new(200).set_body_raw(s.to_string(), "application/json") }

    #[tokio::test] async fn corps_complet_et_pieces_jointes() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/gmail/v1/users/me/messages/18c1c")).and(query_param("format", "full")).and(has_header("authorization", "Bearer t"))
            .respond_with(body(include_str!("../../tests/fixtures/gmail/full_18c1c.json"))).mount(&server).await;
        Mock::given(method("GET")).and(path("/gmail/v1/users/me/messages/18c1d")).and(query_param("format", "full"))
            .respond_with(body(include_str!("../../tests/fixtures/gmail/full_latin1.json"))).mount(&server).await;
        let c = client(&server);
        let m = c.get_full("t", "18c1c").await.unwrap();
        assert_eq!(m.text.as_deref(), Some("Bonjour, voici le compte rendu.\r\n"));
        assert!(m.html.unwrap().contains("<b>compte rendu</b>"));
        assert_eq!(m.attachments, [AttachmentMeta {
            filename: "compte-rendu.pdf".into(), mime_type: "application/pdf".into(), size: 20480, attachment_id: Some("ANGjdJ8abc".into()),
        }]);
        let l = c.get_full("t", "18c1d").await.unwrap();
        assert_eq!((l.text.as_deref(), l.html), (Some("Résumé en pièce jointe."), None), "partie unique en latin-1");
        assert_eq!(c.get_full("t", "absent").await, Err(GmailError::NotFound));
        assert_eq!(c.get_full("t", "../../profile").await, Err(GmailError::NotFound), "id hors format refusé sans requête");
    }

    #[tokio::test] async fn actions() {
        let server = MockServer::start().await;
        let p = |s: &str| path(format!("/gmail/v1/users/me/messages/{s}"));
        Mock::given(method("POST")).and(p("m1/modify")).and(body_json(serde_json::json!({"addLabelIds": [], "removeLabelIds": ["UNREAD"]})))
            .respond_with(body("{}")).expect(1).mount(&server).await;
        Mock::given(method("POST")).and(p("m1/trash")).respond_with(body("{}")).expect(1).mount(&server).await;
        Mock::given(method("POST")).and(p("m1/untrash")).respond_with(body("{}")).expect(1).mount(&server).await;
        Mock::given(method("POST")).and(p("send")).and(body_json(serde_json::json!({"raw": "UkFX", "threadId": "th"})))
            .respond_with(body("{}")).expect(1).mount(&server).await;
        Mock::given(method("POST")).and(p("m2/trash")).respond_with(ResponseTemplate::new(403)).mount(&server).await;
        let c = client(&server);
        c.modify("t", "m1", &[], &["UNREAD"]).await.unwrap();
        c.trash("t", "m1").await.unwrap();
        c.untrash("t", "m1").await.unwrap();
        c.send("t", "UkFX", Some("th")).await.unwrap();
        assert_eq!(c.trash("t", "m2").await, Err(GmailError::Status(403)));
        server.verify().await;
    }

    #[tokio::test] async fn metadonnees_par_lots_de_quatre_dans_l_ordre() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/gmail/v1/users/me/messages/18c1a"))
            .respond_with(body(include_str!("../../tests/fixtures/gmail/meta_18c1a.json")).set_delay(std::time::Duration::from_millis(150)))
            .mount(&server).await;
        let c = client(&server);
        let ids: Vec<String> = ["18c1a", "absent", "18c1a", "18c1a", "18c1a"].map(String::from).to_vec();
        let t = std::time::Instant::now();
        let r = c.get_metadata_many("t", &ids).await;
        assert!(t.elapsed() < std::time::Duration::from_millis(600), "en parallèle");
        assert_eq!(r[1], Err(GmailError::NotFound));
        assert_eq!(r[0].as_ref().unwrap().subject, "Réunion demain à 10 h");
        assert!(r[0].as_ref().unwrap().labels.contains(&"UNREAD".to_string()));
        assert_eq!(r.len(), 5);
    }
}
