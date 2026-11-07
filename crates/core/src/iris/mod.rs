//! IRIS : le planificateur ne voit jamais une chaîne d'un tiers (CaMeL simplifié). Les sorties non fiables sont
//! rangées dans un coffre et remplacées par des références `{{uN}}`, résolues seulement à l'exécution.
pub mod claude;
pub mod exec;
pub mod ollama;
pub mod taint;

pub use claude::{classify, invocation, locate, parse_line, ClaudeBrain, ClaudeError, ClaudeEvent, Invocation, McpConfig, OnDelta};
pub use exec::{unwire, wire, Executor, PlannerTool, VirtualTools};
pub use taint::{project, render, render_partial, resolve_args, Segment, Vault, SAFE_KEYS};
pub use ollama::{BrainError, ChatReply, ChatRequest, LocalBrain, LocalStatus, Msg, Ollama, Role, ToolCallMsg};
