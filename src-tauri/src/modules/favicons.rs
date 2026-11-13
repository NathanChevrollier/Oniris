//! Favicons en tâche de fond : un favori à la fois, une requête par seconde au plus (redirections et icônes
//! indiquées par la page comprises), directement vers le site (aucun service tiers, aucun cookie, aucun proxy).
//! Garde SSRF : chaque saut est résolu puis refusé s'il n'est pas public, et la connexion est épinglée sur
//! l'adresse vérifiée. Journal : des nombres, jamais d'adresse.
use crate::state::AppState;
use oniris_core::events::Topic;
use oniris_core::favorites::icon::{icon_links, is_public_ip, next_pending, record, redirect_target, to_data_url};
use oniris_core::time_util::now_ms;
use oniris_core::Db;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Manager};
use url::{Host, Url};

const MAX_BYTES: usize = 256 * 1024;
const MAX_REDIRECTS: usize = 3;
/// Requêtes au plus pour un favori (favicon.ico, accueil, redirections, icônes de la page).
const MAX_REQUESTS: u32 = 6;
const EMIT_EVERY: u32 = 10;
static RUNNING: AtomicBool = AtomicBool::new(false);
/// Réveil demandé pendant un passage : la file repasse une fois (même schéma que la météo).
static PENDING: AtomicBool = AtomicBool::new(false);

/// Relâche `RUNNING` au `drop`, même si le passage panique.
struct RunGuard;
impl Drop for RunGuard {
    fn drop(&mut self) { RUNNING.store(false, Ordering::SeqCst); }
}

/// Lance la file ou, si elle tourne, demande un nouveau passage ; sans effet s'il n'y a rien à faire.
pub fn spawn(app: &AppHandle) {
    PENDING.store(true, Ordering::SeqCst);
    if RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            {
                let _guard = RunGuard;
                PENDING.store(false, Ordering::SeqCst);
                let (db, events) = {
                    let st = app.state::<AppState>();
                    (st.db.clone(), st.events.clone())
                };
                drain(&db, fetch_icon, || events.changed(Topic::Favorites), now_ms).await;
            }
            // Réveil arrivé pendant le passage : on repasse, sauf si un autre passage a démarré entre-temps.
            if !PENDING.load(Ordering::SeqCst) || RUNNING.swap(true, Ordering::SeqCst) {
                break;
            }
        }
    });
}

/// Traite les favoris sans icône, un par un. L'essai est daté AVANT la requête : un plantage ne laisse jamais
/// le même favori en tête de file. `fetch` tourne dans sa propre tâche : une panique compte comme un échec.
async fn drain<F, Fut>(db: &Db, fetch: F, emit: impl Fn(), now: impl Fn() -> i64)
where
    F: Fn(String) -> Fut,
    Fut: Future<Output = Option<String>> + Send + 'static,
{
    let (mut found, mut tried, mut unsent) = (0u32, 0u32, 0u32);
    loop {
        let next = { next_pending(&db.lock(), now()) };
        let Ok(Some((id, url))) = next else { break };
        if record(&db.lock(), id, None, now()).is_err() {
            break;
        }
        let icon = tokio::spawn(fetch(url)).await.unwrap_or_else(|_| {
            tracing::warn!("favicons : la récupération a échoué brutalement");
            None
        });
        tried += 1;
        if let Some(icon) = icon {
            if record(&db.lock(), id, Some(&icon), now()).is_err() {
                break;
            }
            found += 1;
            unsent += 1;
            if unsent >= EMIT_EVERY {
                unsent = 0;
                emit();
            }
        }
    }
    if unsent > 0 {
        emit();
    }
    if tried > 0 {
        tracing::info!(tried, found, "favicons : file traitée");
    }
}

/// `/favicon.ico`, puis les `<link rel=icon>` de la page d'accueil si le premier essai échoue.
async fn fetch_icon(favorite: String) -> Option<String> {
    let budget = AtomicU32::new(MAX_REQUESTS);
    let home = Url::parse(&favorite).ok().filter(|u| matches!(u.scheme(), "http" | "https") && u.host().is_some())?.join("/").ok()?;
    if let Some(d) = image(&home.join("/favicon.ico").ok()?, &budget).await {
        return Some(d);
    }
    let (page, html) = get(&home, "text/html", &budget).await?;
    for link in icon_links(&String::from_utf8_lossy(&html), &page).into_iter().take(3) {
        if let Some(d) = image(&link, &budget).await {
            return Some(d);
        }
    }
    None
}

/// Télécharge puis décode (hors du fil asynchrone) une image.
async fn image(url: &Url, budget: &AtomicU32) -> Option<String> {
    let (_, bytes) = get(url, "image/", budget).await?;
    tokio::task::spawn_blocking(move || to_data_url(&bytes)).await.ok()?
}

/// GET sans cookie ni proxy, 5 s, 256 Ko, type de contenu imposé, 3 redirections au plus. Chaque requête attend
/// 1 s avant de partir et consomme le budget du favori. Une page HTML trop longue est tronquée, pas rejetée.
async fn get(start: &Url, content_type: &str, budget: &AtomicU32) -> Option<(Url, Vec<u8>)> {
    let mut cur = start.clone();
    for _ in 0..=MAX_REDIRECTS {
        let port = cur.port_or_known_default()?;
        let mut builder = reqwest::Client::builder().timeout(Duration::from_secs(5)).redirect(reqwest::redirect::Policy::none()).no_proxy().user_agent("Oniris/0.1");
        match cur.host()? {
            Host::Ipv4(ip) if is_public_ip(IpAddr::V4(ip)) => {}
            Host::Ipv6(ip) if is_public_ip(IpAddr::V6(ip)) => {}
            Host::Domain(d) => {
                let addrs: Vec<SocketAddr> = tokio::time::timeout(Duration::from_secs(5), tokio::net::lookup_host((d, port))).await.ok()?.ok()?.collect();
                if addrs.is_empty() || addrs.iter().any(|a| !is_public_ip(a.ip())) {
                    return None;
                }
                builder = builder.resolve(d, addrs[0]);
            }
            _ => return None,
        }
        if budget.try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1)).is_err() {
            return None;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
        let mut resp = builder.build().ok()?.get(cur.clone()).send().await.ok()?;
        if resp.status().is_redirection() {
            let loc = resp.headers().get(reqwest::header::LOCATION)?.to_str().ok()?;
            cur = redirect_target(&cur, loc)?;
            continue;
        }
        let ct = resp.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or("").to_ascii_lowercase();
        let html = content_type == "text/html";
        if !resp.status().is_success() || !ct.starts_with(content_type) || (!html && resp.content_length().is_some_and(|n| n > MAX_BYTES as u64)) {
            return None;
        }
        let mut body = Vec::new();
        while let Some(chunk) = resp.chunk().await.ok()? {
            body.extend_from_slice(&chunk);
            if body.len() > MAX_BYTES {
                if html {
                    body.truncate(MAX_BYTES);
                    break;
                }
                return None;
            }
        }
        return Some((cur, body));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use oniris_core::favorites::{add, get as get_fav};
    use parking_lot::Mutex;
    use std::sync::atomic::AtomicI64;
    use std::sync::Arc;

    fn db() -> Db {
        let mut c = rusqlite::Connection::open_in_memory().unwrap();
        oniris_core::migrate(&mut c, oniris_core::CORE_MIGRATIONS).unwrap();
        Arc::new(Mutex::new(c))
    }

    #[tokio::test]
    async fn une_panique_sur_le_premier_favori_n_arrete_pas_la_file() {
        let db = db();
        let (a, b, c) = {
            let c = db.lock();
            (
                add(&c, "A", "https://a.example.com", None, 1).unwrap(),
                add(&c, "B", "https://b.example.com", None, 1).unwrap(),
                add(&c, "C", "https://c.example.com", None, 1).unwrap(),
            )
        };
        let clock = AtomicI64::new(1_000);
        let emitted = AtomicU32::new(0);
        let fetch = |url: String| async move {
            if url.contains("//a.") {
                panic!("décodeur en panne");
            }
            url.contains("//b.").then(|| "data:image/png;base64,AA==".to_string())
        };
        drain(&db, fetch, || { emitted.fetch_add(1, Ordering::SeqCst); }, || clock.fetch_add(1, Ordering::SeqCst)).await;
        let c0 = db.lock();
        assert!(get_fav(&c0, a).unwrap().unwrap().icon.is_none());
        assert!(get_fav(&c0, b).unwrap().unwrap().icon.is_some(), "le deuxième est traité");
        assert!(get_fav(&c0, c).unwrap().unwrap().icon.is_none());
        assert!(next_pending(&c0, 2_000).unwrap().is_none(), "tous datés : rien ne reste en tête de file");
        assert_eq!(emitted.load(Ordering::SeqCst), 1, "une seule émission pour le lot");
    }
}
