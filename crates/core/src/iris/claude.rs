//! Claude Code non interactif (`claude -p`, stream-json) sans aucun outil natif : arguments, lecture du flux,
//! classement des erreurs et trait du cerveau « nuage ». Le lancement du processus est dans `src-tauri`.
use super::Executor;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const CLAUDE_REL: &str = r"npm\node_modules\@anthropic-ai\claude-code\bin\claude.exe";

/// Réglage, puis installation npm, puis installeur natif. Jamais de `.cmd` (il passerait par cmd.exe).
pub fn locate(setting: &str, appdata: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    let exe = |p: &PathBuf| p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe"));
    let setting = (!setting.is_empty()).then(|| PathBuf::from(setting));
    setting
        .into_iter()
        .chain(appdata.map(|a| a.join(CLAUDE_REL)))
        .chain(home.map(|h| h.join(r".local\bin\claude.exe")))
        .find(exe)
}

pub struct McpConfig { pub pipe: String, pub token: String, pub relay_exe: PathBuf }

#[derive(Debug, Clone, PartialEq)]
pub struct Invocation { pub program: PathBuf, pub args: Vec<String>, pub cwd: PathBuf, pub stdin: String }

pub fn invocation(program: PathBuf, cwd: PathBuf, system: &str, model: &str, mcp: Option<&McpConfig>, prompt: String) -> Invocation {
    let mut args: Vec<String> = [
        "-p", "--output-format", "stream-json", "--verbose", "--include-partial-messages", "--tools", "", "--strict-mcp-config",
        "--setting-sources", "project", "--no-session-persistence", "--permission-prompts", "none", "--system-prompt",
    ]
    .map(String::from)
    .into();
    args.push(system.into());
    if !model.is_empty() {
        args.extend(["--model".into(), model.into()]);
    }
    if let Some(m) = mcp {
        let cfg = json!({"mcpServers": {"oniris": {
            "command": m.relay_exe, "args": ["--mcp-relay", m.pipe], "env": {"ONIRIS_MCP_TOKEN": m.token}}}});
        args.extend(["--mcp-config".into(), cfg.to_string(), "--allowedTools".into(), "mcp__oniris".into()]);
    }
    Invocation { program, args, cwd, stdin: prompt }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ClaudeEvent {
    Init { tools: Vec<String> },
    Delta(String),
    Result { text: String, is_error: bool, subtype: String },
}

pub fn parse_line(line: &str) -> Option<ClaudeEvent> {
    let v: Value = serde_json::from_str(line).ok()?;
    let s = |v: &Value| v.as_str().unwrap_or_default().to_string();
    match v["type"].as_str()? {
        "system" if v["subtype"] == "init" => {
            // Sans liste d'outils valide, pas d'Init : le garde refuse alors le flux (échec fermé).
            let tools = v["tools"].as_array()?.iter().map(s).collect();
            Some(ClaudeEvent::Init { tools })
        }
        "stream_event" if v["event"]["type"] == "content_block_delta" && v["event"]["delta"]["type"] == "text_delta" => {
            Some(ClaudeEvent::Delta(s(&v["event"]["delta"]["text"])))
        }
        "result" => Some(ClaudeEvent::Result { text: s(&v["result"]), is_error: v["is_error"].as_bool().unwrap_or(false), subtype: s(&v["subtype"]) }),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ClaudeError {
    #[error("Claude Code n'est pas installé")]
    NotInstalled,
    /// Limite d'usage atteinte ou compte non connecté.
    #[error("Claude est indisponible : {0}")]
    Unavailable(String),
    #[error("Claude Code a proposé des outils interdits : {0:?}")]
    ForbiddenTools(Vec<String>),
    #[error("Claude Code a échoué : {0}")]
    Failed(String),
}

pub fn classify(stderr: &str, result: Option<&ClaudeEvent>, exit: Option<i32>) -> ClaudeError {
    let text = match result {
        Some(ClaudeEvent::Result { text, .. }) if !text.is_empty() => text.as_str(),
        _ => stderr.trim(),
    };
    let low = format!("{text} {stderr}").to_lowercase();
    let msg: String = text.chars().take(200).collect();
    if ["usage limit", "rate limit", "limit reached", "not logged in", "/login", "invalid api key"].iter().any(|p| low.contains(p)) {
        return ClaudeError::Unavailable(msg);
    }
    ClaudeError::Failed(if msg.is_empty() { format!("code de sortie {}", exit.map_or("inconnu".into(), |c| c.to_string())) } else { msg })
}

pub type OnDelta<'a> = &'a mut (dyn for<'x> FnMut(&'x str) + Send);

#[async_trait]
pub trait ClaudeBrain: Send + Sync {
    /// Une réponse de texte, sans outils.
    async fn complete(&self, system: &str, prompt: String, on_delta: OnDelta<'_>) -> Result<String, ClaudeError>;
    /// Un tour avec les outils d'Oniris exposés en MCP (branché en tâche 7).
    async fn plan(&self, system: &str, prompt: String, exec: Arc<Executor>, on_delta: OnDelta<'_>) -> Result<String, ClaudeError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    const STREAM: &str = include_str!("../../tests/fixtures/iris/claude_stream.jsonl");

    fn touch(p: &Path) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"").unwrap();
    }

    #[test]
    fn locate_trouve_l_exe_et_refuse_le_cmd() {
        let d = tempfile::tempdir().unwrap();
        let (app, home) = (d.path().join("app"), d.path().join("home"));
        touch(&app.join(r"npm\claude.cmd"));
        assert_eq!(locate("", Some(&app), Some(&home)), None);
        let npm = app.join(CLAUDE_REL);
        touch(&npm);
        assert_eq!(locate("", Some(&app), Some(&home)), Some(npm.clone()));
        let nat = home.join(r".local\bin\claude.exe");
        touch(&nat);
        assert_eq!(locate("", None, Some(&home)), Some(nat));
        let set = d.path().join("mon.exe");
        touch(&set);
        assert_eq!(locate(set.to_str().unwrap(), Some(&app), Some(&home)), Some(set));
        assert_eq!(locate(d.path().join("absent.exe").to_str().unwrap(), Some(&app), None), Some(npm));
    }

    #[test]
    fn invocation_sans_et_avec_mcp() {
        let i = invocation(PathBuf::from("c.exe"), PathBuf::from("w"), "sys", "", None, "salut".into());
        let attendu: Vec<&str> = "-p --output-format stream-json --verbose --include-partial-messages --tools  --strict-mcp-config --setting-sources project --no-session-persistence --permission-prompts none --system-prompt sys".split(' ').collect();
        assert_eq!(i.args, attendu);
        assert_eq!(i.stdin, "salut");
        let m = McpConfig { pipe: "pipe1".into(), token: "tok".into(), relay_exe: PathBuf::from("oniris.exe") };
        let i = invocation(PathBuf::from("c.exe"), PathBuf::from("w"), "sys", "opus", Some(&m), "p".into());
        assert_eq!(&i.args[14..18], ["sys", "--model", "opus", "--mcp-config"]);
        assert_eq!(&i.args[19..], ["--allowedTools", "mcp__oniris"]);
        let cfg: Value = serde_json::from_str(&i.args[18]).unwrap();
        let srv = &cfg["mcpServers"]["oniris"];
        assert_eq!(srv["command"], "oniris.exe");
        assert_eq!(srv["args"], json!(["--mcp-relay", "pipe1"]));
        assert_eq!(srv["env"]["ONIRIS_MCP_TOKEN"], "tok");
        let i = invocation(PathBuf::from("c.exe"), PathBuf::from("w"), "sys", "opus", None, "p".into());
        assert_eq!(&i.args[14..], ["sys", "--model", "opus"]);
    }

    #[test]
    fn parse_line_sur_le_flux() {
        let ev: Vec<_> = STREAM.lines().filter_map(parse_line).collect();
        assert_eq!(ev, vec![
            ClaudeEvent::Init { tools: vec!["mcp__oniris__list_events".into(), "mcp__oniris__open_app".into()] },
            ClaudeEvent::Delta("Bonjour".into()),
            ClaudeEvent::Result { text: "Bonjour".into(), is_error: false, subtype: "success".into() },
            ClaudeEvent::Result { text: "Claude AI usage limit reached|1760000000".into(), is_error: true, subtype: "error_during_execution".into() },
        ]);
        assert_eq!(parse_line("pas du json"), None);
        assert_eq!(parse_line(r#"{"type":"system","subtype":"init"}"#), None);
    }

    #[test]
    fn classify_cinq_cas() {
        let u = |e: ClaudeError| matches!(e, ClaudeError::Unavailable(_));
        assert!(u(classify("Usage Limit exceeded", None, Some(1))));
        assert!(u(classify("", Some(&ClaudeEvent::Result { text: "Rate limit hit".into(), is_error: true, subtype: "x".into() }), Some(1))));
        assert!(u(classify("5-hour limit reached", None, Some(1))));
        assert!(u(classify("Not logged in · Please run /login", None, Some(1))));
        assert!(u(classify("Invalid API key", None, Some(1))));
        assert_eq!(classify("boom", None, Some(2)), ClaudeError::Failed("boom".into()));
        assert_eq!(classify("", None, Some(2)), ClaudeError::Failed("code de sortie 2".into()));
    }
}
