use oniris_core::events::{Events, Topic};
use tauri::{AppHandle, Emitter};

/// Émet `oniris://changed` vers le panneau et la pilule.
pub struct TauriEvents(pub AppHandle);

impl Events for TauriEvents {
    fn changed(&self, topic: Topic) {
        // Les outils du cœur ne voient pas l'ordonnanceur : tout changement de rappels reprogramme l'échéance ici.
        if topic == Topic::Reminders {
            crate::modules::reminders::reschedule_reminders(&self.0);
        }
        for label in ["panel", "pill"] {
            if let Err(e) = self.0.emit_to(label, "oniris://changed", serde_json::json!({ "topic": topic })) {
                tracing::warn!("événement changed vers {label} : {e}");
            }
        }
    }
}
