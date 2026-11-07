//! `oniris.exe --mcp-relay <tube>` : serveur MCP sur stdio lancé par Claude Code, qui relaie `tools/list` et
//! `tools/call` à l'app par le tube nommé. N'ouvre jamais la base, ne crée aucune fenêtre.
use crate::modules::iris_mcp::read_line;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ListToolsResult, PaginatedRequestParams, ServerCapabilities,
    ServerConfig, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler, ServiceExt};
use serde_json::{json, Value};
use std::process::ExitCode;
use std::sync::Arc;
use tokio::io::{AsyncWriteExt, BufReader, ReadHalf, WriteHalf};
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient};
use tokio::sync::Mutex;

struct Relay {
    token: String,
    pipe: Mutex<(BufReader<ReadHalf<NamedPipeClient>>, WriteHalf<NamedPipeClient>)>,
}

fn down(_: impl std::fmt::Debug) -> McpError {
    McpError::internal_error("Oniris injoignable", None)
}

impl Relay {
    /// Une requête, une réponse ; un refus de l'app devient une erreur de protocole.
    async fn ask(&self, mut req: Value) -> Result<Value, McpError> {
        req["token"] = self.token.clone().into();
        let mut p = self.pipe.lock().await;
        let (r, w) = &mut *p;
        w.write_all(format!("{req}\n").as_bytes()).await.map_err(down)?;
        let line = read_line(r).await.map_err(down)?.ok_or_else(|| down(()))?;
        let v: Value = serde_json::from_str(&line).map_err(down)?;
        match v.get("error") {
            Some(e) => Err(McpError::internal_error(format!("refusé par Oniris : {e}"), None)),
            None => Ok(v),
        }
    }
}

impl ServerHandler for Relay {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
    }

    async fn list_tools(&self, _: Option<PaginatedRequestParams>, _: RequestContext<RoleServer>) -> Result<ListToolsResult, McpError> {
        let v = self.ask(json!({ "op": "list" })).await?;
        let tools = v["tools"].as_array().into_iter().flatten().map(|t| {
            let schema = t["input_schema"].as_object().cloned().unwrap_or_default();
            Tool::new(t["name"].as_str().unwrap_or_default().to_string(), t["description"].as_str().unwrap_or_default().to_string(), Arc::new(schema))
        });
        Ok(ListToolsResult::with_all_items(tools.collect()))
    }

    async fn call_tool(&self, req: CallToolRequestParams, _: RequestContext<RoleServer>) -> Result<CallToolResponse, McpError> {
        let v = self.ask(json!({ "op": "call", "name": req.name, "args": req.arguments.unwrap_or_default() })).await?;
        let text = vec![ContentBlock::text(v["text"].as_str().unwrap_or_default())];
        Ok(if v["is_error"] == true { CallToolResult::error(text) } else { CallToolResult::success(text) }.into())
    }
}

pub fn main(pipe: &str) -> ExitCode {
    let token = std::env::var("ONIRIS_MCP_TOKEN").unwrap_or_default();
    let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => { eprintln!("oniris --mcp-relay : {e}"); return ExitCode::FAILURE; }
    };
    rt.block_on(async {
        let client = match ClientOptions::new().open(pipe) {
            Ok(c) => c,
            Err(e) => { eprintln!("oniris --mcp-relay : tube d'Oniris injoignable ({e})"); return ExitCode::from(2); }
        };
        let (r, w) = tokio::io::split(client);
        let relay = Relay { token, pipe: Mutex::new((BufReader::new(r), w)) };
        match relay.serve(rmcp::transport::stdio()).await {
            Ok(s) => { let _ = s.waiting().await; ExitCode::SUCCESS }
            Err(e) => { eprintln!("oniris --mcp-relay : {e}"); ExitCode::FAILURE }
        }
    })
}
