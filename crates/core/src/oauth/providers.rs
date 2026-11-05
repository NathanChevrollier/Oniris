//! Fournisseurs OAuth : points d'accès, scopes et paramètres propres.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider { Google, Spotify }

impl Provider {
    pub fn as_str(self) -> &'static str {
        match self { Provider::Google => "google", Provider::Spotify => "spotify" }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s { "google" => Some(Provider::Google), "spotify" => Some(Provider::Spotify), _ => None }
    }
}

#[derive(Clone)]
pub struct ProviderConfig {
    pub provider: Provider,
    pub auth_url: String,
    /// Injectable : les tests le pointent sur un faux serveur `127.0.0.1`.
    pub token_url: String,
    pub scopes: &'static [&'static str],
    /// Paramètres en plus dans l'URL d'autorisation.
    pub extra: &'static [(&'static str, &'static str)],
    pub client_id: String,
    pub client_secret: Option<String>,
}

pub const GOOGLE_SCOPES: &[&str] =
    &["openid", "email", "https://www.googleapis.com/auth/gmail.modify", "https://www.googleapis.com/auth/calendar.readonly"];

pub const SPOTIFY_SCOPES: &[&str] = &[
    "user-read-playback-state",
    "user-modify-playback-state",
    "user-read-currently-playing",
    "playlist-read-private",
    "playlist-read-collaborative",
];

/// Application « de bureau » : le secret n'est pas confidentiel, mais traité comme un secret.
/// `access_type=offline` et `prompt=consent` : Google ne donne le jeton d'actualisation qu'ainsi.
pub fn google(client_id: &str, client_secret: Option<String>) -> ProviderConfig {
    ProviderConfig {
        provider: Provider::Google,
        auth_url: "https://accounts.google.com/o/oauth2/v2/auth".into(),
        token_url: "https://oauth2.googleapis.com/token".into(),
        scopes: GOOGLE_SCOPES,
        extra: &[("access_type", "offline"), ("prompt", "consent")],
        client_id: client_id.into(),
        client_secret,
    }
}

/// Sans secret : PKCE seul.
pub fn spotify(client_id: &str) -> ProviderConfig {
    ProviderConfig {
        provider: Provider::Spotify,
        auth_url: "https://accounts.spotify.com/authorize".into(),
        token_url: "https://accounts.spotify.com/api/token".into(),
        scopes: SPOTIFY_SCOPES,
        extra: &[],
        client_id: client_id.into(),
        client_secret: None,
    }
}
