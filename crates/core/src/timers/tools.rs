//! Outils des minuteurs. Chaque outil signale `Topic::Timers` ; la reprogrammation se fait côté Tauri.
use super::{cancel, find_duration, start};
use crate::events::{Events, Topic};
use crate::load_settings;
use crate::tools::helpers::{bad, failed, low, parse, spec};
use crate::tools::{ConfirmationText, Permission, Tool, ToolError, ToolResult, ToolSpec};
use async_trait::async_trait;
use chrono::Local;
use parking_lot::Mutex;
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

type Db = Arc<Mutex<Connection>>;

pub struct StartTimer { pub db: Db, pub events: Arc<dyn Events> }
pub struct CancelTimer { pub db: Db, pub events: Arc<dyn Events> }
pub struct StartFocus { pub db: Db, pub events: Arc<dyn Events> }

#[derive(Deserialize)]
struct TextArgs { text: String }
#[derive(Deserialize)]
struct IdArgs { id: i64 }

fn now_ms() -> i64 { Local::now().timestamp_millis() }

/// Libellé : le texte sans la durée ni les mots « minuteur » / « timer » ; « Minuteur » s'il ne reste rien.
fn label_of(text: &str, dur: std::ops::Range<usize>) -> String {
    let rest = format!("{} {}", &text[..dur.start], &text[dur.end..]);
    let label = rest.split_whitespace()
        .filter(|w| !matches!(w.to_lowercase().as_str(), "minuteur" | "timer"))
        .collect::<Vec<_>>().join(" ");
    if label.is_empty() { "Minuteur".into() } else { label }
}

/// Annulation d'un minuteur ou d'un focus lancé par un outil.
fn undo_start(db: &Db, events: &Arc<dyn Events>, undo: &Value) -> Result<(), ToolError> {
    cancel(&db.lock(), parse::<IdArgs>(undo)?.id).map_err(failed)?;
    events.changed(Topic::Timers);
    Ok(())
}

#[async_trait]
impl Tool for StartTimer {
    fn spec(&self) -> ToolSpec {
        spec("timer.start", "Lance un minuteur : { text } en français (« minuteur 25 minutes thé »).",
            json!({"type":"object","required":["text"],"properties":{"text":{"type":"string"}}}), Permission::Free)
    }
    fn describe(&self, _: &Value) -> ConfirmationText { low("Lancer un minuteur") }
    async fn run(&self, args: &Value) -> Result<ToolResult, ToolError> {
        let a: TextArgs = parse(args)?;
        let (ms, range) = find_duration(&a.text).ok_or_else(|| bad("durée introuvable"))?;
        let t = start(&self.db.lock(), &label_of(&a.text, range), "timer", ms as i64, now_ms()).map_err(failed)?;
        self.events.changed(Topic::Timers);
        Ok(ToolResult { output: json!({ "id": t.id, "ends_ms": t.ends_ms }), undo: Some(json!({ "id": t.id })) })
    }
    async fn undo(&self, undo: &Value) -> Result<(), ToolError> { undo_start(&self.db, &self.events, undo) }
}

#[async_trait]
impl Tool for CancelTimer {
    fn spec(&self) -> ToolSpec {
        spec("timer.cancel", "Annule un minuteur en cours : { id }.",
            json!({"type":"object","required":["id"],"properties":{"id":{"type":"integer"}}}), Permission::Free)
    }
    fn describe(&self, _: &Value) -> ConfirmationText { low("Annuler un minuteur") }
    async fn run(&self, args: &Value) -> Result<ToolResult, ToolError> {
        let a: IdArgs = parse(args)?;
        if !cancel(&self.db.lock(), a.id).map_err(failed)? { return Err(failed("minuteur introuvable")); }
        self.events.changed(Topic::Timers);
        Ok(ToolResult { output: json!({ "id": a.id }), undo: None })
    }
}

#[async_trait]
impl Tool for StartFocus {
    fn spec(&self) -> ToolSpec {
        spec("focus.start", "Lance une session de focus (durée des réglages), suivie d'une pause.",
            json!({"type":"object","properties":{}}), Permission::Free)
    }
    fn describe(&self, _: &Value) -> ConfirmationText { low("Lancer un focus") }
    async fn run(&self, _: &Value) -> Result<ToolResult, ToolError> {
        let t = {
            let c = self.db.lock();
            let minutes = load_settings(&c).map_err(failed)?.focus_minutes;
            start(&c, "Focus", "focus", i64::from(minutes) * 60_000, now_ms()).map_err(failed)?
        };
        self.events.changed(Topic::Timers);
        Ok(ToolResult { output: json!({ "id": t.id, "ends_ms": t.ends_ms }), undo: Some(json!({ "id": t.id })) })
    }
    async fn undo(&self, undo: &Value) -> Result<(), ToolError> { undo_start(&self.db, &self.events, undo) }
}

/// Les trois outils des minuteurs, pour `AppState::open`.
pub fn all(db: &Db, events: &Arc<dyn Events>) -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(StartTimer { db: db.clone(), events: events.clone() }),
        Arc::new(CancelTimer { db: db.clone(), events: events.clone() }),
        Arc::new(StartFocus { db: db.clone(), events: events.clone() }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::NoEvents;
    use crate::timers::{find_duration, running, tests::db};
    use crate::tools::{Origin, Provenance, ToolBroker, ToolCall, ToolOutcome};

    fn setup() -> (tempfile::TempDir, Db, ToolBroker) {
        let (dir, c) = db();
        let db: Db = Arc::new(Mutex::new(c));
        let ev: Arc<dyn Events> = Arc::new(NoEvents);
        let broker = ToolBroker::new(db.clone(), Arc::new(|| 0), all(&db, &ev)).unwrap();
        (dir, db, broker)
    }
    fn call(tool: &str, args: Value) -> ToolCall {
        ToolCall { tool: tool.into(), args, provenance: Provenance::User, origin: Origin::User }
    }

    #[test]
    fn libelle() {
        let l = |t: &str| { let (_, r) = find_duration(t).unwrap(); label_of(t, r) };
        assert_eq!(l("minuteur 25 minutes thé"), "thé");
        assert_eq!(l("25 min"), "Minuteur");
        assert_eq!(l("Timer 1h30 pâtes"), "pâtes");
    }

    #[tokio::test]
    async fn start_puis_undo_et_duree_introuvable() {
        let (_d, db, broker) = setup();
        let ToolOutcome::Done { action_id, .. } = broker.call(call("timer.start", json!({"text": "minuteur 25 minutes thé"}))).await else { panic!() };
        let r = running(&db.lock()).unwrap();
        assert_eq!((r[0].label.as_str(), r[0].duration_ms), ("thé", 25 * 60_000));
        broker.undo(action_id).await.unwrap();
        assert!(running(&db.lock()).unwrap().is_empty());
        let out = broker.call(call("timer.start", json!({"text": "minuteur thé"}))).await;
        assert!(matches!(&out, ToolOutcome::Failed { error, .. } if error.contains("durée introuvable")), "{out:?}");
    }

    #[tokio::test]
    async fn focus_utilise_les_reglages_et_cancel_annule() {
        let (_d, db, broker) = setup();
        let ToolOutcome::Done { output, .. } = broker.call(call("focus.start", json!({}))).await else { panic!() };
        let r = running(&db.lock()).unwrap();
        assert_eq!((r[0].kind.as_str(), r[0].duration_ms), ("focus", 25 * 60_000));
        let id = output["id"].as_i64().unwrap();
        assert!(matches!(broker.call(call("timer.cancel", json!({"id": id}))).await, ToolOutcome::Done { .. }));
        assert!(running(&db.lock()).unwrap().is_empty());
        assert!(matches!(broker.call(call("timer.cancel", json!({"id": id}))).await, ToolOutcome::Failed { .. }));
    }
}
