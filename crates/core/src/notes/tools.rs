//! Outils des notes. `to_reminder` réutilise l'analyse de date et l'insertion des rappels et signale `Topic::Reminders`
//! (la reprogrammation de l'échéance se fait côté Tauri). Le verrou de la base n'est jamais tenu au-delà d'un bloc synchrone.
use super::{get, insert, search, set_deleted, set_pinned, Note};
use crate::events::{Events, Topic};
use crate::reminders;
use crate::tools::helpers::{bad, failed, low, parse, spec};
use crate::tools::{ConfirmationText, Permission, Tool, ToolError, ToolResult, ToolSpec};
use async_trait::async_trait;
use parking_lot::Mutex;
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

type Db = Arc<Mutex<Connection>>;

pub struct Create { pub db: Db, pub events: Arc<dyn Events> }
pub struct Search { pub db: Db }
pub struct Delete { pub db: Db, pub events: Arc<dyn Events> }
pub struct Pin { pub db: Db, pub events: Arc<dyn Events> }
pub struct ToReminder { pub db: Db, pub events: Arc<dyn Events> }

#[derive(Deserialize)]
struct CreateArgs { text: String }
#[derive(Deserialize)]
struct SearchArgs { query: String }
#[derive(Deserialize)]
struct IdArgs { id: i64 }
#[derive(Deserialize)]
struct PinArgs { id: i64, pinned: bool }

const SEARCH_LIMIT: usize = 50;

#[async_trait]
impl Tool for Create {
    fn spec(&self) -> ToolSpec {
        spec("notes.create", "Crée une note rapide : { text }.",
            json!({"type":"object","required":["text"],"properties":{"text":{"type":"string"}}}), Permission::Free)
    }
    fn describe(&self, _: &Value) -> ConfirmationText { low("Créer une note") }
    async fn run(&self, args: &Value) -> Result<ToolResult, ToolError> {
        let a: CreateArgs = parse(args)?;
        let body = a.text.trim();
        if body.is_empty() { return Err(bad("texte manquant")); }
        let id = insert(&self.db.lock(), body).map_err(failed)?;
        self.events.changed(Topic::Notes);
        Ok(ToolResult { output: json!({ "id": id }), undo: Some(json!({ "id": id })) })
    }
    async fn undo(&self, undo: &Value) -> Result<(), ToolError> {
        set_deleted(&self.db.lock(), parse::<IdArgs>(undo)?.id, true).map_err(failed)?;
        self.events.changed(Topic::Notes);
        Ok(())
    }
}

#[async_trait]
impl Tool for Search {
    fn spec(&self) -> ToolSpec {
        spec("notes.search", "Cherche dans les notes : { query } (préfixe de chaque mot). Requête vide : notes récentes.",
            json!({"type":"object","required":["query"],"properties":{"query":{"type":"string"}}}), Permission::Free)
    }
    fn describe(&self, _: &Value) -> ConfirmationText { low("Chercher dans les notes") }
    async fn run(&self, args: &Value) -> Result<ToolResult, ToolError> {
        let a: SearchArgs = parse(args)?;
        let list: Vec<Note> = search(&self.db.lock(), &a.query, SEARCH_LIMIT).map_err(failed)?;
        Ok(ToolResult { output: json!(list), undo: None })
    }
}

#[async_trait]
impl Tool for Delete {
    fn spec(&self) -> ToolSpec {
        spec("notes.delete", "Supprime une note : { id } (annulable).",
            json!({"type":"object","required":["id"],"properties":{"id":{"type":"integer"}}}), Permission::Free)
    }
    fn describe(&self, _: &Value) -> ConfirmationText { low("Supprimer une note") }
    async fn run(&self, args: &Value) -> Result<ToolResult, ToolError> {
        let a: IdArgs = parse(args)?;
        {
            let c = self.db.lock();
            get(&c, a.id).map_err(failed)?.ok_or_else(|| failed("note introuvable"))?;
            set_deleted(&c, a.id, true).map_err(failed)?;
        }
        self.events.changed(Topic::Notes);
        Ok(ToolResult { output: json!({ "id": a.id }), undo: Some(json!({ "id": a.id })) })
    }
    async fn undo(&self, undo: &Value) -> Result<(), ToolError> {
        set_deleted(&self.db.lock(), parse::<IdArgs>(undo)?.id, false).map_err(failed)?;
        self.events.changed(Topic::Notes);
        Ok(())
    }
}

#[async_trait]
impl Tool for Pin {
    fn spec(&self) -> ToolSpec {
        spec("notes.pin", "Épingle ou désépingle une note : { id, pinned }.",
            json!({"type":"object","required":["id","pinned"],"properties":{"id":{"type":"integer"},"pinned":{"type":"boolean"}}}), Permission::Free)
    }
    fn describe(&self, _: &Value) -> ConfirmationText { low("Épingler une note") }
    async fn run(&self, args: &Value) -> Result<ToolResult, ToolError> {
        let a: PinArgs = parse(args)?;
        let before = {
            let c = self.db.lock();
            let before = get(&c, a.id).map_err(failed)?.ok_or_else(|| failed("note introuvable"))?;
            set_pinned(&c, a.id, a.pinned).map_err(failed)?;
            before
        };
        self.events.changed(Topic::Notes);
        Ok(ToolResult { output: json!({ "id": a.id }), undo: Some(json!({ "id": a.id, "pinned": before.pinned })) })
    }
    async fn undo(&self, undo: &Value) -> Result<(), ToolError> {
        let a: PinArgs = parse(undo)?;
        set_pinned(&self.db.lock(), a.id, a.pinned).map_err(failed)?;
        self.events.changed(Topic::Notes);
        Ok(())
    }
}

#[async_trait]
impl Tool for ToReminder {
    fn spec(&self) -> ToolSpec {
        spec("notes.to_reminder", "Crée un rappel à partir du texte d'une note : { id }. Un jour sans heure donne 9 h.",
            json!({"type":"object","required":["id"],"properties":{"id":{"type":"integer"}}}), Permission::Free)
    }
    fn describe(&self, _: &Value) -> ConfirmationText { low("Transformer une note en rappel") }
    async fn run(&self, args: &Value) -> Result<ToolResult, ToolError> {
        let a: IdArgs = parse(args)?;
        let rid = {
            let c = self.db.lock();
            let note = get(&c, a.id).map_err(failed)?.ok_or_else(|| failed("note introuvable"))?;
            let (text, due) = reminders::tools::split(&note.body);
            reminders::insert(&c, &text, due).map_err(failed)?
        };
        self.events.changed(Topic::Reminders);
        Ok(ToolResult { output: json!({ "id": rid }), undo: Some(json!({ "id": rid })) })
    }
    async fn undo(&self, undo: &Value) -> Result<(), ToolError> {
        reminders::set_deleted(&self.db.lock(), parse::<IdArgs>(undo)?.id, true).map_err(failed)?;
        self.events.changed(Topic::Reminders);
        Ok(())
    }
}

/// Les cinq outils des notes, pour `AppState::open`.
pub fn all(db: &Db, events: &Arc<dyn Events>) -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(Create { db: db.clone(), events: events.clone() }),
        Arc::new(Search { db: db.clone() }),
        Arc::new(Delete { db: db.clone(), events: events.clone() }),
        Arc::new(Pin { db: db.clone(), events: events.clone() }),
        Arc::new(ToReminder { db: db.clone(), events: events.clone() }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::{recent, tests::db};
    use crate::tools::{Origin, Provenance, ToolBroker, ToolCall, ToolOutcome};
    use chrono::{Local, TimeZone, Timelike};
    use std::sync::Mutex as StdMutex;

    #[derive(Default)]
    struct Log(StdMutex<Vec<Topic>>);
    impl Events for Log { fn changed(&self, t: Topic) { self.0.lock().unwrap().push(t); } }

    fn setup() -> (tempfile::TempDir, Db, Arc<Log>, ToolBroker) {
        let (dir, c) = db();
        let db: Db = Arc::new(Mutex::new(c));
        let log = Arc::new(Log::default());
        let ev: Arc<dyn Events> = log.clone();
        let broker = ToolBroker::new(db.clone(), Arc::new(|| 0), all(&db, &ev)).unwrap();
        (dir, db, log, broker)
    }
    fn call(tool: &str, args: Value) -> ToolCall {
        ToolCall { tool: tool.into(), args, provenance: Provenance::User, origin: Origin::User }
    }

    #[tokio::test]
    async fn create_puis_undo() {
        let (_d, db, log, broker) = setup();
        let ToolOutcome::Done { action_id, .. } = broker.call(call("notes.create", json!({"text": "  idée  "}))).await else { panic!() };
        assert_eq!(recent(&db.lock(), 50).unwrap()[0].body, "idée");
        broker.undo(action_id).await.unwrap();
        assert!(recent(&db.lock(), 50).unwrap().is_empty());
        assert_eq!(*log.0.lock().unwrap(), vec![Topic::Notes, Topic::Notes]);
        assert!(matches!(broker.call(call("notes.create", json!({"text": "  "}))).await, ToolOutcome::Failed { .. }));
    }

    #[tokio::test]
    async fn search_et_delete_puis_undo() {
        let (_d, db, _l, broker) = setup();
        let id = insert(&db.lock(), "Réunion client").unwrap();
        let ToolOutcome::Done { output, .. } = broker.call(call("notes.search", json!({"query": "reunion"}))).await else { panic!() };
        assert_eq!(output.as_array().unwrap().len(), 1);
        let ToolOutcome::Done { action_id, .. } = broker.call(call("notes.delete", json!({"id": id}))).await else { panic!() };
        let ToolOutcome::Done { output, .. } = broker.call(call("notes.search", json!({"query": "reunion"}))).await else { panic!() };
        assert!(output.as_array().unwrap().is_empty());
        broker.undo(action_id).await.unwrap();
        assert_eq!(recent(&db.lock(), 50).unwrap().len(), 1);
        assert!(matches!(broker.call(call("notes.delete", json!({"id": 99}))).await, ToolOutcome::Failed { .. }));
    }

    #[tokio::test]
    async fn pin_puis_undo() {
        let (_d, db, _l, broker) = setup();
        let a = insert(&db.lock(), "a").unwrap();
        insert(&db.lock(), "b").unwrap();
        let ToolOutcome::Done { action_id, .. } = broker.call(call("notes.pin", json!({"id": a, "pinned": true}))).await else { panic!() };
        assert!(recent(&db.lock(), 50).unwrap()[0].pinned);
        broker.undo(action_id).await.unwrap();
        assert!(!recent(&db.lock(), 50).unwrap()[0].pinned);
    }

    #[tokio::test]
    async fn to_reminder_puis_undo() {
        let (_d, db, log, broker) = setup();
        let id = insert(&db.lock(), "demain appeler la banque").unwrap();
        let ToolOutcome::Done { action_id, .. } = broker.call(call("notes.to_reminder", json!({"id": id}))).await else { panic!() };
        let open = reminders::list_open(&db.lock()).unwrap();
        assert_eq!(open.len(), 1);
        assert!(open[0].text.contains("banque"));
        let due = Local.timestamp_millis_opt(open[0].due_ms.unwrap()).unwrap();
        assert_eq!(due.hour(), 9, "un jour sans heure donne 9 h");
        assert_eq!(recent(&db.lock(), 50).unwrap().len(), 1, "la note reste");
        broker.undo(action_id).await.unwrap();
        assert!(reminders::list_open(&db.lock()).unwrap().is_empty());
        assert_eq!(*log.0.lock().unwrap(), vec![Topic::Reminders, Topic::Reminders]);
    }

    #[tokio::test]
    async fn to_reminder_sans_date_et_note_inconnue() {
        let (_d, db, _l, broker) = setup();
        let id = insert(&db.lock(), "penser au pain").unwrap();
        broker.call(call("notes.to_reminder", json!({"id": id}))).await;
        assert_eq!(reminders::list_open(&db.lock()).unwrap()[0].due_ms, None);
        assert!(matches!(broker.call(call("notes.to_reminder", json!({"id": 99}))).await, ToolOutcome::Failed { .. }));
    }
}
