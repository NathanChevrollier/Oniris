//! Aides communes aux outils des modules (à utiliser plutôt que de les recopier).
use super::{ConfirmationText, Permission, Risk, ToolError, ToolSpec};
use serde::Deserialize;
use serde_json::Value;

/// Arguments invalides (erreur de l'appelant).
pub fn bad(e: impl ToString) -> ToolError { ToolError::BadArgs(e.to_string()) }

/// Échec d'exécution (base, état introuvable…).
pub fn failed(e: impl ToString) -> ToolError { ToolError::Failed(e.to_string()) }

/// Désérialise les arguments (ou un payload d'undo) ; erreur : `BadArgs`.
pub fn parse<T: for<'a> Deserialize<'a>>(a: &Value) -> Result<T, ToolError> {
    serde_json::from_value(a.clone()).map_err(bad)
}

/// Construit un `ToolSpec`.
pub fn spec(name: &str, description: &str, args_schema: Value, permission: Permission) -> ToolSpec {
    ToolSpec { name: name.into(), description: description.into(), args_schema, permission }
}

/// Texte de confirmation sans détail et à risque faible (outils libres : jamais affiché).
pub fn low(summary: &str) -> ConfirmationText {
    ConfirmationText { summary: summary.into(), details: String::new(), risk: Risk::Low }
}
