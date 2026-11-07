//! Côté app du serveur MCP d'Oniris : un tube nommé que relaie `oniris.exe --mcp-relay` (lancé par Claude Code).
//! Chaque appel passe par l'exécuteur face planificateur, avec `Origin::Mcp` : mêmes confirmations, provenance et journal.
use oniris_core::iris::Executor;
use oniris_core::tools::Origin;
use serde_json::{json, Value};
use std::io;
use std::sync::Arc;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use tokio::task::JoinHandle;

/// Taille maximale d'une ligne du protocole (octets, sans le `\n`).
pub const LINE_MAX: usize = 1 << 20;

/// Session d'un tour de Claude : la lâcher arrête le tube.
pub struct McpSession { pub pipe: String, pub token: String, pub task: JoinHandle<()> }

impl Drop for McpSession {
    fn drop(&mut self) { self.task.abort(); }
}

fn hex(len: usize) -> String {
    let mut b = vec![0u8; len / 2];
    getrandom::fill(&mut b).expect("générateur aléatoire du système");
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Comparaison en temps constant (longueur exceptée).
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Une ligne sans son `\n`, au plus `LINE_MAX` octets ; `None` à la fin du flux.
pub async fn read_line<R: AsyncBufRead + Unpin>(r: &mut R) -> io::Result<Option<String>> {
    let mut buf = Vec::new();
    if r.take(LINE_MAX as u64 + 1).read_until(b'\n', &mut buf).await? == 0 {
        return Ok(None);
    }
    if buf.pop() != Some(b'\n') {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "ligne trop longue ou tronquée"));
    }
    String::from_utf8(buf).map(Some).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "ligne non UTF-8"))
}

/// Ouvre le tube (une seule instance, clients distants refusés) et sert une seule connexion.
pub fn serve(exec: Arc<Executor>) -> io::Result<McpSession> {
    let exec = Arc::new(Executor {
        broker: exec.broker.clone(),
        vault: exec.vault.clone(),
        origin: Origin::Mcp,
        on_pending: exec.on_pending.clone(),
        virtuals: exec.virtuals.clone(),
    });
    let pipe = format!(r"\\.\pipe\oniris-{}", hex(16));
    let token = hex(32);
    // `max_instances(1)` : personne d'autre ne peut ouvrir une seconde instance de ce nom.
    let server = ServerOptions::new().first_pipe_instance(true).reject_remote_clients(true).max_instances(1).create(&pipe)?;
    let task = tokio::spawn(session(server, exec, token.clone()));
    Ok(McpSession { pipe, token, task })
}

async fn session(server: NamedPipeServer, exec: Arc<Executor>, token: String) {
    if server.connect().await.is_err() {
        return;
    }
    let (r, mut w) = tokio::io::split(server);
    let mut r = BufReader::new(r);
    while let Ok(Some(line)) = read_line(&mut r).await {
        let req: Value = serde_json::from_str(&line).unwrap_or_default();
        let allowed = req["token"].as_str().is_some_and(|t| same(t, &token));
        let resp = match req["op"].as_str() {
            _ if !allowed => json!({ "error": "refusé" }),
            Some("list") => {
                let tools: Vec<Value> = exec.tools(&["iris.ask_claude"]).into_iter()
                    .map(|t| json!({ "name": t.wire, "description": t.description, "input_schema": t.schema }))
                    .collect();
                json!({ "tools": tools })
            }
            Some("call") => {
                let out = exec.call(req["name"].as_str().unwrap_or_default(), req["args"].clone()).await;
                json!({ "text": out.to_string(), "is_error": out.get("error").is_some() })
            }
            _ => json!({ "error": "opération inconnue" }),
        };
        if w.write_all(format!("{resp}\n").as_bytes()).await.is_err() || !allowed {
            break;
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use oniris_core::events::{Events, NoEvents};
    use oniris_core::iris::Vault;
    use oniris_core::tools::{Brain, PendingAction, Provenance, ToolBroker};
    use parking_lot::Mutex;
    use tokio::io::{ReadHalf, WriteHalf};
    use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient};

    /// Exécuteur sur les outils des notes, base migrée dans `dir`.
    pub(crate) fn exec(dir: &std::path::Path) -> Arc<Executor> {
        let mut c = oniris_core::open_db(&dir.join("t.db")).unwrap();
        oniris_core::migrate(&mut c, oniris_core::CORE_MIGRATIONS).unwrap();
        let db = Arc::new(Mutex::new(c));
        let events: Arc<dyn Events> = Arc::new(NoEvents);
        let broker = Arc::new(ToolBroker::new(db.clone(), Arc::new(|| 0), oniris_core::notes::tools::all(&db, &events)).unwrap());
        Arc::new(Executor {
            broker, vault: Arc::new(Mutex::new(Vault::default())), origin: Origin::Assistant { brain: Brain::Claude },
            on_pending: Arc::new(|_: &PendingAction| {}), virtuals: None,
        })
    }

    type Conn = (BufReader<ReadHalf<NamedPipeClient>>, WriteHalf<NamedPipeClient>);

    fn connect(pipe: &str) -> Conn {
        let (r, w) = tokio::io::split(ClientOptions::new().open(pipe).unwrap());
        (BufReader::new(r), w)
    }

    async fn ask((r, w): &mut Conn, req: Value) -> Option<Value> {
        w.write_all(format!("{req}\n").as_bytes()).await.ok()?;
        read_line(r).await.ok()?.map(|l| serde_json::from_str(&l).unwrap())
    }

    #[tokio::test]
    async fn jeton_faux_refuse_et_connexion_fermee() {
        let d = tempfile::tempdir().unwrap();
        let s = serve(exec(d.path())).unwrap();
        let mut c = connect(&s.pipe);
        assert_eq!(ask(&mut c, json!({"token": "x".repeat(32), "op": "list"})).await, Some(json!({"error": "refusé"})));
        assert_eq!(ask(&mut c, json!({"token": s.token, "op": "list"})).await, None);
    }

    #[tokio::test]
    async fn liste_sans_ask_claude() {
        let d = tempfile::tempdir().unwrap();
        let s = serve(exec(d.path())).unwrap();
        let mut c = connect(&s.pipe);
        let r = ask(&mut c, json!({"token": s.token, "op": "list"})).await.unwrap();
        let names: Vec<&str> = r["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert!(names.contains(&"notes__create") && !names.iter().any(|n| n.contains("ask_claude")), "{names:?}");
        assert!(r["tools"][0]["input_schema"].is_object());
    }

    #[tokio::test]
    async fn appel_libre_ok_et_journalise_en_mcp() {
        let d = tempfile::tempdir().unwrap();
        let e = exec(d.path());
        let s = serve(e.clone()).unwrap();
        let mut c = connect(&s.pipe);
        let r = ask(&mut c, json!({"token": s.token, "op": "call", "name": "notes__create", "args": {"text": "pain"}})).await.unwrap();
        assert_eq!(r["is_error"], false);
        assert!(serde_json::from_str::<Value>(r["text"].as_str().unwrap()).unwrap()["ok"]["id"].is_i64(), "{r}");
        let j = &e.broker.journal(1)[0];
        assert_eq!((j.origin.clone(), j.provenance.clone()), (Origin::Mcp, Provenance::User));
    }

    #[tokio::test]
    async fn reference_donne_pending_non_fiable() {
        let d = tempfile::tempdir().unwrap();
        let e = exec(d.path());
        let r = e.vault.lock().put("mail:1/abc", "texte d'un tiers".into());
        let s = serve(e.clone()).unwrap();
        let mut c = connect(&s.pipe);
        let out = ask(&mut c, json!({"token": s.token, "op": "call", "name": "notes__create", "args": {"text": r}})).await.unwrap();
        assert_eq!(serde_json::from_str::<Value>(out["text"].as_str().unwrap()).unwrap()["status"], "pending");
        assert_eq!(e.broker.journal(1)[0].provenance, Provenance::Untrusted { source: "mail:1/abc".into() });
    }

    #[tokio::test]
    async fn erreur_signalee_et_seconde_connexion_refusee() {
        let d = tempfile::tempdir().unwrap();
        let s = serve(exec(d.path())).unwrap();
        let mut c = connect(&s.pipe);
        let r = ask(&mut c, json!({"token": s.token, "op": "call", "name": "notes__create", "args": {}})).await.unwrap();
        assert_eq!(r["is_error"], true);
        assert!(ClientOptions::new().open(&s.pipe).is_err());
    }

    #[tokio::test]
    async fn le_drop_arrete_le_tube() {
        let d = tempfile::tempdir().unwrap();
        let s = serve(exec(d.path())).unwrap();
        let pipe = s.pipe.clone();
        assert!(pipe.starts_with(r"\\.\pipe\oniris-") && pipe.len() == r"\\.\pipe\oniris-".len() + 16 && s.token.len() == 32);
        drop(s);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert_eq!(ClientOptions::new().open(&pipe).unwrap_err().kind(), io::ErrorKind::NotFound);
    }

    #[tokio::test]
    async fn ligne_trop_longue_refusee() {
        let mut r = BufReader::new(&[b'a'; LINE_MAX + 1][..]);
        assert!(read_line(&mut r).await.is_err());
        let mut r = BufReader::new(&b"{}\n"[..]);
        assert_eq!(read_line(&mut r).await.unwrap(), Some("{}".into()));
    }
}
