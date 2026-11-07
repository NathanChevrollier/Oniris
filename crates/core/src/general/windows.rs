//! Outils `apps.focus` (libre) et `apps.close` (confirmation, `WM_CLOSE`) ; le vrai contrôle vit dans src-tauri.
use crate::apps::Inventory;
use crate::tools::helpers::{bad, failed, low, parse, spec};
use crate::tools::{ConfirmationText, Permission, Risk, Tool, ToolError, ToolResult, ToolSpec};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

/// `app_key` : clé de l'inventaire des applications (chemin du raccourci `.lnk`).
pub trait WindowControl: Send + Sync {
    fn focus(&self, app_key: &str) -> Result<(), String>;
    /// Demande polie (`WM_CLOSE`) : l'application peut proposer d'enregistrer. Jamais `TerminateProcess`.
    fn close(&self, app_key: &str) -> Result<(), String>;
}

#[derive(Deserialize)]
struct KeyArgs {
    key: String,
}

pub struct AppsFocus {
    pub windows: Arc<dyn WindowControl>,
    pub inventory: Arc<Inventory>,
}
pub struct AppsClose {
    pub windows: Arc<dyn WindowControl>,
    pub inventory: Arc<Inventory>,
}

fn known(inv: &Inventory, a: &Value) -> Result<String, ToolError> {
    let KeyArgs { key } = parse(a)?;
    if !inv.contains_key(&key) {
        return Err(bad("application inconnue"));
    }
    Ok(key)
}

fn key_schema() -> Value {
    json!({"type":"object","properties":{"key":{"type":"string"}},"required":["key"]})
}

#[async_trait]
impl Tool for AppsFocus {
    fn spec(&self) -> ToolSpec {
        spec("apps.focus", "Met au premier plan une application ouverte de l'inventaire.", key_schema(), Permission::Free)
    }
    fn check(&self, a: &Value) -> Result<(), ToolError> {
        known(&self.inventory, a).map(|_| ())
    }
    fn describe(&self, _: &Value) -> ConfirmationText {
        low("Mettre une application au premier plan")
    }
    async fn run(&self, a: &Value) -> Result<ToolResult, ToolError> {
        self.windows.focus(&known(&self.inventory, a)?).map_err(failed)?;
        Ok(ToolResult { output: json!({"focused": true}), undo: None })
    }
}

#[async_trait]
impl Tool for AppsClose {
    fn spec(&self) -> ToolSpec {
        spec("apps.close", "Ferme les fenêtres d'une application de l'inventaire (confirmation requise).", key_schema(), Permission::Confirm)
    }
    fn check(&self, a: &Value) -> Result<(), ToolError> {
        known(&self.inventory, a).map(|_| ())
    }
    fn describe(&self, a: &Value) -> ConfirmationText {
        let key = a["key"].as_str().unwrap_or_default();
        let title = self.inventory.items.read().iter().find(|i| i.key == key).map(|i| i.title.clone()).unwrap_or_default();
        ConfirmationText { summary: format!("Fermer {title}"), details: "Ses fenêtres reçoivent une demande de fermeture : rien n'est tué de force.".into(), risk: Risk::Medium }
    }
    async fn run(&self, a: &Value) -> Result<ToolResult, ToolError> {
        self.windows.close(&known(&self.inventory, a)?).map_err(failed)?;
        Ok(ToolResult { output: json!({"closed": true}), undo: None })
    }
}

pub fn all(windows: &Arc<dyn WindowControl>, inventory: &Arc<Inventory>) -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(AppsFocus { windows: windows.clone(), inventory: inventory.clone() }),
        Arc::new(AppsClose { windows: windows.clone(), inventory: inventory.clone() }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::{Item, ItemKind};
    use crate::test_util::db;
    use crate::tools::{Origin, Provenance, ToolBroker, ToolCall, ToolOutcome};
    use parking_lot::Mutex;

    #[derive(Default)]
    struct Fake(Mutex<Vec<String>>);
    impl WindowControl for Fake {
        fn focus(&self, k: &str) -> Result<(), String> {
            self.0.lock().push(format!("focus {k}"));
            Ok(())
        }
        fn close(&self, k: &str) -> Result<(), String> {
            self.0.lock().push(format!("close {k}"));
            Ok(())
        }
    }

    #[tokio::test]
    async fn focus_libre_close_sur_carte() {
        let inv = Arc::new(Inventory::default());
        inv.set(vec![Item { kind: ItemKind::App, key: r"C:\m\App.lnk".into(), title: "App".into(), subtitle: String::new() }], 1);
        let fake = Arc::new(Fake::default());
        let w: Arc<dyn WindowControl> = fake.clone();
        let b = ToolBroker::new(Arc::new(Mutex::new(db())), Arc::new(|| 1_000), all(&w, &inv)).unwrap();
        let call = |tool: &str, key: &str| ToolCall { tool: tool.into(), args: json!({"key": key}), provenance: Provenance::User, origin: Origin::User };
        assert!(matches!(b.call(call("apps.focus", r"C:\m\App.lnk")).await, ToolOutcome::Done { .. }));
        let ToolOutcome::NeedsConfirmation(p) = b.call(call("apps.close", r"C:\m\App.lnk")).await else { panic!() };
        assert_eq!(p.text.summary, "Fermer App");
        assert_eq!(*fake.0.lock(), [r"focus C:\m\App.lnk"]);
        assert!(matches!(b.resolve(p.action_id, true).await, ToolOutcome::Done { .. }));
        assert_eq!(fake.0.lock().len(), 2);
        assert!(matches!(b.call(call("apps.close", r"C:\Windows\notepad.exe")).await, ToolOutcome::Failed { .. }));
        assert_eq!(fake.0.lock().len(), 2);
    }
}
