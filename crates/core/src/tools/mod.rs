//! Courtier d'outils : toute action d'IRIS passe par `ToolBroker`.
pub mod helpers;
mod broker;
mod journal;

pub use broker::{Clock, ToolBroker, PENDING_TTL_MS};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Permission { Free, Confirm }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Risk { Low, Medium, High }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Brain { Local, Claude }
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Origin { User, Assistant { brain: Brain }, Mcp }
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Provenance { User, Untrusted { source: String } }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub args_schema: Value,
    pub permission: Permission,
    /// Sortie à ne jamais journaliser (contenu utilisateur) : le journal n'en garde que `{redacted, count}`.
    #[serde(default)]
    pub sensitive_output: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub tool: String,
    pub args: Value,
    pub provenance: Provenance,
    pub origin: Origin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfirmationText { pub summary: String, pub details: String, pub risk: Risk }

#[derive(Debug, Clone)]
pub struct ToolResult { pub output: Value, pub undo: Option<Value> }

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ToolError {
    #[error("arguments invalides : {0}")]
    BadArgs(String),
    #[error("échec : {0}")]
    Failed(String),
    #[error("action non annulable")]
    NotUndoable,
    #[error("outil inconnu : {0}")]
    UnknownTool(String),
    #[error("demande expirée")]
    Expired,
    #[error("aucune demande en attente pour cette action")]
    NotPending,
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn spec(&self) -> ToolSpec;
    fn describe(&self, args: &Value) -> ConfirmationText;
    async fn run(&self, args: &Value) -> Result<ToolResult, ToolError>;
    async fn undo(&self, _undo: &Value) -> Result<(), ToolError> { Err(ToolError::NotUndoable) }
    /// Arguments écrits au journal (`actions.args`). Un outil qui reçoit un contenu à ne pas garder en base
    /// (corps d'un mail) en rend une forme masquée ; le courtier garde alors les vrais arguments en mémoire
    /// jusqu'à la décision. Le masque ne doit pas être exécutable (`run` le refuse).
    fn journal_args(&self, args: &Value) -> Value { args.clone() }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingAction {
    pub action_id: i64,
    pub tool: String,
    pub text: ConfirmationText,
    pub provenance: Provenance,
    pub expires_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ToolOutcome {
    Done { action_id: i64, output: Value },
    NeedsConfirmation(PendingAction),
    Rejected { action_id: i64 },
    Failed { action_id: i64, error: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JournalEntry {
    pub id: i64,
    pub ts_ms: u64,
    pub tool: String,
    pub args: Value,
    pub origin: Origin,
    pub provenance: Provenance,
    pub status: String,
    pub result: Option<Value>,
    pub undoable: bool,
}
