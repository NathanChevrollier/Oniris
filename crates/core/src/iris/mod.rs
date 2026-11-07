//! IRIS : le planificateur ne voit jamais une chaîne d'un tiers (CaMeL simplifié). Les sorties non fiables sont
//! rangées dans un coffre et remplacées par des références `{{uN}}`, résolues seulement à l'exécution.
pub mod agent;
pub mod briefing;
pub mod claude;
pub mod exec;
pub mod fastpath;
pub mod memory;
pub mod ollama;
pub mod prompt;
pub mod quarantine;
pub mod taint;
pub mod triage;

pub use claude::{classify, invocation, locate, parse_line, ClaudeBrain, ClaudeError, ClaudeEvent, Invocation, McpConfig, OnDelta};
pub use exec::{unwire, wire, Executor, PlannerTool, VirtualTools};
pub use taint::{project, render, render_partial, resolve_args, Segment, Vault, SAFE_KEYS};
pub use ollama::{BrainError, ChatReply, ChatRequest, LocalBrain, LocalStatus, Msg, Ollama, Role, ToolCallMsg};
pub use agent::{Agent, BrainUsed, IrisEventKind, PromptCtx, TurnEnd, TurnInput};
pub use quarantine::Quarantine;
