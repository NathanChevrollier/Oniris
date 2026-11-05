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

/// windows-1252, octets 0x80..0x9F (les cinq non définis restent les caractères de contrôle latin-1, comme le WHATWG).
const CP1252: [char; 32] = [
    '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8D}', 'Ž', '\u{8F}',
    '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9D}', 'ž', 'Ÿ',
];

/// iso-8859-15 : latin-1 sauf huit positions.
fn latin9(b: u8) -> char {
    match b {
        0xA4 => '€', 0xA6 => 'Š', 0xA8 => 'š', 0xB4 => 'Ž', 0xB8 => 'ž', 0xBC => 'Œ', 0xBD => 'œ', 0xBE => 'Ÿ',
        b => b as char,
    }
}

/// Octets vers texte selon le jeu de caractères ; inconnu : UTF-8 avec remplacement.
pub fn to_text(bytes: &[u8], charset: &str) -> String {
    match charset.trim().to_ascii_lowercase().as_str() {
        "iso-8859-1" | "latin1" | "iso_8859-1" => bytes.iter().map(|&b| b as char).collect(),
        "iso-8859-15" | "latin9" | "iso_8859-15" => bytes.iter().map(|&b| latin9(b)).collect(),
        "windows-1252" | "cp1252" => bytes.iter().map(|&b| if (0x80..0xA0).contains(&b) { CP1252[(b - 0x80) as usize] } else { b as char }).collect(),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// Mot encodé `=?charset?B|Q?texte?=` en tête de `s` : (jeu de caractères en minuscules, octets, longueur lue).
fn parse_word(s: &str) -> Option<(String, Vec<u8>, usize)> {
    let inner = s.strip_prefix("=?")?;
    let q1 = inner.find('?')?;
    let charset = &inner[..q1];
    let r = &inner.as_bytes()[q1 + 1..];
    if charset.is_empty() || charset.contains(char::is_whitespace) || r.len() < 2 || r[1] != b'?' || !r[0].is_ascii() {
        return None;
    }
    let tail = &inner[q1 + 3..];
    let end = tail.find("?=")?;
    let text = &tail[..end];
    if text.contains(|c: char| c == '?' || c.is_whitespace()) {
        return None;
    }
    let bytes = match r[0] {
        b'B' | b'b' => B64.decode(text).ok()?,
        b'Q' | b'q' => {
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
    // RFC 2231 : `charset*langue`.
    let charset = charset.split('*').next()?.to_ascii_lowercase();
    Some((charset, bytes, 2 + q1 + 3 + end + 2))
}

/// Décode les mots encodés RFC 2047. Les blancs entre deux mots encodés disparaissent, et les octets de mots
/// voisins de même jeu de caractères sont joints avant décodage (caractère multioctet coupé en deux).
pub fn decode_header(s: &str) -> String {
    let flush = |out: &mut String, p: Option<(String, Vec<u8>)>| {
        if let Some((cs, b)) = p {
            out.push_str(&to_text(&b, &cs));
        }
    };
    let (mut out, mut gap, mut rest) = (String::new(), String::new(), s);
    let mut pending: Option<(String, Vec<u8>)> = None;
    while let Some(c) = rest.chars().next() {
        if let Some((cs, bytes, len)) = rest.starts_with("=?").then(|| parse_word(rest)).flatten() {
            match pending.as_mut() {
                Some((p, b)) if *p == cs => b.extend(bytes),
                _ => {
                    flush(&mut out, pending.take());
                    pending = Some((cs, bytes));
                }
            }
            gap.clear();
            rest = &rest[len..];
            continue;
        }
        rest = &rest[c.len_utf8()..];
        if pending.is_some() && c.is_whitespace() {
            gap.push(c);
            continue;
        }
        flush(&mut out, pending.take());
        out.push_str(&gap);
        gap.clear();
        out.push(c);
    }
    flush(&mut out, pending);
    out.push_str(&gap);
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
        assert_eq!(decode_header("a =? b =?UTF-8?Q?=C3=A9?= c"), "a =? b é c", "un =? isolé n'arrête pas le décodage");
        assert_eq!(decode_header("=?UTF-8?Q?=C3?= =?utf-8?Q?=A9t=C3=A9?="), "été", "multioctet coupé entre deux mots");
        assert_eq!(decode_header("=?UTF-8?Q?a?= =?ISO-8859-1?Q?=E9?= fin"), "aé fin");
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
        assert_eq!(to_text(&[0x80, 0x92, 0x93, 0x94, 0x85, 0x9C, 0x81, 0xE9], "Windows-1252"), "€’“”…œ\u{81}é");
        assert_eq!(to_text(&[0xA4, 0xBD, 0xE9], "iso-8859-15"), "€œé");
        assert_eq!(to_text(&[0xA4], "iso-8859-1"), "¤");
        assert_eq!(unescape_snippet("l&#39;accueil &amp;lt;"), "l'accueil &lt;");
    }
}
