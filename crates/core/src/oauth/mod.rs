//! OAuth 2 : code d'autorisation avec PKCE (S256), échange et actualisation des jetons.
//! Jetons et codes ne sont jamais journalisés ; les erreurs ne portent que des statuts et codes d'erreur.
pub mod pkce;
pub mod providers;

pub use providers::{Provider, ProviderConfig};
use serde::Deserialize;

/// Port fixe de la redirection en boucle locale (déclaré tel quel chez Google et Spotify).
pub const LOOPBACK_PORT: u16 = 47829;
pub const REDIRECT_URI: &str = "http://127.0.0.1:47829/callback";

/// Avance prise sur l'expiration : un jeton qui expire dans moins de 60 s est actualisé.
pub const REFRESH_MARGIN_MS: i64 = 60_000;

#[derive(Clone, PartialEq)]
pub struct Tokens {
    pub access: String,
    /// Nouveau jeton d'actualisation, si le fournisseur en donne un (premier échange ou rotation).
    pub refresh: Option<String>,
    pub expires_at_ms: i64,
}

/// Masque les jetons : un `{:?}` égaré ne doit rien divulguer.
impl std::fmt::Debug for Tokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tokens").field("refresh", &self.refresh.is_some()).field("expires_at_ms", &self.expires_at_ms).finish()
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum OAuthError {
    /// `invalid_grant` : jeton révoqué ou expiré, il faut reconnecter le compte.
    #[error("accès révoqué, reconnecte le compte")]
    Revoked,
    #[error("réseau indisponible")]
    Network,
    #[error("{0}")]
    Other(String),
}

/// URL d'autorisation à ouvrir dans le navigateur.
pub fn auth_url(cfg: &ProviderConfig, redirect: &str, state: &str, challenge: &str) -> String {
    let scope = cfg.scopes.join(" ");
    let mut q: Vec<(&str, &str)> = vec![
        ("response_type", "code"),
        ("client_id", &cfg.client_id),
        ("redirect_uri", redirect),
        ("scope", &scope),
        ("state", state),
        ("code_challenge", challenge),
        ("code_challenge_method", "S256"),
    ];
    q.extend_from_slice(cfg.extra);
    url::Url::parse_with_params(&cfg.auth_url, &q).map(String::from).unwrap_or_default()
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default = "default_expires_in")]
    expires_in: i64,
    refresh_token: Option<String>,
}
fn default_expires_in() -> i64 { 3600 }

#[derive(Deserialize)]
struct ErrorResponse { error: String }

async fn post(http: &reqwest::Client, cfg: &ProviderConfig, mut form: Vec<(&str, &str)>) -> Result<Tokens, OAuthError> {
    form.push(("client_id", &cfg.client_id));
    if let Some(s) = &cfg.client_secret {
        form.push(("client_secret", s));
    }
    let resp = http.post(&cfg.token_url).form(&form).send().await.map_err(|_| OAuthError::Network)?;
    let status = resp.status();
    let body = resp.text().await.map_err(|_| OAuthError::Network)?;
    if status.is_success() {
        let t: TokenResponse = serde_json::from_str(&body).map_err(|_| OAuthError::Other("réponse du fournisseur illisible".into()))?;
        return Ok(Tokens {
            access: t.access_token,
            refresh: t.refresh_token.filter(|r| !r.is_empty()),
            expires_at_ms: crate::time_util::now_ms() + t.expires_in.clamp(0, 86_400 * 365) * 1000,
        });
    }
    let code = serde_json::from_str::<ErrorResponse>(&body).map(|e| e.error).unwrap_or_default();
    if code == "invalid_grant" {
        return Err(OAuthError::Revoked);
    }
    if status.is_server_error() {
        return Err(OAuthError::Network);
    }
    // Le code d'erreur OAuth est un identifiant normalisé, sans contenu utilisateur ; rogné par prudence.
    let code: String = code.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_').take(40).collect();
    Err(OAuthError::Other(format!("le fournisseur refuse (HTTP {}{}{})", status.as_u16(), if code.is_empty() { "" } else { ", " }, code)))
}

/// Échange le code reçu sur la redirection contre les jetons.
pub async fn exchange(http: &reqwest::Client, cfg: &ProviderConfig, code: &str, verifier: &str, redirect: &str) -> Result<Tokens, OAuthError> {
    post(http, cfg, vec![("grant_type", "authorization_code"), ("code", code), ("code_verifier", verifier), ("redirect_uri", redirect)]).await
}

/// Nouveau jeton d'accès ; `Tokens::refresh` est `Some` si le fournisseur fait tourner le jeton d'actualisation.
pub async fn refresh(http: &reqwest::Client, cfg: &ProviderConfig, refresh_token: &str) -> Result<Tokens, OAuthError> {
    post(http, cfg, vec![("grant_type", "refresh_token"), ("refresh_token", refresh_token)]).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn cfg_at(server: &MockServer) -> ProviderConfig {
        ProviderConfig { token_url: format!("{}/token", server.uri()), ..providers::google("cid", Some("sec".into())) }
    }

    #[test] fn url_d_autorisation() {
        let u = auth_url(&providers::spotify("abc"), REDIRECT_URI, "st4te", "chal");
        let parsed = url::Url::parse(&u).unwrap();
        assert!(u.starts_with("https://accounts.spotify.com/authorize?"));
        let q: std::collections::HashMap<String, String> = parsed.query_pairs().into_owned().collect();
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["code_challenge"], "chal");
        assert_eq!(q["state"], "st4te");
        assert_eq!(q["redirect_uri"], "http://127.0.0.1:47829/callback");
        assert_eq!(q["client_id"], "abc");
        assert_eq!(q["response_type"], "code");
        assert_eq!(q["scope"], "user-read-playback-state user-modify-playback-state user-read-currently-playing playlist-read-private playlist-read-collaborative");
        assert!(!q.contains_key("client_secret"));
        let g = url::Url::parse(&auth_url(&providers::google("id", Some("s".into())), REDIRECT_URI, "s", "c")).unwrap();
        let q: std::collections::HashMap<String, String> = g.query_pairs().into_owned().collect();
        assert_eq!((q["access_type"].as_str(), q["prompt"].as_str()), ("offline", "consent"));
        assert_eq!(q["scope"], "openid email https://www.googleapis.com/auth/gmail.modify https://www.googleapis.com/auth/calendar.readonly");
        assert!(!q.contains_key("client_secret"), "le secret ne va que dans la requête de jeton");
    }

    #[tokio::test] async fn echange_reussi() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/token"))
            .and(body_string_contains("grant_type=authorization_code"))
            .and(body_string_contains("code=c0de"))
            .and(body_string_contains("code_verifier=v3r"))
            .and(body_string_contains("client_secret=sec"))
            .and(body_string_contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A47829%2Fcallback"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "acc", "expires_in": 3599, "refresh_token": "ref", "token_type": "Bearer"
            })))
            .expect(1).mount(&server).await;
        let before = crate::time_util::now_ms();
        let t = exchange(&reqwest::Client::new(), &cfg_at(&server), "c0de", "v3r", REDIRECT_URI).await.unwrap();
        let after = crate::time_util::now_ms();
        assert_eq!((t.access.as_str(), t.refresh.as_deref()), ("acc", Some("ref")));
        assert!((before + 3_599_000..=after + 3_599_000).contains(&t.expires_at_ms));
        assert!(!format!("{t:?}").contains("acc"));
    }

    #[tokio::test] async fn invalid_grant_donne_revoked() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({"error": "invalid_grant"})))
            .mount(&server).await;
        let http = reqwest::Client::new();
        assert_eq!(refresh(&http, &cfg_at(&server), "r").await, Err(OAuthError::Revoked));
        assert_eq!(exchange(&http, &cfg_at(&server), "c", "v", REDIRECT_URI).await, Err(OAuthError::Revoked));
    }

    #[tokio::test] async fn autres_erreurs() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/token"))
            .and(body_string_contains("refresh_token=bad"))
            .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({"error": "invalid_client"})))
            .mount(&server).await;
        Mock::given(method("POST")).and(path("/token"))
            .and(body_string_contains("refresh_token=down"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server).await;
        let http = reqwest::Client::new();
        let cfg = cfg_at(&server);
        assert_eq!(refresh(&http, &cfg, "bad").await, Err(OAuthError::Other("le fournisseur refuse (HTTP 401, invalid_client)".into())));
        assert_eq!(refresh(&http, &cfg, "down").await, Err(OAuthError::Network));
        let gone = ProviderConfig { token_url: "http://127.0.0.1:1/token".into(), ..cfg };
        assert_eq!(refresh(&http, &gone, "x").await, Err(OAuthError::Network));
    }

    #[tokio::test] async fn rotation_sans_nouveau_jeton() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/token"))
            .and(body_string_contains("grant_type=refresh_token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"access_token": "a2", "expires_in": 60})))
            .mount(&server).await;
        let t = refresh(&reqwest::Client::new(), &cfg_at(&server), "r").await.unwrap();
        assert_eq!((t.access.as_str(), t.refresh), ("a2", None));
    }
}
