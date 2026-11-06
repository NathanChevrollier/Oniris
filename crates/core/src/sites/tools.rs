//! Outil `sites.status` (libre) : état des sites surveillés.
use super::{SiteView, VpsStatus};
use crate::tools::helpers::{low, spec};
use crate::tools::{ConfirmationText, Permission, Tool, ToolError, ToolResult, ToolSpec};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

/// Vues courantes (réglages, base et état « hors ligne » vivent côté application).
pub type Source = Arc<dyn Fn() -> Result<Vec<SiteView>, String> + Send + Sync>;

pub struct Status { pub source: Source }

#[async_trait]
impl Tool for Status {
    fn spec(&self) -> ToolSpec {
        spec("sites.status", "État des sites surveillés (en ligne, lent, en panne, hors ligne) avec le dernier temps de réponse.", json!({"type":"object","properties":{}}), Permission::Free)
    }
    fn describe(&self, _: &Value) -> ConfirmationText { low("Voir l'état des sites") }
    async fn run(&self, _: &Value) -> Result<ToolResult, ToolError> {
        let v = (self.source)().map_err(ToolError::Failed)?;
        Ok(ToolResult { output: json!({ "sites": v.iter().map(|s| json!({"name": s.name, "state": s.state, "last_ms": s.last_ms})).collect::<Vec<_>>() }), undo: None })
    }
}

/// Dernier état connu de la charge du VPS (aucun appel à Zenytt depuis l'outil).
pub type VpsSource = Arc<dyn Fn() -> VpsStatus + Send + Sync>;

pub struct Vps { pub source: VpsSource }

#[async_trait]
impl Tool for Vps {
    fn spec(&self) -> ToolSpec {
        spec("vps.status", "Charge du VPS lue par Zenytt (CPU, mémoire, disque, charge, alertes) : dernier relevé connu.", json!({"type":"object","properties":{}}), Permission::Free)
    }
    fn describe(&self, _: &Value) -> ConfirmationText { low("Voir la charge du VPS") }
    async fn run(&self, _: &Value) -> Result<ToolResult, ToolError> {
        let st = (self.source)();
        if !st.zenytt {
            return Err(ToolError::Failed("Zenytt n'est pas installé sur ce PC".into()));
        }
        Ok(ToolResult { output: json!({ "vps": st.view, "error": st.error }), undo: None })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test] async fn vps_libre_et_absent_sans_zenytt() {
        use crate::sites::VpsView;
        let t = Vps { source: Arc::new(VpsStatus::default) };
        assert_eq!(t.spec().permission, Permission::Free);
        assert!(t.run(&json!({})).await.is_err());
        let view = VpsView { server: "vps".into(), cpu_pct: 12.5, mem_pct: 40.0, disk_pct: 61.0, load1: 0.4, alerts: 1, at_ms: 1 };
        let t = Vps { source: Arc::new(move || VpsStatus { zenytt: true, view: Some(view.clone()), error: None }) };
        let out = t.run(&json!({})).await.unwrap().output;
        assert_eq!(out["vps"]["cpu_pct"], 12.5);
        assert_eq!(out["vps"]["alerts"], 1);
    }
    use crate::sites::SiteState;
    #[tokio::test] async fn libre_et_sans_url_dans_la_sortie() {
        let v = SiteView { name: "Blog".into(), url: "https://secret.example.com".into(), state: SiteState::Up, last_ms: Some(80), spark: vec![80] };
        let t = Status { source: Arc::new(move || Ok(vec![v.clone()])) };
        assert_eq!(t.spec().permission, Permission::Free);
        let out = t.run(&json!({})).await.unwrap().output;
        assert_eq!(out["sites"][0]["state"], "up");
        assert!(!out.to_string().contains("secret"));
    }
}
