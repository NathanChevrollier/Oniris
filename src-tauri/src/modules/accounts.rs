//! Comptes en ligne : commandes de la feuille « Comptes ». Journal : ni adresse, ni jeton, ni secret.
use crate::state::{provider_config, AppState};
use oniris_core::accounts::{self, AccountView};
use oniris_core::events::Topic;
use oniris_core::oauth::{self, pkce, Provider};
use oniris_core::secrets::GOOGLE_CLIENT_SECRET;
use oniris_core::{kv, time_util::now_ms};
use parking_lot::Mutex;
use tauri::{AppHandle, Manager, State};
use tokio::sync::oneshot;

const INTRO_KEY: &str = "accounts_intro_seen";

#[tauri::command]
pub async fn accounts_list(state: State<'_, AppState>) -> Result<Vec<AccountView>, String> {
    let list = accounts::list(&state.db.lock()).map_err(|e| e.to_string())?;
    Ok(list.iter().map(AccountView::from).collect())
}

/// Identifiant client dans les réglages, secret dans le coffre (jamais renvoyé). Identifiant vide : tout est effacé.
/// Secret laissé vide alors qu'il existe déjà : on le garde, seul l'identifiant change.
#[tauri::command]
pub async fn accounts_set_google_app(app: AppHandle, client_id: String, client_secret: String) -> Result<(), String> {
    let st = app.state::<AppState>();
    let id = client_id.trim().to_string();
    let secret = client_secret.trim();
    let mut s = st.settings.read().clone();
    let old = s.clone();
    if !id.is_empty() {
        if secret.is_empty() && st.secrets.get(GOOGLE_CLIENT_SECRET).is_none() {
            return Err("Colle aussi le secret client.".into());
        }
        if !secret.is_empty() && (secret.len() > 500 || secret.chars().any(char::is_whitespace)) {
            return Err("Le secret client est invalide : colle-le tel quel, sans espace.".into());
        }
    }
    s.google_client_id = id.clone();
    crate::commands::apply_settings(&app, s)?;
    if id.is_empty() {
        st.secrets.delete(GOOGLE_CLIENT_SECRET);
    } else if !secret.is_empty() && st.secrets.set(GOOGLE_CLIENT_SECRET, secret).is_err() {
        // Pas d'identifiant sans secret : on revient aux réglages précédents.
        let _ = crate::commands::apply_settings(&app, old);
        return Err("Le coffre Windows a refusé le secret, réessaie.".into());
    }
    st.events.changed(Topic::Accounts);
    Ok(())
}

#[tauri::command]
pub async fn accounts_set_spotify_app(app: AppHandle, client_id: String) -> Result<(), String> {
    let st = app.state::<AppState>();
    let mut s = st.settings.read().clone();
    s.spotify_client_id = client_id.trim().to_string();
    crate::commands::apply_settings(&app, s)?;
    st.events.changed(Topic::Accounts);
    Ok(())
}

/// Une seule connexion à la fois ; `cancel` la termine et libère le port.
#[derive(Default)]
pub struct Flight(Mutex<Option<oneshot::Sender<()>>>);

/// Libère la place à la fin (réussite, échec ou annulation). À déclarer AVANT le récepteur : celui-ci est
/// alors détruit en premier, et on ne retire que son propre émetteur (jamais celui d'une connexion suivante).
pub struct Permit<'a>(&'a Flight);

impl Flight {
    pub fn start(&self) -> Result<(Permit<'_>, oneshot::Receiver<()>), String> {
        let mut g = self.0.lock();
        if g.is_some() {
            return Err("Une connexion est déjà en cours.".into());
        }
        let (tx, rx) = oneshot::channel();
        *g = Some(tx);
        Ok((Permit(self), rx))
    }

    /// Vrai si une connexion était en cours.
    pub fn cancel(&self) -> bool {
        self.0.lock().take().is_some_and(|tx| tx.send(()).is_ok())
    }
}

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        self.0 .0.lock().take_if(|tx| tx.is_closed());
    }
}

/// Attend le code ; l'annulation abandonne l'écoute (l'écouteur est détruit, le port libéré).
async fn wait(listener: tokio::net::TcpListener, state: &str, timeout: std::time::Duration, cancel: oneshot::Receiver<()>) -> Result<String, String> {
    tokio::select! {
        r = crate::oauth_loopback::serve(listener, state, timeout) => r,
        _ = cancel => Err("Connexion annulée.".into()),
    }
}

/// Navigateur, code, jetons, compte. Le port d'écoute est lié avant l'ouverture du navigateur.
#[tauri::command]
pub async fn accounts_connect(app: AppHandle, provider: String) -> Result<AccountView, String> {
    let provider = Provider::parse(&provider).ok_or("Fournisseur inconnu.")?;
    let st = app.state::<AppState>();
    let (_permit, cancel) = st.flight.start()?;
    let cfg = provider_config(&app, provider).ok_or("Configure d'abord l'application (guide ci-dessous).")?;
    let (verifier, challenge) = pkce::new();
    let state = pkce::state();
    let listener = crate::oauth_loopback::listen().await?;
    let url = oauth::auth_url(&cfg, oauth::REDIRECT_URI, &state, &challenge);
    if !url.starts_with("https://") {
        return Err("Adresse de connexion invalide.".into());
    }
    crate::launch::open_url(&url);
    let code = wait(listener, &state, crate::oauth_loopback::TIMEOUT, cancel).await?;
    let view = accounts::complete_connect(
        &st.db, &st.accounts, &*st.events, &st.account_http, &cfg, &code, &verifier, oauth::REDIRECT_URI, accounts::identity_url(provider),
    )
    .await
    .inspect_err(|e| tracing::warn!("comptes : connexion échouée : {e}"))?;
    tracing::info!("compte {} connecté", view.id);
    // Fournisseur seulement : jamais l'adresse.
    crate::tray::notify(&app, "Compte connecté", if provider == Provider::Google { "Gmail" } else { "Spotify" });
    Ok(view)
}

/// Annule la connexion en cours (libère le port) ; sans effet s'il n'y en a pas.
#[tauri::command]
pub fn accounts_cancel(state: State<'_, AppState>) {
    state.flight.cancel();
}

#[tauri::command]
pub async fn accounts_remove(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    accounts::remove(&state.db.lock(), &*state.secrets, id).map_err(|e| e.to_string())?;
    state.accounts.forget(id);
    state.events.changed(Topic::Accounts);
    state.events.changed(Topic::Mail); // ses mails ont quitté le cache
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
    use super::{guide_url, wait, Flight};

    #[test]
    fn seules_les_pages_des_guides_s_ouvrent() {
        assert!(guide_url("https://console.cloud.google.com/projectcreate").is_some());
        assert!(guide_url("https://developer.spotify.com/dashboard").is_some());
        assert!(guide_url("http://console.cloud.google.com/").is_none());
        assert!(guide_url("https://console.cloud.google.com.evil.com/").is_none());
        assert!(guide_url("https://example.com/").is_none());
        assert!(guide_url("file:///C:/Windows/notepad.exe").is_none());
        assert!(guide_url("https://console.cloud.google.com@evil.com/").is_none());
        assert!(guide_url("https://a@console.cloud.google.com/").is_none());
    }

    #[test]
    fn une_seule_connexion_a_la_fois() {
        let f = Flight::default();
        let first = f.start().unwrap();
        assert_eq!(f.start().err().as_deref(), Some("Une connexion est déjà en cours."));
        assert!(f.cancel());
        assert!(!f.cancel(), "rien à annuler une seconde fois");
        drop(first);
        assert!(f.start().is_ok(), "place libérée");
    }

    #[test]
    fn la_fin_d_une_connexion_ne_retire_pas_la_suivante() {
        let f = Flight::default();
        let (p1, rx1) = f.start().unwrap();
        assert!(f.cancel());
        drop(rx1);
        let second = f.start().unwrap();
        drop(p1); // la première se termine après le démarrage de la seconde
        assert!(f.start().is_err(), "la seconde garde sa place");
        drop(second);
    }

    #[tokio::test]
    async fn annuler_libere_le_port() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let f = Flight::default();
        let (_p, rx) = f.start().unwrap();
        let task = tokio::spawn(async move { wait(listener, "s", std::time::Duration::from_secs(30), rx).await });
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        assert!(f.cancel());
        assert_eq!(task.await.unwrap().unwrap_err(), "Connexion annulée.");
        assert!(tokio::net::TcpListener::bind(("127.0.0.1", port)).await.is_ok(), "port libre après annulation");
    }
}
