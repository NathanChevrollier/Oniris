//! Client de la Web API Spotify (mode développement, février 2026) : lecteur, playlists de l'utilisateur, recherche.
//! Jamais `/playlists/{id}/tracks`, `GET /tracks?ids=`, `/browse/*` ni `/artists/{id}/top-tracks`.
//! Journal : ni noms, ni requêtes de recherche, seulement des statuts HTTP.
use crate::time_util::now_ms;
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::{json, Value};
use std::future::Future;
use std::sync::Arc;

pub const BASE: &str = "https://api.spotify.com/v1";
/// `limit` maximal de la recherche en mode développement.
pub const SEARCH_MAX: u32 = 10;
/// Pages de playlists suivies, au plus (50 par page).
const PAGES_MAX: u32 = 4;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SpotifyError {
    /// 404 `NO_ACTIVE_DEVICE`.
    #[error("Ouvre Spotify sur ce PC")]
    NoActiveDevice,
    /// 403 `PREMIUM_REQUIRED`.
    #[error("Spotify Premium est nécessaire pour piloter la lecture.")]
    NotPremium,
    #[error("Spotify demande de patienter {0} s.")]
    RateLimited(u64),
    /// 401 : jeton d'accès refusé (à oublier, voir `tools`).
    #[error("Spotify a refusé le jeton.")]
    Unauthorized,
    /// Compte à reconnecter.
    #[error("Reconnecte Spotify dans Comptes")]
    Revoked,
    #[error("Spotify est injoignable.")]
    Network,
    #[error("Spotify a refusé la demande (HTTP {0}).")]
    Other(u16),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Playlist { pub id: String, pub name: String, pub image: Option<String>, pub owner_is_me: bool }

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Device { pub id: String, pub name: String, pub kind: String, pub active: bool, pub volume: Option<u8> }

/// Résultat de recherche. Les noms sont du contenu utilisateur.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct SearchResult { pub tracks: Vec<Hit>, pub artists: Vec<Hit>, pub playlists: Vec<Hit> }

#[derive(Debug, Clone, PartialEq)]
pub struct Hit { pub uri: String, pub name: String }

pub enum PlayTarget { Context(String), Tracks(Vec<String>) }

#[derive(Clone)]
pub struct Client {
    pub http: reqwest::Client,
    /// `https://api.spotify.com/v1` ; les tests le pointent sur un faux serveur.
    pub base: String,
    /// Aucun appel avant cet instant (ms Unix) après un 429.
    hold_until: Arc<Mutex<i64>>,
}

fn s(v: &Value, k: &str) -> Option<String> { v.get(k)?.as_str().map(str::to_string) }

impl Client {
    pub fn new(http: reqwest::Client) -> Self { Self::with_base(http, BASE) }
    pub fn with_base(http: reqwest::Client, base: &str) -> Self { Self { http, base: base.into(), hold_until: Default::default() } }

    async fn call(&self, token: &str, method: reqwest::Method, path: &str, query: &[(&str, String)], body: Option<Value>) -> Result<Value, SpotifyError> {
        let wait = *self.hold_until.lock() - now_ms();
        if wait > 0 {
            return Err(SpotifyError::RateLimited((wait as u64).div_ceil(1000)));
        }
        let mut req = self.http.request(method.clone(), format!("{}{path}", self.base)).bearer_auth(token).query(query);
        if let Some(b) = body {
            req = req.json(&b);
        } else if method != reqwest::Method::GET {
            req = req.header("Content-Length", "0");
        }
        let resp = req.send().await.map_err(|_| SpotifyError::Network)?;
        let status = resp.status().as_u16();
        if status == 429 {
            let secs = resp.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| v.trim().parse::<u64>().ok()).unwrap_or(5).clamp(1, 3600);
            *self.hold_until.lock() = now_ms() + secs as i64 * 1000;
            tracing::warn!("spotify : limite de débit, pause de {secs} s");
            return Err(SpotifyError::RateLimited(secs));
        }
        let text = resp.text().await.map_err(|_| SpotifyError::Network)?;
        let json: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        if (200..300).contains(&status) {
            return Ok(json);
        }
        let reason = json.pointer("/error/reason").and_then(Value::as_str).unwrap_or("");
        tracing::info!("spotify : HTTP {status}");
        Err(match (status, reason) {
            (401, _) => SpotifyError::Unauthorized,
            (404, "NO_ACTIVE_DEVICE") => SpotifyError::NoActiveDevice,
            (403, "PREMIUM_REQUIRED") => SpotifyError::NotPremium,
            _ => SpotifyError::Other(status),
        })
    }

    async fn get(&self, token: &str, path: &str, query: &[(&str, String)]) -> Result<Value, SpotifyError> {
        self.call(token, reqwest::Method::GET, path, query, None).await
    }

    /// Playlists de l'utilisateur (`owner_is_me` : propriétaire = `/me`), 4 pages au plus.
    pub async fn my_playlists(&self, token: &str, limit: u32) -> Result<Vec<Playlist>, SpotifyError> {
        let limit = limit.clamp(1, 50);
        let me = s(&self.get(token, "/me", &[]).await?, "id").unwrap_or_default();
        let mut out = vec![];
        for page in 0..PAGES_MAX {
            let v = self.get(token, "/me/playlists", &[("limit", limit.to_string()), ("offset", (page * limit).to_string())]).await?;
            for p in v["items"].as_array().into_iter().flatten() {
                let (Some(id), Some(name)) = (s(p, "id"), s(p, "name")) else { continue };
                out.push(Playlist {
                    id, name,
                    image: p["images"].as_array().and_then(|a| a.first()).and_then(|i| s(i, "url")),
                    owner_is_me: p.pointer("/owner/id").and_then(Value::as_str) == Some(me.as_str()),
                });
            }
            if v["next"].is_null() {
                break;
            }
        }
        Ok(out)
    }

    pub async fn devices(&self, token: &str) -> Result<Vec<Device>, SpotifyError> {
        let v = self.get(token, "/me/player/devices", &[]).await?;
        Ok(v["devices"].as_array().into_iter().flatten().filter_map(|d| {
            Some(Device {
                id: s(d, "id")?, name: s(d, "name").unwrap_or_default(), kind: s(d, "type").unwrap_or_default(),
                active: d["is_active"].as_bool().unwrap_or(false),
                volume: d["volume_percent"].as_u64().map(|n| n.min(100) as u8),
            })
        }).collect())
    }

    pub async fn play(&self, token: &str, device: Option<&str>, target: PlayTarget) -> Result<(), SpotifyError> {
        let body = match target { PlayTarget::Context(u) => json!({"context_uri": u}), PlayTarget::Tracks(u) => json!({"uris": u}) };
        let q: Vec<_> = device.map(|d| ("device_id", d.to_string())).into_iter().collect();
        self.call(token, reqwest::Method::PUT, "/me/player/play", &q, Some(body)).await.map(drop)
    }

    pub async fn transfer(&self, token: &str, device_id: &str, play: bool) -> Result<(), SpotifyError> {
        self.call(token, reqwest::Method::PUT, "/me/player", &[], Some(json!({"device_ids": [device_id], "play": play}))).await.map(drop)
    }

    pub async fn search(&self, token: &str, q: &str, types: &str, limit: u32) -> Result<SearchResult, SpotifyError> {
        let v = self.get(token, "/search", &[("q", q.into()), ("type", types.into()), ("limit", limit.clamp(1, SEARCH_MAX).to_string())]).await?;
        // Les listes de playlists contiennent parfois des `null`.
        let hits = |k: &str| -> Vec<Hit> {
            v[k]["items"].as_array().into_iter().flatten().filter_map(|i| Some(Hit { uri: s(i, "uri")?, name: s(i, "name")? })).collect()
        };
        Ok(SearchResult { tracks: hits("tracks"), artists: hits("artists"), playlists: hits("playlists") })
    }

    pub async fn volume(&self, token: &str, percent: u8) -> Result<(), SpotifyError> {
        self.call(token, reqwest::Method::PUT, "/me/player/volume", &[("volume_percent", percent.min(100).to_string())], None).await.map(drop)
    }

    pub async fn queue(&self, token: &str, uri: &str) -> Result<(), SpotifyError> {
        self.call(token, reqwest::Method::POST, "/me/player/queue", &[("uri", uri.into())], None).await.map(drop)
    }

    pub async fn seek(&self, token: &str, ms: u32) -> Result<(), SpotifyError> {
        self.call(token, reqwest::Method::PUT, "/me/player/seek", &[("position_ms", ms.to_string())], None).await.map(drop)
    }
}

/// Exécute `op` ; sur `NoActiveDevice`, transfère vers le premier appareil « Computer » puis réessaie, une seule fois.
/// Sans appareil « Computer », l'erreur reste `NoActiveDevice` (« Ouvre Spotify sur ce PC »).
pub async fn with_device<T, F, Fut>(client: &Client, token: &str, op: F) -> Result<T, SpotifyError>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, SpotifyError>>,
{
    match op().await {
        Err(SpotifyError::NoActiveDevice) => {
            let pc = client.devices(token).await?.into_iter().find(|d| d.kind.eq_ignore_ascii_case("computer"));
            let Some(pc) = pc else { return Err(SpotifyError::NoActiveDevice) };
            client.transfer(token, &pc.id, false).await?;
            op().await
        }
        r => r,
    }
}

/// Playlist demandée par son nom : insensible à la casse et aux accents, exact d'abord, puis préfixe, puis contenu.
pub fn match_playlist<'a>(list: &'a [Playlist], name: &str) -> Option<&'a Playlist> {
    let want = crate::when::norm(name);
    if want.is_empty() {
        return None;
    }
    let normed: Vec<String> = list.iter().map(|p| crate::when::norm(&p.name)).collect();
    let find = |f: &dyn Fn(&String) -> bool| normed.iter().position(f);
    find(&|n| *n == want).or_else(|| find(&|n| n.starts_with(&want))).or_else(|| find(&|n| n.contains(&want))).map(|i| &list[i])
}

/// Volume visé : `delta` part du volume courant, borné à 0..=100.
pub fn target_volume(current: u8, delta: i32) -> u8 { (current as i32 + delta).clamp(0, 100) as u8 }

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn fx(name: &str) -> Value {
        serde_json::from_str(&std::fs::read_to_string(format!("{}/tests/fixtures/spotify/{name}.json", env!("CARGO_MANIFEST_DIR"))).unwrap()).unwrap()
    }
    fn ok(name: &str) -> ResponseTemplate { ResponseTemplate::new(200).set_body_json(fx(name)) }
    fn client(server: &MockServer) -> Client { Client::with_base(reqwest::Client::new(), &server.uri()) }

    #[tokio::test]
    async fn playlists_pagination_et_proprietaire() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/me")).respond_with(ok("me")).mount(&server).await;
        Mock::given(method("GET")).and(path("/me/playlists")).and(query_param("offset", "0")).respond_with(ok("playlists_p1")).mount(&server).await;
        Mock::given(method("GET")).and(path("/me/playlists")).and(query_param("offset", "2")).respond_with(ok("playlists_p2")).mount(&server).await;
        let l = client(&server).my_playlists("t", 2).await.unwrap();
        assert_eq!(l.iter().map(|p| (p.id.as_str(), p.owner_is_me, p.image.is_some())).collect::<Vec<_>>(), [("pl1xxxxxxxxxxxxxxxxxxx", true, true), ("pl2", false, false), ("pl3", true, false)]);
    }

    #[tokio::test]
    async fn appareils() {
        let server = MockServer::start().await;
        Mock::given(path("/me/player/devices")).respond_with(ok("devices")).mount(&server).await;
        let d = client(&server).devices("t").await.unwrap();
        assert_eq!(d[1], Device { id: "d-pc".into(), name: "PC-Nathan".into(), kind: "Computer".into(), active: true, volume: Some(35) });
    }

    #[tokio::test]
    async fn recherche_limite_a_dix_et_null_tolere() {
        let server = MockServer::start().await;
        Mock::given(path("/search")).and(query_param("limit", "10")).respond_with(ok("search")).expect(1).mount(&server).await;
        let r = client(&server).search("t", "q", "track,artist,playlist", 50).await.unwrap();
        assert_eq!((r.tracks.len(), r.artists.len(), r.playlists.len()), (1, 1, 1));
        server.verify().await;
    }

    #[tokio::test]
    async fn aucun_appareil_actif_transfere_vers_le_pc_une_seule_fois() {
        let server = MockServer::start().await;
        let nope = ResponseTemplate::new(404).set_body_json(fx("err_no_device"));
        Mock::given(method("PUT")).and(path("/me/player/play")).respond_with(nope.clone()).up_to_n_times(1).expect(1).mount(&server).await;
        Mock::given(method("GET")).and(path("/me/player/devices")).respond_with(ok("devices")).expect(1).mount(&server).await;
        Mock::given(method("PUT")).and(path("/me/player")).respond_with(ResponseTemplate::new(204)).expect(1).mount(&server).await;
        Mock::given(method("PUT")).and(path("/me/player/play")).respond_with(ResponseTemplate::new(204)).expect(1).mount(&server).await;
        let c = client(&server);
        with_device(&c, "t", || c.play("t", None, PlayTarget::Context("spotify:playlist:pl1xxxxxxxxxxxxxxxxxxx".into()))).await.unwrap();
        server.verify().await;
    }

    #[tokio::test]
    async fn sans_appareil_ouvre_spotify() {
        let server = MockServer::start().await;
        Mock::given(path("/me/player/play")).respond_with(ResponseTemplate::new(404).set_body_json(fx("err_no_device"))).expect(1).mount(&server).await;
        Mock::given(path("/me/player/devices")).respond_with(ok("devices_none")).mount(&server).await;
        let c = client(&server);
        let e = with_device(&c, "t", || c.play("t", None, PlayTarget::Tracks(vec![]))).await.unwrap_err();
        assert_eq!(e, SpotifyError::NoActiveDevice);
        assert!(e.to_string().contains("Ouvre Spotify"));
    }

    #[tokio::test]
    async fn premium_requis() {
        let server = MockServer::start().await;
        Mock::given(path("/me/player/volume")).respond_with(ResponseTemplate::new(403).set_body_json(fx("err_premium"))).mount(&server).await;
        let e = client(&server).volume("t", 10).await.unwrap_err();
        assert_eq!(e, SpotifyError::NotPremium);
        assert!(e.to_string().contains("Premium"));
    }

    #[tokio::test]
    async fn limite_de_debit_aucun_appel_avant_le_delai() {
        let server = MockServer::start().await;
        Mock::given(path("/me/player/devices")).respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "7")).expect(1).mount(&server).await;
        let c = client(&server);
        assert_eq!(c.devices("t").await.unwrap_err(), SpotifyError::RateLimited(7));
        assert!(matches!(c.devices("t").await.unwrap_err(), SpotifyError::RateLimited(n) if (1..=7).contains(&n)));
        server.verify().await; // une seule requête est partie
    }

    #[tokio::test]
    async fn chemins_interdits_jamais_appeles() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/me")).respond_with(ok("me")).mount(&server).await;
        Mock::given(path("/me/playlists")).respond_with(ok("playlists_p2")).mount(&server).await;
        Mock::given(path("/search")).respond_with(ok("search")).mount(&server).await;
        let c = client(&server);
        c.my_playlists("t", 50).await.unwrap();
        c.search("t", "x", "track", 1).await.unwrap();
        let paths: Vec<String> = server.received_requests().await.unwrap().iter().map(|r| format!("{}?{}", r.url.path(), r.url.query().unwrap_or(""))).collect();
        assert!(!paths.iter().any(|p| p.starts_with("/tracks") || p.contains("/playlists/") || p.starts_with("/browse") || p.contains("top-tracks")), "{paths:?}");
    }

    #[test]
    fn nom_de_playlist_sans_casse_ni_accents_prefixe_d_abord() {
        let p = |id: &str, name: &str| Playlist { id: id.into(), name: name.into(), image: None, owner_is_me: true };
        let l = [p("1", "Road Focus"), p("2", "Focus Deep Work"), p("3", "Été 2025")];
        assert_eq!(match_playlist(&l, "focus").unwrap().id, "2", "préfixe avant contenu");
        assert_eq!(match_playlist(&l, "ete").unwrap().id, "3");
        assert_eq!(match_playlist(&l, "road").unwrap().id, "1");
        let l2 = [p("1", "Focus Flow"), p("2", "Focus"), p("3", "Música")];
        assert_eq!(match_playlist(&l2, "Focus").unwrap().id, "2", "exact avant préfixe");
        assert_eq!(match_playlist(&l2, " musica ").unwrap().id, "3");
        assert!(match_playlist(&l, "jazz").is_none() && match_playlist(&l, "  ").is_none());
    }

    #[test]
    fn volume_delta_borne() {
        assert_eq!(target_volume(35, -10), 25);
        assert_eq!(target_volume(5, -10), 0);
        assert_eq!(target_volume(95, 20), 100);
    }
}
