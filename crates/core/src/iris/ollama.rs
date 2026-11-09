//! Cerveau local : client HTTP d'Ollama (`/api/chat` en NDJSON, `/api/tags`, `/api/pull`, `/api/generate`).
//! Le journal ne contient jamais de texte de l'utilisateur ni de réponse du modèle.
use super::exec::PlannerTool;
use async_trait::async_trait;
use reqwest::Response;
use serde::Serialize;
use serde_json::{json, Value};
use std::time::Duration;

pub const DEFAULT_BASE: &str = "http://127.0.0.1:11434";
const TOTAL: Duration = Duration::from_secs(60);
const IDLE: Duration = Duration::from_secs(10);
/// Un téléchargement peut marquer des pauses (vérification des empreintes) : plus large qu'un flux de chat.
const PULL_IDLE: Duration = Duration::from_secs(120);
/// Requêtes courtes (`tags`, `unload`).
const SHORT: Duration = Duration::from_secs(5);
const NUM_CTX: u32 = 16384;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role { System, User, Assistant, Tool }

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCallMsg { pub name: String, pub args: Value }

#[derive(Debug, Clone, PartialEq)]
pub struct Msg { pub role: Role, pub content: String, pub tool_calls: Vec<ToolCallMsg> }

#[derive(Debug, Clone)]
pub struct ChatRequest { pub model: String, pub messages: Vec<Msg>, pub tools: Vec<PlannerTool>, pub keep_alive_min: u8 }

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChatReply { pub text: String, pub tool_calls: Vec<ToolCallMsg> }

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum BrainError {
    #[error("Ollama est indisponible")]
    Unavailable,
    #[error("modèle absent d'Ollama")]
    NoModel,
    #[error("Ollama ne répond pas assez vite")]
    Timeout,
    #[error("réponse d'Ollama inattendue : {0}")]
    Bad(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalStatus { Down, NoModel, Ready }

#[async_trait]
pub trait LocalBrain: Send + Sync {
    async fn chat(&self, req: ChatRequest, on_delta: &mut (dyn for<'a> FnMut(&'a str) + Send)) -> Result<ChatReply, BrainError>;
    async fn structured(&self, system: &str, content: &str, schema: &Value) -> Result<Value, BrainError>;
}

pub struct Ollama { http: reqwest::Client, base: String, model: String, keep_alive_min: u8, total: Duration, idle: Duration }

fn net(e: reqwest::Error) -> BrainError {
    if e.is_timeout() { BrainError::Timeout } else if e.is_connect() { BrainError::Unavailable } else { BrainError::Bad("réseau".into()) }
}

fn wire_msg(m: &Msg) -> Value {
    let mut v = json!({ "role": m.role, "content": m.content });
    if !m.tool_calls.is_empty() {
        v["tool_calls"] = m.tool_calls.iter().map(|c| json!({ "function": { "name": c.name, "arguments": c.args } })).collect();
    }
    v
}

fn tool_calls_of(msg: &Value, out: &mut Vec<ToolCallMsg>) {
    for c in msg["tool_calls"].as_array().into_iter().flatten() {
        let f = &c["function"];
        let Some(name) = f["name"].as_str() else { continue };
        let args = match &f["arguments"] {
            Value::String(s) => serde_json::from_str(s).unwrap_or(Value::Null),
            Value::Null => json!({}),
            v => v.clone(),
        };
        out.push(ToolCallMsg { name: name.into(), args });
    }
}

impl Ollama {
    pub fn new(model: &str, keep_alive_min: u8) -> Self { Self::with_base(DEFAULT_BASE, model, keep_alive_min, TOTAL, IDLE) }

    /// Adresse et délais injectables (tests).
    pub fn with_base(base: &str, model: &str, keep_alive_min: u8, total: Duration, idle: Duration) -> Self {
        let http = reqwest::Client::builder().no_proxy().build().unwrap_or_default();
        Self { http, base: base.trim_end_matches('/').into(), model: model.into(), keep_alive_min, total, idle }
    }

    /// Erreur HTTP : 404 « not found » = modèle absent.
    async fn check(resp: Response) -> Result<Response, BrainError> {
        let status = resp.status();
        if status.is_success() { return Ok(resp); }
        let body = resp.text().await.unwrap_or_default();
        if status.as_u16() == 404 && body.contains("not found") { return Err(BrainError::NoModel); }
        Err(BrainError::Bad(format!("HTTP {}", status.as_u16())))
    }

    /// Lit un flux NDJSON ligne par ligne ; `idle` sans octet reçu = `Timeout`.
    async fn each_line(mut resp: Response, idle: Duration, mut f: impl FnMut(Value) -> Result<(), BrainError>) -> Result<(), BrainError> {
        let mut buf: Vec<u8> = Vec::new();
        loop {
            let chunk = tokio::time::timeout(idle, resp.chunk()).await.map_err(|_| BrainError::Timeout)?.map_err(net)?;
            let done = chunk.is_none();
            if let Some(c) = chunk { buf.extend_from_slice(&c); }
            while let Some(i) = buf.iter().position(|&b| b == b'\n') {
                let line: Vec<u8> = buf.drain(..=i).collect();
                parse_line(&line, &mut f)?;
            }
            if done {
                parse_line(&buf, &mut f)?;
                return Ok(());
            }
        }
    }

    async fn chat_inner(&self, req: ChatRequest, on_delta: &mut (dyn for<'a> FnMut(&'a str) + Send)) -> Result<ChatReply, BrainError> {
        let mut body = json!({
            "model": req.model,
            "messages": req.messages.iter().map(wire_msg).collect::<Vec<_>>(),
            "stream": true,
            "think": false,
            "keep_alive": format!("{}m", req.keep_alive_min),
            "options": { "temperature": 0.2, "num_ctx": NUM_CTX },
        });
        if !req.tools.is_empty() {
            body["tools"] = req.tools.iter().map(|t| json!({ "type": "function", "function": { "name": t.wire, "description": t.description, "parameters": t.schema } })).collect();
        }
        let resp = Self::check(self.http.post(format!("{}/api/chat", self.base)).json(&body).send().await.map_err(net)?).await?;
        let mut reply = ChatReply::default();
        Self::each_line(resp, self.idle, |v| {
            let msg = &v["message"];
            if let Some(t) = msg["content"].as_str().filter(|t| !t.is_empty()) {
                on_delta(t);
                reply.text.push_str(t);
            }
            tool_calls_of(msg, &mut reply.tool_calls);
            Ok(())
        })
        .await?;
        Ok(reply)
    }

    async fn structured_inner(&self, system: &str, content: &str, schema: &Value) -> Result<Value, BrainError> {
        let body = json!({
            "model": self.model,
            "messages": [{ "role": "system", "content": system }, { "role": "user", "content": content }],
            "stream": false,
            "think": false,
            "format": schema,
            "keep_alive": format!("{}m", self.keep_alive_min),
            "options": { "temperature": 0.2, "num_ctx": NUM_CTX },
        });
        let resp = Self::check(self.http.post(format!("{}/api/chat", self.base)).json(&body).send().await.map_err(net)?).await?;
        let v: Value = resp.json().await.map_err(net)?;
        let text = v["message"]["content"].as_str().ok_or_else(|| BrainError::Bad("réponse vide".into()))?;
        serde_json::from_str(text).map_err(|_| BrainError::Bad("JSON invalide".into()))
    }

    /// Ollama absent = `Down` ; modèle non téléchargé = `NoModel` (`qwen3:8b` correspond aussi à `qwen3:8b` seul).
    pub async fn status(&self, model: &str) -> LocalStatus {
        let Ok(resp) = self.http.get(format!("{}/api/tags", self.base)).timeout(SHORT).send().await else { return LocalStatus::Down };
        let Ok(v) = resp.json::<Value>().await else { return LocalStatus::Down };
        let want = if model.contains(':') { model.to_string() } else { format!("{model}:latest") };
        let has = v["models"].as_array().into_iter().flatten().any(|m| m["name"].as_str() == Some(want.as_str()) || m["model"].as_str() == Some(want.as_str()));
        if has { LocalStatus::Ready } else { LocalStatus::NoModel }
    }

    /// Libère la mémoire ; silencieux si Ollama est absent.
    pub async fn unload(&self, model: &str) {
        let _ = self.http.post(format!("{}/api/generate", self.base)).timeout(SHORT).json(&json!({ "model": model, "keep_alive": 0 })).send().await;
    }

    /// Télécharge le modèle ; `on_progress(téléchargé, total)` à chaque ligne qui en porte un.
    pub async fn pull(&self, model: &str, mut on_progress: impl FnMut(u64, u64)) -> Result<(), BrainError> {
        let send = self.http.post(format!("{}/api/pull", self.base)).json(&json!({ "model": model, "stream": true })).send();
        let resp = tokio::time::timeout(PULL_IDLE, send).await.map_err(|_| BrainError::Timeout)?.map_err(net)?;
        Self::each_line(Self::check(resp).await?, PULL_IDLE, |v| {
            if let (Some(done), Some(total)) = (v["completed"].as_u64(), v["total"].as_u64()) { on_progress(done, total); }
            Ok(())
        })
        .await
    }
}

fn parse_line(line: &[u8], f: &mut impl FnMut(Value) -> Result<(), BrainError>) -> Result<(), BrainError> {
    if line.iter().all(u8::is_ascii_whitespace) { return Ok(()); }
    let v: Value = serde_json::from_slice(line).map_err(|_| BrainError::Bad("ligne illisible".into()))?;
    if let Some(e) = v["error"].as_str() {
        return Err(if e.contains("not found") { BrainError::NoModel } else { BrainError::Bad("erreur d'Ollama".into()) });
    }
    f(v)
}

#[async_trait]
impl LocalBrain for Ollama {
    async fn chat(&self, req: ChatRequest, on_delta: &mut (dyn for<'a> FnMut(&'a str) + Send)) -> Result<ChatReply, BrainError> {
        tokio::time::timeout(self.total, self.chat_inner(req, on_delta)).await.map_err(|_| BrainError::Timeout)?
    }
    async fn structured(&self, system: &str, content: &str, schema: &Value) -> Result<Value, BrainError> {
        tokio::time::timeout(self.total, self.structured_inner(system, content, schema)).await.map_err(|_| BrainError::Timeout)?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn req(tools: Vec<PlannerTool>) -> ChatRequest {
        ChatRequest { model: "qwen3:8b".into(), messages: vec![Msg { role: Role::User, content: "salut".into(), tool_calls: vec![] }], tools, keep_alive_min: 5 }
    }
    /// Vrai Ollama local (R8 : hors des tests courants). `ONIRIS_TEST_MODEL` change le modèle (défaut `qwen3:14b`).
    /// Un premier tour charge le modèle, le second est mesuré : budget de 1 s, modèle chargé.
    #[tokio::test]
    #[ignore]
    async fn iris_latence_reelle() {
        let model = std::env::var("ONIRIS_TEST_MODEL").unwrap_or_else(|_| "qwen3:14b".into());
        let brain = Ollama::new(&model, 5);
        let tool = PlannerTool {
            wire: "music__play_playlist".into(),
            description: "Lance une playlist par son nom.".into(),
            schema: json!({ "type": "object", "properties": { "name": { "type": "string" } }, "required": ["name"] }),
        };
        let ask = |text: &str| ChatRequest {
            model: model.clone(),
            messages: vec![Msg { role: Role::User, content: text.into(), tool_calls: vec![] }],
            tools: vec![tool.clone()],
            keep_alive_min: 5,
        };
        brain.chat(ask("Bonjour"), &mut |_| {}).await.expect("chargement du modèle");
        let t = std::time::Instant::now();
        let r = brain.chat(ask("Mets la musique de sport"), &mut |_| {}).await.expect("tour mesuré");
        let took = t.elapsed();
        println!("iris_latence_reelle ({model}) : {took:?}, appels d'outils : {:?}", r.tool_calls);
        assert!(took < Duration::from_secs(1), "{took:?}");
    }

    /// Vraie chaîne voix (R8 : hors des tests courants). `ONIRIS_TEST_WAV` : WAV 16 kHz mono « mets la musique en pause » ;
    /// `ONIRIS_TEST_WHISPER` : whisper-server déjà lancé (défaut http://127.0.0.1:8178) ; `ONIRIS_TEST_MODEL` : modèle Ollama.
    /// Un tour de chauffe charge Whisper et le modèle, le second tour (transcription puis appel d'outil) est mesuré : moins de 1 s.
    #[tokio::test]
    #[ignore]
    async fn voix_latence_reelle() {
        let wav = std::fs::read(std::env::var("ONIRIS_TEST_WAV").expect("ONIRIS_TEST_WAV : chemin du WAV de test")).expect("lecture du WAV");
        let whisper = std::env::var("ONIRIS_TEST_WHISPER").unwrap_or_else(|_| "http://127.0.0.1:8178".into());
        let model = std::env::var("ONIRIS_TEST_MODEL").unwrap_or_else(|_| "qwen3:14b".into());
        let brain = Ollama::new(&model, 5);
        let http = reqwest::Client::new();
        let tool = PlannerTool {
            wire: "music__toggle".into(),
            description: "Met la musique en pause ou la relance.".into(),
            schema: json!({ "type": "object", "properties": {} }),
        };
        let run = |text: String| ChatRequest {
            model: model.clone(),
            messages: vec![Msg { role: Role::User, content: text, tool_calls: vec![] }],
            tools: vec![tool.clone()],
            keep_alive_min: 5,
        };
        let heard = crate::voice::stt::transcribe(&http, &whisper, wav.clone()).await.expect("chauffe Whisper");
        brain.chat(run(heard), &mut |_| {}).await.expect("chauffe du modèle");
        let t = std::time::Instant::now();
        let heard = crate::voice::stt::transcribe(&http, &whisper, wav).await.expect("transcription");
        let heard = crate::voice::stt::clean(&heard).expect("aucune parole reconnue");
        let r = brain.chat(run(heard), &mut |_| {}).await.expect("tour mesuré");
        let took = t.elapsed();
        println!("voix_latence_reelle ({model}) : {took:?}, appels d'outils : {:?}", r.tool_calls);
        assert!(!r.tool_calls.is_empty(), "aucun appel d'outil");
        assert!(took < Duration::from_secs(1), "{took:?}");
    }

    fn client(s: &MockServer) -> Ollama { Ollama::with_base(&s.uri(), "qwen3:8b", 7, Duration::from_secs(5), Duration::from_secs(5)) }
    fn ndjson(lines: &[Value]) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_string(lines.iter().map(|l| format!("{l}\n")).collect::<String>())
    }

    #[tokio::test]
    async fn flux_en_trois_morceaux_et_requete_conforme() {
        let s = MockServer::start().await;
        Mock::given(method("POST")).and(path("/api/chat"))
            .and(body_partial_json(json!({ "think": false, "keep_alive": "5m", "stream": true, "options": { "num_ctx": 16384 },
                "tools": [{ "type": "function", "function": { "name": "a__b", "description": "d", "parameters": { "type": "object" } } }] })))
            .respond_with(ndjson(&[
                json!({ "message": { "role": "assistant", "content": "Bon" }, "done": false }),
                json!({ "message": { "role": "assistant", "content": "jour" }, "done": false }),
                json!({ "message": { "role": "assistant", "content": "", "tool_calls": [{ "function": { "name": "a__b", "arguments": { "x": 1 } } }] }, "done": true }),
            ]))
            .mount(&s).await;
        let mut got = Vec::new();
        let tool = PlannerTool { wire: "a__b".into(), description: "d".into(), schema: json!({ "type": "object" }) };
        let r = client(&s).chat(req(vec![tool]), &mut |d| got.push(d.to_string())).await.unwrap();
        assert_eq!(got, ["Bon", "jour"]);
        assert_eq!(r.text, "Bonjour");
        assert_eq!(r.tool_calls, [ToolCallMsg { name: "a__b".into(), args: json!({ "x": 1 }) }]);
    }

    #[tokio::test]
    async fn modele_absent() {
        let s = MockServer::start().await;
        Mock::given(method("POST")).respond_with(ResponseTemplate::new(404).set_body_string(r#"{"error":"model 'x' not found"}"#)).mount(&s).await;
        assert_eq!(client(&s).chat(req(vec![]), &mut |_| {}).await.unwrap_err(), BrainError::NoModel);
    }

    #[tokio::test]
    async fn port_ferme() {
        let port = { let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap(); l.local_addr().unwrap().port() };
        let o = Ollama::with_base(&format!("http://127.0.0.1:{port}"), "m", 5, Duration::from_secs(5), Duration::from_secs(5));
        assert_eq!(o.chat(req(vec![]), &mut |_| {}).await.unwrap_err(), BrainError::Unavailable);
        assert_eq!(o.status("m").await, LocalStatus::Down);
        o.unload("m").await; // sans erreur
    }

    #[tokio::test]
    async fn flux_muet_donne_timeout() {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move {
            let (mut sock, _) = l.accept().await.unwrap();
            let mut b = [0u8; 4096];
            let _ = tokio::io::AsyncReadExt::read(&mut sock, &mut b).await;
            let line = format!("{}\n", json!({ "message": { "content": "a" }, "done": false }));
            let head = format!("HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n{:x}\r\n{line}\r\n", line.len());
            sock.write_all(head.as_bytes()).await.unwrap();
            tokio::time::sleep(Duration::from_secs(3)).await; // puis plus rien
        });
        let o = Ollama::with_base(&base, "m", 5, Duration::from_secs(5), Duration::from_millis(200));
        let mut got = Vec::new();
        assert_eq!(o.chat(req(vec![]), &mut |d| got.push(d.to_string())).await.unwrap_err(), BrainError::Timeout);
        assert_eq!(got, ["a"]);
    }

    #[tokio::test]
    async fn delai_total() {
        let s = MockServer::start().await;
        Mock::given(method("POST")).respond_with(ndjson(&[json!({ "done": true })]).set_delay(Duration::from_secs(2))).mount(&s).await;
        let o = Ollama::with_base(&s.uri(), "m", 5, Duration::from_millis(200), Duration::from_secs(5));
        assert_eq!(o.chat(req(vec![]), &mut |_| {}).await.unwrap_err(), BrainError::Timeout);
    }

    #[tokio::test]
    async fn structure_envoie_le_format() {
        let s = MockServer::start().await;
        let schema = json!({ "type": "object", "properties": { "n": { "type": "integer" } } });
        Mock::given(method("POST")).and(path("/api/chat")).and(body_partial_json(json!({ "format": schema, "stream": false, "think": false, "keep_alive": "7m" })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "message": { "content": "{\"n\":3}" }, "done": true })))
            .mount(&s).await;
        assert_eq!(client(&s).structured("sys", "x", &schema).await.unwrap(), json!({ "n": 3 }));
    }

    #[tokio::test]
    async fn statut_trois_cas() {
        let s = MockServer::start().await;
        Mock::given(method("GET")).and(path("/api/tags"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "models": [{ "name": "qwen3:8b" }] }))).mount(&s).await;
        let o = client(&s);
        assert_eq!(o.status("qwen3:8b").await, LocalStatus::Ready);
        assert_eq!(o.status("autre:1b").await, LocalStatus::NoModel);
        let port = { let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap(); l.local_addr().unwrap().port() };
        let down = Ollama::with_base(&format!("http://127.0.0.1:{port}"), "m", 5, TOTAL, IDLE);
        assert_eq!(down.status("m").await, LocalStatus::Down);
    }

    #[tokio::test]
    async fn pull_lit_la_progression() {
        let s = MockServer::start().await;
        Mock::given(method("POST")).and(path("/api/pull")).and(body_partial_json(json!({ "model": "qwen3:8b" })))
            .respond_with(ndjson(&[json!({ "status": "pulling manifest" }), json!({ "status": "pulling", "total": 100, "completed": 40 }), json!({ "status": "pulling", "total": 100, "completed": 100 }), json!({ "status": "success" })]))
            .mount(&s).await;
        let mut seen = Vec::new();
        client(&s).pull("qwen3:8b", |d, t| seen.push((d, t))).await.unwrap();
        assert_eq!(seen, [(40, 100), (100, 100)]);
    }

    #[tokio::test]
    async fn pull_erreur_en_cours_de_flux() {
        let s = MockServer::start().await;
        Mock::given(method("POST")).and(path("/api/pull"))
            .respond_with(ndjson(&[json!({ "status": "pulling", "total": 100, "completed": 10 }), json!({ "error": "disque plein" })]))
            .mount(&s).await;
        assert!(matches!(client(&s).pull("qwen3:8b", |_, _| {}).await, Err(BrainError::Bad(_))));
    }

    #[tokio::test]
    async fn unload_envoie_keep_alive_zero() {
        let s = MockServer::start().await;
        Mock::given(method("POST")).and(path("/api/generate")).and(body_partial_json(json!({ "model": "qwen3:8b", "keep_alive": 0 })))
            .respond_with(ResponseTemplate::new(200)).expect(1).mount(&s).await;
        client(&s).unload("qwen3:8b").await;
    }
}
