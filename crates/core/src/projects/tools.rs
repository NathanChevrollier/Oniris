//! Outils `projects.list`, `projects.status` (libres, en lecture) et `projects.resume` (libre) ; l'inventaire est fourni par l'appelant.
use super::resume::{execute, plan, Env, LaunchStep, Mode, Prompts};
use super::{find, sort, status_text, Project};
use crate::launch::Opener;
use crate::time_util::now_ms;
use crate::tools::helpers::{bad, failed, low, parse, spec};
use crate::tools::{ConfirmationText, Permission, Tool, ToolError, ToolResult, ToolSpec};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

pub type Source = Arc<dyn Fn() -> Vec<Project> + Send + Sync>;

pub struct List { pub source: Source }
pub struct Status { pub source: Source }

/// Contenu utilisateur (noms, titres de session) : sortie non journalisée.
fn sensitive(s: ToolSpec) -> ToolSpec {
    ToolSpec { sensitive_output: true, ..s }
}

#[async_trait]
impl Tool for List {
    fn spec(&self) -> ToolSpec {
        sensitive(spec("projects.list", "Liste les projets de l'utilisateur (épinglés d'abord, puis activité récente).", json!({"type":"object","properties":{}}), Permission::Free))
    }
    fn describe(&self, _: &Value) -> ConfirmationText { low("Lister les projets") }
    async fn run(&self, _: &Value) -> Result<ToolResult, ToolError> {
        let mut l: Vec<Project> = (self.source)().into_iter().filter(|p| !p.hidden).collect();
        sort(&mut l);
        let out: Vec<Value> = l
            .iter()
            .map(|p| json!({
                "name": p.name, "last_activity_ms": p.last_activity_ms, "branch": p.branch, "dirty": p.dirty, "pinned": p.pinned,
                "plan": p.plan.as_ref().map(|x| json!({"done": x.done, "total": x.total, "current": x.current})),
                "last_session": p.last_session.as_ref().and_then(|s| s.title.clone()),
            }))
            .collect();
        Ok(ToolResult { output: json!(out), undo: None })
    }
}

#[derive(Deserialize)]
struct NameArgs { name: String }

#[async_trait]
impl Tool for Status {
    fn spec(&self) -> ToolSpec {
        sensitive(spec(
            "projects.status",
            "Où en est un projet : plan, dernière session Claude, branche git et dernière activité. Le nom est cherché sans tenir compte de la casse, par préfixe.",
            json!({"type":"object","properties":{"name":{"type":"string"}},"required":["name"]}),
            Permission::Free,
        ))
    }
    fn describe(&self, _: &Value) -> ConfirmationText { low("Lire l'état d'un projet") }
    async fn run(&self, args: &Value) -> Result<ToolResult, ToolError> {
        let a: NameArgs = parse(args)?;
        let list = (self.source)();
        let p = find(&list, &a.name).ok_or_else(|| bad("projet inconnu"))?;
        Ok(ToolResult { output: json!(status_text(p, now_ms())), undo: None })
    }
}

/// Environnement de reprise (lu sur un fil bloquant : processus `where`, lecture de l'extension) et invites des réglages.
pub type ResumeEnv = Arc<dyn Fn() -> (Env, Prompts) + Send + Sync>;

#[derive(Deserialize)]
struct ResumeArgs {
    name: String,
    mode: Mode,
    #[serde(default)]
    path: Option<String>,
}

pub struct Resume {
    pub source: Source,
    pub env: ResumeEnv,
    pub opener: Arc<dyn Opener>,
}

#[async_trait]
impl Tool for Resume {
    fn spec(&self) -> ToolSpec {
        spec(
            "projects.resume",
            "Reprend un projet : ouvre VS Code sur le dossier puis la dernière session Claude Code avec une invite de reprise              (mode « orchestrator » : sous-agents et graphify). Le nom est cherché sans tenir compte de la casse, par préfixe.",
            json!({"type":"object","properties":{
                "name":{"type":"string"},
                "mode":{"type":"string","enum":["normal","orchestrator"]},
                "path":{"type":"string","description":"Chemin exact du projet (prioritaire sur le nom)."}
            },"required":["name","mode"]}),
            Permission::Free,
        )
    }
    fn describe(&self, _: &Value) -> ConfirmationText { low("Reprendre un projet") }
    async fn run(&self, args: &Value) -> Result<ToolResult, ToolError> {
        let a: ResumeArgs = parse(args)?;
        let list = (self.source)();
        let found = match &a.path {
            Some(path) => list.iter().find(|p| &p.path == path),
            None => find(&list, &a.name),
        };
        let p = found.ok_or_else(|| bad("projet inconnu"))?.clone();
        let env = self.env.clone();
        let (env, prompts) = tokio::task::spawn_blocking(move || env()).await.map_err(failed)?;
        let (steps, info) = plan(&p, a.mode, &prompts, &env);
        let kinds: Vec<&str> = steps.iter().map(LaunchStep::kind).collect();
        let opener = self.opener.clone();
        std::thread::Builder::new().name("oniris-resume".into()).spawn(move || execute(&*opener, &steps)).map_err(failed)?;
        Ok(ToolResult { output: json!({"steps": kinds, "info": info}), undo: None })
    }
}

pub fn all(source: &Source) -> Vec<Arc<dyn Tool>> {
    vec![Arc::new(List { source: source.clone() }), Arc::new(Status { source: source.clone() })]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::db;
    use crate::tools::{Origin, Provenance, ToolBroker, ToolCall, ToolOutcome};
    use parking_lot::Mutex;

    fn p(name: &str, hidden: bool) -> Project {
        Project { path: format!(r"C:\p\{name}"), name: name.into(), last_activity_ms: 1, branch: Some("main".into()), dirty: 0, activity_14d: [0; 14], plan: None, last_session: None, pinned: false, hidden }
    }

    async fn call(tool: &str, args: Value) -> ToolOutcome {
        let src: Source = Arc::new(|| vec![p("MyEmpire", false), p("Secret", true)]);
        let broker = ToolBroker::new(Arc::new(Mutex::new(db())), Arc::new(|| 0), all(&src)).unwrap();
        broker.call(ToolCall { tool: tool.into(), args, provenance: Provenance::User, origin: Origin::User }).await
    }

    #[tokio::test]
    async fn liste_sans_les_caches() {
        let ToolOutcome::Done { output, .. } = call("projects.list", json!({})).await else { panic!("Free") };
        assert_eq!(output.as_array().unwrap().len(), 1);
        assert_eq!(output[0]["name"], "MyEmpire");
    }

    #[tokio::test]
    async fn statut_par_prefixe_et_projet_inconnu() {
        let ToolOutcome::Done { output, .. } = call("projects.status", json!({"name": "myemp"})).await else { panic!("Free") };
        assert!(output.as_str().unwrap().starts_with("MyEmpire — "));
        assert!(matches!(call("projects.status", json!({"name": "secret"})).await, ToolOutcome::Failed { .. }));
    }

    /// Faux `Opener` : chaque lancement part dans le canal, rien n'est réellement ouvert (R8).
    struct Rec(Mutex<std::sync::mpsc::Sender<String>>);
    impl Opener for Rec {
        fn open_path(&self, _: &std::path::Path) {}
        fn open_url(&self, u: &str) {
            let _ = self.0.lock().send(u.to_string());
        }
        fn spawn_detached(&self, program: &str, args: &[String], _: Option<&std::path::Path>) -> std::io::Result<()> {
            let _ = self.0.lock().send(format!("{program} {}", args.join("|")));
            Ok(())
        }
    }

    #[tokio::test]
    async fn reprise_libre_par_prefixe_et_mode() {
        let (tx, rx) = std::sync::mpsc::channel();
        let src: Source = Arc::new(|| vec![p("MyEmpire", false), p("Secret", true)]);
        let env: ResumeEnv = Arc::new(|| {
            (Env { vscode_ok: false, code: Some("Code.exe".into()), wt: Some("wt".into()) }, Prompts { normal: "N".into(), orchestrator: "O".into() })
        });
        let resume: Arc<dyn Tool> = Arc::new(Resume { source: src, env, opener: Arc::new(Rec(Mutex::new(tx))) });
        let broker = ToolBroker::new(Arc::new(Mutex::new(db())), Arc::new(|| 0), vec![resume]).unwrap();
        let go = |args: Value| broker.call(ToolCall { tool: "projects.resume".into(), args, provenance: Provenance::User, origin: Origin::User });
        let ToolOutcome::Done { output, .. } = go(json!({"name": "myemp", "mode": "orchestrator"})).await else { panic!("Free") };
        assert_eq!(output, json!({"steps": ["code", "terminal"], "info": null}));
        let t = std::time::Duration::from_secs(5);
        assert_eq!(rx.recv_timeout(t).unwrap(), r"Code.exe C:\p\MyEmpire");
        assert_eq!(rx.recv_timeout(t).unwrap(), r"wt -d|C:\p\MyEmpire|claude|--continue|O");
        assert!(matches!(go(json!({"name": "secret", "mode": "normal"})).await, ToolOutcome::Failed { .. }), "caché");
        assert!(matches!(go(json!({"name": "myemp", "mode": "fou"})).await, ToolOutcome::Failed { .. }));
        // Chemin exact (interface) prioritaire sur le nom.
        let ToolOutcome::Done { .. } = go(json!({"name": "x", "path": r"C:\p\MyEmpire", "mode": "normal"})).await else { panic!("chemin") };
        assert_eq!(rx.recv_timeout(t).unwrap(), r"Code.exe C:\p\MyEmpire");
    }
}
