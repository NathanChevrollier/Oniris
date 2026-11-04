//! Outil `system.status` : un instantané à la demande (l'échantillonnage est fourni par l'appelant).
use super::SystemStatus;
use crate::tools::helpers::{failed, low, spec};
use crate::tools::{ConfirmationText, Permission, Tool, ToolError, ToolResult, ToolSpec};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

pub type Snapshot = Arc<dyn Fn() -> SystemStatus + Send + Sync>;

pub struct Status { pub snapshot: Snapshot }

#[async_trait]
impl Tool for Status {
    fn spec(&self) -> ToolSpec {
        spec("system.status", "État du PC : CPU, RAM, GPU, VRAM et mémoire d'Oniris.", json!({"type":"object","properties":{}}), Permission::Free)
    }
    fn describe(&self, _: &Value) -> ConfirmationText { low("Lire l'état du PC") }
    async fn run(&self, _: &Value) -> Result<ToolResult, ToolError> {
        // L'échantillon dort 200 ms : hors du worker async.
        let snap = self.snapshot.clone();
        let st = tokio::task::spawn_blocking(move || snap()).await.map_err(failed)?;
        Ok(ToolResult { output: serde_json::to_value(st).map_err(failed)?, undo: None })
    }
}

pub fn all(snapshot: &Snapshot) -> Vec<Arc<dyn Tool>> {
    vec![Arc::new(Status { snapshot: snapshot.clone() })]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system::NOT_INSTALLED;
    use crate::tools::{Origin, Provenance, ToolBroker, ToolCall, ToolOutcome};
    use crate::test_util::db;
    use parking_lot::Mutex;

    #[tokio::test]
    async fn system_status_libre_renvoie_un_instantane() {
        let c = db();
        let snap: Snapshot = Arc::new(|| SystemStatus { cpu_pct: 12.5, ram_used_mb: 1, ram_total_mb: 2, gpu_pct: None, vram_used_mb: None, vram_total_mb: None, oniris_mb: 3, local_ai: NOT_INSTALLED.into(), voice: NOT_INSTALLED.into() });
        let broker = ToolBroker::new(Arc::new(Mutex::new(c)), Arc::new(|| 0), all(&snap)).unwrap();
        let call = ToolCall { tool: "system.status".into(), args: json!({}), provenance: Provenance::User, origin: Origin::User };
        let ToolOutcome::Done { output, .. } = broker.call(call).await else { panic!("Free : exécuté sans confirmation") };
        assert_eq!(output["cpu_pct"], 12.5);
        assert!(output["gpu_pct"].is_null());
    }
}
