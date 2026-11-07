//! Outil `shell.run` : commande PowerShell, garde-fous fixes, liste blanche et niveau de risque.
use crate::tools::helpers::{bad, failed, parse, spec, untrusted};
use crate::tools::{ConfirmationText, Permission, Risk, Tool, ToolError, ToolResult, ToolSpec};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

pub const MAX_COMMAND: usize = 4000;
pub const MAX_EXPLANATION: usize = 500;
/// Sortie gardée (stdout puis stderr).
pub const MAX_OUTPUT: usize = 16 * 1024;
const DEFAULT_TIMEOUT_S: u8 = 30;
const MAX_TIMEOUT_S: u8 = 120;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellOutput {
    pub exit_code: Option<i32>,
    pub output: String,
    pub timed_out: bool,
}

/// Exécution réelle injectée (le vrai PowerShell vit dans src-tauri).
#[async_trait]
pub trait ShellExec: Send + Sync {
    async fn run(&self, command: &str, cwd: &Path, timeout: Duration) -> Result<ShellOutput, String>;
}

/// Liste blanche relue à chaque appel (les réglages peuvent changer).
pub type Allowlist = Arc<dyn Fn() -> Vec<String> + Send + Sync>;

const FORBIDDEN: &str = ";|&><`$(){}@";

/// Jeton d'argument sûr : `^-{0,2}[A-Za-z0-9][\w.\-/\\:]*$`, sans `=`.
fn safe_token(t: &str) -> bool {
    let rest = t.strip_prefix("--").or_else(|| t.strip_prefix('-')).unwrap_or(t);
    let mut c = rest.chars();
    c.next().is_some_and(|f| f.is_ascii_alphanumeric())
        && c.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-' | '/' | '\\' | ':'))
}

/// Vrai si `cmd` est une entrée de la liste (préfixe de jetons, sans casse) suivie d'arguments sûrs.
pub fn allowlisted(cmd: &str, list: &[String]) -> bool {
    if !cmd.is_ascii() || cmd.chars().any(|c| FORBIDDEN.contains(c) || c == '\r' || c == '\n') {
        return false;
    }
    let toks: Vec<&str> = cmd.split_whitespace().collect();
    list.iter().any(|e| {
        let et: Vec<&str> = e.split_whitespace().collect();
        !et.is_empty()
            && toks.len() >= et.len()
            && et.iter().zip(&toks).all(|(a, b)| a.eq_ignore_ascii_case(b))
            && toks[et.len()..].iter().all(|t| safe_token(t))
    })
}

const HIGH: &[&str] = &[
    "remove-item", "rm", "del", "erase", "rd", "rmdir", "format", "diskpart", "reg", "set-itemproperty", "stop-computer",
    "restart-computer", "shutdown", "invoke-webrequest", "iwr", "curl", "wget", "invoke-expression", "iex", "start-process",
    "schtasks", "bcdedit", "cipher", "takeown", "icacls", "new-service", "set-executionpolicy",
];

/// High : mot dangereux (mot entier, sans casse) ; Low : liste blanche ; Medium sinon.
pub fn risk(cmd: &str, list: &[String]) -> Risk {
    let lower = cmd.to_lowercase();
    if lower.split(|c: char| !(c.is_alphanumeric() || c == '-')).any(|w| HIGH.contains(&w)) {
        Risk::High
    } else if allowlisted(cmd, list) {
        Risk::Low
    } else {
        Risk::Medium
    }
}

/// Garde-fous fixes : pas d'élévation, pas de commande encodée (elle masquerait le texte confirmé).
pub fn guard(cmd: &str) -> Result<(), String> {
    let lower = cmd.to_lowercase();
    for t in lower.split(|c: char| c.is_whitespace() || ";|&()'\"`,".contains(c)) {
        let encoded = t.len() >= 2 && "-encodedcommand".starts_with(t) && t.starts_with("-e");
        if encoded || matches!(t, "-verb" | "sudo" | "gsudo") || t.starts_with("-verb:") {
            return Err("élévation de droits ou commande encodée refusée".into());
        }
    }
    if lower.contains("runas") {
        return Err("élévation de droits refusée".into());
    }
    Ok(())
}

#[derive(Deserialize)]
struct Args {
    command: String,
    explanation: Option<String>,
    cwd: Option<String>,
    timeout_s: Option<u8>,
}

pub struct ShellRun {
    pub shell: Arc<dyn ShellExec>,
    pub allowlist: Allowlist,
    /// Dossier par défaut (dossier utilisateur).
    pub home: PathBuf,
}

impl ShellRun {
    fn cwd(&self, cwd: Option<&str>) -> PathBuf {
        cwd.map(PathBuf::from).unwrap_or_else(|| self.home.clone())
    }
}

#[async_trait]
impl Tool for ShellRun {
    fn spec(&self) -> ToolSpec {
        untrusted(spec(
            "shell.run",
            "Exécute une commande PowerShell (confirmation requise sauf commande en liste blanche).",
            json!({"type":"object","properties":{
                "command":{"type":"string","maxLength":MAX_COMMAND},
                "explanation":{"type":"string","maxLength":MAX_EXPLANATION},
                "cwd":{"type":"string"},
                "timeout_s":{"type":"integer","minimum":1,"maximum":MAX_TIMEOUT_S}
            },"required":["command"]}),
            Permission::Confirm,
        ))
    }
    fn check(&self, a: &Value) -> Result<(), ToolError> {
        let a: Args = parse(a)?;
        if a.command.trim().is_empty() || a.command.chars().count() > MAX_COMMAND {
            return Err(bad(format!("commande : 1 à {MAX_COMMAND} caractères")));
        }
        if a.explanation.as_deref().is_some_and(|e| e.chars().count() > MAX_EXPLANATION) {
            return Err(bad(format!("explication : {MAX_EXPLANATION} caractères au plus")));
        }
        if !(1..=MAX_TIMEOUT_S).contains(&a.timeout_s.unwrap_or(DEFAULT_TIMEOUT_S)) {
            return Err(bad(format!("délai : 1 à {MAX_TIMEOUT_S} s")));
        }
        guard(&a.command).map_err(bad)?;
        let cwd = self.cwd(a.cwd.as_deref());
        if !cwd.is_absolute() || cwd.to_string_lossy().starts_with(r"\\") || !cwd.is_dir() {
            return Err(bad("dossier : chemin absolu, existant et non réseau"));
        }
        Ok(())
    }
    fn needs_confirm(&self, a: &Value) -> bool {
        a["command"].as_str().is_none_or(|c| !allowlisted(c, &(self.allowlist)()))
    }
    fn describe(&self, a: &Value) -> ConfirmationText {
        let (cmd, expl) = (a["command"].as_str().unwrap_or_default(), a["explanation"].as_str());
        let cwd = self.cwd(a["cwd"].as_str());
        ConfirmationText {
            summary: "Exécuter une commande PowerShell".into(),
            details: format!("{cmd}\n{}\n{}", expl.unwrap_or("Commande saisie par toi"), cwd.display()),
            risk: risk(cmd, &(self.allowlist)()),
        }
    }
    async fn run(&self, a: &Value) -> Result<ToolResult, ToolError> {
        self.check(a)?; // refait à l'exécution : rien d'autre ne s'interpose
        let args: Args = parse(a)?;
        let t = Duration::from_secs(args.timeout_s.unwrap_or(DEFAULT_TIMEOUT_S).into());
        let o = self.shell.run(&args.command, &self.cwd(args.cwd.as_deref()), t).await.map_err(failed)?;
        Ok(ToolResult { output: json!({"exit_code": o.exit_code, "output": o.output, "timed_out": o.timed_out}), undo: None })
    }
}

pub fn all(shell: &Arc<dyn ShellExec>, allowlist: Allowlist, home: PathBuf) -> Vec<Arc<dyn Tool>> {
    vec![Arc::new(ShellRun { shell: shell.clone(), allowlist, home })]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::db;
    use crate::tools::{Origin, Provenance, ToolBroker, ToolCall, ToolOutcome};
    use parking_lot::Mutex;

    fn list() -> Vec<String> {
        ["git status", "Get-ChildItem"].map(String::from).to_vec()
    }

    #[test]
    fn liste_blanche() {
        let l = list();
        for ok in ["git status", "git status -s", "GIT STATUS", r"Get-ChildItem C:\x"] {
            assert!(allowlisted(ok, &l), "{ok}");
        }
        for ko in [
            "git status; Remove-Item x", "git status | iex", "git status\nrm x", "git -c core.pager=x status",
            "git status --output=x", "git status ＆ x", "dir $(rm x)", "git", "git status --",
        ] {
            assert!(!allowlisted(ko, &l), "{ko}");
        }
        assert!(!allowlisted("git log --output=x", &["git log".to_string()]));
    }

    #[test]
    fn niveaux_de_risque() {
        let l = list();
        assert_eq!(risk("Remove-Item a", &l), Risk::High);
        assert_eq!(risk("del a", &l), Risk::High);
        assert_eq!(risk("git status", &l), Risk::Low);
        assert_eq!(risk("npm run build", &l), Risk::Medium);
        assert_eq!(risk("echo perform", &l), Risk::Medium);
    }

    #[test]
    fn garde_fous() {
        for ko in ["Start-Process pwsh -Verb RunAs", "runas /user:x", "powershell -enc AAAA", "pwsh -EncodedCommand AA", "pwsh -e AA"] {
            assert!(guard(ko).is_err(), "{ko}");
        }
        assert!(guard("git status").is_ok());
    }

    struct Fake(Mutex<Vec<String>>);
    #[async_trait]
    impl ShellExec for Fake {
        async fn run(&self, c: &str, _: &Path, _: Duration) -> Result<ShellOutput, String> {
            self.0.lock().push(c.into());
            Ok(ShellOutput { exit_code: Some(0), output: "ok".into(), timed_out: false })
        }
    }

    fn setup() -> (ToolBroker, Arc<Fake>, tempfile::TempDir) {
        let home = tempfile::tempdir().unwrap();
        let fake = Arc::new(Fake(Mutex::new(vec![])));
        let sh: Arc<dyn ShellExec> = fake.clone();
        let tools = all(&sh, Arc::new(list), home.path().to_path_buf());
        let b = ToolBroker::new(Arc::new(Mutex::new(db())), Arc::new(|| 1_000), tools).unwrap();
        (b, fake, home)
    }

    async fn call(b: &ToolBroker, cmd: &str, p: Provenance) -> ToolOutcome {
        b.call(ToolCall { tool: "shell.run".into(), args: json!({"command": cmd}), provenance: p, origin: Origin::User }).await
    }

    #[tokio::test]
    async fn outil_et_confirmation() {
        let (b, fake, home) = setup();
        assert!(matches!(call(&b, "git status", Provenance::User).await, ToolOutcome::Done { .. }));
        assert_eq!(fake.0.lock().len(), 1);
        let unt = Provenance::Untrusted { source: "mail".into() };
        assert!(matches!(call(&b, "git status", unt).await, ToolOutcome::NeedsConfirmation(_)));
        let ToolOutcome::NeedsConfirmation(p) = call(&b, "Remove-Item a", Provenance::User).await else { panic!() };
        assert_eq!(p.text.risk, Risk::High);
        assert_eq!(p.text.details, format!("Remove-Item a\nCommande saisie par toi\n{}", home.path().display()));
        assert_eq!(fake.0.lock().len(), 1);
        assert!(matches!(call(&b, "runas x", Provenance::User).await, ToolOutcome::Failed { .. }));
    }
}
