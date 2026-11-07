//! Rendu sûr d'un corps de mail (contenu non fiable). `ammonia` ne garde que texte et mise en forme ; les liens
//! deviennent `data-href` (aucun `href` actif) et ne s'ouvrent que par `mail.open_link` (donc `safe_url`).
//! L'interface affiche le résultat dans une `iframe` `sandbox` sans scripts, sous une CSP qui bloque le distant.
use std::collections::HashSet;

/// Entrée gardée au plus (octets).
pub const MAX_INPUT: usize = 1 << 20;
/// Liens rendus au plus.
pub const MAX_LINKS: usize = 50;

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Sanitized {
    pub html: String,
    /// Valeurs `data-href`, dans l'ordre, sans doublon, `MAX_LINKS` au plus.
    pub links: Vec<String>,
    /// L'entrée dépassait `MAX_INPUT` : la fin est coupée.
    pub truncated: bool,
}

pub fn truncate(s: &str, max: usize) -> &str {
    let mut end = s.len().min(max);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Déclarations CSS sans ressource : `url(`, `image-set`, `expression`, `@import`, échappements et commentaires retirés.
fn clean_style(v: &str) -> Option<String> {
    let kept: Vec<&str> = v
        .split(';')
        .filter(|d| {
            let l = d.to_ascii_lowercase();
            !d.trim().is_empty() && !["url(", "image-set", "expression", "@import", "\\", "/*"].iter().any(|k| l.contains(k))
        })
        .collect();
    (!kept.is_empty()).then(|| kept.join(";"))
}

fn builder() -> ammonia::Builder<'static> {
    let mut b = ammonia::Builder::default();
    b.add_generic_attributes(["style", "align", "dir"])
        .url_schemes(HashSet::from(["http", "https", "mailto", "cid"]))
        .attribute_filter(|el, attr, value| match (el, attr) {
            (_, "style") => clean_style(value).map(Into::into),
            ("img", "src") => {
                let l = value.trim().to_ascii_lowercase();
                (l.starts_with("https://") || l.starts_with("http://") || l.starts_with("cid:")).then_some(value.into())
            }
            _ => Some(value.into()),
        });
    b
}

/// Sortie d'`ammonia` (attributs toujours entre `"`, `"` échappé dedans) : chaque ` href="` d'une balise devient
/// ` data-href="` et sa valeur entre dans `links`. Le texte, hors balise, n'est jamais touché.
fn rewrite_links(html: &str) -> Sanitized {
    let (mut out, mut links) = (String::with_capacity(html.len() + 64), Vec::new());
    let (mut in_tag, mut in_quote) = (false, false);
    let mut rest = html;
    while let Some(c) = rest.chars().next() {
        if in_tag && !in_quote && rest.starts_with(" href=\"") {
            let v = &rest[7..];
            let end = v.find('"').unwrap_or(v.len());
            let url = v[..end].replace("&quot;", "\"").replace("&nbsp;", "\u{a0}").replace("&amp;", "&");
            if links.len() < MAX_LINKS && !links.contains(&url) {
                links.push(url);
            }
            out.push_str(" data-href=\"");
            out.push_str(&v[..end]);
            out.push('"');
            rest = &v[(end + 1).min(v.len())..];
            continue;
        }
        match c {
            '<' if !in_tag => in_tag = true,
            '"' if in_tag => in_quote = !in_quote,
            '>' if in_tag && !in_quote => in_tag = false,
            _ => {}
        }
        out.push(c);
        rest = &rest[c.len_utf8()..];
    }
    Sanitized { html: out, links, truncated: false }
}

/// HTML non fiable vers HTML affichable (entrée tronquée à 1 Mo).
pub fn sanitize(html: &str) -> Sanitized {
    let mut s = rewrite_links(&builder().clean(truncate(html, MAX_INPUT)).to_string());
    s.truncated = html.len() > MAX_INPUT;
    s
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;")
}

/// Texte brut vers HTML : échappé, retours à la ligne gardés, URL http(s) cliquables (`data-href`).
pub fn text_to_html(text: &str) -> Sanitized {
    let full = text;
    let text = truncate(text, MAX_INPUT);
    let (mut out, mut links, mut rest) = (String::new(), Vec::new(), text);
    loop {
        let at = [rest.find("https://"), rest.find("http://")].into_iter().flatten().min();
        let Some(at) = at else { break };
        out += &escape(&rest[..at]);
        let tail = &rest[at..];
        let end = tail.find(|c: char| c.is_whitespace() || "<>\"".contains(c)).unwrap_or(tail.len());
        let url = tail[..end].trim_end_matches(|c: char| ".,;:!?)]}'".contains(c));
        let scheme = if tail.starts_with("https://") { 8 } else { 7 };
        if url.len() <= scheme {
            // Schéma seul : du texte.
            out += &escape(&tail[..scheme]);
            rest = &tail[scheme..];
            continue;
        }
        let e = escape(url);
        out += &format!("<a data-href=\"{e}\">{e}</a>");
        if links.len() < MAX_LINKS && !links.iter().any(|l| l == url) {
            links.push(url.to_string());
        }
        rest = &tail[url.len()..];
    }
    out += &escape(rest);
    Sanitized { html: out.replace("\r\n", "\n").replace('\n', "<br>"), links, truncated: full.len() > MAX_INPUT }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::launch::safe_url;

    fn no_active_href(h: &str) {
        assert!(!h.contains(" href="), "href actif : {h}");
    }

    #[test] fn retire_le_dangereux() {
        let s = sanitize(r#"<p>a<script>alert(1)</script>b</p><img src=x onerror="alert(1)"><b onclick="x()">gras</b>"#);
        assert!(!s.html.contains("script") && !s.html.contains("alert") && !s.html.contains("onerror") && !s.html.contains("onclick"), "{}", s.html);
        assert!(s.html.contains("<b>gras</b>") && s.html.contains("<img>"), "{}", s.html);
        let s = sanitize(r#"<div style="color:red;background:url(https://track/x.gif);BACKGROUND-IMAGE: URL('//t')">t</div>"#);
        assert_eq!(s.html, r#"<div style="color:red">t</div>"#);
        let s = sanitize(r#"<span style="background:u\72l(https://t)">t</span>"#);
        assert!(!s.html.contains("style"), "échappement CSS : {}", s.html);
        let s = sanitize(r#"<a href="javascript:alert(1)">x</a><a href="https://ok.fr/a?b=1&amp;c=2">ok</a>"#);
        no_active_href(&s.html);
        assert!(s.links.iter().all(|l| safe_url(l).is_some()), "{:?}", s.links);
        assert!(!s.html.contains("javascript"), "{}", s.html);
        assert_eq!(s.links, ["https://ok.fr/a?b=1&c=2"]);
        assert!(s.html.contains(r#"<a data-href="https://ok.fr/a?b=1&amp;c=2" rel="noopener noreferrer">ok</a>"#), "{}", s.html);
        let s = sanitize(r#"<form action="https://x"><input name=a><button>Envoyer</button></form><meta http-equiv="refresh" content="0;url=https://x"><iframe src="https://x"></iframe><object data="x"></object><link rel=stylesheet href="https://x/s.css"><style>p{}</style>"#);
        for t in ["form", "input", "meta", "refresh", "iframe", "object", "link", "stylesheet", "<style"] {
            assert!(!s.html.contains(t), "{t} : {}", s.html);
        }
        no_active_href(&s.html);
        assert!(s.links.is_empty());
    }

    #[test] fn images_et_liens() {
        let s = sanitize(r#"<img src="cid:logo@x"><img src="https://img.fr/a.png" alt="a"><img src="data:image/png;base64,AA"><img src="//x/y.png">"#);
        assert!(s.html.contains(r#"src="cid:logo@x""#) && s.html.contains(r#"src="https://img.fr/a.png""#), "{}", s.html);
        assert!(!s.html.contains("data:") && !s.html.contains("//x/y"), "{}", s.html);
        let many: String = (0..80).map(|i| format!(r#"<a href="https://x.fr/{i}">l</a><a href="https://x.fr/{i}">doublon</a>"#)).collect();
        let s = sanitize(&many);
        assert_eq!(s.links.len(), MAX_LINKS);
        assert_eq!(s.links[1], "https://x.fr/1");
        no_active_href(&s.html);
        let s = sanitize(r#"<p>texte href="x" et <a title='a " href="y' href="https://z.fr">z</a></p>"#);
        assert!(s.html.contains(r#"texte href="x" et"#), "le texte n'est pas réécrit : {}", s.html);
        assert_eq!(s.links, ["https://z.fr"]);
    }

    #[test] fn grosse_entree_tronquee() {
        let big = format!("<p>{}</p>", "é".repeat(5 * 1024 * 1024 / 2));
        let s = sanitize(&big);
        assert!(s.html.len() <= MAX_INPUT + 16, "{}", s.html.len());
        assert!(s.html.starts_with("<p>éé"));
        assert!(s.truncated && !sanitize("<p>a</p>").truncated);
        assert!(text_to_html(&"a".repeat(MAX_INPUT + 1)).truncated && !text_to_html("a").truncated);
        assert_eq!(truncate("aé", 2), "a", "coupe sur une frontière de caractère");
    }

    #[test] fn texte_brut() {
        let s = text_to_html("Voir https://a.fr/x?y=1&z=2. Et <b>http://b.fr</b>\nfin");
        assert_eq!(s.links, ["https://a.fr/x?y=1&z=2", "http://b.fr"]);
        assert_eq!(s.html, r#"Voir <a data-href="https://a.fr/x?y=1&amp;z=2">https://a.fr/x?y=1&amp;z=2</a>. Et &lt;b&gt;<a data-href="http://b.fr">http://b.fr</a>&lt;/b&gt;<br>fin"#);
        assert_eq!(text_to_html("http:// rien").html, "http:// rien");
    }
}
