//! Outils des alarmes. Ils signalent `Topic::Timers` (même carte et même pilule que les minuteurs) ;
//! la reprogrammation se fait côté Tauri.
use super::{add, cancel, list, parse_alarm};
use crate::events::{Events, Topic};
use crate::tools::helpers::{bad, failed, low, parse, spec, IdArgs};
use crate::tools::{ConfirmationText, Permission, Tool, ToolError, ToolResult, ToolSpec};
use crate::Db;
use async_trait::async_trait;
use chrono::{Local, TimeZone};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

pub struct SetAlarm { pub db: Db, pub events: Arc<dyn Events> }
pub struct ListAlarms { pub db: Db }
pub struct CancelAlarm { pub db: Db, pub events: Arc<dyn Events> }

#[derive(Deserialize)]
struct TextArgs { text: String }

/// « 2026-11-18 07:00 » : lisible par le modèle sans calcul de fuseau.
fn when(ms: i64) -> String {
    Local.timestamp_millis_opt(ms).single().map(|d| d.format("%Y-%m-%d %H:%M").to_string()).unwrap_or_default()
}

#[async_trait]
impl Tool for SetAlarm {
    fn spec(&self) -> ToolSpec {
        spec("alarm.set", "Programme une alarme qui sonne à une heure donnée : { text } en français (« réveille-moi à 7 h », « alarme à 14 h 30 demain », « en semaine à 6h45 », « tous les lundis à 8h piscine »).",
            json!({"type":"object","required":["text"],"properties":{"text":{"type":"string"}}}), Permission::Free)
    }
    fn describe(&self, _: &Value) -> ConfirmationText { low("Programmer une alarme") }
    async fn run(&self, args: &Value) -> Result<ToolResult, ToolError> {
        let a: TextArgs = parse(args)?;
        let s = parse_alarm(&a.text).ok_or_else(|| bad("heure introuvable"))?;
        let al = add(&self.db.lock(), &s).map_err(failed)?;
        self.events.changed(Topic::Timers);
        Ok(ToolResult {
            output: json!({ "id": al.id, "label": al.label, "next": when(al.next_ms), "repeat": al.repeat }),
            undo: Some(json!({ "id": al.id })),
        })
    }
    async fn undo(&self, undo: &Value) -> Result<(), ToolError> {
        cancel(&self.db.lock(), parse::<IdArgs>(undo)?.id).map_err(failed)?;
        self.events.changed(Topic::Timers);
        Ok(())
    }
}

#[async_trait]
impl Tool for ListAlarms {
    fn spec(&self) -> ToolSpec {
        spec("alarm.list", "Liste les alarmes programmées (id, libellé, prochaine sonnerie, répétition).", json!({"type":"object","properties":{}}), Permission::Free)
    }
    fn describe(&self, _: &Value) -> ConfirmationText { low("Lister les alarmes") }
    async fn run(&self, _: &Value) -> Result<ToolResult, ToolError> {
        let l = list(&self.db.lock()).map_err(failed)?;
        let out: Vec<Value> = l.iter().map(|a| json!({ "id": a.id, "label": a.label, "next": when(a.next_ms), "repeat": a.repeat })).collect();
        Ok(ToolResult { output: json!(out), undo: None })
    }
}

#[async_trait]
impl Tool for CancelAlarm {
    fn spec(&self) -> ToolSpec {
        spec("alarm.cancel", "Supprime une alarme : { id } (voir alarm.list).",
            json!({"type":"object","required":["id"],"properties":{"id":{"type":"integer"}}}), Permission::Free)
    }
    fn describe(&self, _: &Value) -> ConfirmationText { low("Supprimer une alarme") }
    async fn run(&self, args: &Value) -> Result<ToolResult, ToolError> {
        let a: IdArgs = parse(args)?;
        if !cancel(&self.db.lock(), a.id).map_err(failed)? { return Err(failed("alarme introuvable")); }
        self.events.changed(Topic::Timers);
        Ok(ToolResult { output: json!({ "id": a.id }), undo: None })
    }
}

pub fn all(db: &Db, events: &Arc<dyn Events>) -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(SetAlarm { db: db.clone(), events: events.clone() }),
        Arc::new(ListAlarms { db: db.clone() }),
        Arc::new(CancelAlarm { db: db.clone(), events: events.clone() }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::NoEvents;
    use crate::test_util::db;
    use crate::tools::{Origin, Provenance, ToolBroker, ToolCall, ToolOutcome};
    use parking_lot::Mutex;

    fn call(tool: &str, args: Value) -> ToolCall {
        ToolCall { tool: tool.into(), args, provenance: Provenance::User, origin: Origin::User }
    }

    #[tokio::test]
    async fn programmer_lister_annuler_et_undo() {
        let db: Db = Arc::new(Mutex::new(db()));
        let ev: Arc<dyn Events> = Arc::new(NoEvents);
        let b = ToolBroker::new(db.clone(), Arc::new(|| 0), all(&db, &ev)).unwrap();
        let ToolOutcome::Done { action_id, output } = b.call(call("alarm.set", json!({"text": "réveille-moi en semaine à 7 h"}))).await else { panic!() };
        assert_eq!((output["label"].as_str(), output["repeat"].as_str()), (Some("Réveil"), Some("en semaine")));
        let ToolOutcome::Done { output, .. } = b.call(call("alarm.list", json!({}))).await else { panic!() };
        assert_eq!(output.as_array().unwrap().len(), 1);
        b.undo(action_id).await.unwrap();
        assert!(list(&db.lock()).unwrap().is_empty());
        let out = b.call(call("alarm.set", json!({"text": "alarme demain"}))).await;
        assert!(matches!(&out, ToolOutcome::Failed { error, .. } if error.contains("heure introuvable")), "{out:?}");
        let ToolOutcome::Done { output, .. } = b.call(call("alarm.set", json!({"text": "alarme à 9h"}))).await else { panic!() };
        let id = output["id"].as_i64().unwrap();
        assert!(matches!(b.call(call("alarm.cancel", json!({"id": id}))).await, ToolOutcome::Done { .. }));
        assert!(matches!(b.call(call("alarm.cancel", json!({"id": id}))).await, ToolOutcome::Failed { .. }));
    }
}
