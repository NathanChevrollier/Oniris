//! Outils `projects.list` et `projects.status` (libres, en lecture) ; l'inventaire est fourni par l'appelant.
use super::{find, sort, status_text, Project};
use crate::time_util::now_ms;
use crate::tools::helpers::{bad, low, parse, spec};
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
}
