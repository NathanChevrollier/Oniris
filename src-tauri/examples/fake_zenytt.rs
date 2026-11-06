//! Faux `Zenytt.exe --mcp` pour les tests de `modules::zenytt` : mêmes outils et même forme de réponse
//! (texte JSON) que `Projets/zenytt/crates/mcp`, avec des valeurs figées. Second argument « hang » :
//! `server_status` ne répond jamais (test du délai). Aucun réseau.
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, ServerCapabilities, ServerConfig};
use rmcp::{schemars, tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler, ServiceExt};
use serde_json::json;

#[derive(Clone)]
struct Fake {
    hang: bool,
    tool_router: ToolRouter<Self>,
}

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct ServerArg {
    server: String,
}

fn text(v: serde_json::Value) -> Result<CallToolResult, McpError> {
    Ok(CallToolResult::success(vec![ContentBlock::text(serde_json::to_string_pretty(&v).unwrap())]))
}

#[tool_router]
impl Fake {
    #[tool(description = "Liste les serveurs")]
    async fn list_servers(&self) -> Result<CallToolResult, McpError> {
        text(json!([{ "name": "vps-test", "host": "203.0.113.5", "id": "s1" }, { "name": "autre", "host": "203.0.113.6", "id": "s2" }]))
    }

    #[tool(description = "État d'un serveur")]
    async fn server_status(&self, Parameters(a): Parameters<ServerArg>) -> Result<CallToolResult, McpError> {
        if self.hang {
            std::future::pending::<()>().await;
        }
        if a.server != "s1" {
            return Ok(CallToolResult::error(vec![ContentBlock::text("clé du serveur jamais approuvée : connecte-toi d'abord une fois depuis Zenytt")]));
        }
        text(json!({
            "metrics": {
                "timestamp": 1, "cpuPercent": 12.5, "cpuCount": 4, "memUsed": 400, "memTotal": 1000, "swapUsed": 0, "swapTotal": 0,
                "load": [0.42, 0.3, 0.2], "uptimeSecs": 100.0, "netRxRate": 0.0, "netTxRate": 0.0,
                "disks": [
                    { "mount": "/", "device": "/dev/sda1", "total": 100, "used": 61, "readOnly": false, "ignored": false },
                    { "mount": "/snap/core", "device": "/dev/loop0", "total": 10, "used": 10, "readOnly": true, "ignored": false },
                    { "mount": "/backup", "device": "/dev/sdb1", "total": 100, "used": 95, "readOnly": false, "ignored": true },
                    { "mount": "/data", "device": "/dev/sdc1", "total": 100, "used": 30, "readOnly": false, "ignored": false }
                ]
            },
            "memPercent": 40.0,
            "agentInstalled": true,
            "activeAlerts": [{ "key": "cpu", "title": "CPU", "message": "haut", "since": 1 }]
        }))
    }

    #[tool(description = "Sites nginx")]
    async fn list_sites(&self, Parameters(a): Parameters<ServerArg>) -> Result<CallToolResult, McpError> {
        let name = if a.server == "s1" { "blog.example.com" } else { "shop.example.net" };
        text(json!({
            "installed": true, "version": "1.24", "running": true, "certbot": true, "others": [], "confRoot": "/etc/nginx", "certificates": [],
            "files": [{ "path": "/etc/nginx/sites-enabled/a", "realPath": "/etc/nginx/sites-available/a", "enabled": true, "servers": [
                { "serverNames": [name, format!("www.{name}")], "listen": ["443 ssl"], "ssl": true, "locations": [], "line": 1, "upstreamPorts": [] },
                { "serverNames": ["plain.example.com"], "listen": ["80"], "ssl": false, "locations": [], "line": 9, "upstreamPorts": [] },
                { "serverNames": ["_", "*.example.com", "~^api\\d+\\.example\\.com$", "localhost"], "listen": ["443 ssl"], "ssl": true, "locations": [], "line": 20, "upstreamPorts": [] }
            ]}],
            "disabled": [{ "path": "/etc/nginx/sites-available/old", "realPath": "/etc/nginx/sites-available/old", "enabled": false, "servers": [
                { "serverNames": ["old.example.com"], "listen": ["443 ssl"], "ssl": true, "locations": [], "line": 1, "upstreamPorts": [] }
            ]}]
        }))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for Fake {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let hang = std::env::args().any(|a| a == "hang");
    let service = Fake { hang, tool_router: Fake::tool_router() }.serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}
