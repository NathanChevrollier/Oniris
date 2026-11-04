use crate::modules::{reminders, timers};
use crate::scheduler::{reschedule, NextFn};
use oniris_core::events::{Events, Topic};
use tauri::{AppHandle, Emitter};

/// Sujets dont l'échéance vit dans l'ordonnanceur. Les outils du cœur ne le voient pas : tout changement
/// (outil, UI ou undo) reprogramme ici, en un seul endroit.
const RESCHEDULE: [(Topic, &str, NextFn); 2] = [
    (Topic::Reminders, reminders::KEY, oniris_core::reminders::next_due),
    (Topic::Timers, timers::KEY, oniris_core::timers::next_end),
];

/// Émet `oniris://changed` vers le panneau et la pilule.
pub struct TauriEvents(pub AppHandle);

impl Events for TauriEvents {
    fn changed(&self, topic: Topic) {
        for (t, key, next_fn) in RESCHEDULE {
            if t == topic {
                reschedule(&self.0, key, next_fn);
            }
        }
        for label in ["panel", "pill"] {
            if let Err(e) = self.0.emit_to(label, "oniris://changed", serde_json::json!({ "topic": topic })) {
                tracing::warn!("événement changed vers {label} : {e}");
            }
        }
    }
}
