//! Favicons en tâche de fond : un favori à la fois, une requête par seconde au plus, directement vers le site
//! (aucun service tiers, aucun cookie). Garde SSRF : chaque saut est résolu puis refusé s'il n'est pas public,
//! et la connexion est épinglée sur l'adresse vérifiée. Journal : des nombres, jamais d'adresse.
use crate::state::AppState;
use oniris_core::events::Topic;
use oniris_core::favorites::icon::{icon_links, is_public_ip, next_pending, record, redirect_target, to_data_url};
use oniris_core::time_util::now_ms;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Manager};
use url::{Host, Url};

const MAX_BYTES: usize = 256 * 1024;
const MAX_REDIRECTS: usize = 3;
static RUNNING: AtomicBool = AtomicBool::new(false);

/// Lance la file si elle ne tourne pas déjà ; sans effet s'il n'y a rien à faire.
pub fn spawn(app: &AppHandle) {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        run(&app).await;
        RUNNING.store(false, Ordering::SeqCst);
    });
}

async fn run(app: &AppHandle) {
    let (mut found, mut tried) = (0u32, 0u32);
    loop {
        let st = app.state::<AppState>();
        let next = next_pending(&st.db.lock(), now_ms());
        let Ok(Some((id, url))) = next else { break };
        let icon = fetch_icon(&url).await;
        tried += 1;
        found += u32::from(icon.is_some());
        if let Err(e) = record(&st.db.lock(), id, icon.as_deref(), now_ms()) {
            tracing::warn!("favicons : enregistrement impossible : {e}");
            break;
        }
        if icon.is_some() {
            st.events.changed(Topic::Favorites);
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    if tried > 0 {
        tracing::info!(tried, found, "favicons : file traitée");
    }
}

/// `/favicon.ico`, puis les `<link rel=icon>` de la page d'accueil si le premier essai échoue.
async fn fetch_icon(favorite: &str) -> Option<String> {
    let home = Url::parse(favorite).ok().filter(|u| matches!(u.scheme(), "http" | "https") && u.host().is_some())?.join("/").ok()?;
    if let Some(d) = get(&home.join("/favicon.ico").ok()?, "image/").await.and_then(|(_, b)| to_data_url(&b)) {
        return Some(d);
    }
    let (page, html) = get(&home, "text/html").await?;
    for link in icon_links(&String::from_utf8_lossy(&html), &page).into_iter().take(3) {
        if let Some(d) = get(&link, "image/").await.and_then(|(_, b)| to_data_url(&b)) {
            return Some(d);
        }
    }
    None
}

/// GET sans cookie, 5 s, 256 Ko, type de contenu imposé, 3 redirections au plus. Renvoie l'adresse finale et le corps.
async fn get(start: &Url, content_type: &str) -> Option<(Url, Vec<u8>)> {
    let mut cur = start.clone();
    for _ in 0..=MAX_REDIRECTS {
        let port = cur.port_or_known_default()?;
        let mut builder = reqwest::Client::builder().timeout(Duration::from_secs(5)).redirect(reqwest::redirect::Policy::none()).user_agent("Oniris/0.1");
        match cur.host()? {
            Host::Ipv4(ip) if is_public_ip(IpAddr::V4(ip)) => {}
            Host::Ipv6(ip) if is_public_ip(IpAddr::V6(ip)) => {}
            Host::Domain(d) => {
                let addrs: Vec<SocketAddr> = tokio::net::lookup_host((d, port)).await.ok()?.collect();
                if addrs.is_empty() || addrs.iter().any(|a| !is_public_ip(a.ip())) {
                    return None;
                }
                builder = builder.resolve(d, addrs[0]);
            }
            _ => return None,
        }
        let mut resp = builder.build().ok()?.get(cur.clone()).send().await.ok()?;
        if resp.status().is_redirection() {
            let loc = resp.headers().get(reqwest::header::LOCATION)?.to_str().ok()?;
            cur = redirect_target(&cur, loc)?;
            continue;
        }
        let ct = resp.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_ascii_lowercase();
        if !resp.status().is_success() || !ct.starts_with(content_type) || resp.content_length().is_some_and(|n| n > MAX_BYTES as u64) {
            return None;
        }
        let mut body = Vec::new();
        while let Some(chunk) = resp.chunk().await.ok()? {
            body.extend_from_slice(&chunk);
            if body.len() > MAX_BYTES {
                return None;
            }
        }
        return Some((cur, body));
    }
    None
}
