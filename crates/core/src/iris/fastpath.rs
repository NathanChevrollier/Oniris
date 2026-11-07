//! Raccourci déterministe : commandes courantes reconnues sans modèle (sans accents ni casse), appelées
//! directement par le courtier en provenance « User ».
use crate::when::norm;
use chrono::{DateTime, Local};
use serde_json::{json, Value};

/// Outils appelés par le raccourci ; `src-tauri` vérifie qu'ils sont bien enregistrés.
pub const TOOLS: &[&str] = &["music.toggle", "music.next", "music.previous", "music.volume", "timer.start", "reminders.create", "notes.create"];
const VOLUME_STEP: i32 = 10;

/// `now` est réservé : `reminders.create` et `timer.start` analysent eux-mêmes date et durée.
pub fn parse(text: &str, _now: DateTime<Local>) -> Option<(&'static str, Value)> {
    let t = norm(text).split_whitespace().collect::<Vec<_>>().join(" ");
    let fixed = match t.as_str() {
        "pause" | "stop la musique" | "reprends la musique" | "play" => Some(("music.toggle", json!({}))),
        "suivant" => Some(("music.next", json!({}))),
        "precedent" => Some(("music.previous", json!({}))),
        "plus fort" => Some(("music.volume", json!({ "delta": VOLUME_STEP }))),
        "moins fort" => Some(("music.volume", json!({ "delta": -VOLUME_STEP }))),
        _ => None,
    };
    if fixed.is_some() {
        return fixed;
    }
    let w: Vec<&str> = t.split(' ').collect();
    // Le reste garde la casse et les accents d'origine.
    let rest = text.split_whitespace().skip(1).collect::<Vec<_>>().join(" ");
    match w[0] {
        "volume" if w.len() == 2 => {
            let n: i32 = w[1].parse().ok().filter(|n| (0..=100).contains(n))?;
            Some(("music.volume", json!({ "percent": n })))
        }
        "minuteur" | "timer" if w.len() >= 3 && w[1].parse::<u32>().is_ok_and(|n| n > 0) && matches!(w[2], "min" | "minutes" | "s") => {
            Some(("timer.start", json!({ "text": text.trim() })))
        }
        "rappelle-moi" if !rest.is_empty() => Some(("reminders.create", json!({ "text": rest }))),
        "note" if !rest.is_empty() => Some(("notes.create", json!({ "text": rest }))),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(t: &str) -> Option<(&'static str, Value)> { parse(t, Local::now()) }

    #[test]
    fn commandes_reconnues() {
        for t in ["Pause", "stop la musique", "Reprends la musique !", "PLAY"] {
            assert_eq!(p(t), Some(("music.toggle", json!({}))), "{t}");
        }
        assert_eq!(p("Suivant"), Some(("music.next", json!({}))));
        assert_eq!(p("Précédent."), Some(("music.previous", json!({}))));
        assert_eq!(p("volume 40"), Some(("music.volume", json!({"percent": 40}))));
        assert_eq!(p("Plus fort"), Some(("music.volume", json!({"delta": 10}))));
        assert_eq!(p("moins  fort"), Some(("music.volume", json!({"delta": -10}))));
        assert_eq!(p("Minuteur 25 minutes thé"), Some(("timer.start", json!({"text": "Minuteur 25 minutes thé"}))));
        assert_eq!(p("timer 30 s"), Some(("timer.start", json!({"text": "timer 30 s"}))));
        assert_eq!(p("Rappelle-moi d'appeler la banque demain à 10 h"), Some(("reminders.create", json!({"text": "d'appeler la banque demain à 10 h"}))));
        assert_eq!(p("note acheter du Pain"), Some(("notes.create", json!({"text": "acheter du Pain"}))));
    }

    #[test]
    fn le_reste_va_au_modele() {
        for t in ["", "suivant le mail de Paul", "volume 101", "volume fort", "minuteur", "minuteur 0 min", "timer 5 heures",
            "rappelle-moi", "note", "quels mails importants ?", "pause café demain"] {
            assert_eq!(p(t), None, "{t}");
        }
    }
}
