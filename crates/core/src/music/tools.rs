//! Outils `music.toggle`, `music.next`, `music.previous` et `music.seek` (libres, sans réseau, par `MediaControl`).
use super::MediaControl;
use crate::tools::helpers::{failed, low, parse, spec};
use crate::tools::{ConfirmationText, Permission, Tool, ToolError, ToolResult, ToolSpec};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Clone, Copy)]
enum Action { Toggle, Next, Previous, Seek }

pub struct Control { action: Action, media: Arc<dyn MediaControl> }

#[derive(Deserialize)]
struct SeekArgs { position_ms: u32 }

impl Control {
    fn texts(&self) -> (&'static str, &'static str, &'static str) {
        match self.action {
            Action::Toggle => ("music.toggle", "Lecture ou pause du titre en cours.", "Lecture / pause"),
            Action::Next => ("music.next", "Passe au titre suivant.", "Titre suivant"),
            Action::Previous => ("music.previous", "Revient au titre précédent.", "Titre précédent"),
            Action::Seek => ("music.seek", "Va à une position du titre en cours (en millisecondes).", "Changer la position"),
        }
    }
}

#[async_trait]
impl Tool for Control {
    fn spec(&self) -> ToolSpec {
        let (name, description, _) = self.texts();
        let args = match self.action {
            Action::Seek => json!({"type":"object","properties":{"position_ms":{"type":"integer","minimum":0}},"required":["position_ms"]}),
            _ => json!({"type":"object","properties":{}}),
        };
        spec(name, description, args, Permission::Free)
    }
    fn describe(&self, _: &Value) -> ConfirmationText { low(self.texts().2) }
    async fn run(&self, args: &Value) -> Result<ToolResult, ToolError> {
        let seek = match self.action {
            Action::Seek => Some(parse::<SeekArgs>(args)?.position_ms as i64),
            _ => None,
        };
        let (action, media) = (self.action, self.media.clone());
        // Appels WinRT bloquants : hors du worker async.
        tokio::task::spawn_blocking(move || match action {
            Action::Toggle => media.toggle(),
            Action::Next => media.next(),
            Action::Previous => media.previous(),
            Action::Seek => media.seek(seek.unwrap_or(0)),
        })
        .await
        .map_err(failed)?
        .map_err(ToolError::Failed)?;
        Ok(ToolResult { output: json!({}), undo: None })
    }
}

pub fn all(media: &Arc<dyn MediaControl>) -> Vec<Arc<dyn Tool>> {
    [Action::Toggle, Action::Next, Action::Previous, Action::Seek]
        .into_iter()
        .map(|action| Arc::new(Control { action, media: media.clone() }) as Arc<dyn Tool>)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::db;
    use crate::tools::{Origin, Provenance, ToolBroker, ToolCall, ToolOutcome};
    use parking_lot::Mutex;

    /// Faux lecteur : note les commandes reçues, refuse le changement de position si `unseekable`.
    #[derive(Default)]
    struct Fake { calls: Mutex<Vec<String>>, unseekable: bool }

    impl MediaControl for Fake {
        fn toggle(&self) -> Result<(), String> { self.calls.lock().push("toggle".into()); Ok(()) }
        fn next(&self) -> Result<(), String> { self.calls.lock().push("next".into()); Ok(()) }
        fn previous(&self) -> Result<(), String> { self.calls.lock().push("previous".into()); Ok(()) }
        fn seek(&self, ms: i64) -> Result<(), String> {
            if self.unseekable {
                return Err("Ce lecteur ne permet pas de changer la position.".into());
            }
            self.calls.lock().push(format!("seek {ms}"));
            Ok(())
        }
    }

    async fn call(broker: &ToolBroker, tool: &str, args: Value) -> ToolOutcome {
        broker.call(ToolCall { tool: tool.into(), args, provenance: Provenance::User, origin: Origin::User }).await
    }

    #[tokio::test]
    async fn commandes_libres_transmises_au_lecteur() {
        let fake = Arc::new(Fake::default());
        let media: Arc<dyn MediaControl> = fake.clone();
        let broker = ToolBroker::new(Arc::new(Mutex::new(db())), Arc::new(|| 0), all(&media)).unwrap();
        for (tool, args) in [("music.toggle", json!({})), ("music.next", json!({})), ("music.previous", json!({})), ("music.seek", json!({"position_ms": 61_000}))] {
            assert!(matches!(call(&broker, tool, args).await, ToolOutcome::Done { .. }), "{tool} : libre, exécuté sans confirmation");
        }
        assert_eq!(*fake.calls.lock(), ["toggle", "next", "previous", "seek 61000"]);
        assert!(matches!(call(&broker, "music.seek", json!({"position_ms": -1})).await, ToolOutcome::Failed { .. }));
        assert_eq!(fake.calls.lock().len(), 4, "position négative refusée avant le lecteur");
    }

    #[tokio::test]
    async fn position_refusee_message_clair() {
        let media: Arc<dyn MediaControl> = Arc::new(Fake { unseekable: true, ..Default::default() });
        let broker = ToolBroker::new(Arc::new(Mutex::new(db())), Arc::new(|| 0), all(&media)).unwrap();
        let ToolOutcome::Failed { error, .. } = call(&broker, "music.seek", json!({"position_ms": 0})).await else { panic!("échec attendu") };
        assert!(error.contains("ne permet pas de changer la position"), "{error}");
    }
}
