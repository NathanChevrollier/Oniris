//! Outil de l'historique du presse-papiers : recherche en lecture seule, textes seulement (jamais d'image).
use super::search;
use crate::tools::helpers::{failed, low, parse, spec, untrusted};
use crate::tools::{ConfirmationText, Permission, Tool, ToolError, ToolResult, ToolSpec};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

use crate::Db;

pub struct Search { pub db: Db }

#[derive(Deserialize)]
struct SearchArgs { query: String }

const SEARCH_LIMIT: usize = 20;
const SNIPPET_CHARS: usize = 200;

#[async_trait]
impl Tool for Search {
    fn spec(&self) -> ToolSpec {
        // Sortie sensible : le texte copié ne doit pas survivre dans le journal des actions.
        untrusted(ToolSpec { sensitive_output: true, ..spec("clipboard.search", "Cherche dans l'historique du presse-papiers : { query }. Textes seulement (200 premiers caractères).",
            json!({"type":"object","required":["query"],"properties":{"query":{"type":"string"}}}), Permission::Free) })
    }
    fn describe(&self, _: &Value) -> ConfirmationText { low("Chercher dans le presse-papiers") }
    async fn run(&self, args: &Value) -> Result<ToolResult, ToolError> {
        let a: SearchArgs = parse(args)?;
        let list = search(&self.db.lock(), &a.query, SEARCH_LIMIT).map_err(failed)?;
        let out: Vec<Value> = list.into_iter().filter_map(|e| {
            let text: String = e.text?.chars().take(SNIPPET_CHARS).collect();
            Some(json!({ "id": e.id, "text": text, "created_ms": e.created_ms }))
        }).collect();
        Ok(ToolResult { output: json!(out), undo: None })
    }
}

/// L'outil du presse-papiers, pour `AppState::open`.
pub fn all(db: &Db) -> Vec<Arc<dyn Tool>> {
    vec![Arc::new(Search { db: db.clone() })]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::db;
    use parking_lot::Mutex;
    use crate::clipboard::{record_image, record_text};
    use crate::tools::{Origin, Provenance, ToolBroker, ToolCall, ToolOutcome};

    #[tokio::test]
    async fn search_textes_seulement_extrait_de_200_caracteres() {
        let c = db();
        let db: Db = Arc::new(Mutex::new(c));
        let long = format!("clé {}", "é".repeat(300));
        let id = record_text(&db.lock(), &long, 1).unwrap();
        record_image(&db.lock(), b"png", b"th", 2).unwrap();
        let broker = ToolBroker::new(db.clone(), Arc::new(|| 0), all(&db)).unwrap();
        let call = |q: &str| ToolCall { tool: "clipboard.search".into(), args: json!({"query": q}), provenance: Provenance::User, origin: Origin::User };
        let ToolOutcome::Done { output, .. } = broker.call(call("")).await else { panic!() };
        let list = output.as_array().unwrap();
        assert_eq!(list.len(), 1, "pas d'image");
        assert_eq!(list[0]["id"], id);
        assert_eq!(list[0]["text"].as_str().unwrap().chars().count(), SNIPPET_CHARS);
        assert!(list[0].get("thumb").is_none() && list[0].get("png").is_none());
        let journal: Vec<String> = db.lock().prepare("SELECT result FROM actions").unwrap()
            .query_map([], |r| r.get(0)).unwrap().collect::<Result<_, _>>().unwrap();
        assert_eq!(journal, vec![r#"{"count":1,"redacted":true}"#], "aucun texte copié dans le journal");
        let ToolOutcome::Done { output, .. } = broker.call(call("CLé")).await else { panic!() };
        assert_eq!(output.as_array().unwrap().len(), 1);
        let ToolOutcome::Done { output, .. } = broker.call(call("absent")).await else { panic!() };
        assert!(output.as_array().unwrap().is_empty());
    }
}
