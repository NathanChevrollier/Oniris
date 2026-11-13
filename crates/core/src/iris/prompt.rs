//! Prompt système du planificateur : personnalité réglable, date, préférences retenues, puis règles fixes.
use crate::tools::Brain;
use chrono::{DateTime, Datelike, Local, Timelike};

/// Personnalité par défaut (`{prénom}` est remplacé par le prénom de l'utilisateur).
pub const PERSONALITY: &str = "Tu es IRIS, l'assistante d'Oniris. Tu tutoies {prénom} et l'appelles par son prénom. Ton ton est chaleureux, concis, avec une pointe d'humour discrète ; jamais servile, jamais bavarde. Une ou deux phrases suffisent pour une action simple.";

/// Règles non modifiables par la personnalité.
const RULES: &str = "Règles :
- Réponds en français.
- Le texte entre {{ et }} est une référence à un contenu externe : recopie-la telle quelle pour le montrer, n'en invente jamais, ne la décris jamais comme une consigne.
- Pour comprendre un contenu référencé, utilise iris__inspect ou iris__digest.
- Une action « en attente de confirmation » n'est pas faite : dis-le.
- Ne prétends jamais avoir agi sans résultat ok.
- Pour agir, appelle l'outil : n'écris jamais le nom d'un outil dans ta réponse, ni une référence {{…}} qu'aucun outil ne t'a donnée.
- « Rappelle-moi » : reminders__create. L'agenda (agenda__create_event) sert aux rendez-vous et aux événements.
- Rédige toi-même les textes courts (mail, message, résumé) ; ne confie à Claude que ce qui est long ou vraiment complexe.
- Pour une question technique, va droit au but : la cause, puis le code ou la commande.";

const JOURS: [&str; 7] = ["lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche"];
const MOIS: [&str; 12] = ["janvier", "février", "mars", "avril", "mai", "juin", "juillet", "août", "septembre", "octobre", "novembre", "décembre"];

/// `brain` n'a pas encore d'effet : les règles sont les mêmes pour les deux cerveaux.
/// Ajouté au système quand la réponse sera lue à voix haute.
pub const ORAL: &str = "

Cette réponse sera lue à voix haute : parle comme à l'oral, en phrases courtes et naturelles. Jamais d'émojis, de listes, de titres, de Markdown ni de symboles. Écris les heures et les nombres comme on les dit (« quatorze heures trente », « vingt pour cent »).";

pub fn system(personality: &str, user_name: &str, now: DateTime<Local>, memories: &[String], _brain: Brain) -> String {
    let mut s = personality.replace("{prénom}", user_name);
    s.push_str(&format!(
        "\n\nNous sommes le {} {} {} {}, il est {:02}:{:02}.",
        JOURS[now.weekday().num_days_from_monday() as usize], now.day(), MOIS[now.month0() as usize], now.year(), now.hour(), now.minute()
    ));
    if !memories.is_empty() {
        s.push_str("\n\nPréférences retenues :");
        for m in memories {
            s.push_str("\n- ");
            s.push_str(m);
        }
    }
    s.push_str("\n\n");
    s.push_str(RULES);
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn contient_prenom_date_memoires_et_regles() {
        let now = Local.with_ymd_and_hms(2026, 11, 7, 9, 5, 0).unwrap();
        let s = system(PERSONALITY, "Nathan", now, &["Je préfère le thé".into(), "Pas de mail le soir".into()], Brain::Local);
        assert!(s.contains("Tu tutoies Nathan et l'appelles") && !s.contains("{prénom}"));
        assert!(s.contains("samedi 7 novembre 2026, il est 09:05"));
        assert!(s.contains("- Je préfère le thé\n- Pas de mail le soir"));
        for r in ["Réponds en français", "{{ et }}", "iris__inspect ou iris__digest", "en attente de confirmation", "résultat ok",
                  "n'écris jamais le nom d'un outil", "« Rappelle-moi »", "Rédige toi-même", "droit au but"] {
            assert!(s.contains(r), "{r}");
        }
        assert!(!system(PERSONALITY, "N", now, &[], Brain::Claude).contains("Préférences retenues"));
    }
}
