//! Outils des rappels. Chaque outil signale `Topic::Reminders` ; la reprogrammation de l'échéance
//! se fait côté Tauri (`TauriEvents::changed` appelle `reschedule_reminders`), le cœur ignore l'ordonnanceur.
use super::{get, insert, set_deleted, set_done};
use crate::events::{Events, Topic};
use crate::tools::helpers::{bad, failed, low, parse, spec};
use crate::tools::{ConfirmationText, Permission, Tool, ToolError, ToolResult, ToolSpec};
use crate::when::{at_default_hour, parse_when};
use async_trait::async_trait;
use chrono::Local;
use parking_lot::Mutex;
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

type Db = Arc<Mutex<Connection>>;

pub struct Create { pub db: Db, pub events: Arc<dyn Events> }
pub struct Complete { pub db: Db, pub events: Arc<dyn Events> }
pub struct Delete { pub db: Db, pub events: Arc<dyn Events> }

#[derive(Deserialize)]
struct CreateArgs { text: String }
#[derive(Deserialize)]
struct IdArgs { id: i64 }
#[derive(Deserialize)]
struct CompleteArgs { id: i64, done: bool }

/// Texte et échéance : un jour sans heure donne 9 h ; sans date, pas d'échéance.
fn split(text: &str) -> (String, Option<i64>) {
    match parse_when(text, Local::now()) {
        Some(w) => {
            let title = if w.rest.trim().is_empty() { text.trim().to_string() } else { w.rest.clone() };
            (title, Some(at_default_hour(&w).timestamp_millis()))
        }
        None => (text.trim().to_string(), None),
    }
}

#[async_trait]
impl Tool for Create {
    fn spec(&self) -> ToolSpec {
        spec("reminders.create", "Crée un rappel : { text } en français (« demain appeler la banque »). Sans date, le rappel n'a pas d'échéance.",
            json!({"type":"object","required":["text"],"properties":{"text":{"type":"string"}}}), Permission::Free)
    }
    fn describe(&self, _: &Value) -> ConfirmationText { low("Créer un rappel") }
    async fn run(&self, args: &Value) -> Result<ToolResult, ToolError> {
        let a: CreateArgs = parse(args)?;
        let (text, due) = split(&a.text);
        if text.is_empty() { return Err(bad("texte manquant")); }
        let id = insert(&self.db.lock(), &text, due).map_err(failed)?;
        self.events.changed(Topic::Reminders);
        Ok(ToolResult { output: json!({ "id": id, "due_ms": due }), undo: Some(json!({ "id": id })) })
    }
    async fn undo(&self, undo: &Value) -> Result<(), ToolError> {
        set_deleted(&self.db.lock(), parse::<IdArgs>(undo)?.id, true).map_err(failed)?;
        self.events.changed(Topic::Reminders);
        Ok(())
    }
}

#[async_trait]
impl Tool for Complete {
    fn spec(&self) -> ToolSpec {
        spec("reminders.complete", "Coche ou décoche un rappel : { id, done }.",
            json!({"type":"object","required":["id","done"],"properties":{"id":{"type":"integer"},"done":{"type":"boolean"}}}), Permission::Free)
    }
    fn describe(&self, _: &Value) -> ConfirmationText { low("Cocher un rappel") }
    async fn run(&self, args: &Value) -> Result<ToolResult, ToolError> {
        let a: CompleteArgs = parse(args)?;
        let before = {
            let c = self.db.lock();
            let before = get(&c, a.id).map_err(failed)?.ok_or_else(|| failed("rappel introuvable"))?;
            set_done(&c, a.id, a.done).map_err(failed)?;
            before
        };
        self.events.changed(Topic::Reminders);
        Ok(ToolResult { output: json!({ "id": a.id }), undo: Some(json!({ "id": a.id, "done": before.done })) })
    }
    async fn undo(&self, undo: &Value) -> Result<(), ToolError> {
        let a: CompleteArgs = parse(undo)?;
        set_done(&self.db.lock(), a.id, a.done).map_err(failed)?;
        self.events.changed(Topic::Reminders);
        Ok(())
    }
}

#[async_trait]
impl Tool for Delete {
    fn spec(&self) -> ToolSpec {
        spec("reminders.delete", "Supprime un rappel : { id } (annulable).",
            json!({"type":"object","required":["id"],"properties":{"id":{"type":"integer"}}}), Permission::Free)
    }
    fn describe(&self, _: &Value) -> ConfirmationText { low("Supprimer un rappel") }
    async fn run(&self, args: &Value) -> Result<ToolResult, ToolError> {
        let a: IdArgs = parse(args)?;
        {
            let c = self.db.lock();
            get(&c, a.id).map_err(failed)?.ok_or_else(|| failed("rappel introuvable"))?;
            set_deleted(&c, a.id, true).map_err(failed)?;
        }
        self.events.changed(Topic::Reminders);
        Ok(ToolResult { output: json!({ "id": a.id }), undo: Some(json!({ "id": a.id })) })
    }
    async fn undo(&self, undo: &Value) -> Result<(), ToolError> {
        set_deleted(&self.db.lock(), parse::<IdArgs>(undo)?.id, false).map_err(failed)?;
        self.events.changed(Topic::Reminders);
        Ok(())
    }
}

/// Les trois outils des rappels, pour `AppState::open`.
pub fn all(db: &Db, events: &Arc<dyn Events>) -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(Create { db: db.clone(), events: events.clone() }),
        Arc::new(Complete { db: db.clone(), events: events.clone() }),
        Arc::new(Delete { db: db.clone(), events: events.clone() }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reminders::{list_open, tests::db};
    use crate::tools::{Origin, Provenance, ToolBroker, ToolCall, ToolOutcome};
    use chrono::{Duration, Timelike};
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct Count(AtomicUsize);
    impl Events for Count {
        fn changed(&self, t: Topic) { assert_eq!(t, Topic::Reminders); self.0.fetch_add(1, Ordering::SeqCst); }
    }
    fn setup() -> (tempfile::TempDir, Db, Arc<Count>, ToolBroker) {
        let (dir, c) = db();
        let db: Db = Arc::new(Mutex::new(c));
        let count = Arc::new(Count::default());
        let ev: Arc<dyn Events> = count.clone();
        let broker = ToolBroker::new(db.clone(), Arc::new(|| 0), all(&db, &ev)).unwrap();
        (dir, db, count, broker)
    }
    fn call(tool: &str, args: Value) -> ToolCall {
        ToolCall { tool: tool.into(), args, provenance: Provenance::User, origin: Origin::User }
    }

    #[tokio::test]
    async fn create_demain_donne_le_lendemain_a_9h() {
        let (_d, db, count, broker) = setup();
        let out = broker.call(call("reminders.create", json!({"text": "demain appeler la banque"}))).await;
        assert!(matches!(out, ToolOutcome::Done { .. }), "{out:?}");
        let r = &list_open(&db.lock()).unwrap()[0];
        assert!(r.text.contains("appeler la banque"));
        let due = chrono::DateTime::from_timestamp_millis(r.due_ms.unwrap()).unwrap().with_timezone(&Local);
        assert_eq!(due.date_naive(), (Local::now() + Duration::days(1)).date_naive());
        assert_eq!((due.hour(), due.minute()), (9, 0));
        assert_eq!(count.0.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn create_sans_date_n_a_pas_d_echeance_et_undo_supprime() {
        let (_d, db, _c, broker) = setup();
        let ToolOutcome::Done { action_id, .. } = broker.call(call("reminders.create", json!({"text": "penser au pain"}))).await else { panic!() };
        let r = list_open(&db.lock()).unwrap();
        assert_eq!((r[0].text.as_str(), r[0].due_ms), ("penser au pain", None));
        broker.undo(action_id).await.unwrap();
        assert!(list_open(&db.lock()).unwrap().is_empty());
    }

    #[tokio::test]
    async fn complete_puis_undo() {
        let (_d, db, count, broker) = setup();
        let id = insert(&db.lock(), "x", Some(10)).unwrap();
        let ToolOutcome::Done { action_id, .. } = broker.call(call("reminders.complete", json!({"id": id, "done": true}))).await else { panic!() };
        assert!(list_open(&db.lock()).unwrap().is_empty());
        broker.undo(action_id).await.unwrap();
        assert_eq!(list_open(&db.lock()).unwrap().len(), 1);
        assert_eq!(count.0.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn delete_puis_undo_et_rappel_inconnu() {
        let (_d, db, _c, broker) = setup();
        let id = insert(&db.lock(), "x", None).unwrap();
        let ToolOutcome::Done { action_id, .. } = broker.call(call("reminders.delete", json!({"id": id}))).await else { panic!() };
        assert!(list_open(&db.lock()).unwrap().is_empty());
        broker.undo(action_id).await.unwrap();
        assert_eq!(list_open(&db.lock()).unwrap().len(), 1);
        assert!(matches!(broker.call(call("reminders.delete", json!({"id": 99}))).await, ToolOutcome::Failed { .. }));
    }
}
