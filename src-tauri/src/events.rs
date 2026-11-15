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

/// Fenêtres qui écoutent un sujet : la pilule affiche les minuteurs, le titre en cours et le compte des mails.
fn windows_for(topic: Topic) -> &'static [&'static str] {
    if matches!(topic, Topic::Timers | Topic::Music | Topic::Mail) { &["panel", "pill"] } else { &["panel"] }
}

/// Émet `oniris://changed` vers les fenêtres concernées (`windows_for`) et reprogramme les échéances.
pub struct TauriEvents(pub AppHandle);

impl Events for TauriEvents {
    fn changed(&self, topic: Topic) {
        if topic == Topic::Accounts {
            crate::modules::mail::wake(); // compte ajouté, retiré ou reconnecté
        }
        if topic == Topic::Favorites {
            crate::modules::favorites::refresh(&self.0);
            crate::modules::favicons::spawn(&self.0);
        }
        if topic == Topic::Notes {
            crate::modules::search::refresh_notes(&self.0);
        }
        crate::modules::iris_briefing::on_topic(&self.0, topic);
        for (t, key, next_fn) in RESCHEDULE {
            if t == topic {
                reschedule(&self.0, key, next_fn);
            }
        }
        for label in windows_for(topic) {
            if let Err(e) = self.0.emit_to(label, "oniris://changed", serde_json::json!({ "topic": topic })) {
                tracing::warn!("événement changed vers {label} : {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn la_pilule_n_ecoute_que_minuteurs_musique_et_mails() {
        for t in [Topic::Timers, Topic::Music, Topic::Mail] {
            assert_eq!(windows_for(t), ["panel", "pill"], "{t:?}");
        }
        for t in [Topic::Agenda, Topic::Reminders, Topic::Notes, Topic::Clipboard, Topic::Weather, Topic::Favorites, Topic::Projects, Topic::Accounts, Topic::Sites, Topic::Iris] {
            assert_eq!(windows_for(t), ["panel"], "{t:?}");
        }
    }
}
