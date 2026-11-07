//! `oniris.exe --mcp-relay` réel, piloté en JSON-RPC sur stdin/stdout, relié au tube servi par `iris_mcp::serve`
//! sur une base de test. Le relais sort sans jamais construire Tauri : aucune fenêtre.
use oniris_core::events::{Events, NoEvents};
use oniris_core::iris::{Executor, Vault};
use oniris_core::tools::{Brain, Origin, PendingAction, ToolBroker};
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{ChildStdin, ChildStdout, Command};

const NO_WINDOW: u32 = 0x0800_0000;

fn relay(pipe: &str) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_oniris"));
    c.args(["--mcp-relay", pipe]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    c.creation_flags(NO_WINDOW);
    c
}

async fn send(stdin: &mut ChildStdin, v: Value) {
    stdin.write_all(format!("{v}\n").as_bytes()).await.unwrap();
}

async fn reply(out: &mut Lines<BufReader<ChildStdout>>, id: u64) -> Value {
    loop {
        let line = tokio::time::timeout(Duration::from_secs(10), out.next_line()).await.expect("délai").unwrap().expect("fin du flux");
        let v: Value = serde_json::from_str(&line).unwrap();
        if v["id"] == id {
            return v;
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn relais_liste_et_appelle_par_le_tube() {
    let d = tempfile::tempdir().unwrap();
    let mut c = oniris_core::open_db(&d.path().join("t.db")).unwrap();
    oniris_core::migrate(&mut c, oniris_core::CORE_MIGRATIONS).unwrap();
    let db = Arc::new(Mutex::new(c));
    let events: Arc<dyn Events> = Arc::new(NoEvents);
    let broker = Arc::new(ToolBroker::new(db.clone(), Arc::new(|| 0), oniris_core::notes::tools::all(&db, &events)).unwrap());
    let exec = Arc::new(Executor {
        broker, vault: Arc::new(Mutex::new(Vault::default())), origin: Origin::Assistant { brain: Brain::Claude },
        on_pending: Arc::new(|_: &PendingAction| {}), virtuals: None,
    });
    let s = oniris_lib::iris_mcp::serve(exec).unwrap();

    let mut child = relay(&s.pipe).env("ONIRIS_MCP_TOKEN", &s.token).spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap()).lines();

    send(&mut stdin, json!({"jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}})).await;
    assert!(reply(&mut out, 1).await["result"]["capabilities"]["tools"].is_object());
    send(&mut stdin, json!({"jsonrpc": "2.0", "method": "notifications/initialized"})).await;

    send(&mut stdin, json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"})).await;
    let r = reply(&mut out, 2).await;
    let names: Vec<&str> = r["result"]["tools"].as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"notes__create") && names.iter().all(|n| !n.contains('.')), "{names:?}");

    send(&mut stdin, json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "notes__create", "arguments": {"text": "Acheter du pain"}}})).await;
    let r = reply(&mut out, 3).await;
    assert_eq!(r["result"]["isError"], false, "{r}");
    let text: Value = serde_json::from_str(r["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert!(text["ok"]["id"].is_i64(), "{text}");
    let notes = oniris_core::notes::recent(&db.lock(), 10).unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].body, "Acheter du pain");

    drop(stdin);
    let code = tokio::time::timeout(Duration::from_secs(10), child.wait()).await.expect("le relais doit sortir").unwrap();
    assert!(code.success());
}

#[tokio::test]
async fn tube_absent_sort_avec_le_code_2() {
    let pipe = format!(r"\\.\pipe\oniris-absent-{}", std::process::id());
    let o = tokio::time::timeout(Duration::from_secs(10), relay(&pipe).output()).await.expect("le relais doit sortir").unwrap();
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).contains("injoignable"));
}
