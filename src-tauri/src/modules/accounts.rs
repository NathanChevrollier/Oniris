//! Comptes en ligne : commandes de la feuille « Comptes ». Journal : ni adresse, ni jeton, ni secret.
use crate::state::{provider_config, AppState};
use oniris_core::accounts::{self, AccountView};
use oniris_core::events::Topic;
use oniris_core::oauth::{self, pkce, Provider};
use oniris_core::secrets::GOOGLE_CLIENT_SECRET;
use oniris_core::{kv, time_util::now_ms};
use tauri::{AppHandle, Manager, State};

const INTRO_KEY: &str = "accounts_intro_seen";

#[tauri::command]
pub async fn accounts_list(state: State<'_, AppState>) -> Result<Vec<AccountView>, String> {
    let list = accounts::list(&state.db.lock()).map_err(|e| e.to_string())?;
    Ok(list.iter().map(AccountView::from).collect())
}

/// Identifiant client dans les réglages, secret dans le coffre (jamais renvoyé). Identifiant vide : tout est effacé.
#[tauri::command]
pub async fn accounts_set_google_app(app: AppHandle, client_id: String, client_secret: String) -> Result<(), String> {
    let st = app.state::<AppState>();
    let id = client_id.trim().to_string();
    let secret = client_secret.trim();
    if id.is_empty() {
        st.secrets.delete(GOOGLE_CLIENT_SECRET);
    } else {
        if secret.is_empty() || secret.len() > 500 || secret.chars().any(char::is_whitespace) {
            return Err("Secret client invalide.".into());
        }
        st.secrets.set(GOOGLE_CLIENT_SECRET, secret).map_err(|_| "Le coffre Windows a refusé le secret.".to_string())?;
    }
    let mut s = st.settings.read().clone();
    s.google_client_id = id;
    crate::commands::apply_settings(&app, s)?;
    st.events.changed(Topic::Accounts);
    Ok(())
}

#[tauri::command]
pub async fn accounts_set_spotify_app(app: AppHandle, client_id: String) -> Result<(), String> {
    let st = app.state::<AppState>();
    let mut s = st.settings.read().clone();
    s.spotify_client_id = client_id;
    crate::commands::apply_settings(&app, s)?;
    st.events.changed(Topic::Accounts);
    Ok(())
}

/// Navigateur, code, jetons, compte. Le port d'écoute est lié avant l'ouverture du navigateur.
#[tauri::command]
pub async fn accounts_connect(app: AppHandle, provider: String) -> Result<AccountView, String> {
    let provider = Provider::parse(&provider).ok_or("Fournisseur inconnu.")?;
    let st = app.state::<AppState>();
    let cfg = provider_config(&app, provider).ok_or("Configure d'abord l'application (guide ci-dessous).")?;
    let (verifier, challenge) = pkce::new();
    let state = pkce::state();
    let listener = crate::oauth_loopback::listen().await?;
    let url = oauth::auth_url(&cfg, oauth::REDIRECT_URI, &state, &challenge);
    if !url.starts_with("https://") {
        return Err("Adresse de connexion invalide.".into());
    }
    crate::launch::open_url(&url);
    let code = crate::oauth_loopback::serve(listener, &state, crate::oauth_loopback::TIMEOUT).await?;
    let view = accounts::complete_connect(
        &st.db, &st.accounts, &*st.events, &st.account_http, &cfg, &code, &verifier, oauth::REDIRECT_URI, accounts::identity_url(provider),
    )
    .await
    .inspect_err(|e| tracing::warn!("comptes : connexion échouée : {e}"))?;
    tracing::info!("compte {} connecté", view.id);
    Ok(view)
}

#[tauri::command]
pub async fn accounts_remove(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    accounts::remove(&state.db.lock(), &*state.secrets, id).map_err(|e| e.to_string())?;
    state.accounts.forget(id);
    state.events.changed(Topic::Accounts);
    Ok(())
}

#[tauri::command]
pub async fn accounts_set_color(state: State<'_, AppState>, id: i64, color: String) -> Result<(), String> {
    accounts::set_color(&state.db.lock(), id, &color).map_err(|e| e.to_string())?;
    state.events.changed(Topic::Accounts);
    Ok(())
}

/// Vrai une seule fois : au premier lancement sans compte ni identifiant client (la feuille s'ouvre seule).
#[tauri::command]
pub async fn accounts_intro_take(state: State<'_, AppState>) -> Result<bool, String> {
    let seen = kv::get(&state.db.lock(), INTRO_KEY).map_err(|e| e.to_string())?.is_some();
    if seen {
        return Ok(false);
    }
    kv::set(&state.db.lock(), INTRO_KEY, "1", now_ms()).map_err(|e| e.to_string())?;
    let empty = accounts::list(&state.db.lock()).map_err(|e| e.to_string())?.is_empty();
    let s = state.settings.read();
    Ok(empty && s.google_client_id.is_empty() && s.spotify_client_id.is_empty())
}

/// Pages des guides seulement (https, hôtes connus) : le panneau n'ouvre pas une adresse quelconque.
fn guide_url(u: &str) -> Option<String> {
    let url = oniris_core::launch::safe_url(u)?;
    let p = url::Url::parse(&url).ok()?;
    (p.scheme() == "https" && matches!(p.host_str(), Some("console.cloud.google.com" | "developer.spotify.com" | "mail.google.com"))).then_some(url)
}

#[tauri::command]
pub fn accounts_open_guide(url: String) -> Result<(), String> {
    let u = guide_url(&url).ok_or("Adresse non autorisée.")?;
    crate::launch::open_url(&u);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::guide_url;

    #[test]
    fn seules_les_pages_des_guides_s_ouvrent() {
        assert!(guide_url("https://console.cloud.google.com/projectcreate").is_some());
        assert!(guide_url("https://developer.spotify.com/dashboard").is_some());
        assert!(guide_url("http://console.cloud.google.com/").is_none());
        assert!(guide_url("https://console.cloud.google.com.evil.com/").is_none());
        assert!(guide_url("https://example.com/").is_none());
        assert!(guide_url("file:///C:/Windows/notepad.exe").is_none());
    }
}
