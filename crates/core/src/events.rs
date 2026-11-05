use serde::Serialize;

/// Sujet de données dont l'UI doit se rafraîchir.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Topic { Agenda, Reminders, Notes, Timers, Clipboard, Weather, Favorites, Projects }

/// Notifie l'UI qu'un sujet a changé.
///
/// Ne jamais appeler `changed` en tenant le verrou de la base : les implémentations peuvent le prendre
/// (côté Tauri, la reprogrammation relit la prochaine échéance). Lier d'abord le résultat avec `let`.
pub trait Events: Send + Sync { fn changed(&self, topic: Topic); }

/// Implémentation vide, pour les tests.
pub struct NoEvents;
impl Events for NoEvents { fn changed(&self, _topic: Topic) {} }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topic_serializes_lowercase() {
        assert_eq!(serde_json::to_string(&Topic::Reminders).unwrap(), "\"reminders\"");
        NoEvents.changed(Topic::Agenda);
    }
}
