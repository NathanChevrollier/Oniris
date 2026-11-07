//! Outils de l'agenda. Le verrou de la base n'est jamais tenu au-delà d'un bloc synchrone (aucun `.await`).
use super::{between, get, insert, set_deleted, update_times, Event, NewEvent};
use crate::events::{Events, Topic};
use crate::time_util::local_day_bounds;
use crate::tools::helpers::{bad, failed, low, parse, spec, untrusted, IdArgs};
use crate::tools::{ConfirmationText, Permission, Risk, Tool, ToolError, ToolResult, ToolSpec};
use crate::when::{at_default_hour, parse_when};
use async_trait::async_trait;
use chrono::Local;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::Db;

pub struct CreateEvent { pub db: Db, pub events: Arc<dyn Events> }
pub struct MoveEvent { pub db: Db, pub events: Arc<dyn Events> }
pub struct DeleteEvent { pub db: Db, pub events: Arc<dyn Events> }
pub struct ListEvents { pub db: Db }

#[derive(Deserialize)]
struct CreateArgs { text: Option<String>, title: Option<String>, start_ms: Option<i64>, end_ms: Option<i64>, all_day: Option<bool> }

impl CreateArgs {
    fn into_new(self) -> Result<NewEvent, ToolError> {
        let ev = if let Some(text) = self.text {
            let w = parse_when(&text, Local::now()).ok_or_else(|| bad("date introuvable"))?;
            let title = if w.rest.trim().is_empty() { text.trim().to_string() } else { w.rest.clone() };
            NewEvent {
                title,
                start_ms: if w.all_day { w.start } else { at_default_hour(&w) }.timestamp_millis(),
                end_ms: w.end.map(|e| e.timestamp_millis()),
                all_day: w.all_day,
                ..Default::default()
            }
        } else {
            let title = self.title.filter(|t| !t.trim().is_empty()).ok_or_else(|| bad("titre manquant"))?;
            let start_ms = self.start_ms.ok_or_else(|| bad("start_ms manquant"))?;
            if self.end_ms.is_some_and(|e| e < start_ms) { return Err(bad("la fin précède le début")); }
            NewEvent { title, start_ms, end_ms: self.end_ms, all_day: self.all_day.unwrap_or(false), ..Default::default() }
        };
        // Journée entière : minuit local → minuit local suivant (23 h ou 25 h les jours de changement d'heure).
        Ok(if ev.all_day {
            let (start_ms, end) = local_day_bounds(ev.start_ms);
            NewEvent { start_ms, end_ms: Some(end), ..ev }
        } else { ev })
    }
}

#[derive(Deserialize)]
struct MoveArgs { id: i64, start_ms: i64, end_ms: Option<i64> }
#[derive(Deserialize)]
struct DeleteArgs { id: i64, key: Option<String> }
#[derive(Deserialize)]
struct ListArgs { from_ms: i64, to_ms: i64 }

#[async_trait]
impl Tool for CreateEvent {
    fn spec(&self) -> ToolSpec {
        spec("agenda.create_event", "Crée un événement : { text } en français (« demain 14h dentiste ») ou { title, start_ms, end_ms?, all_day? }.",
            json!({"type":"object","properties":{"text":{"type":"string"},"title":{"type":"string"},"start_ms":{"type":"integer"},"end_ms":{"type":"integer"},"all_day":{"type":"boolean"}}}),
            Permission::Free)
    }
    fn describe(&self, _: &Value) -> ConfirmationText { low("Créer un événement") }
    async fn run(&self, args: &Value) -> Result<ToolResult, ToolError> {
        let new = parse::<CreateArgs>(args)?.into_new()?;
        let id = insert(&self.db.lock(), &new).map_err(failed)?;
        self.events.changed(Topic::Agenda);
        Ok(ToolResult { output: json!({ "id": id }), undo: Some(json!({ "id": id })) })
    }
    async fn undo(&self, undo: &Value) -> Result<(), ToolError> {
        set_deleted(&self.db.lock(), parse::<IdArgs>(undo)?.id, true).map_err(failed)?;
        self.events.changed(Topic::Agenda);
        Ok(())
    }
}

#[async_trait]
impl Tool for MoveEvent {
    fn spec(&self) -> ToolSpec {
        spec("agenda.move_event", "Déplace un événement : { id, start_ms, end_ms? }.",
            json!({"type":"object","required":["id","start_ms"],"properties":{"id":{"type":"integer"},"start_ms":{"type":"integer"},"end_ms":{"type":"integer"}}}),
            Permission::Free)
    }
    fn describe(&self, _: &Value) -> ConfirmationText { low("Déplacer un événement") }
    async fn run(&self, args: &Value) -> Result<ToolResult, ToolError> {
        let a: MoveArgs = parse(args)?;
        if a.end_ms.is_some_and(|e| e < a.start_ms) { return Err(bad("la fin précède le début")); }
        let old = {
            let c = self.db.lock();
            let old = get(&c, a.id).map_err(failed)?.ok_or_else(|| failed("événement introuvable"))?;
            update_times(&c, a.id, a.start_ms, a.end_ms).map_err(failed)?;
            old
        };
        self.events.changed(Topic::Agenda);
        Ok(ToolResult {
            output: json!({ "id": a.id }),
            undo: Some(json!({ "id": a.id, "start_ms": old.start_ms, "end_ms": old.end_ms })),
        })
    }
    async fn undo(&self, undo: &Value) -> Result<(), ToolError> {
        let a: MoveArgs = parse(undo)?;
        update_times(&self.db.lock(), a.id, a.start_ms, a.end_ms).map_err(failed)?;
        self.events.changed(Topic::Agenda);
        Ok(())
    }
}

#[async_trait]
impl Tool for DeleteEvent {
    fn spec(&self) -> ToolSpec {
        spec("agenda.delete_event", "Supprime un événement : { id }. Demande confirmation.",
            json!({"type":"object","required":["id"],"properties":{"id":{"type":"integer"},"key":{"type":"string"}}}), Permission::Confirm)
    }
    fn check(&self, args: &Value) -> Result<(), ToolError> {
        let a: DeleteArgs = parse(args)?;
        // Les événements Google ont l'id 0 et une clé `g:…` ; un événement marqué lecture seule est refusé aussi.
        let readonly = a.id == 0 || a.key.as_deref().is_some_and(|k| k.starts_with("g:"))
            || get(&self.db.lock(), a.id).ok().flatten().is_some_and(|e| e.readonly);
        if readonly { Err(failed("Événement Google en lecture seule")) } else { Ok(()) }
    }
    fn describe(&self, args: &Value) -> ConfirmationText {
        let title = parse::<IdArgs>(args).ok()
            .and_then(|a| get(&self.db.lock(), a.id).ok().flatten())
            .map_or_else(|| "cet événement".to_string(), |e| format!("« {} »", e.title));
        ConfirmationText { summary: format!("Supprimer {title}"), details: "L'événement disparaît de l'agenda (annulable).".into(), risk: Risk::Medium }
    }
    async fn run(&self, args: &Value) -> Result<ToolResult, ToolError> {
        let a: IdArgs = parse(args)?;
        {
            let c = self.db.lock();
            get(&c, a.id).map_err(failed)?.ok_or_else(|| failed("événement introuvable"))?;
            set_deleted(&c, a.id, true).map_err(failed)?;
        }
        self.events.changed(Topic::Agenda);
        Ok(ToolResult { output: json!({ "id": a.id }), undo: Some(json!({ "id": a.id })) })
    }
    async fn undo(&self, undo: &Value) -> Result<(), ToolError> {
        set_deleted(&self.db.lock(), parse::<IdArgs>(undo)?.id, false).map_err(failed)?;
        self.events.changed(Topic::Agenda);
        Ok(())
    }
}

#[async_trait]
impl Tool for ListEvents {
    fn spec(&self) -> ToolSpec {
        // Contient aussi les titres des agendas Google : le journal ne garde que le nombre d'éléments.
        untrusted(ToolSpec { sensitive_output: true, ..spec("agenda.list", "Liste les événements qui chevauchent [from_ms, to_ms[.",
            json!({"type":"object","required":["from_ms","to_ms"],"properties":{"from_ms":{"type":"integer"},"to_ms":{"type":"integer"}}}),
            Permission::Free) })
    }
    fn describe(&self, _: &Value) -> ConfirmationText { low("Lire l'agenda") }
    async fn run(&self, args: &Value) -> Result<ToolResult, ToolError> {
        let a: ListArgs = parse(args)?;
        let list: Vec<Event> = between(&self.db.lock(), a.from_ms, a.to_ms).map_err(failed)?;
        Ok(ToolResult { output: json!(list), undo: None })
    }
}

/// Les quatre outils de l'agenda, pour `AppState::open`.
pub fn all(db: &Db, events: &Arc<dyn Events>) -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(CreateEvent { db: db.clone(), events: events.clone() }),
        Arc::new(MoveEvent { db: db.clone(), events: events.clone() }),
        Arc::new(DeleteEvent { db: db.clone(), events: events.clone() }),
        Arc::new(ListEvents { db: db.clone() }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;
    use crate::test_util::db;
    use crate::tools::{Origin, Provenance, ToolBroker, ToolCall, ToolOutcome};
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct Count(AtomicUsize);
    impl Events for Count {
        fn changed(&self, t: Topic) { assert_eq!(t, Topic::Agenda); self.0.fetch_add(1, Ordering::SeqCst); }
    }

    fn setup() -> (Db, Arc<Count>, ToolBroker) {
        let c = db();
        let db: Db = Arc::new(Mutex::new(c));
        let count = Arc::new(Count::default());
        let ev: Arc<dyn Events> = count.clone();
        let broker = ToolBroker::new(db.clone(), Arc::new(|| 0), all(&db, &ev)).unwrap();
        (db, count, broker)
    }
    fn call(tool: &str, args: Value) -> ToolCall {
        ToolCall { tool: tool.into(), args, provenance: Provenance::User, origin: Origin::User }
    }
    fn rows(db: &Db) -> Vec<Event> { between(&db.lock(), 0, i64::MAX).unwrap() }

    #[tokio::test]
    async fn create_avec_texte_cree_la_ligne_et_signale_le_sujet() {
        let (db, count, broker) = setup();
        let out = broker.call(call("agenda.create_event", json!({"text": "demain 14h dentiste"}))).await;
        assert!(matches!(out, ToolOutcome::Done { .. }), "{out:?}");
        let r = rows(&db);
        assert_eq!(r.len(), 1);
        assert!(r[0].title.contains("dentiste"));
        assert_eq!(count.0.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn create_avec_champs_explicites_puis_undo() {
        let (db, count, broker) = setup();
        let ToolOutcome::Done { action_id, .. } =
            broker.call(call("agenda.create_event", json!({"title": "Revue", "start_ms": 1000, "end_ms": 5000}))).await else { panic!() };
        assert_eq!(rows(&db)[0].end_ms, Some(5000));
        broker.undo(action_id).await.unwrap();
        assert!(rows(&db).is_empty());
        assert_eq!(count.0.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn journee_entiere_va_de_minuit_a_minuit() {
        let (db, _c, broker) = setup();
        let noon = Local::now().timestamp_millis();
        broker.call(call("agenda.create_event", json!({"title": "Congé", "start_ms": noon, "all_day": true}))).await;
        let e = &rows(&db)[0];
        assert_eq!((e.start_ms, e.end_ms), { let (a, b) = local_day_bounds(noon); (a, Some(b)) });
        assert!(e.all_day);
    }

    #[tokio::test]
    async fn texte_sans_date_refuse() {
        let (db, count, broker) = setup();
        let ToolOutcome::Failed { error, .. } = broker.call(call("agenda.create_event", json!({"text": "dentiste"}))).await else { panic!() };
        assert!(error.contains("date introuvable"), "{error}");
        assert!(rows(&db).is_empty());
        assert_eq!(count.0.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn delete_passe_par_la_confirmation_et_l_undo_restaure() {
        let (db, _c, broker) = setup();
        let id = insert(&db.lock(), &NewEvent { title: "Dentiste".into(), start_ms: 10, ..Default::default() }).unwrap();
        let ToolOutcome::NeedsConfirmation(p) = broker.call(call("agenda.delete_event", json!({"id": id}))).await else { panic!() };
        assert!(p.text.summary.contains("Dentiste"));
        assert_eq!(rows(&db).len(), 1, "rien n'est supprimé avant l'accord");
        let ToolOutcome::Done { action_id, .. } = broker.resolve(p.action_id, true).await else { panic!() };
        assert!(rows(&db).is_empty());
        broker.undo(action_id).await.unwrap();
        assert_eq!(rows(&db).len(), 1);
    }

    #[tokio::test]
    async fn delete_d_un_evenement_google_echoue_sans_carte() {
        let (db, _c, broker) = setup();
        let o = broker.call(call("agenda.delete_event", json!({"id": 0, "key": "g:1:cal:abc"}))).await;
        let ToolOutcome::Failed { error, .. } = o else { panic!("{o:?}") };
        assert!(error.contains("Événement Google en lecture seule"), "{error}");
        assert!(rows(&db).is_empty());
    }

    #[tokio::test]
    async fn move_puis_undo() {
        let (db, _c, broker) = setup();
        let id = insert(&db.lock(), &NewEvent { title: "x".into(), start_ms: 100, end_ms: Some(200), ..Default::default() }).unwrap();
        let ToolOutcome::Done { action_id, .. } =
            broker.call(call("agenda.move_event", json!({"id": id, "start_ms": 500}))).await else { panic!() };
        assert_eq!((rows(&db)[0].start_ms, rows(&db)[0].end_ms), (500, None));
        broker.undo(action_id).await.unwrap();
        assert_eq!((rows(&db)[0].start_ms, rows(&db)[0].end_ms), (100, Some(200)));
    }

    #[tokio::test]
    async fn list_et_evenement_inconnu() {
        let (db, _c, broker) = setup();
        insert(&db.lock(), &NewEvent { title: "x".into(), start_ms: 100, ..Default::default() }).unwrap();
        let ToolOutcome::Done { output, .. } = broker.call(call("agenda.list", json!({"from_ms": 0, "to_ms": 1000}))).await else { panic!() };
        assert_eq!(output.as_array().unwrap().len(), 1);
        assert!(matches!(broker.call(call("agenda.move_event", json!({"id": 99, "start_ms": 1}))).await, ToolOutcome::Failed { .. }));
    }
}
