use oniris_core::events::{Events, Topic};
use tauri::{AppHandle, Emitter};

/// Émet `oniris://changed` vers le panneau et la pilule.
pub struct TauriEvents(pub AppHandle);

impl Events for TauriEvents {
    fn changed(&self, topic: Topic) {
        for label in ["panel", "pill"] {
            if let Err(e) = self.0.emit_to(label, "oniris://changed", serde_json::json!({ "topic": topic })) {
                tracing::warn!("événement changed vers {label} : {e}");
            }
        }
    }
}
