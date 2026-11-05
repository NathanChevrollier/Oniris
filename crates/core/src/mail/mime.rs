//! Décodage des en-têtes (RFC 2047, `From`) et des corps base64url de l'API Gmail.
use base64::alphabet;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use base64::Engine;

const INDIFFERENT: GeneralPurposeConfig = GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent);
/// Corps Gmail : base64url, avec ou sans `=`.
const B64URL: GeneralPurpose = GeneralPurpose::new(&alphabet::URL_SAFE, INDIFFERENT);
const B64: GeneralPurpose = GeneralPurpose::new(&alphabet::STANDARD, INDIFFERENT);

pub fn b64url(data: &str) -> Option<Vec<u8>> {
    B64URL.decode(data.trim()).ok()
}

/// Octets vers texte selon le jeu de caractères ; inconnu : UTF-8 avec remplacement.
pub fn to_text(bytes: &[u8], charset: &str) -> String {
    match charset.trim().to_ascii_lowercase().as_str() {
        // shortcut: windows-1252 traité comme latin-1 (0x80..0x9F diffèrent), à revoir si des € manquent.
        "iso-8859-1" | "latin1" | "iso-8859-15" | "windows-1252" | "cp1252" => bytes.iter().map(|&b| b as char).collect(),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// Un mot encodé `=?charset?B|Q?texte?=` ; `None` s'il est mal formé.
fn encoded_word(w: &str) -> Option<String> {
    let inner = w.strip_prefix("=?")?.strip_suffix("?=")?;
    let mut it = inner.splitn(3, '?');
    let (charset, enc, text) = (it.next()?, it.next()?, it.next()?);
    // RFC 2231 : `charset*langue`.
    let charset = charset.split('*').next()?;
    let bytes = match enc {
        "B" | "b" => B64.decode(text).ok()?,
        "Q" | "q" => {
            let (mut out, b) = (Vec::new(), text.as_bytes());
            let mut i = 0;
            while i < b.len() {
                match b[i] {
                    b'_' => out.push(b' '),
                    b'=' => {
                        out.push(u8::from_str_radix(std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?, 16).ok()?);
                        i += 2;
                    }
                    c => out.push(c),
                }
                i += 1;
            }
            out
        }
        _ => return None,
    };
    Some(to_text(&bytes, charset))
}

/// Décode les mots encodés RFC 2047 ; les blancs entre deux mots encodés disparaissent.
pub fn decode_header(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    let mut prev_encoded = false;
    while let Some(start) = rest.find("=?") {
        // Fin du mot : `?=` après les deux `?` internes.
        let end = rest[start + 2..]
            .match_indices('?')
            .nth(2)
            .map(|(i, _)| start + 2 + i)
            .filter(|&i| rest[i..].starts_with("?="));
        let Some(end) = end else { break };
        let word = &rest[start..end + 2];
        match encoded_word(word) {
            Some(text) => {
                let gap = &rest[..start];
                if !(prev_encoded && gap.trim().is_empty()) {
                    out.push_str(gap);
                }
                out.push_str(&text);
                prev_encoded = true;
            }
            None => {
                out.push_str(&rest[..end + 2]);
                prev_encoded = false;
            }
        }
        rest = &rest[end + 2..];
    }
    out.push_str(rest);
    out
}

/// `"Dupont, Jean" <j@x.fr>` -> (`Dupont, Jean`, `j@x.fr`). Sans nom : l'adresse sert de nom.
pub fn parse_from(raw: &str) -> (String, String) {
    let raw = raw.trim();
    let (name, addr) = match (raw.rfind('<'), raw.rfind('>')) {
        (Some(l), Some(r)) if l < r => (raw[..l].trim(), raw[l + 1..r].trim()),
        _ => ("", raw),
    };
    let name = match name.strip_prefix('"').and_then(|n| n.strip_suffix('"')) {
        Some(q) => q.replace("\\\"", "\"").replace("\\\\", "\\"),
        None => name.to_string(),
    };
    let name = decode_header(&name).trim().to_string();
    (if name.is_empty() { addr.to_string() } else { name }, addr.to_string())
}

/// Les extraits Gmail sont échappés en HTML : entités courantes seulement.
pub fn unescape_snippet(s: &str) -> String {
    s.replace("&#39;", "'").replace("&quot;", "\"").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] fn rfc_2047() {
        assert_eq!(decode_header("=?UTF-8?B?UsOpdW5pb24gZGVtYWluIMOgIDEwIGg=?="), "Réunion demain à 10 h");
        assert_eq!(decode_header("=?ISO-8859-1?Q?Boutique_=E9t=E9?="), "Boutique été");
        assert_eq!(decode_header("=?utf-8?q?caf=C3=A9?= =?utf-8?q?_cr=C3=A8me?="), "café crème", "blanc entre mots encodés retiré");
        assert_eq!(decode_header("Re: =?UTF-8?Q?=C3=A9t=C3=A9?= ok"), "Re: été ok");
        assert_eq!(decode_header("Prix =?x"), "Prix =?x");
        assert_eq!(decode_header("=?UTF-8?X?abc?= reste"), "=?UTF-8?X?abc?= reste", "encodage inconnu laissé tel quel");
        assert_eq!(decode_header("=?UTF-8?Q?a=?="), "=?UTF-8?Q?a=?=", "Q tronqué laissé tel quel");
    }

    #[test] fn expediteur() {
        assert_eq!(parse_from(r#""Dupont, Jean" <j@x.fr>"#), ("Dupont, Jean".into(), "j@x.fr".into()));
        assert_eq!(parse_from("Jean <j@x.fr>"), ("Jean".into(), "j@x.fr".into()));
        assert_eq!(parse_from("j@x.fr"), ("j@x.fr".into(), "j@x.fr".into()));
        assert_eq!(parse_from(r#""A \"B\"" <a@b.c>"#), (r#"A "B""#.into(), "a@b.c".into()));
        assert_eq!(parse_from("=?UTF-8?Q?Andr=C3=A9?= <a@b.c>"), ("André".into(), "a@b.c".into()));
    }

    #[test] fn corps_et_extraits() {
        assert_eq!(b64url("SGVsbG8_Pz4-").unwrap(), b"Hello??>>");
        assert_eq!(b64url("YQ").unwrap(), b"a");
        assert_eq!(b64url("YQ==").unwrap(), b"a");
        assert_eq!(to_text(&[0x52, 0xE9], "ISO-8859-1"), "Ré");
        assert_eq!(unescape_snippet("l&#39;accueil &amp;lt;"), "l'accueil &lt;");
    }
}
