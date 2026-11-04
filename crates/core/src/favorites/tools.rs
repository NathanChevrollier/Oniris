//! Outils des favoris : `favorites.open`, `favorites.add`, `favorites.delete` (tous libres).
use super::{add, get, set_deleted};
use crate::events::{Events, Topic};
use crate::launch::{safe_url, Opener};
use crate::time_util::now_ms;
use crate::tools::helpers::{bad, failed, low, parse, spec, IdArgs};
use crate::tools::{ConfirmationText, Permission, Tool, ToolError, ToolResult, ToolSpec};
use crate::{CoreError, Db};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

pub struct Open { pub db: Db, pub opener: Arc<dyn Opener> }
pub struct Add { pub db: Db, pub events: Arc<dyn Events> }
pub struct Delete { pub db: Db, pub events: Arc<dyn Events> }

#[derive(Deserialize)]
struct AddArgs { title: Option<String>, url: String, category: Option<String> }

fn map(e: CoreError) -> ToolError {
    match e { CoreError::Invalid(m) => bad(m), e => failed(e) }
}

#[async_trait]
impl Tool for Open {
    fn spec(&self) -> ToolSpec {
        spec("favorites.open", "Ouvre un favori : { id }.", json!({"type":"object","required":["id"],"properties":{"id":{"type":"integer"}}}), Permission::Free)
    }
    fn describe(&self, _: &Value) -> ConfirmationText { low("Ouvrir un favori") }
    async fn run(&self, a: &Value) -> Result<ToolResult, ToolError> {
        let IdArgs { id } = parse(a)?;
        let fav = get(&self.db.lock(), id).map_err(failed)?.ok_or_else(|| failed("favori introuvable"))?;
        let url = safe_url(&fav.url).ok_or_else(|| bad("adresse invalide"))?; // deuxième garde, avant le lancement
        self.opener.open_url(&url);
        Ok(ToolResult { output: json!({"opened": true}), undo: None })
    }
}

#[async_trait]
impl Tool for Add {
    fn spec(&self) -> ToolSpec {
        spec("favorites.add", "Ajoute un favori : { url, title?, category? } (annulable). Sans titre, le domaine.",
            json!({"type":"object","required":["url"],"properties":{"url":{"type":"string"},"title":{"type":"string"},"category":{"type":"string"}}}), Permission::Free)
    }
    fn describe(&self, _: &Value) -> ConfirmationText { low("Ajouter un favori") }
    async fn run(&self, a: &Value) -> Result<ToolResult, ToolError> {
        let a: AddArgs = parse(a)?;
        let id = add(&self.db.lock(), a.title.as_deref().unwrap_or(""), &a.url, a.category.as_deref(), now_ms()).map_err(map)?;
        self.events.changed(Topic::Favorites);
        Ok(ToolResult { output: json!({ "id": id }), undo: Some(json!({ "id": id })) })
    }
    async fn undo(&self, undo: &Value) -> Result<(), ToolError> {
        set_deleted(&self.db.lock(), parse::<IdArgs>(undo)?.id, true).map_err(failed)?;
        self.events.changed(Topic::Favorites);
        Ok(())
    }
}

#[async_trait]
impl Tool for Delete {
    fn spec(&self) -> ToolSpec {
        spec("favorites.delete", "Supprime un favori : { id } (annulable).", json!({"type":"object","required":["id"],"properties":{"id":{"type":"integer"}}}), Permission::Free)
    }
    fn describe(&self, _: &Value) -> ConfirmationText { low("Supprimer un favori") }
    async fn run(&self, a: &Value) -> Result<ToolResult, ToolError> {
        let IdArgs { id } = parse(a)?;
        {
            let c = self.db.lock();
            get(&c, id).map_err(failed)?.ok_or_else(|| failed("favori introuvable"))?;
            set_deleted(&c, id, true).map_err(failed)?;
        }
        self.events.changed(Topic::Favorites);
        Ok(ToolResult { output: json!({ "id": id }), undo: Some(json!({ "id": id })) })
    }
    async fn undo(&self, undo: &Value) -> Result<(), ToolError> {
        set_deleted(&self.db.lock(), parse::<IdArgs>(undo)?.id, false).map_err(map)?;
        self.events.changed(Topic::Favorites);
        Ok(())
    }
}

pub fn all(db: &Db, events: &Arc<dyn Events>, opener: &Arc<dyn Opener>) -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(Open { db: db.clone(), opener: opener.clone() }),
        Arc::new(Add { db: db.clone(), events: events.clone() }),
        Arc::new(Delete { db: db.clone(), events: events.clone() }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::NoEvents;
    use crate::favorites::list;
    use crate::test_util::db;
    use crate::tools::{Origin, Provenance, ToolBroker, ToolCall, ToolOutcome};
    use parking_lot::Mutex;
    use std::path::Path;

    #[derive(Default)]
    struct Fake(Mutex<Vec<String>>);
    impl Opener for Fake {
        fn open_path(&self, _: &Path) {}
        fn open_url(&self, u: &str) { self.0.lock().push(u.into()); }
        fn spawn_detached(&self, _: &str, _: &[String], _: Option<&Path>) -> std::io::Result<()> { Ok(()) }
    }

    fn setup() -> (Db, Arc<Fake>, ToolBroker) {
        let db: Db = Arc::new(Mutex::new(db()));
        let fake = Arc::new(Fake::default());
        let (ev, op): (Arc<dyn Events>, Arc<dyn Opener>) = (Arc::new(NoEvents), fake.clone());
        let b = ToolBroker::new(db.clone(), Arc::new(|| 0), all(&db, &ev, &op)).unwrap();
        (db, fake, b)
    }
    async fn call(b: &ToolBroker, tool: &str, args: Value) -> ToolOutcome {
        b.call(ToolCall { tool: tool.into(), args, provenance: Provenance::User, origin: Origin::User }).await
    }

    #[tokio::test]
    async fn ajout_refus_doublon_et_undo() {
        let (db, _, b) = setup();
        assert!(!matches!(call(&b, "favorites.add", json!({"url": "javascript:alert(1)"})).await, ToolOutcome::Done { .. }));
        let ToolOutcome::Done { action_id, .. } = call(&b, "favorites.add", json!({"url": "https://www.example.com/p"})).await else { panic!("Done attendu") };
        assert_eq!(list(&db.lock()).unwrap()[0].title, "example.com");
        assert!(!matches!(call(&b, "favorites.add", json!({"url": "https://www.example.com/p"})).await, ToolOutcome::Done { .. }), "doublon");
        b.undo(action_id).await.unwrap();
        assert!(list(&db.lock()).unwrap().is_empty());
    }

    #[tokio::test]
    async fn ouverture_suppression_et_undo() {
        let (db, fake, b) = setup();
        let ToolOutcome::Done { output, .. } = call(&b, "favorites.add", json!({"url": "https://a.com", "title": "A"})).await else { panic!() };
        let id = output["id"].as_i64().unwrap();
        assert!(matches!(call(&b, "favorites.open", json!({"id": id})).await, ToolOutcome::Done { .. }));
        assert_eq!(*fake.0.lock(), ["https://a.com/"]);
        let ToolOutcome::Done { action_id, .. } = call(&b, "favorites.delete", json!({"id": id})).await else { panic!() };
        assert!(list(&db.lock()).unwrap().is_empty());
        assert!(!matches!(call(&b, "favorites.open", json!({"id": id})).await, ToolOutcome::Done { .. }), "supprimé : plus ouvrable");
        b.undo(action_id).await.unwrap();
        assert_eq!(list(&db.lock()).unwrap().len(), 1);
    }

    #[tokio::test]
    async fn open_reverifie_l_url_stockee() {
        let (db, fake, b) = setup();
        db.lock().execute("INSERT INTO favorites(title, url, position, created_ms) VALUES ('x', 'javascript:alert(1)', 0, 1)", []).unwrap();
        let id = db.lock().last_insert_rowid();
        assert!(!matches!(call(&b, "favorites.open", json!({"id": id})).await, ToolOutcome::Done { .. }));
        assert!(fake.0.lock().is_empty());
    }
}
