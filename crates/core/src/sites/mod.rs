//! Surveillance des sites : sondes HTTP, état déduit de l'historique, transitions à notifier.
//! Journal : jamais d'URL ni de nom de site (contenu utilisateur).
pub mod tools;

use crate::{CoreError, Site};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::time::{Duration, Instant};

pub const TIMEOUT: Duration = Duration::from_secs(10);
/// Corps lu au plus sur 64 Ko, puis abandonné.
const BODY_MAX: usize = 64 * 1024;
/// Sondes gardées 24 h.
const KEEP_MS: i64 = 24 * 3_600_000;
pub const SPARK_POINTS: usize = 30;

/// Client dédié : sans cookie, 3 redirections au plus (un schéma autre que http(s) échoue à la requête).
/// Pas de garde SSRF : les adresses sont celles de l'utilisateur, validées http(s) dans les réglages.
pub fn client() -> Result<reqwest::Client, CoreError> {
    reqwest::Client::builder()
        .user_agent(concat!("Oniris-sonde/", env!("CARGO_PKG_VERSION")))
        .redirect(reqwest::redirect::Policy::limited(3))
        .build()
        .map_err(|e| CoreError::Invalid(format!("client de sondes : {}", e.without_url())))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Probe {
    /// Durée jusqu'à la fin de la lecture du corps ; `None` sans réponse.
    pub ms: Option<u32>,
    pub status: Option<u16>,
    /// Code de 200 à 399.
    pub ok: bool,
}

/// `GET` de l'adresse : jamais d'erreur, un échec réseau donne `ok: false`.
pub async fn probe(http: &reqwest::Client, url: &str, timeout: Duration) -> Probe {
    let t = Instant::now();
    let fail = Probe { ms: None, status: None, ok: false };
    let Ok(mut resp) = http.get(url).timeout(timeout).send().await else { return fail };
    let status = resp.status().as_u16();
    let mut read = 0;
    while read < BODY_MAX {
        match resp.chunk().await {
            Ok(Some(c)) => read += c.len(),
            Ok(None) => break,
            Err(_) => return Probe { status: Some(status), ..fail },
        }
    }
    drop(resp);
    Probe { ms: Some(t.elapsed().as_millis().min(u32::MAX as u128) as u32), status: Some(status), ok: (200..400).contains(&status) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SiteState {
    Unknown,
    Up,
    Slow,
    Down,
    /// Affichage seulement (`views`) : tous les sites échouent, c'est le PC qui est hors ligne.
    Offline,
}

/// `history` : du plus ancien au plus récent. Down après 2 échecs de suite ; Slow si les 3 dernières
/// mesures réussies dépassent `slow_ms` ; un succès rapide redonne Up aussitôt.
pub fn state(history: &[Probe], slow_ms: u32) -> SiteState {
    let n = history.len();
    if n == 0 {
        return SiteState::Unknown;
    }
    if n >= 2 && history[n - 2..].iter().all(|p| !p.ok) {
        return SiteState::Down;
    }
    if n >= 3 && history[n - 3..].iter().all(|p| p.ok && p.ms.is_some_and(|m| m > slow_ms)) {
        return SiteState::Slow;
    }
    SiteState::Up
}

/// Tous les sites ont échoué dans ce tour, avec au moins 2 sites : le PC est hors ligne, pas les sites.
/// Un site seul ne permet pas de distinguer les deux : il est traité normalement.
pub fn all_failed(round: &[Probe]) -> bool {
    round.len() >= 2 && round.iter().all(|p| !p.ok)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind { Down, Slow, Recovered }

/// `site` : index dans les listes `prev`/`next`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Notice { pub site: usize, pub kind: NoticeKind }

/// Notifications d'un tour : passage à Down ou à Slow, retour à Up après Down. Aucune si `all_failed`.
pub fn transitions(prev: &[SiteState], next: &[SiteState], all_failed: bool) -> Vec<Notice> {
    if all_failed {
        return Vec::new();
    }
    prev.iter().zip(next).enumerate().filter_map(|(site, (&p, &n))| {
        let kind = match (p, n) {
            (a, SiteState::Down) if a != SiteState::Down => NoticeKind::Down,
            (a, SiteState::Slow) if a != SiteState::Slow => NoticeKind::Slow,
            (SiteState::Down, SiteState::Up) => NoticeKind::Recovered,
            _ => return None,
        };
        Some(Notice { site, kind })
    }).collect()
}

/// Titre et texte de la notification Windows (le nom reste à l'écran de l'utilisateur, jamais au journal).
pub fn notice_text(kind: NoticeKind, name: &str) -> (&'static str, String) {
    match kind {
        NoticeKind::Down => ("Site en panne", format!("{name} ne répond plus.")),
        NoticeKind::Slow => ("Site lent", format!("{name} répond lentement.")),
        NoticeKind::Recovered => ("Site rétabli", format!("{name} répond de nouveau.")),
    }
}

pub fn record(c: &Connection, site: &str, at_ms: i64, p: &Probe) -> Result<(), CoreError> {
    c.execute(
        "INSERT INTO probes(site, at_ms, ms, status, ok) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![site, at_ms, p.ms, p.status, p.ok],
    )?;
    Ok(())
}

/// Les `limit` dernières sondes du site, du plus ancien au plus récent.
pub fn history(c: &Connection, site: &str, limit: usize) -> Result<Vec<Probe>, CoreError> {
    let mut st = c.prepare_cached("SELECT ms, status, ok FROM probes WHERE site = ?1 ORDER BY at_ms DESC, rowid DESC LIMIT ?2")?;
    let mut v = st
        .query_map(params![site, limit as i64], |r| Ok(Probe { ms: r.get(0)?, status: r.get(1)?, ok: r.get(2)? }))?
        .collect::<Result<Vec<_>, _>>()?;
    v.reverse();
    Ok(v)
}

/// Supprime les sondes de plus de 24 h (une seule requête par tour).
pub fn prune(c: &Connection, now_ms: i64) -> Result<usize, CoreError> {
    Ok(c.execute("DELETE FROM probes WHERE at_ms < ?1", [now_ms - KEEP_MS])?)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SiteView {
    pub name: String,
    pub url: String,
    pub state: SiteState,
    pub last_ms: Option<u32>,
    /// Les 30 dernières mesures réussies (ms), de la plus ancienne à la plus récente.
    pub spark: Vec<u32>,
}

/// Vue de la carte. `offline` : le dernier tour a échoué partout, l'état connu est remplacé par « hors ligne ».
pub fn views(c: &Connection, sites: &[Site], offline: bool) -> Result<Vec<SiteView>, CoreError> {
    sites.iter().map(|s| {
        let h = history(c, &s.url, SPARK_POINTS)?;
        let st = if offline { SiteState::Offline } else { state(&h, s.slow_ms) };
        Ok(SiteView {
            name: s.name.clone(),
            url: s.url.clone(),
            state: st,
            last_ms: h.last().and_then(|p| p.ms.filter(|_| p.ok)),
            spark: h.iter().filter_map(|p| p.ms.filter(|_| p.ok)).collect(),
        })
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header_exists, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const OK: Probe = Probe { ms: Some(100), status: Some(200), ok: true };
    const KO: Probe = Probe { ms: None, status: None, ok: false };
    const SLOW: Probe = Probe { ms: Some(2000), status: Some(200), ok: true };

    #[test] fn un_echec_reste_up_deux_donnent_down() {
        assert_eq!(state(&[], 1500), SiteState::Unknown);
        assert_eq!(state(&[OK, KO], 1500), SiteState::Up);
        assert_eq!(state(&[OK, KO, KO], 1500), SiteState::Down);
        assert_eq!(state(&[KO, KO, OK], 1500), SiteState::Up, "retour immédiat");
        assert_eq!(state(&[KO], 1500), SiteState::Up);
    }
    #[test] fn trois_mesures_lentes_donnent_slow() {
        assert_eq!(state(&[SLOW, SLOW], 1500), SiteState::Up);
        assert_eq!(state(&[OK, SLOW, SLOW, SLOW], 1500), SiteState::Slow);
        assert_eq!(state(&[SLOW, SLOW, OK], 1500), SiteState::Up);
        assert_eq!(state(&[SLOW, SLOW, SLOW], 2500), SiteState::Up, "seuil du site");
        assert_eq!(state(&[SLOW, SLOW, KO], 1500), SiteState::Up);
    }
    #[test] fn hors_ligne_seulement_avec_au_moins_deux_sites() {
        assert!(all_failed(&[KO, KO]));
        assert!(!all_failed(&[KO, OK]));
        assert!(!all_failed(&[KO]), "un seul site : traité normalement");
        assert!(!all_failed(&[]));
    }
    #[test] fn transitions_notifient_et_se_taisent_hors_ligne() {
        use SiteState::*;
        let n = |p: &[SiteState], x: &[SiteState]| transitions(p, x, false).iter().map(|n| (n.site, n.kind)).collect::<Vec<_>>();
        assert_eq!(n(&[Up, Up, Up], &[Down, Slow, Up]), vec![(0, NoticeKind::Down), (1, NoticeKind::Slow)]);
        assert_eq!(n(&[Down], &[Up]), vec![(0, NoticeKind::Recovered)]);
        assert_eq!(n(&[Down], &[Down]), vec![], "pas de répétition");
        assert_eq!(n(&[Slow], &[Up]), vec![]);
        assert_eq!(n(&[Unknown], &[Up]), vec![]);
        assert_eq!(n(&[Slow], &[Down]), vec![(0, NoticeKind::Down)]);
        assert!(transitions(&[Up, Up], &[Down, Down], true).is_empty());
    }

    #[tokio::test] async fn probe_200_503_redirection_et_delai() {
        let srv = MockServer::start().await;
        Mock::given(method("GET")).and(path("/ok")).and(header_exists("user-agent"))
            .respond_with(ResponseTemplate::new(200).set_body_string("x".repeat(200_000))).mount(&srv).await;
        Mock::given(path("/ko")).respond_with(ResponseTemplate::new(503)).mount(&srv).await;
        Mock::given(path("/old")).respond_with(ResponseTemplate::new(301).insert_header("location", "/ok")).mount(&srv).await;
        Mock::given(path("/ftp")).respond_with(ResponseTemplate::new(302).insert_header("location", "ftp://127.0.0.1/x")).mount(&srv).await;
        Mock::given(path("/loop")).respond_with(ResponseTemplate::new(302).insert_header("location", "/loop")).mount(&srv).await;
        Mock::given(path("/slow")).respond_with(ResponseTemplate::new(200).set_delay(Duration::from_millis(600))).mount(&srv).await;
        let http = client().unwrap();
        let get = |p: &str| { let u = format!("{}{p}", srv.uri()); let h = http.clone(); async move { probe(&h, &u, Duration::from_millis(300)).await } };
        let p = get("/ok").await;
        assert!(p.ok && p.status == Some(200) && p.ms.is_some());
        let p = get("/ko").await;
        assert!(!p.ok && p.status == Some(503) && p.ms.is_some());
        let p = get("/old").await;
        assert!(p.ok && p.status == Some(200), "redirection suivie");
        assert_eq!(get("/ftp").await, KO, "schéma non http(s) refusé");
        assert_eq!(get("/loop").await, KO, "plus de 3 redirections");
        assert_eq!(get("/slow").await, KO, "délai dépassé");
        let ua = srv.received_requests().await.unwrap().iter().find(|r| r.url.path() == "/ok").unwrap().headers.get("user-agent").unwrap().to_str().unwrap().to_string();
        assert!(ua.starts_with("Oniris-sonde/"), "{ua}");
    }
    #[tokio::test] async fn probe_sans_serveur_echoue() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        drop(l);
        assert_eq!(probe(&client().unwrap(), &url, Duration::from_secs(2)).await, KO);
    }

    #[test] fn purge_des_mesures_de_plus_de_24h() {
        let c = crate::test_util::db();
        let now = 10 * KEEP_MS;
        record(&c, "u", now - KEEP_MS - 1, &OK).unwrap();
        record(&c, "u", now - KEEP_MS + 1, &OK).unwrap();
        record(&c, "u", now, &KO).unwrap();
        assert_eq!(prune(&c, now).unwrap(), 1);
        assert_eq!(history(&c, "u", 10).unwrap(), vec![OK, KO]);
    }
    #[test] fn vue_courbe_et_hors_ligne() {
        let c = crate::test_util::db();
        let site = Site { name: "A".into(), url: "u".into(), slow_ms: 1500 };
        for i in 0..40 { record(&c, "u", i, &Probe { ms: Some(i as u32), status: Some(200), ok: true }).unwrap(); }
        record(&c, "u", 40, &KO).unwrap();
        let v = views(&c, std::slice::from_ref(&site), false).unwrap();
        assert_eq!(v[0].spark.len(), 29, "30 dernières sondes, l'échec n'a pas de mesure");
        assert_eq!((v[0].state, v[0].last_ms), (SiteState::Up, None));
        assert_eq!(views(&c, &[site], true).unwrap()[0].state, SiteState::Offline);
    }
}
