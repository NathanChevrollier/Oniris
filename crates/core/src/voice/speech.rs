//! Texte à dire par IRIS : ce qui se lit à voix haute, et le découpage en phrases pour Piper.
use crate::iris::taint::Segment;

/// Au-delà, IRIS renvoie vers le panneau plutôt que de tout lire.
const MAX_CHARS: usize = 300;
/// Début de la phrase dite à la place d'une réponse trop longue.
pub const TO_PANEL: &str = "Je t'ai mis la réponse dans le panneau";

/// Abréviations après lesquelles un point ne finit pas la phrase (minuscules, sans le point).
const ABBREVIATIONS: [&str; 10] = ["m", "mm", "mme", "mlle", "dr", "pr", "st", "etc", "cf", "ex"];

/// Texte à dire à partir de la réponse rendue ; `None` si rien à dire.
pub fn speakable(segments: &[Segment], user_name: &str) -> Option<String> {
    let raw: String = segments
        .iter()
        .map(|s| match s {
            Segment::Text { text } | Segment::Untrusted { text, .. } => text.as_str(),
        })
        .collect::<Vec<_>>()
        .join(" ");
    let text = strip_markup(&raw);
    if text.is_empty() {
        return None;
    }
    if text.chars().count() > MAX_CHARS {
        return Some(format!("{TO_PANEL}, {user_name}."));
    }
    Some(text)
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
            let w: String = w.chars().filter(|c| !matches!(c, '*' | '_' | '`' | '~' | '#')).collect();
            if !w.is_empty() {
                words.push(w);
            }
        }
    }
    words.join(" ")
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
    fn decoupe_en_phrases_sans_casser_les_abreviations() {
        assert_eq!(sentences("Il est 10 h 30. M. Dupont arrive, etc. Super !"), ["Il est 10 h 30.", "M. Dupont arrive, etc. Super !"]);
        assert_eq!(sentences("Pi vaut 3.14. Vraiment ? Oui… Non"), ["Pi vaut 3.14.", "Vraiment ?", "Oui…", "Non"]);
        assert!(sentences("  ").is_empty());
    }
}
