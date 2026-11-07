//! Comptes en ligne (table `accounts`, migration 13) et cache mémoire des jetons d'accès.
//! Le jeton d'actualisation vit dans le coffre (`secrets::account_key`), jamais en base.
pub mod tools;
use crate::events::{Events, Topic};
use crate::oauth::{self, OAuthError, Provider, ProviderConfig, Tokens};
use crate::secrets::{account_key, SecretStore};
use crate::{CoreError, Db};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;

/// Couleurs de `tokens.css`, attribuées dans l'ordre (la première libre).
pub const PALETTE: [&str; 8] = ["#ff7448", "#0a84ff", "#30d158", "#ffd60a", "#bf5af2", "#64d2ff", "#ff3d7f", "#5e5ce6"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status { Ok, Reauth }

impl Status {
    fn as_str(self) -> &'static str { match self { Status::Ok => "ok", Status::Reauth => "reauth" } }
}

#[derive(Clone, PartialEq, Serialize)]
pub struct Account {
    pub id: i64,
    pub provider: Provider,
    /// Adresse ou nom affiché : contenu utilisateur, jamais journalisé.
    pub label: String,
    pub color: String,
    pub status: Status,
    pub created_ms: i64,
}

/// Sans le nom : c'est souvent une adresse.
impl std::fmt::Debug for Account {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Account").field("id", &self.id).field("provider", &self.provider).field("status", &self.status).finish_non_exhaustive()
    }
}

const COLS: &str = "id, provider, label, color, status, created_ms";

fn from_row(r: &Row) -> rusqlite::Result<Account> {
    let provider: String = r.get(1)?;
    let status: String = r.get(4)?;
    Ok(Account {
        id: r.get(0)?,
        provider: Provider::parse(&provider)
            .ok_or_else(|| rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, "fournisseur inconnu".into()))?,
        label: r.get(2)?,
        color: r.get(3)?,
        status: if status == "ok" { Status::Ok } else { Status::Reauth },
        created_ms: r.get(5)?,
    })
}

/// Ajoute le compte, ou le repasse à `ok` s'il existe déjà (reconnexion : même id, même couleur).
pub fn add(c: &Connection, provider: Provider, label: &str, now_ms: i64) -> Result<i64, CoreError> {
    let label = label.trim();
    if label.is_empty() {
        return Err(CoreError::Invalid("compte sans nom".into()));
    }
    let used: Vec<String> = c.prepare("SELECT color FROM accounts")?.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
    let color = PALETTE.iter().find(|p| !used.iter().any(|u| u == *p)).unwrap_or(&PALETTE[used.len() % PALETTE.len()]);
    Ok(c.query_row(
        "INSERT INTO accounts(provider, label, color, status, created_ms) VALUES (?1, ?2, ?3, 'ok', ?4)
         ON CONFLICT(provider, label) DO UPDATE SET status = 'ok' RETURNING id",
        params![provider.as_str(), label, color, now_ms],
        |r| r.get(0),
    )?)
}

pub fn list(c: &Connection) -> Result<Vec<Account>, CoreError> {
    let mut st = c.prepare(&format!("SELECT {COLS} FROM accounts ORDER BY id"))?;
    let rows = st.query_map([], from_row)?.collect::<Result<_, _>>()?;
    Ok(rows)
}

pub fn get(c: &Connection, id: i64) -> Result<Option<Account>, CoreError> {
    Ok(c.query_row(&format!("SELECT {COLS} FROM accounts WHERE id = ?1"), [id], from_row).optional()?)
}

pub fn set_status(c: &Connection, id: i64, status: Status) -> Result<(), CoreError> {
    if c.execute("UPDATE accounts SET status = ?2 WHERE id = ?1", params![id, status.as_str()])? == 0 {
        return Err(CoreError::NotFound("compte introuvable".into()));
    }
    Ok(())
}

/// Supprime le compte, ses mails et événements Google en cache, ses jetons de synchronisation et son jeton
/// d'actualisation (le secret part même si la ligne manquait).
pub fn remove(c: &Connection, secrets: &dyn SecretStore, id: i64) -> Result<(), CoreError> {
    let tx = c.unchecked_transaction()?;
    tx.execute("DELETE FROM mails WHERE account_id = ?1", [id])?;
    tx.execute("DELETE FROM mail_sync WHERE account_id = ?1", [id])?;
    tx.execute("DELETE FROM external_events WHERE account_id = ?1", [id])?;
    tx.execute("DELETE FROM kv WHERE key LIKE ?1 OR key LIKE ?2", [format!("gcal_sync/{id}/%"), format!("gcal_full/{id}/%")])?;
    let n = tx.execute("DELETE FROM accounts WHERE id = ?1", [id])?;
    tx.commit()?;
    secrets.delete(&account_key(id));
    if n == 0 {
        return Err(CoreError::NotFound("compte introuvable".into()));
    }
    Ok(())
}

/// Ce que voit l'interface : pas de date de création, jamais de jeton.
#[derive(Clone, Serialize)]
pub struct AccountView {
    pub id: i64,
    pub provider: Provider,
    /// Adresse ou nom affiché : seul contenu utilisateur renvoyé à l'interface.
    pub label: String,
    pub color: String,
    pub status: Status,
}

impl From<&Account> for AccountView {
    fn from(a: &Account) -> Self {
        Self { id: a.id, provider: a.provider, label: a.label.clone(), color: a.color.clone(), status: a.status }
    }
}

/// Change la couleur d'un compte (une de la palette).
pub fn set_color(c: &Connection, id: i64, color: &str) -> Result<(), CoreError> {
    if !PALETTE.contains(&color) {
        return Err(CoreError::Invalid("couleur inconnue".into()));
    }
    if c.execute("UPDATE accounts SET color = ?2 WHERE id = ?1", params![id, color])? == 0 {
        return Err(CoreError::NotFound("compte introuvable".into()));
    }
    Ok(())
}

/// Client des API de comptes : délai de 15 s, agent générique.
pub fn client() -> Result<reqwest::Client, CoreError> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("Oniris/0.1")
        .build()
        .map_err(|e| CoreError::Invalid(format!("réseau : {}", e.without_url())))
}

/// Points d'accès de l'identité (injectables : les tests les pointent sur un faux serveur).
pub fn identity_url(p: Provider) -> &'static str {
    match p {
        Provider::Google => "https://openidconnect.googleapis.com/v1/userinfo",
        Provider::Spotify => "https://api.spotify.com/v1/me",
    }
}

/// Fin de la connexion, une fois le code reçu : échange, identité, compte (créé ou repassé à `ok`), jetons rangés,
/// `Topic::Accounts` émis. Aucun navigateur ici ; erreurs en français, sans jeton ni adresse.
#[allow(clippy::too_many_arguments)]
pub async fn complete_connect(
    db: &Db, tokens: &TokenCache, events: &dyn Events, http: &reqwest::Client,
    cfg: &ProviderConfig, code: &str, verifier: &str, redirect: &str, identity_url: &str,
) -> Result<AccountView, String> {
    let t = oauth::exchange(http, cfg, code, verifier, redirect).await.map_err(|e| e.to_string())?;
    if t.refresh.is_none() {
        return Err("Le fournisseur n'a pas accordé l'accès permanent, réessaie.".into());
    }
    let resp = http.get(identity_url).bearer_auth(&t.access).send().await.map_err(|_| "Réseau indisponible, réessaie.".to_string())?;
    if !resp.status().is_success() {
        return Err(format!("Identité du compte illisible (HTTP {}).", resp.status().as_u16()));
    }
    let body: serde_json::Value = resp.json().await.map_err(|_| "Identité du compte illisible.".to_string())?;
    let text = |f: &str| body.get(f).and_then(|v| v.as_str()).map(str::trim).filter(|l| !l.is_empty()).map(str::to_string);
    let label = match cfg.provider {
        Provider::Google => text("email"),
        // `display_name` est null pour certains comptes Spotify : repli sur l'identifiant.
        Provider::Spotify => text("display_name").or_else(|| text("id")),
    }
    .ok_or("Identité du compte introuvable.")?;
    let (account, existed) = {
        let c = db.lock();
        let existed = c
            .query_row("SELECT 1 FROM accounts WHERE provider = ?1 AND label = ?2", params![cfg.provider.as_str(), label], |_| Ok(()))
            .optional()
            .map_err(|e| e.to_string())?
            .is_some();
        let id = add(&c, cfg.provider, &label, crate::time_util::now_ms()).map_err(|e| e.to_string())?;
        (get(&c, id).map_err(|e| e.to_string())?.ok_or("compte introuvable")?, existed)
    };
    if tokens.insert(account.id, t).is_err() {
        // Coffre refusé : pas de compte « ok » sans jeton d'actualisation.
        {
            let c = db.lock();
            let r = if existed { set_status(&c, account.id, Status::Reauth) } else { c.execute("DELETE FROM accounts WHERE id = ?1", [account.id]).map(drop).map_err(CoreError::from) };
            if let Err(e) = r {
                tracing::warn!("compte {} : état non rétabli : {e}", account.id);
            }
        }
        events.changed(Topic::Accounts);
        return Err("Le coffre Windows a refusé le jeton, réessaie.".into());
    }
    events.changed(Topic::Accounts);
    Ok(AccountView::from(&account))
}

/// Configuration courante d'un fournisseur (identifiant client des réglages, secret du coffre) ; `None` si non configuré.
pub type Configs = Arc<dyn Fn(Provider) -> Option<ProviderConfig> + Send + Sync>;

/// Jetons d'accès en mémoire seulement (`account_id -> Tokens`), actualisés à la demande.
pub struct TokenCache {
    db: Db,
    secrets: Arc<dyn SecretStore>,
    events: Arc<dyn Events>,
    http: reqwest::Client,
    configs: Configs,
    tokens: parking_lot::Mutex<HashMap<i64, Tokens>>,
    /// Un verrou par compte : une seule actualisation à la fois.
    locks: parking_lot::Mutex<HashMap<i64, Arc<tokio::sync::Mutex<()>>>>,
}

impl TokenCache {
    pub fn new(db: Db, secrets: Arc<dyn SecretStore>, events: Arc<dyn Events>, http: reqwest::Client, configs: Configs) -> Self {
        Self { db, secrets, events, http, configs, tokens: Default::default(), locks: Default::default() }
    }

    /// Après un échange réussi : garde le jeton d'accès et range le jeton d'actualisation dans le coffre.
    pub fn insert(&self, account_id: i64, tokens: Tokens) -> Result<(), String> {
        if let Some(r) = &tokens.refresh {
            self.secrets.set(&account_key(account_id), r)?;
        }
        self.tokens.lock().insert(account_id, tokens);
        Ok(())
    }

    /// À appeler à la suppression du compte.
    pub fn forget(&self, account_id: i64) {
        self.tokens.lock().remove(&account_id);
        self.locks.lock().remove(&account_id);
    }

    fn fresh(&self, account_id: i64) -> Option<String> {
        let now = crate::time_util::now_ms();
        self.tokens.lock().get(&account_id).filter(|t| t.expires_at_ms - now > oauth::REFRESH_MARGIN_MS).map(|t| t.access.clone())
    }

    /// Jeton d'accès valable au moins 60 s. `Revoked` : le compte passe à `reauth` et `Topic::Accounts` est émis.
    pub async fn access_token(&self, account_id: i64) -> Result<String, OAuthError> {
        if let Some(a) = self.fresh(account_id) {
            return Ok(a);
        }
        let lock = self.locks.lock().entry(account_id).or_default().clone();
        let _guard = lock.lock().await;
        // Un appel concurrent a pu actualiser pendant l'attente.
        if let Some(a) = self.fresh(account_id) {
            return Ok(a);
        }
        let account = get(&self.db.lock(), account_id).map_err(|e| OAuthError::Other(e.to_string()))?;
        let account = account.ok_or_else(|| OAuthError::Other("compte introuvable".into()))?;
        let cfg = (self.configs)(account.provider).ok_or_else(|| OAuthError::Other("fournisseur non configuré".into()))?;
        let result = match self.secrets.get(&account_key(account_id)) {
            Some(r) => oauth::refresh(&self.http, &cfg, &r).await,
            None => Err(OAuthError::Revoked),
        };
        match result {
            Ok(t) => {
                if let Some(r) = &t.refresh {
                    // Rotation : l'ancien jeton d'actualisation ne vaut plus rien, ne pas perdre le nouveau.
                    if let Err(e) = self.secrets.set(&account_key(account_id), r) {
                        tracing::error!("compte {account_id} : jeton d'actualisation non enregistré : {e}");
                    }
                }
                let access = t.access.clone();
                self.tokens.lock().insert(account_id, t);
                Ok(access)
            }
            Err(OAuthError::Revoked) => {
                self.tokens.lock().remove(&account_id);
                let r = set_status(&self.db.lock(), account_id, Status::Reauth);
                if let Err(e) = r {
                    tracing::warn!("compte {account_id} : statut non mis à jour : {e}");
                }
                tracing::info!("compte {account_id} : accès révoqué, reconnexion nécessaire");
                self.events.changed(Topic::Accounts);
                Err(OAuthError::Revoked)
            }
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::MemorySecrets;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test] fn ajout_couleurs_et_reconnexion() {
        let c = crate::test_util::db();
        let a = add(&c, Provider::Google, "a@gmail.com", 1).unwrap();
        let b = add(&c, Provider::Google, " b@gmail.com ", 2).unwrap();
        add(&c, Provider::Spotify, "a@gmail.com", 3).unwrap();
        let l = list(&c).unwrap();
        assert_eq!(l.iter().map(|x| x.color.as_str()).collect::<Vec<_>>(), &PALETTE[..3]);
        assert_eq!(l[1].label, "b@gmail.com");
        set_status(&c, a, Status::Reauth).unwrap();
        assert_eq!(get(&c, a).unwrap().unwrap().status, Status::Reauth);
        assert_eq!(add(&c, Provider::Google, "a@gmail.com", 9).unwrap(), a, "reconnexion : même compte");
        let again = get(&c, a).unwrap().unwrap();
        assert_eq!((again.status, again.color.as_str(), again.created_ms), (Status::Ok, PALETTE[0], 1));
        // Couleur libérée : réattribuée en premier.
        remove(&c, &MemorySecrets::default(), b).unwrap();
        add(&c, Provider::Google, "c@gmail.com", 4).unwrap();
        assert_eq!(list(&c).unwrap().last().unwrap().color, PALETTE[1]);
        assert!(add(&c, Provider::Google, "  ", 5).is_err());
        assert!(!format!("{:?}", get(&c, a).unwrap()).contains("gmail"), "le nom n'est pas dans le Debug");
        assert!(matches!(set_status(&c, 999, Status::Ok), Err(CoreError::NotFound(_))));
        // Au-delà de 8 comptes : la palette reboucle.
        for i in 0..6 { add(&c, Provider::Google, &format!("x{i}"), 6).unwrap(); }
        assert_eq!(list(&c).unwrap().len(), 9);
        assert_eq!(list(&c).unwrap()[8].color, PALETTE[0]);
    }

    fn left_of(c: &Connection, t: &str) -> Option<String> {
        c.query_row(&format!("SELECT group_concat(account_id) FROM {t}"), [], |r| r.get(0)).unwrap()
    }

    #[test] fn supprimer_efface_le_secret() {
        let c = crate::test_util::db();
        let s = MemorySecrets::default();
        let id = add(&c, Provider::Spotify, "moi", 1).unwrap();
        s.set(&account_key(id), "ref").unwrap();
        s.set(&account_key(id + 1), "autre").unwrap();
        for acc in [id, id + 1] {
            c.execute("INSERT INTO mails VALUES (?1, 'm', 't', 'n', 'a', 's', '', 1, 0, '[]', 'read', 'heuristic', NULL, NULL, 0)", [acc]).unwrap();
            c.execute("INSERT INTO mail_sync(account_id, history_id) VALUES (?1, '1')", [acc]).unwrap();
        }
        for acc in [id, id + 1] {
            c.execute("INSERT INTO external_events VALUES (?1, 'e', 'cal', 't', 1, 2, 0, NULL)", [acc]).unwrap();
            crate::kv::set(&c, &format!("gcal_sync/{acc}/cal"), "tok", 1).unwrap();
            crate::kv::set(&c, &format!("gcal_full/{acc}/cal"), "1", 1).unwrap();
        }
        remove(&c, &s, id).unwrap();
        assert_eq!(left_of(&c, "external_events"), Some((id + 1).to_string()));
        let keys: String = c.query_row("SELECT group_concat(key) FROM (SELECT key FROM kv WHERE key LIKE 'gcal_%' ORDER BY key)", [], |r| r.get(0)).unwrap();
        assert_eq!(keys, format!("gcal_full/{0}/cal,gcal_sync/{0}/cal", id + 1), "jetons de synchronisation du compte effacés, pas ceux des autres");
        let left = |t: &str| c.query_row(&format!("SELECT group_concat(account_id) FROM {t}"), [], |r| r.get::<_, Option<String>>(0)).unwrap();
        assert_eq!((left("mails"), left("mail_sync")), (Some((id + 1).to_string()), Some((id + 1).to_string())), "mails du compte effacés, pas ceux des autres");
        assert_eq!(s.get(&account_key(id)), None);
        assert_eq!(s.get(&account_key(id + 1)).as_deref(), Some("autre"));
        assert!(list(&c).unwrap().is_empty());
        assert!(matches!(remove(&c, &s, id), Err(CoreError::NotFound(_))));
    }

    #[derive(Default)]
    struct Count(AtomicUsize);
    impl Events for Count {
        fn changed(&self, t: Topic) { assert_eq!(t, Topic::Accounts); self.0.fetch_add(1, Ordering::SeqCst); }
    }

    fn cache(server: &MockServer) -> (TokenCache, Db, Arc<MemorySecrets>, Arc<Count>, i64) {
        let db: Db = Arc::new(parking_lot::Mutex::new(crate::test_util::db()));
        let id = add(&db.lock(), Provider::Spotify, "moi", 1).unwrap();
        let secrets = Arc::new(MemorySecrets::default());
        secrets.set(&account_key(id), "r1").unwrap();
        let events = Arc::new(Count::default());
        let url = format!("{}/token", server.uri());
        let configs: Configs = Arc::new(move |p| {
            assert_eq!(p, Provider::Spotify);
            Some(ProviderConfig { token_url: url.clone(), ..oauth::providers::spotify("cid") })
        });
        let tc = TokenCache::new(db.clone(), secrets.clone(), events.clone(), reqwest::Client::new(), configs);
        (tc, db, secrets, events, id)
    }

    #[tokio::test] async fn connexion_et_reconnexion_sans_doublon() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"access_token": "a", "expires_in": 3600, "refresh_token": "r9"})))
            .mount(&server).await;
        Mock::given(method("GET")).and(path("/me")).and(wiremock::matchers::header("authorization", "Bearer a"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"display_name": "moi", "email": "x@y.z"})))
            .mount(&server).await;
        let (tc, db, secrets, events, id) = cache(&server);
        let cfg = ProviderConfig { token_url: format!("{}/token", server.uri()), ..oauth::providers::spotify("cid") };
        let url = format!("{}/me", server.uri());
        set_status(&db.lock(), id, Status::Reauth).unwrap();
        let v = complete_connect(&db, &tc, &*events, &reqwest::Client::new(), &cfg, "code", "ver", oauth::REDIRECT_URI, &url).await.unwrap();
        assert_eq!((v.id, v.label.as_str(), v.status), (id, "moi", Status::Ok), "même compte, repassé à ok");
        assert_eq!(list(&db.lock()).unwrap().len(), 1);
        assert_eq!(secrets.get(&account_key(id)).as_deref(), Some("r9"));
        assert_eq!(tc.access_token(id).await.unwrap(), "a", "jeton d'accès déjà en mémoire");
        assert_eq!(events.0.load(Ordering::SeqCst), 1);
        // Google : champ `email`, et un compte neuf est ajouté.
        let gcfg = ProviderConfig { token_url: cfg.token_url.clone(), ..oauth::providers::google("g", None) };
        let v2 = complete_connect(&db, &tc, &*events, &reqwest::Client::new(), &gcfg, "c", "v", oauth::REDIRECT_URI, &url).await.unwrap();
        assert_eq!(v2.label, "x@y.z");
        assert_eq!(list(&db.lock()).unwrap().len(), 2);
        // Identité refusée : rien n'est ajouté.
        let bad_url = format!("{}/absent", server.uri());
        assert!(complete_connect(&db, &tc, &*events, &reqwest::Client::new(), &cfg, "c", "v", oauth::REDIRECT_URI, &bad_url).await.is_err());
        assert_eq!(list(&db.lock()).unwrap().len(), 2);
    }

    #[tokio::test] async fn identite_spotify_sans_nom_et_coffre_refuse() {
        struct Refuse;
        impl SecretStore for Refuse {
            fn get(&self, _: &str) -> Option<String> { None }
            fn set(&self, _: &str, _: &str) -> Result<(), String> { Err("refus".into()) }
            fn delete(&self, _: &str) {}
        }
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"access_token": "a", "expires_in": 3600, "refresh_token": "r"})))
            .mount(&server).await;
        Mock::given(method("GET")).and(path("/me"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"display_name": null, "id": "user42"})))
            .mount(&server).await;
        let db: Db = Arc::new(parking_lot::Mutex::new(crate::test_util::db()));
        let events = Arc::new(Count::default());
        let cfg = ProviderConfig { token_url: format!("{}/token", server.uri()), ..oauth::providers::spotify("cid") };
        let url = format!("{}/me", server.uri());
        let configs: Configs = Arc::new(|_| None);
        let good = TokenCache::new(db.clone(), Arc::new(MemorySecrets::default()), events.clone(), reqwest::Client::new(), configs.clone());
        let v = complete_connect(&db, &good, &*events, &reqwest::Client::new(), &cfg, "c", "v", oauth::REDIRECT_URI, &url).await.unwrap();
        assert_eq!(v.label, "user42", "repli sur l'identifiant");
        // Coffre qui refuse : compte existant -> reauth, événement émis.
        let bad = TokenCache::new(db.clone(), Arc::new(Refuse), events.clone(), reqwest::Client::new(), configs.clone());
        let before = events.0.load(Ordering::SeqCst);
        assert!(complete_connect(&db, &bad, &*events, &reqwest::Client::new(), &cfg, "c", "v", oauth::REDIRECT_URI, &url).await.is_err());
        assert_eq!(get(&db.lock(), v.id).unwrap().unwrap().status, Status::Reauth);
        assert_eq!(events.0.load(Ordering::SeqCst), before + 1);
        // Compte neuf : la ligne insérée est retirée.
        Mock::given(method("GET")).and(path("/me2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"display_name": "Neuf"})))
            .mount(&server).await;
        let url2 = format!("{}/me2", server.uri());
        assert!(complete_connect(&db, &bad, &*events, &reqwest::Client::new(), &cfg, "c", "v", oauth::REDIRECT_URI, &url2).await.is_err());
        assert_eq!(list(&db.lock()).unwrap().len(), 1, "pas de compte neuf sans jeton");
        assert_eq!(events.0.load(Ordering::SeqCst), before + 2);
    }

    #[test] fn couleur_de_la_palette_seulement() {
        let c = crate::test_util::db();
        let id = add(&c, Provider::Google, "a", 1).unwrap();
        set_color(&c, id, PALETTE[3]).unwrap();
        assert_eq!(get(&c, id).unwrap().unwrap().color, PALETTE[3]);
        assert!(set_color(&c, id, "#123456").is_err());
        assert!(matches!(set_color(&c, 99, PALETTE[0]), Err(CoreError::NotFound(_))));
    }

    #[tokio::test] async fn une_seule_actualisation_concurrente_et_rotation() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/token")).and(body_string_contains("refresh_token=r1"))
            .respond_with(ResponseTemplate::new(200)
                .set_delay(std::time::Duration::from_millis(200))
                .set_body_json(serde_json::json!({"access_token": "a1", "expires_in": 3600, "refresh_token": "r2"})))
            .expect(1).mount(&server).await;
        let (tc, _db, secrets, _ev, id) = cache(&server);
        let (x, y) = tokio::join!(tc.access_token(id), tc.access_token(id));
        assert_eq!((x.unwrap(), y.unwrap()), ("a1".to_string(), "a1".to_string()));
        assert_eq!(tc.access_token(id).await.unwrap(), "a1", "servi depuis le cache");
        assert_eq!(secrets.get(&account_key(id)).as_deref(), Some("r2"), "jeton tourné enregistré");
        server.verify().await;
    }

    #[tokio::test] async fn expiration_proche_actualise() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"access_token": "neuf", "expires_in": 3600})))
            .expect(1).mount(&server).await;
        let (tc, _db, secrets, _ev, id) = cache(&server);
        tc.insert(id, Tokens { access: "vieux".into(), refresh: None, expires_at_ms: crate::time_util::now_ms() + 30_000 }).unwrap();
        assert_eq!(tc.access_token(id).await.unwrap(), "neuf");
        assert_eq!(secrets.get(&account_key(id)).as_deref(), Some("r1"), "sans rotation, l'ancien jeton reste");
    }

    #[tokio::test] async fn revoque_passe_en_reauth() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({"error": "invalid_grant"})))
            .mount(&server).await;
        let (tc, db, _secrets, ev, id) = cache(&server);
        assert_eq!(tc.access_token(id).await, Err(OAuthError::Revoked));
        assert_eq!(get(&db.lock(), id).unwrap().unwrap().status, Status::Reauth);
        assert_eq!(ev.0.load(Ordering::SeqCst), 1);
    }

    #[tokio::test] async fn sans_jeton_d_actualisation_reauth() {
        let server = MockServer::start().await;
        let (tc, db, secrets, ev, id) = cache(&server);
        secrets.delete(&account_key(id));
        assert_eq!(tc.access_token(id).await, Err(OAuthError::Revoked));
        assert_eq!(get(&db.lock(), id).unwrap().unwrap().status, Status::Reauth);
        assert_eq!(ev.0.load(Ordering::SeqCst), 1);
    }
}
