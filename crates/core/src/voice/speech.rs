//! Texte à dire par IRIS : ce qui se lit à voix haute, et le découpage en phrases pour Piper.
use crate::iris::taint::Segment;

/// Au-delà, IRIS renvoie vers le panneau plutôt que de tout lire.
const MAX_CHARS: usize = 300;
/// Début de la phrase dite à la place d'une réponse trop longue.
pub const TO_PANEL: &str = "Je t'ai mis la réponse dans le panneau";

/// Abréviations après lesquelles un point ne finit pas la phrase (minuscules, sans le point).
const ABBREVIATIONS: [&str; 10] = ["m", "mm", "mme", "mlle", "dr", "pr", "st", "etc", "cf", "ex"];

/// Phrase dite quand une réponse en flux dépasse la limite après ses premières phrases.
pub const REST_TO_PANEL: &str = "Je t'ai mis la suite dans le panneau";

/// Texte à dire à partir de la réponse rendue ; `None` si rien à dire.
pub fn speakable(segments: &[Segment], user_name: &str) -> Option<String> {
    let text = oral(segments);
    if text.is_empty() {
        return None;
    }
    if text.chars().count() > MAX_CHARS {
        return Some(format!("{TO_PANEL}, {user_name}."));
    }
    Some(text)
}

/// Réponse rendue nettoyée pour l'oral, sans limite de longueur.
fn oral(segments: &[Segment]) -> String {
    let raw: String = segments
        .iter()
        .map(|s| match s {
            Segment::Text { text } | Segment::Untrusted { text, .. } => text.as_str(),
        })
        .collect::<Vec<_>>()
        .join(" ");
    strip_markup(&raw)
}

/// Réponse en flux dite phrase par phrase : chaque phrase complète part dès qu'elle arrive, une seule fois.
/// Au-delà de `MAX_CHARS`, plus rien ne part (`over`) : le reste est pour le panneau.
#[derive(Debug, Default)]
pub struct Utterance {
    /// Phrases déjà parties du flux en cours (un nouveau flux, après un outil, repart de zéro).
    said: Vec<String>,
    total: usize,
    over: bool,
}

impl Utterance {
    /// Réponse partielle (cumulée) : ses phrases complètes pas encore dites ; la dernière attend la suite.
    pub fn partial(&mut self, segments: &[Segment]) -> Vec<String> { self.next(segments, false) }

    /// Réponse finale : tout ce qui n'a pas encore été dit.
    pub fn finish(&mut self, segments: &[Segment]) -> Vec<String> { self.next(segments, true) }

    pub fn over(&self) -> bool { self.over }

    /// Au moins une phrase est partie.
    pub fn started(&self) -> bool { self.total > 0 }

    fn next(&mut self, segments: &[Segment], all: bool) -> Vec<String> {
        if self.over {
            return Vec::new();
        }
        let text = oral(segments);
        if text.chars().count() > MAX_CHARS {
            self.over = true;
            return Vec::new();
        }
        let mut s = sentences(&text);
        if !all {
            s.pop();
        }
        // Texte qui ne prolonge pas ce qui a été dit : nouvelle étape du tour (après un outil).
        if self.said.iter().zip(&s).any(|(a, b)| a != b) {
            self.said.clear();
        }
        let new: Vec<String> = s.into_iter().skip(self.said.len()).collect();
        self.said.extend(new.iter().cloned());
        self.total += new.len();
        new
    }
}

/// Retire blocs de code, liens (garde le libellé), URL nues et marques Markdown.
fn strip_markup(s: &str) -> String {
    let mut out = String::new();
    for (i, part) in s.split("```").enumerate() {
        if i % 2 == 0 {
            out.push_str(part); // les morceaux impairs sont dans un bloc de code
            out.push(' ');
        }
    }
    let out = links(&out);
    let mut words: Vec<String> = Vec::new();
    for line in out.lines() {
        let line = line.trim_start().trim_start_matches(['#', '>']).trim_start();
        for w in line.split_whitespace() {
            if w.starts_with("http://") || w.starts_with("https://") {
                continue;
            }
            let w = match w {
                "→" | "->" | "=>" | "–" | "—" | "-" => ",".to_string(),
                "&" => "et".to_string(),
                _ => {
                    let w: String = w.chars().filter(|c| !matches!(c, '*' | '_' | '`' | '~' | '#' | '•' | '|') && !pictograph(*c)).collect();
                    clock(&w).or_else(|| own_name(&w)).unwrap_or(w)
                }
            };
            if !w.is_empty() {
                words.push(w);
            }
        }
    }
    words.join(" ").replace(" ,", ",").trim_start_matches([',', ' ']).to_string()
}

/// Émojis, pictogrammes et leurs liants : Piper les lirait (« visage souriant »).
fn pictograph(c: char) -> bool {
    matches!(c as u32,
        0x1F000..=0x1FAFF | 0x2600..=0x27BF | 0x2B00..=0x2BFF | 0x2300..=0x23FF | 0x2190..=0x21FF
        | 0xFE00..=0xFE0F | 0x200D | 0x20E3 | 0xE0000..=0xE007F)
}

/// « 14:30 » se dit « 14 h 30 » (sinon : « quatorze deux-points trente »).
/// « IRIS » se lirait « iri » (s final muet en français) : graphie phonétique, ponctuation et élision gardées
/// (« d'IRIS » -> « d'Irisse »).
fn own_name(w: &str) -> Option<String> {
    let start = w.rfind(['\'', '’']).map_or(0, |i| i + 1);
    let end = w.trim_end_matches(|c: char| !c.is_alphanumeric()).len();
    (end > start && w[start..end].eq_ignore_ascii_case("iris")).then(|| format!("{}Irisse{}", &w[..start], &w[end..]))
}

fn clock(w: &str) -> Option<String> {
    let (core, tail) = w.split_at(w.trim_end_matches(|c: char| !c.is_ascii_digit()).len());
    let (h, m) = core.split_once(':')?;
    let ok = (1..=2).contains(&h.len()) && m.len() == 2 && h.chars().chain(m.chars()).all(|c| c.is_ascii_digit());
    ok.then(|| if m == "00" { format!("{h} h{tail}") } else { format!("{h} h {m}{tail}") })
}

/// `[libellé](url)` devient `libellé`.
fn links(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find("](") {
        match (rest[..i].rfind('['), rest[i..].find(')')) {
            (Some(open), Some(close)) => {
                out.push_str(&rest[..open]);
                out.push_str(&rest[open + 1..i]);
                rest = &rest[i + close + 1..];
            }
            _ => break,
        }
    }
    out.push_str(rest);
    out
}

/// Découpe sur `. ! ? …` suivis d'un espace ; garde « M. », « etc. » ; les décimaux ne sont pas coupés (pas d'espace).
pub fn sentences(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut start = 0;
    for i in 0..chars.len() {
        if !matches!(chars[i], '.' | '!' | '?' | '…') || !chars.get(i + 1).is_some_and(|c| c.is_whitespace()) {
            continue;
        }
        if chars[i] == '.' && is_abbreviation(&chars[start..i]) {
            continue;
        }
        push(&mut out, &chars[start..=i]);
        start = i + 1;
    }
    push(&mut out, &chars[start..]);
    out
}

fn is_abbreviation(before: &[char]) -> bool {
    let word: String = before.iter().rev().take_while(|c| c.is_alphabetic()).collect::<Vec<_>>().into_iter().rev().collect();
    ABBREVIATIONS.contains(&word.to_lowercase().as_str())
}

fn push(out: &mut Vec<String>, part: &[char]) {
    let s: String = part.iter().collect();
    let s = s.trim();
    if !s.is_empty() {
        out.push(s.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iris_prononce_son_nom() {
        assert_eq!(strip_markup("Je suis IRIS, l'assistante d'Iris. iris !"), "Je suis Irisse, l'assistante d'Irisse. Irisse !");
        assert_eq!(strip_markup("Irisée, Osiris et iris9 restent tels quels"), "Irisée, Osiris et iris9 restent tels quels");
    }

    #[test]
    fn emojis_symboles_et_heures_pour_l_oral() {
        let say = |t: &str| strip_markup(t);
        assert_eq!(say("Bonjour 👋😊 ! Tout va bien ✅"), "Bonjour ! Tout va bien");
        assert_eq!(say("❤️ Rendez-vous à 14:30, puis 9:00."), "Rendez-vous à 14 h 30, puis 9 h.");
        assert_eq!(say("- Lait
- Pain → boulangerie"), "Lait, Pain, boulangerie");
        assert_eq!(say("Tom & Léa 👨‍👩‍👧"), "Tom et Léa");
    }

    fn t(s: &str) -> Vec<Segment> {
        vec![Segment::Text { text: s.into() }]
    }

    #[test]
    fn au_dela_de_300_caracteres_renvoie_vers_le_panneau() {
        assert_eq!(speakable(&t(&"a".repeat(300)), "Nathan").unwrap().len(), 300);
        assert_eq!(speakable(&t(&"a".repeat(301)), "Nathan").unwrap(), "Je t'ai mis la réponse dans le panneau, Nathan.");
    }

    #[test]
    fn markdown_url_et_code_sont_retires() {
        let s = speakable(&t("# Salut **toi**, voir [la doc](https://x.fr/a) ou https://x.fr `code` ok\n```rust\nlet a = 1;\n```\nfin"), "N").unwrap();
        assert_eq!(s, "Salut toi, voir la doc ou code ok fin");
    }

    #[test]
    fn le_texte_non_fiable_est_lu_et_le_vide_donne_none() {
        let segs = vec![Segment::Untrusted { text: "Objet du mail".into(), source: "mail".into() }];
        assert_eq!(speakable(&segs, "N").unwrap(), "Objet du mail");
        assert_eq!(speakable(&t(" ** \n```x```"), "N"), None);
        assert_eq!(speakable(&[], "N"), None);
    }

    #[test]
    fn le_flux_dit_chaque_phrase_complete_une_fois() {
        let mut u = Utterance::default();
        assert!(u.partial(&t("Bonjour")).is_empty(), "phrase pas finie");
        assert!(u.partial(&t("Bonjour **Nathan**.")).is_empty(), "rien après le point : peut-être pas finie");
        assert_eq!(u.partial(&t("Bonjour **Nathan**. Il est 14:30")), ["Bonjour Nathan."]);
        assert!(u.partial(&t("Bonjour **Nathan**. Il est 14:30, ")).is_empty());
        assert_eq!(u.finish(&t("Bonjour **Nathan**. Il est 14:30, IRIS.")), ["Il est 14 h 30, Irisse."]);
        assert!(u.started() && !u.over());
        assert!(u.finish(&t("Bonjour **Nathan**. Il est 14:30, IRIS.")).is_empty(), "rien deux fois");
    }

    #[test]
    fn le_flux_suit_une_nouvelle_etape_et_s_arrete_au_dela_de_la_limite() {
        let mut u = Utterance::default();
        assert_eq!(u.partial(&t("Je regarde. ")), Vec::<String>::new());
        assert_eq!(u.partial(&t("Je regarde. Ton")), ["Je regarde."]);
        assert_eq!(u.finish(&t("Tu as deux mails. Voilà.")), ["Tu as deux mails.", "Voilà."], "réponse après un outil");
        let mut u = Utterance::default();
        let long = format!("Première phrase. {}", "mot ".repeat(80));
        assert_eq!(u.partial(&t(&long[..30])), ["Première phrase."]);
        assert!(u.partial(&t(&long)).is_empty() && u.over());
        assert!(u.finish(&t(&long)).is_empty());
        let mut quiet = Utterance::default();
        assert!(quiet.finish(&t(" ** ")).is_empty() && !quiet.started());
    }

    #[test]
    fn decoupe_en_phrases_sans_casser_les_abreviations() {
        assert_eq!(sentences("Il est 10 h 30. M. Dupont arrive, etc. Super !"), ["Il est 10 h 30.", "M. Dupont arrive, etc. Super !"]);
        assert_eq!(sentences("Pi vaut 3.14. Vraiment ? Oui… Non"), ["Pi vaut 3.14.", "Vraiment ?", "Oui…", "Non"]);
        assert!(sentences("  ").is_empty());
    }
}
