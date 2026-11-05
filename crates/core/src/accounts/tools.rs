//! Outil `accounts.list` : comptes en ligne connectés (lecture seule).
use super::{list, AccountView};
use crate::tools::helpers::{failed, low, spec};
use crate::tools::{ConfirmationText, Permission, Tool, ToolError, ToolResult, ToolSpec};
use crate::Db;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

pub struct List { pub db: Db }

#[async_trait]
impl Tool for List {
    fn spec(&self) -> ToolSpec {
        // Les adresses sont du contenu utilisateur : la sortie n'entre pas au journal.
        ToolSpec {
            sensitive_output: true,
            ..spec("accounts.list", "Comptes en ligne connectés (Gmail, Spotify) et leur état.", json!({"type":"object","properties":{}}), Permission::Free)
        }
    }
    fn describe(&self, _: &Value) -> ConfirmationText { low("Lister les comptes") }
    async fn run(&self, _: &Value) -> Result<ToolResult, ToolError> {
        let accounts = list(&self.db.lock()).map_err(failed)?;
        let views: Vec<AccountView> = accounts.iter().map(AccountView::from).collect();
        Ok(ToolResult { output: serde_json::to_value(views).map_err(failed)?, undo: None })
    }
}

pub fn all(db: &Db) -> Vec<Arc<dyn Tool>> {
    vec![Arc::new(List { db: db.clone() })]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oauth::Provider;

    #[tokio::test] async fn liste_sans_jeton_ni_date() {
        let db: Db = Arc::new(parking_lot::Mutex::new(crate::test_util::db()));
        super::super::add(&db.lock(), Provider::Google, "a@gmail.com", 1).unwrap();
        let out = List { db }.run(&json!({})).await.unwrap().output;
        assert_eq!(out[0]["label"], "a@gmail.com");
        assert_eq!(out[0]["provider"], "google");
        assert_eq!(out[0]["status"], "ok");
        assert!(out[0].get("created_ms").is_none());
    }
}
