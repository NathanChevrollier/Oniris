//! Favicons : parties pures (garde SSRF, extraction de `<link rel=icon>`, redimensionnement) et file d'attente
//! en base. Le réseau est dans `src-tauri` ; rien ici ne fait de requête.
use crate::CoreError;
use rusqlite::{params, Connection};
use std::net::IpAddr;
use url::Url;

/// Délai avant un nouvel essai après un échec.
pub const RETRY_MS: i64 = 7 * 24 * 3600 * 1000;

/// Vrai pour une adresse joignable sur Internet : ni boucle locale, ni lien local, ni privée (RFC 1918, CGNAT, ULA).
pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            let o = v.octets();
            !(v.is_private() || v.is_loopback() || v.is_link_local() || v.is_unspecified() || v.is_broadcast() || v.is_multicast()
                || o[0] == 0 // 0.0.0.0/8
                || (o[0] == 100 && (o[1] & 0xc0) == 64) // 100.64.0.0/10 (CGNAT)
                || (o[0] == 192 && o[1] == 0 && o[2] == 0) // 192.0.0.0/24
                || (o[0] == 198 && (o[1] & 0xfe) == 18) // 198.18.0.0/15
                || o[0] >= 240) // 240.0.0.0/4
        }
        IpAddr::V6(v) => {
            let s = v.segments();
            let v4 = |hi: u16, lo: u16| IpAddr::V4(std::net::Ipv4Addr::new((hi >> 8) as u8, hi as u8, (lo >> 8) as u8, lo as u8));
            if let Some(m) = v.to_ipv4_mapped() {
                is_public_ip(IpAddr::V4(m))
            } else if s[..6] == [0, 0, 0, 0, 0, 0] || s[..6] == [0x64, 0xff9b, 0, 0, 0, 0] {
                is_public_ip(v4(s[6], s[7])) // ::/96 (dont :: et ::1) et NAT64 64:ff9b::/96
            } else if s[0] == 0x2002 {
                is_public_ip(v4(s[1], s[2])) // 6to4
            } else {
                !(v.is_loopback() || v.is_unspecified() || v.is_multicast() || (s[0] & 0xfe00) == 0xfc00 || (s[0] & 0xffc0) == 0xfe80 || (s[0] & 0xffc0) == 0xfec0)
            }
        }
    }
}

/// Cible d'une redirection : http ou https seulement, relative résolue.
pub fn redirect_target(from: &Url, location: &str) -> Option<Url> {
    from.join(location).ok().filter(|u| matches!(u.scheme(), "http" | "https"))
}

/// Adresses des icônes déclarées par `<link rel="icon" | "shortcut icon" | "apple-touch-icon" href>`, résolues sur `base`.
pub fn icon_links(html: &str, base: &Url) -> Vec<Url> {
    // shortcut: balayage de texte, pas un vrai analyseur HTML ; suffisant pour les <link> usuels.
    let lower = html.to_ascii_lowercase(); // ASCII seulement : les indices restent valides dans `html`
    let (mut out, mut at) = (Vec::new(), 0);
    while let Some(i) = lower[at..].find("<link") {
        let start = at + i + 5;
        let end = lower[start..].find('>').map_or(lower.len(), |e| start + e);
        at = end;
        let attrs = attributes(&html[start..end]);
        let get = |k: &str| attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
        let rel = get("rel").unwrap_or("").to_ascii_lowercase();
        if !rel.split_whitespace().any(|r| matches!(r, "icon" | "apple-touch-icon")) {
            continue;
        }
        if let Some(u) = get("href").and_then(|h| redirect_target(base, h.trim())) {
            out.push(u);
        }
    }
    out
}

/// Attributs `nom=valeur` (valeur entre guillemets ou nue), noms en minuscules.
fn attributes(s: &str) -> Vec<(String, String)> {
    let b = s.as_bytes();
    let (mut i, mut out) = (0, Vec::new());
    while i < b.len() {
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b'/') { i += 1; }
        let n0 = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'=' && b[i] != b'/' { i += 1; }
        let name = s[n0..i].to_ascii_lowercase();
        while i < b.len() && b[i].is_ascii_whitespace() { i += 1; }
        let mut val = "";
        if i < b.len() && b[i] == b'=' {
            i += 1;
            while i < b.len() && b[i].is_ascii_whitespace() { i += 1; }
            if i < b.len() && (b[i] == b'"' || b[i] == b'\'') {
                let (q, v0) = (b[i], i + 1);
                i = v0;
                while i < b.len() && b[i] != q { i += 1; }
                val = &s[v0..i];
                i += 1;
            } else {
                let v0 = i;
                while i < b.len() && !b[i].is_ascii_whitespace() { i += 1; }
                val = &s[v0..i];
            }
        }
        if !name.is_empty() { out.push((name, val.to_string())); }
    }
    out
}

/// Décode (ico, png, gif, jpeg, webp), réduit à 64 px au plus et renvoie une URL `data:image/png;base64,…`.
pub fn to_data_url(bytes: &[u8]) -> Option<String> {
    use base64::Engine;
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(1024);
    limits.max_image_height = Some(1024);
    limits.max_alloc = Some(16 * 1024 * 1024);
    reader.limits(limits);
    let img = reader.decode().ok()?;
    let img = if img.width() > 64 || img.height() > 64 { img.resize(64, 64, image::imageops::FilterType::Lanczos3) } else { img };
    let mut png = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).ok()?;
    Some(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png)))
}

/// Prochain favori sans icône dont la fenêtre de nouvel essai est passée.
pub fn next_pending(c: &Connection, now_ms: i64) -> Result<Option<(i64, String)>, CoreError> {
    let mut st = c.prepare(
        "SELECT id, url FROM favorites WHERE deleted = 0 AND icon IS NULL AND (icon_tried_ms IS NULL OR icon_tried_ms <= ?1) ORDER BY id LIMIT 1",
    )?;
    let mut rows = st.query_map([now_ms - RETRY_MS], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.next().transpose()?)
}

/// Enregistre l'icône (ou, sans icône, seulement l'échec) et la date de la tentative.
pub fn record(c: &Connection, id: i64, icon: Option<&str>, now_ms: i64) -> Result<(), CoreError> {
    c.execute("UPDATE favorites SET icon = ?2, icon_tried_ms = ?3 WHERE id = ?1", params![id, icon, now_ms])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::favorites::add;
    use crate::test_util::db;

    fn ip(s: &str) -> IpAddr { s.parse().unwrap() }

    #[test]
    fn adresses_publiques_et_privees() {
        for p in ["8.8.8.8", "1.1.1.1", "93.184.216.34", "2606:4700:4700::1111"] { assert!(is_public_ip(ip(p)), "{p}"); }
        for p in ["127.0.0.1", "10.0.0.5", "172.16.3.4", "172.31.255.255", "192.168.1.1", "169.254.169.254", "0.0.0.0", "100.64.0.1",
                  "255.255.255.255", "224.0.0.1", "0.1.2.3", "192.0.0.8", "198.18.0.1", "198.19.255.1", "240.0.0.1", "fec0::1", "64:ff9b::7f00:1", "64:ff9b::a00:1", "2002:7f00:1::", "2002:c0a8:101::", "::7f00:1", "::a00:1", "::1", "::", "fe80::1", "fc00::1", "fd12::1", "::ffff:127.0.0.1", "::ffff:10.1.1.1", "ff02::1"] {
            assert!(!is_public_ip(ip(p)), "{p}");
        }
        assert!(is_public_ip(ip("64:ff9b::808:808")) && is_public_ip(ip("2002:808:808::")) && is_public_ip(ip("198.20.0.1")) && is_public_ip(ip("172.32.0.1")) && is_public_ip(ip("::ffff:8.8.8.8")));
    }

    #[test]
    fn redirections_http_et_https_seulement() {
        let from = Url::parse("https://a.com/x/y").unwrap();
        assert_eq!(redirect_target(&from, "/z").unwrap().as_str(), "https://a.com/z");
        assert_eq!(redirect_target(&from, "http://b.com/").unwrap().as_str(), "http://b.com/");
        for bad in ["file:///c:/x", "ftp://b.com", "javascript:1", "data:text/plain,x"] { assert!(redirect_target(&from, bad).is_none(), "{bad}"); }
    }

    #[test]
    fn extraction_des_liens_d_icone_sur_du_html_enregistre() {
        let html = include_str!("../../tests/fixtures/favorites/home.html");
        let base = Url::parse("https://site.example.com/app/").unwrap();
        let got: Vec<String> = icon_links(html, &base).iter().map(Url::to_string).collect();
        assert_eq!(got, [
            "https://site.example.com/static/fav.png",
            "https://site.example.com/app/old.ico",
            "https://cdn.example.net/touch.png",
        ]);
        assert!(icon_links("<html><head><link rel=stylesheet href=a.css></head>", &base).is_empty());
    }

    #[test]
    fn image_reduite_en_png_64_et_octets_invalides_refuses() {
        let mut png = Vec::new();
        image::DynamicImage::new_rgba8(200, 100).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let url = to_data_url(&png).unwrap();
        let b64 = url.strip_prefix("data:image/png;base64,").unwrap();
        let out = image::load_from_memory(&base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64).unwrap()).unwrap();
        assert_eq!((out.width(), out.height()), (64, 32));
        assert!(to_data_url(b"<html>pas une image</html>").is_none());
    }

    #[test]
    fn file_d_attente_et_fenetre_de_nouvel_essai() {
        let c = db();
        let a = add(&c, "A", "https://a.com", None, 1).unwrap();
        let b = add(&c, "B", "https://b.com", None, 1).unwrap();
        let now = 10 * RETRY_MS;
        assert_eq!(next_pending(&c, now).unwrap().unwrap(), (a, "https://a.com/".to_string()));
        record(&c, a, None, now).unwrap(); // échec : pas avant 7 jours
        assert_eq!(next_pending(&c, now + 1).unwrap().unwrap().0, b);
        record(&c, b, Some("data:image/png;base64,AA=="), now).unwrap();
        assert!(next_pending(&c, now + RETRY_MS - 1).unwrap().is_none());
        assert_eq!(next_pending(&c, now + RETRY_MS + 1).unwrap().unwrap().0, a);
        assert_eq!(crate::favorites::get(&c, b).unwrap().unwrap().icon.as_deref(), Some("data:image/png;base64,AA=="));
    }
}
