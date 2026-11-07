//! Outils musique, tous libres. `music.toggle|next|previous|seek` passent par `MediaControl` (SMTC) ;
//! `music.play_playlist|play_search|volume|queue|devices|transfer` par la Web API Spotify (compte connecté requis).
//! Journal : jamais de nom de playlist, de titre ni de recherche.
use super::spotify::{self, with_device, Client, Device, PlayTarget, Playlist, SpotifyError};
use super::MediaControl;
use crate::accounts::{self, Status, TokenCache};
use crate::events::{Events, Topic};
use crate::oauth::{OAuthError, Provider};
use crate::tools::helpers::{bad, failed, low, parse, spec, untrusted};
use crate::tools::{ConfirmationText, Permission, Tool, ToolError, ToolResult, ToolSpec};
use crate::{kv, time_util::now_ms, Db};
use async_trait::async_trait;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::future::Future;
use std::sync::Arc;

pub const NO_ACCOUNT: &str = "Connecte Spotify dans Comptes";
const FAVORITES_KEY: &str = "music_favorites";
const FAVORITES_MAX: usize = 8;
const FAVORITES_DEFAULT: usize = 6;
/// Durée de vie des playlists en cache.
const CACHE_MS: i64 = 10 * 60_000;

/// Contexte des outils Web API : compte Spotify, client, cache des playlists (mémoire seulement).
pub struct Web {
    db: Db,
    events: Arc<dyn Events>,
    tokens: Arc<TokenCache>,
    client: Client,
    /// `(dernière tentative, dernières playlists connues)`.
    cache: Mutex<(i64, Vec<Playlist>)>,
}

/// Ce que `music_playlists` rend à l'interface.
#[derive(Serialize)]
pub struct PlaylistsView { pub connected: bool, pub favorites: Vec<Playlist>, pub all: Vec<Playlist> }

fn token_err(e: OAuthError) -> ToolError {
    match e {
        OAuthError::Revoked => failed("Reconnecte Spotify dans Comptes"),
        OAuthError::Network => failed("Réseau indisponible."),
        OAuthError::Other(m) => failed(m),
    }
}

impl Web {
    pub fn new(db: Db, events: Arc<dyn Events>, tokens: Arc<TokenCache>, client: Client) -> Arc<Self> {
        Arc::new(Self { db, events, tokens, client, cache: Mutex::new((0, vec![])) })
    }

    /// Le compte Spotify (un seul, statut `ok`).
    pub fn account(&self) -> Result<i64, ToolError> {
        let list = accounts::list(&self.db.lock()).map_err(failed)?;
        list.iter().find(|a| a.provider == Provider::Spotify && a.status == Status::Ok).map(|a| a.id).ok_or_else(|| failed(NO_ACCOUNT))
    }

    /// Lance `f` avec un jeton d'accès. 401 : jeton oublié puis un seul nouvel essai ; un second 401 passe le compte en `reauth`.
    async fn run<T, Fut>(&self, f: impl Fn(String) -> Fut) -> Result<T, ToolError>
    where
        Fut: Future<Output = Result<T, SpotifyError>>,
    {
        let id = self.account()?;
        for _ in 0..2 {
            let token = self.tokens.access_token(id).await.map_err(token_err)?;
            match f(token).await {
                Err(SpotifyError::Unauthorized) => self.tokens.forget(id),
                r => return r.map_err(failed),
            }
        }
        let r = accounts::set_status(&self.db.lock(), id, Status::Reauth);
        if let Err(e) = r {
            tracing::warn!("compte {id} : statut non mis à jour : {e}");
        }
        self.events.changed(Topic::Accounts);
        Err(failed("Reconnecte Spotify dans Comptes"))
    }

    /// Playlists depuis Spotify (requête réelle), mises en cache.
    pub async fn fetch_playlists(&self) -> Result<Vec<Playlist>, ToolError> {
        self.cache.lock().0 = now_ms(); // une tentative ratée compte aussi : pas de rafale
        let c = &self.client;
        let list = self.run(|t| async move { c.my_playlists(&t, 50).await }).await?;
        self.cache.lock().1.clone_from(&list);
        Ok(list)
    }

    /// Vrai si le cache n'a pas été rempli depuis 10 minutes (ou jamais).
    pub fn stale(&self) -> bool { now_ms() - self.cache.lock().0 > CACHE_MS }

    /// À l'ouverture du panneau (déjà dans une tâche) : une relève ; sans compte, aucune requête.
    pub async fn refresh_if_stale(&self) {
        if !self.stale() || self.account().is_err() {
            return;
        }
        if let Err(e) = self.fetch_playlists().await {
            tracing::info!("musique : playlists non relevées : {e:?}");
        }
        self.events.changed(Topic::Music);
    }

    /// Favoris : `music_favorites` (8 au plus), sinon les 6 premières playlists. Lit le cache, jamais le réseau.
    pub fn favorites(&self) -> Vec<Playlist> {
        let all = self.cache.lock().1.clone();
        let stored = kv::get(&self.db.lock(), FAVORITES_KEY).ok().flatten().and_then(|(v, _)| serde_json::from_str::<Vec<String>>(&v).ok()).unwrap_or_default();
        if stored.is_empty() {
            return all.into_iter().take(FAVORITES_DEFAULT).collect();
        }
        stored.iter().filter_map(|id| all.iter().find(|p| &p.id == id).cloned()).take(FAVORITES_MAX).collect()
    }

    pub fn set_favorites(&self, ids: &[String]) -> Result<(), String> {
        let mut seen: Vec<&String> = vec![];
        for id in ids {
            if !seen.contains(&id) {
                seen.push(id);
            }
        }
        if seen.len() > FAVORITES_MAX {
            return Err(format!("{FAVORITES_MAX} favoris au plus."));
        }
        let r = kv::set(&self.db.lock(), FAVORITES_KEY, &serde_json::to_string(&seen).map_err(|e| e.to_string())?, now_ms());
        r.map_err(|e| e.to_string())?;
        self.events.changed(Topic::Music);
        Ok(())
    }

    pub fn view(&self) -> PlaylistsView {
        PlaylistsView { connected: self.account().is_ok(), favorites: self.favorites(), all: self.cache.lock().1.clone() }
    }

    /// Premier résultat d'une recherche d'un type.
    async fn first(&self, query: &str, kind: &str) -> Result<spotify::Hit, ToolError> {
        let c = &self.client;
        let r = self.run(|t| async move { c.search(&t, query, kind, 1).await }).await?;
        let hits = match kind { "artist" => r.artists, "playlist" => r.playlists, _ => r.tracks };
        hits.into_iter().next().ok_or_else(|| failed("Aucun résultat sur Spotify."))
    }

    async fn play(&self, target: impl Fn() -> PlayTarget) -> Result<(), ToolError> {
        match target() {
            PlayTarget::Context(u) => check_uri(&u)?,
            PlayTarget::Tracks(us) => us.iter().try_for_each(|u| check_uri(u))?,
        }
        let (c, target) = (&self.client, &target);
        self.run(|t| async move { with_device(c, &t, || c.play(&t, None, target())).await }).await
    }

    pub async fn seek(&self, ms: u32) -> Result<(), ToolError> {
        let c = &self.client;
        self.run(|t| async move { with_device(c, &t, || c.seek(&t, ms)).await }).await
    }
}

#[derive(Clone, Copy)]
enum WebAction { PlayPlaylist, PlaySearch, Volume, Queue, Devices, Transfer }

pub struct WebTool { action: WebAction, web: Arc<Web> }

#[derive(Deserialize)]
struct PlaylistArgs { id: Option<String>, name: Option<String> }
#[derive(Deserialize)]
struct SearchArgs { query: String, kind: Option<String> }
#[derive(Deserialize)]
struct VolumeArgs { percent: Option<i32>, delta: Option<i32> }
#[derive(Deserialize)]
struct QueueArgs { query: String }
#[derive(Deserialize)]
struct DeviceArgs { device_id: String }

/// URI acceptée : `spotify:(track|album|playlist|artist):` suivi de 22 caractères alphanumériques.
fn check_uri(uri: &str) -> Result<(), ToolError> {
    let ok = uri.strip_prefix("spotify:").and_then(|r| r.split_once(':')).is_some_and(|(kind, id)| {
        ["track", "album", "playlist", "artist"].contains(&kind) && id.len() == 22 && id.bytes().all(|b| b.is_ascii_alphanumeric())
    });
    if ok { Ok(()) } else { Err(bad("URI Spotify invalide")) }
}

/// Un identifiant Spotify ne contient que des caractères alphanumériques.
fn valid_id(id: &str) -> bool { !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric()) }

impl WebTool {
    fn texts(&self) -> (&'static str, &'static str, &'static str, Value) {
        match self.action {
            WebAction::PlayPlaylist => ("music.play_playlist", "Lance une de mes playlists Spotify, par identifiant ou par nom.", "Lancer une playlist",
                json!({"type":"object","properties":{"id":{"type":"string"},"name":{"type":"string","maxLength":200}}})),
            WebAction::PlaySearch => ("music.play_search", "Cherche sur Spotify et lance le premier résultat (titre, artiste ou playlist).", "Lancer une recherche",
                json!({"type":"object","properties":{"query":{"type":"string","maxLength":200},"kind":{"type":"string","enum":["track","artist","playlist"]}},"required":["query"]})),
            WebAction::Volume => ("music.volume", "Règle le volume de Spotify : `percent` (0 à 100) ou `delta` (relatif).", "Régler le volume",
                json!({"type":"object","properties":{"percent":{"type":"integer","minimum":0,"maximum":100},"delta":{"type":"integer","minimum":-100,"maximum":100}}})),
            WebAction::Queue => ("music.queue", "Ajoute à la file d'attente Spotify le premier titre trouvé.", "Ajouter à la file",
                json!({"type":"object","properties":{"query":{"type":"string","maxLength":200}},"required":["query"]})),
            WebAction::Devices => ("music.devices", "Liste les appareils Spotify.", "Lister les appareils", json!({"type":"object","properties":{}})),
            WebAction::Transfer => ("music.transfer", "Déplace la lecture Spotify vers un autre appareil.", "Changer d'appareil",
                json!({"type":"object","properties":{"device_id":{"type":"string"}},"required":["device_id"]})),
        }
    }

    async fn volume(&self, a: VolumeArgs) -> Result<Value, ToolError> {
        let c = &self.web.client;
        let percent = match (a.percent, a.delta) {
            (Some(p), None) if (0..=100).contains(&p) => p as u8,
            (None, Some(d)) => {
                let devices = self.web.run(|t| async move { c.devices(&t).await }).await?;
                let cur = devices.iter().find(|d| d.active).and_then(|d| d.volume).ok_or_else(|| failed("Aucun appareil Spotify actif."))?;
                spotify::target_volume(cur, d)
            }
            _ => return Err(bad("donne `percent` (0 à 100) ou `delta`, pas les deux")),
        };
        self.web.run(|t| async move { with_device(c, &t, || c.volume(&t, percent)).await }).await?;
        Ok(json!({ "percent": percent }))
    }
}

#[async_trait]
impl Tool for WebTool {
    fn spec(&self) -> ToolSpec {
        let (name, desc, _, schema) = self.texts();
        // Les sorties portent des noms de playlists, titres ou appareils : pas dans le journal.
        let s = ToolSpec { sensitive_output: !matches!(self.action, WebAction::Volume | WebAction::Transfer), ..spec(name, desc, schema, Permission::Free) };
        if s.sensitive_output { untrusted(s) } else { s }
    }
    fn describe(&self, _: &Value) -> ConfirmationText { low(self.texts().2) }
    // Une recherche est un contenu de l'utilisateur : le journal n'en garde que la taille.
    fn journal_args(&self, a: &Value) -> Value {
        match self.action {
            WebAction::PlaySearch | WebAction::Queue => json!({ "query_chars": a["query"].as_str().map_or(0, |q| q.chars().count()) }),
            _ => a.clone(),
        }
    }
    async fn run(&self, args: &Value) -> Result<ToolResult, ToolError> {
        let w = &self.web;
        let c = &w.client;
        let output = match self.action {
            WebAction::PlayPlaylist => {
                let a: PlaylistArgs = parse(args)?;
                let (id, name) = match (a.id, a.name) {
                    (Some(id), _) if valid_id(&id) => (id, None),
                    (Some(_), _) => return Err(bad("identifiant de playlist invalide")),
                    (None, Some(name)) => {
                        let list = w.fetch_playlists().await?;
                        let p = spotify::match_playlist(&list, &name).ok_or_else(|| failed("Aucune de tes playlists ne porte ce nom."))?;
                        (p.id.clone(), Some(p.name.clone()))
                    }
                    (None, None) => return Err(bad("donne `id` ou `name`")),
                };
                let uri = format!("spotify:playlist:{id}");
                w.play(|| PlayTarget::Context(uri.clone())).await?;
                json!({ "id": id, "name": name })
            }
            WebAction::PlaySearch => {
                let a: SearchArgs = parse(args)?;
                let kind = a.kind.unwrap_or_else(|| "track".into());
                if !["track", "artist", "playlist"].contains(&kind.as_str()) {
                    return Err(bad("kind : track, artist ou playlist"));
                }
                let hit = w.first(&a.query, &kind).await?;
                // Un titre se joue seul ; un artiste ou une playlist par son contexte.
                w.play(|| if kind == "track" { PlayTarget::Tracks(vec![hit.uri.clone()]) } else { PlayTarget::Context(hit.uri.clone()) }).await?;
                json!({ "name": hit.name })
            }
            WebAction::Volume => self.volume(parse(args)?).await?,
            WebAction::Queue => {
                let hit = w.first(&parse::<QueueArgs>(args)?.query, "track").await?;
                let uri = &hit.uri;
                check_uri(uri)?;
                w.run(|t| async move { with_device(c, &t, || c.queue(&t, uri)).await }).await?;
                json!({ "name": hit.name })
            }
            WebAction::Devices => {
                let d: Vec<Device> = w.run(|t| async move { c.devices(&t).await }).await?;
                json!({ "devices": d })
            }
            WebAction::Transfer => {
                let id = parse::<DeviceArgs>(args)?.device_id;
                w.run(|t| { let id = &id; async move { c.transfer(&t, id, false).await } }).await?;
                json!({})
            }
        };
        Ok(ToolResult { output, undo: None })
    }
}

#[derive(Clone, Copy)]
enum Action { Toggle, Next, Previous, Seek }

pub struct Control { action: Action, media: Arc<dyn MediaControl>, web: Option<Arc<Web>> }

#[derive(Deserialize)]
struct SeekArgs { position_ms: u32 }

impl Control {
    fn texts(&self) -> (&'static str, &'static str, &'static str) {
        match self.action {
            Action::Toggle => ("music.toggle", "Lecture ou pause du titre en cours.", "Lecture / pause"),
            Action::Next => ("music.next", "Passe au titre suivant.", "Titre suivant"),
            Action::Previous => ("music.previous", "Revient au titre précédent.", "Titre précédent"),
            Action::Seek => ("music.seek", "Va à une position du titre en cours (en millisecondes).", "Changer la position"),
        }
    }
}

#[async_trait]
impl Tool for Control {
    fn spec(&self) -> ToolSpec {
        let (name, description, _) = self.texts();
        let args = match self.action {
            Action::Seek => json!({"type":"object","properties":{"position_ms":{"type":"integer","minimum":0}},"required":["position_ms"]}),
            _ => json!({"type":"object","properties":{}}),
        };
        spec(name, description, args, Permission::Free)
    }
    fn describe(&self, _: &Value) -> ConfirmationText { low(self.texts().2) }
    async fn run(&self, args: &Value) -> Result<ToolResult, ToolError> {
        let seek = match self.action {
            Action::Seek => Some(parse::<SeekArgs>(args)?.position_ms as i64),
            _ => None,
        };
        let (action, media) = (self.action, self.media.clone());
        // Appels WinRT bloquants : hors du worker async.
        let r = tokio::task::spawn_blocking(move || match action {
            Action::Toggle => media.toggle(),
            Action::Next => media.next(),
            Action::Previous => media.previous(),
            Action::Seek => media.seek(seek.unwrap_or(0)),
        })
        .await
        .map_err(failed)?;
        match (r, seek, &self.web) {
            // P4-R2 : lecteur sans changement de position, repli sur la Web API si Spotify est connecté.
            (Err(_), Some(ms), Some(w)) if w.account().is_ok() => w.seek(ms as u32).await?,
            (r, _, _) => r.map_err(ToolError::Failed)?,
        }
        Ok(ToolResult { output: json!({}), undo: None })
    }
}

pub fn all(media: &Arc<dyn MediaControl>, web: &Arc<Web>) -> Vec<Arc<dyn Tool>> {
    let local = [Action::Toggle, Action::Next, Action::Previous, Action::Seek]
        .into_iter()
        .map(|action| Arc::new(Control { action, media: media.clone(), web: Some(web.clone()) }) as Arc<dyn Tool>);
    let remote = [WebAction::PlayPlaylist, WebAction::PlaySearch, WebAction::Volume, WebAction::Queue, WebAction::Devices, WebAction::Transfer]
        .into_iter()
        .map(|action| Arc::new(WebTool { action, web: web.clone() }) as Arc<dyn Tool>);
    local.chain(remote).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::NoEvents;
    use crate::oauth::{providers, ProviderConfig, Tokens};
    use crate::secrets::{account_key, MemorySecrets, SecretStore};
    use crate::test_util::db;
    use crate::tools::{Origin, Provenance, ToolBroker, ToolCall, ToolOutcome};
    use wiremock::matchers::{body_partial_json, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Faux lecteur : note les commandes reçues, refuse le changement de position si `unseekable`.
    #[derive(Default)]
    struct Fake { calls: Mutex<Vec<String>>, unseekable: bool }

    impl MediaControl for Fake {
        fn toggle(&self) -> Result<(), String> { self.calls.lock().push("toggle".into()); Ok(()) }
        fn next(&self) -> Result<(), String> { self.calls.lock().push("next".into()); Ok(()) }
        fn previous(&self) -> Result<(), String> { self.calls.lock().push("previous".into()); Ok(()) }
        fn seek(&self, ms: i64) -> Result<(), String> {
            if self.unseekable {
                return Err("Ce lecteur ne permet pas de changer la position.".into());
            }
            self.calls.lock().push(format!("seek {ms}"));
            Ok(())
        }
    }

    fn fx(name: &str) -> Value {
        serde_json::from_str(&std::fs::read_to_string(format!("{}/tests/fixtures/spotify/{name}.json", env!("CARGO_MANIFEST_DIR"))).unwrap()).unwrap()
    }
    fn ok(name: &str) -> ResponseTemplate { ResponseTemplate::new(200).set_body_json(fx(name)) }

    struct Env { broker: ToolBroker, web: Arc<Web>, db: Db, account: Option<i64>, fake: Arc<Fake> }

    /// `account` : un compte Spotify connecté, avec un jeton d'accès valide « tok » ; le jeton « tok2 » vient de `/token`.
    async fn env(server: &MockServer, account: bool, unseekable: bool) -> Env {
        let db: Db = Arc::new(Mutex::new(db()));
        let secrets = Arc::new(MemorySecrets::default());
        let url = format!("{}/token", server.uri());
        let configs: crate::accounts::Configs = Arc::new(move |_| Some(ProviderConfig { token_url: url.clone(), ..providers::spotify("cid") }));
        let events: Arc<dyn Events> = Arc::new(NoEvents);
        let tokens = Arc::new(TokenCache::new(db.clone(), secrets.clone(), events.clone(), reqwest::Client::new(), configs));
        let id = account.then(|| {
            let id = accounts::add(&db.lock(), Provider::Spotify, "moi", 1).unwrap();
            secrets.set(&account_key(id), "refresh").unwrap();
            tokens.insert(id, Tokens { access: "tok".into(), refresh: None, expires_at_ms: now_ms() + 3_600_000 }).unwrap();
            id
        });
        let web = Web::new(db.clone(), events, tokens, Client::with_base(reqwest::Client::new(), &server.uri()));
        let fake = Arc::new(Fake { unseekable, ..Default::default() });
        let media: Arc<dyn MediaControl> = fake.clone();
        let broker = ToolBroker::new(db.clone(), Arc::new(|| 0), all(&media, &web)).unwrap();
        Env { broker, web, db, account: id, fake }
    }

    impl Env {
        async fn call(&self, tool: &str, args: Value) -> ToolOutcome {
            self.broker.call(ToolCall { tool: tool.into(), args, provenance: Provenance::User, origin: Origin::User }).await
        }
    }

    fn failed_with(o: ToolOutcome) -> String {
        let ToolOutcome::Failed { error, .. } = o else { panic!("échec attendu : {o:?}") };
        error
    }

    async fn playlists_mocks(server: &MockServer) {
        Mock::given(path("/me")).respond_with(ok("me")).mount(server).await;
        Mock::given(path("/me/playlists")).and(query_param("offset", "0")).respond_with(ok("playlists_p1")).mount(server).await;
        Mock::given(path("/me/playlists")).and(query_param("offset", "50")).respond_with(ok("playlists_p2")).mount(server).await;
    }

    #[tokio::test]
    async fn commandes_libres_transmises_au_lecteur() {
        let server = MockServer::start().await;
        let e = env(&server, false, false).await;
        for (tool, args) in [("music.toggle", json!({})), ("music.next", json!({})), ("music.previous", json!({})), ("music.seek", json!({"position_ms": 61_000}))] {
            assert!(matches!(e.call(tool, args).await, ToolOutcome::Done { .. }), "{tool} : libre, exécuté sans confirmation");
        }
        assert_eq!(*e.fake.calls.lock(), ["toggle", "next", "previous", "seek 61000"]);
        assert!(matches!(e.call("music.seek", json!({"position_ms": -1})).await, ToolOutcome::Failed { .. }));
        assert_eq!(e.fake.calls.lock().len(), 4, "position négative refusée avant le lecteur");
        assert!(server.received_requests().await.unwrap().is_empty(), "SMTC seul : aucune requête Spotify");
    }

    #[tokio::test]
    async fn position_refusee_sans_compte_message_clair() {
        let server = MockServer::start().await;
        let e = env(&server, false, true).await;
        assert!(failed_with(e.call("music.seek", json!({"position_ms": 0})).await).contains("ne permet pas de changer la position"));
    }

    #[tokio::test]
    async fn position_refusee_repli_sur_la_web_api() {
        let server = MockServer::start().await;
        Mock::given(method("PUT")).and(path("/me/player/seek")).and(query_param("position_ms", "5000")).respond_with(ResponseTemplate::new(204)).expect(1).mount(&server).await;
        let e = env(&server, true, true).await;
        assert!(matches!(e.call("music.seek", json!({"position_ms": 5000})).await, ToolOutcome::Done { .. }));
        server.verify().await;
    }

    #[tokio::test]
    async fn sans_compte_message_convivial() {
        let server = MockServer::start().await;
        let e = env(&server, false, false).await;
        for (tool, args) in [("music.devices", json!({})), ("music.play_playlist", json!({"name": "x"})), ("music.volume", json!({"percent": 5}))] {
            assert!(failed_with(e.call(tool, args).await).contains(NO_ACCOUNT), "{tool}");
        }
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn play_playlist_par_nom_focus_trouve_focus_deep_work() {
        let server = MockServer::start().await;
        playlists_mocks(&server).await;
        Mock::given(method("PUT")).and(path("/me/player/play")).and(body_partial_json(json!({"context_uri": "spotify:playlist:pl1xxxxxxxxxxxxxxxxxxx"}))).respond_with(ResponseTemplate::new(204)).expect(1).mount(&server).await;
        let e = env(&server, true, false).await;
        let ToolOutcome::Done { output, .. } = e.call("music.play_playlist", json!({"name": "focus"})).await else { panic!() };
        assert_eq!(output["name"], "Focus Deep Work");
        server.verify().await;
    }

    #[tokio::test]
    async fn play_search_artiste_joue_son_contexte() {
        let server = MockServer::start().await;
        Mock::given(path("/search")).and(query_param("type", "artist")).and(query_param("limit", "1")).respond_with(ok("search")).mount(&server).await;
        Mock::given(method("PUT")).and(path("/me/player/play")).and(body_partial_json(json!({"context_uri": "spotify:artist:a1xxxxxxxxxxxxxxxxxxxx"}))).respond_with(ResponseTemplate::new(204)).expect(1).mount(&server).await;
        let e = env(&server, true, false).await;
        assert!(matches!(e.call("music.play_search", json!({"query": "a", "kind": "artist"})).await, ToolOutcome::Done { .. }));
        assert!(matches!(e.call("music.play_search", json!({"query": "a", "kind": "album"})).await, ToolOutcome::Failed { .. }));
        server.verify().await;
    }

    #[test]
    fn uri_spotify_strictes() {
        assert!(check_uri("spotify:track:6rqhFgbbKwnb9MLmUQDhG6").is_ok());
        for u in ["spotify:track:abc", "spotify:user:6rqhFgbbKwnb9MLmUQDhG6", "spotify:track:6rqhFgbbKwnb9MLmUQDhG6x", "http://x", "spotify:track:6rqhFgbbKwnb9MLmUQDhG!"] {
            assert!(check_uri(u).is_err(), "{u}");
        }
    }

    #[tokio::test]
    async fn recherche_journalisee_sans_le_texte() {
        let server = MockServer::start().await;
        Mock::given(path("/search")).respond_with(ok("search")).mount(&server).await;
        Mock::given(method("PUT")).and(path("/me/player/play")).respond_with(ResponseTemplate::new(204)).mount(&server).await;
        let e = env(&server, true, false).await;
        let ToolOutcome::Done { action_id, .. } = e.call("music.play_search", json!({"query": "secretxyz"})).await else { panic!() };
        let args: String = e.db.lock().query_row("SELECT args FROM actions WHERE id=?", [action_id], |r| r.get(0)).unwrap();
        assert_eq!(args, r#"{"query_chars":9}"#);
    }

    #[tokio::test]
    async fn volume_delta_part_du_volume_actif() {
        let server = MockServer::start().await;
        Mock::given(path("/me/player/devices")).respond_with(ok("devices")).mount(&server).await;
        Mock::given(method("PUT")).and(path("/me/player/volume")).and(query_param("volume_percent", "25")).respond_with(ResponseTemplate::new(204)).expect(1).mount(&server).await;
        let e = env(&server, true, false).await;
        let ToolOutcome::Done { output, .. } = e.call("music.volume", json!({"delta": -10})).await else { panic!() };
        assert_eq!(output["percent"], 25);
        assert!(matches!(e.call("music.volume", json!({"percent": 101})).await, ToolOutcome::Failed { .. }));
        assert!(matches!(e.call("music.volume", json!({"percent": 5, "delta": 1})).await, ToolOutcome::Failed { .. }));
        server.verify().await;
    }

    #[tokio::test]
    async fn jeton_refuse_oublie_puis_un_seul_nouvel_essai() {
        let server = MockServer::start().await;
        Mock::given(path("/token")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"access_token": "tok2", "expires_in": 3600}))).expect(1).mount(&server).await;
        Mock::given(method("PUT")).and(path("/me/player/volume")).and(wiremock::matchers::header("authorization", "Bearer tok")).respond_with(ResponseTemplate::new(401)).expect(1).mount(&server).await;
        Mock::given(method("PUT")).and(path("/me/player/volume")).and(wiremock::matchers::header("authorization", "Bearer tok2")).respond_with(ResponseTemplate::new(204)).expect(1).mount(&server).await;
        let e = env(&server, true, false).await;
        assert!(matches!(e.call("music.volume", json!({"percent": 40})).await, ToolOutcome::Done { .. }));
        server.verify().await;
    }

    #[tokio::test]
    async fn second_401_passe_le_compte_en_reconnexion() {
        let server = MockServer::start().await;
        Mock::given(path("/token")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"access_token": "tok2", "expires_in": 3600}))).mount(&server).await;
        Mock::given(path("/me/player/volume")).respond_with(ResponseTemplate::new(401)).expect(2).mount(&server).await;
        let e = env(&server, true, false).await;
        assert!(failed_with(e.call("music.volume", json!({"percent": 40})).await).contains("Reconnecte Spotify"));
        assert_eq!(accounts::get(&e.db.lock(), e.account.unwrap()).unwrap().unwrap().status, Status::Reauth);
        assert!(failed_with(e.call("music.devices", json!({})).await).contains(NO_ACCOUNT), "plus de compte utilisable");
        server.verify().await;
    }

    #[tokio::test]
    async fn favoris_par_defaut_puis_choisis_et_cache_de_dix_minutes() {
        let server = MockServer::start().await;
        playlists_mocks(&server).await;
        let e = env(&server, true, false).await;
        assert!(e.web.stale() && e.web.favorites().is_empty(), "rien avant la relève");
        e.web.refresh_if_stale().await;
        assert_eq!(e.web.favorites().len(), 3, "6 premières, ici 3 existent");
        assert!(!e.web.stale());
        let before = server.received_requests().await.unwrap().len();
        e.web.refresh_if_stale().await;
        assert_eq!(server.received_requests().await.unwrap().len(), before, "cache : pas de nouvelle requête");
        e.web.set_favorites(&["pl3".into(), "inconnue".into(), "pl3".into(), "pl1xxxxxxxxxxxxxxxxxxx".into()]).unwrap();
        assert_eq!(e.web.favorites().iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["pl3", "pl1xxxxxxxxxxxxxxxxxxx"]);
        let nine: Vec<String> = (0..9).map(|i| format!("p{i}")).collect();
        assert!(e.web.set_favorites(&nine).is_err());
    }

    #[tokio::test]
    async fn sans_compte_le_panneau_ne_demande_rien() {
        let server = MockServer::start().await;
        let e = env(&server, false, false).await;
        e.web.refresh_if_stale().await;
        assert!(server.received_requests().await.unwrap().is_empty());
        assert!(!e.web.view().connected);
    }
}

