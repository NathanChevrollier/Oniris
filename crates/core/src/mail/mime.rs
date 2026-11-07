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

/// En-tête encodé RFC 2047 (`=?UTF-8?B?…?=`, mots de 75 caractères au plus) s'il n'est pas en ASCII imprimable.
pub fn encode_header(s: &str) -> String {
    if s.bytes().all(|b| (0x20..0x7F).contains(&b)) && !s.contains("=?") {
        return s.to_string();
    }
    let (mut words, mut chunk) = (Vec::new(), String::new());
    for c in s.chars() {
        // 45 octets -> 60 en base64, + 12 d'enrobage : 72.
        if chunk.len() + c.len_utf8() > 45 {
            words.push(std::mem::take(&mut chunk));
        }
        chunk.push(c);
    }
    words.push(chunk);
    words.iter().map(|w| format!("=?UTF-8?B?{}?=", B64.encode(w))).collect::<Vec<_>>().join("\r\n ")
}

/// Ajoute `Re: ` sauf s'il y est déjà (sans casse).
pub fn reply_subject(subject: &str) -> String {
    let s = subject.trim();
    if s.get(..3).is_some_and(|p| p.eq_ignore_ascii_case("re:")) { s.to_string() } else { format!("Re: {s}") }
}

/// Adresse seule de forme `local@domaine` : ASCII imprimable, sans blanc ni caractère spécial d'en-tête.
pub fn valid_address(a: &str) -> bool {
    let Some((local, domain)) = a.split_once('@') else { return false };
    let ok = |p: &str| !p.is_empty() && p.bytes().all(|b| b.is_ascii_graphic() && !br#"<>()[],;:"\@"#.contains(&b));
    a.len() <= 254 && ok(local) && ok(domain) && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.')
}

#[derive(Debug, PartialEq, thiserror::Error)]
pub enum MimeError {
    #[error("destinataire invalide")]
    BadRecipient,
    #[error("sujet invalide (retour à la ligne)")]
    BadSubject,
}

/// Message RFC 822 `text/plain; charset=utf-8` (base64), encodé base64url pour `messages.send`.
/// `in_reply_to` présent : réponse (`In-Reply-To` et `References`) ; le sujet est donné tel quel (voir `reply_subject`).
/// Retour à la ligne dans le sujet, destinataire autre qu'une adresse : refusé (injection d'en-tête).
pub fn reply(from: &str, to: &str, subject: &str, body_text: &str, in_reply_to: Option<&str>, references: Option<&str>) -> Result<String, MimeError> {
    if !valid_address(to) || !valid_address(from) {
        return Err(MimeError::BadRecipient);
    }
    if subject.contains(['\r', '\n']) {
        return Err(MimeError::BadSubject);
    }
    // Valeurs venues du serveur : ids ASCII imprimables de 250 caractères au plus, 20 derniers, un par ligne pliée.
    let ids = |v: &str| v.split_whitespace().filter(|i| i.len() <= 250 && i.bytes().all(|b| b.is_ascii_graphic())).map(String::from).collect::<Vec<_>>();
    let mut m = format!("From: {from}\r\nTo: {to}\r\nSubject: {}\r\nMIME-Version: 1.0\r\n", encode_header(subject.trim()));
    if let Some(id) = in_reply_to.map(ids).and_then(|v| v.into_iter().next()) {
        let mut refs = references.map(ids).unwrap_or_default();
        refs.push(id.clone());
        let refs = &refs[refs.len().saturating_sub(20)..];
        m += &format!("In-Reply-To: {id}\r\nReferences: {}\r\n", refs.join("\r\n "));
    }
    m += "Content-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\n";
    let body = body_text.replace("\r\n", "\n").replace('\n', "\r\n");
    let b64 = B64.encode(body.as_bytes());
    for line in b64.as_bytes().chunks(76) {
        m += std::str::from_utf8(line).unwrap_or_default();
        m += "\r\n";
    }
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(m))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decoded(raw: &str) -> String { String::from_utf8(b64url(raw).unwrap()).unwrap() }

    #[test] fn reponse() {
        let m = decoded(&reply("moi@gmail.com", "j@x.fr", "Été à Noël", "Bonjour,\nà demain", Some("<a@x>"), Some("<z@x>")).unwrap());
        let subject = m.lines().find(|l| l.starts_with("Subject: ")).unwrap();
        assert!(subject.contains("=?UTF-8?B?"), "{subject}");
        assert_eq!(decode_header(&subject["Subject: ".len()..]), "Été à Noël");
        assert!(m.contains("\r\nIn-Reply-To: <a@x>\r\nReferences: <z@x>\r\n <a@x>\r\n"), "{m}");
        assert!(m.contains("Content-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\n"));
        let body = m.split("\r\n\r\n").nth(1).unwrap().replace("\r\n", "");
        assert_eq!(B64.decode(body).unwrap(), "Bonjour,\r\nà demain".as_bytes());
        assert_eq!(reply_subject("RE: Point"), "RE: Point", "pas de Re: Re:");
        let m = decoded(&reply("moi@gmail.com", "j@x.fr", "Point", "x", Some("<a@x>"), None).unwrap());
        assert!(m.contains("Subject: Point\r\n") && m.contains("References: <a@x>\r\n"), "sujet tel quel");
        let m = decoded(&reply("moi@gmail.com", "j@x.fr", "Point", "x", None, None).unwrap());
        assert!(!m.contains("In-Reply-To"));
        let long = "é".repeat(100);
        let enc = encode_header(&long);
        assert!(enc.split("\r\n ").all(|w| w.len() <= 75));
        assert_eq!(decode_header(&enc.replace("\r\n", "")), long);
    }

    #[test] fn references_longue_chaine() {
        let chain: Vec<String> = (0..60).map(|i| format!("<{i}-{}@x>", "a".repeat(240))).collect();
        let long = format!("<{}@x>", "b".repeat(300));
        let refs = format!("{} {long} <é@x> <ok@x>", chain.join(" "));
        let m = decoded(&reply("moi@gmail.com", "j@x.fr", "S", "x", Some("<last@x>"), Some(&refs)).unwrap());
        assert!(m.lines().all(|l| l.len() <= 998), "ligne trop longue");
        let head = m.split("\r\n\r\n").next().unwrap();
        let block = head.split("References: ").nth(1).unwrap().split("\r\nContent-Type").next().unwrap();
        let ids: Vec<&str> = block.split("\r\n ").collect();
        assert_eq!(ids.len(), 20, "20 derniers");
        assert_eq!(ids[19], "<last@x>");
        assert_eq!(ids[18], "<ok@x>");
        assert!(!block.contains('é') && !block.contains("bbbb"), "ids non ASCII ou trop longs écartés");
    }

    #[test] fn injection_d_en_tete_refusee() {
        let r = |to: &str, s: &str| reply("moi@gmail.com", to, s, "x", None, None);
        assert_eq!(r("j@x.fr", "a\r\nBcc: z@y.fr"), Err(MimeError::BadSubject));
        assert_eq!(r("j@x.fr", "a\nb"), Err(MimeError::BadSubject));
        assert_eq!(r("j@x.fr\r\nBcc: z@y.fr", "a"), Err(MimeError::BadRecipient));
        for bad in ["", "jx.fr", "j@x", "j@x.fr, k@y.fr", "Jean <j@x.fr>", "j k@x.fr", "j@@x.fr", "j@.fr", "é@x.fr"] {
            assert!(!valid_address(bad), "{bad}");
        }
        assert!(valid_address("jean.dupont+oniris@sous.exemple.fr"));
    }

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
