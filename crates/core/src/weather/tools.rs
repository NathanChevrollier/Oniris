//! Outil `weather.today` : renvoie la dernière météo en cache (aucune requête).
use crate::kv;
use crate::tools::helpers::{failed, low, spec};
use crate::tools::{ConfirmationText, Permission, Tool, ToolError, ToolResult, ToolSpec};
use async_trait::async_trait;
use crate::Db;
use serde_json::{json, Value};
use std::sync::Arc;

/// Clé `kv` de la dernière météo (JSON de `Weather`).
pub const LAST_KEY: &str = "weather.last";
/// Clé `kv` du lieu géocodé (JSON de `Place`).
pub const PLACE_KEY: &str = "weather.place";

pub struct Today { pub db: Db }

#[async_trait]
impl Tool for Today {
    fn spec(&self) -> ToolSpec {
        spec("weather.today", "Météo du jour (dernière valeur connue) : température, conditions, pluie à venir.", json!({"type":"object","properties":{}}), Permission::Free)
    }
    fn describe(&self, _: &Value) -> ConfirmationText { low("Lire la météo") }
    async fn run(&self, _: &Value) -> Result<ToolResult, ToolError> {
        let cached = kv::get(&self.db.lock(), LAST_KEY).map_err(failed)?;
        let output = match cached {
            Some((json, _)) => serde_json::from_str(&json).map_err(failed)?,
            None => Value::Null,
        };
        Ok(ToolResult { output, undo: None })
    }
}

pub fn all(db: &Db) -> Vec<Arc<dyn Tool>> {
    vec![Arc::new(Today { db: db.clone() })]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::db;
    use crate::tools::{Origin, Provenance, ToolBroker, ToolCall, ToolOutcome};

    #[tokio::test]
    async fn weather_today_libre_renvoie_le_cache_ou_null() {
        let c = db();
        let db = Arc::new(parking_lot::Mutex::new(c));
        let broker = ToolBroker::new(db.clone(), Arc::new(|| 0), all(&db)).unwrap();
        let call = || ToolCall { tool: "weather.today".into(), args: json!({}), provenance: Provenance::User, origin: Origin::User };
        let ToolOutcome::Done { output, .. } = broker.call(call()).await else { panic!("Free : exécuté sans confirmation") };
        assert!(output.is_null());
        kv::set(&db.lock(), LAST_KEY, r#"{"place":"Paris","temp_c":12.0}"#, 1).unwrap();
        let ToolOutcome::Done { output, .. } = broker.call(call()).await else { panic!() };
        assert_eq!(output["place"], "Paris");
    }
}
