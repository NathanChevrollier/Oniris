//! Premier lancement : état enregistré dans `kv` (clé `onboarding`) et consentement au démarrage automatique.
//! Rien n'est écrit dans la clé Run de Windows tant que l'accueil n'est pas terminé avec un choix explicite.
use crate::state::AppState;
use oniris_core::kv;
use oniris_core::time_util::now_ms;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

const KEY: &str = "onboarding";

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Onboarding {
    pub step: u8,
    pub done: bool,
    pub autostart_consent: Option<bool>,
}

pub fn load(c: &rusqlite::Connection) -> Onboarding {
    kv::get(c, KEY).ok().flatten().and_then(|(v, _)| serde_json::from_str(&v).ok()).unwrap_or_default()
}

/// État souhaité de l'entrée Run : `None` = n'y toucher en aucun cas (accueil en cours ou sans choix).
/// Une fois l'accueil terminé avec un choix, le réglage fait foi (« Non » puis coché dans Paramètres → écrit).
pub fn autostart_target(ob: &Onboarding, setting: bool) -> Option<bool> {
    if !ob.done {
        return None;
    }
    ob.autostart_consent.map(|_| setting)
}

/// État de reprise et dossier de projets proposé (s'il existe).
#[derive(Serialize)]
pub struct OnboardingView {
    #[serde(flatten)]
    state: Onboarding,
    suggested_root: Option<String>,
}

fn suggested_root() -> Option<String> {
    let p = std::path::Path::new(&std::env::var_os("USERPROFILE")?).join("Desktop").join("Utilitaires").join("Projets");
    p.is_dir().then(|| p.to_string_lossy().into_owned())
}

#[tauri::command]
pub async fn onboarding_get(state: State<'_, AppState>) -> Result<OnboardingView, String> {
    Ok(OnboardingView { state: load(&state.db.lock()), suggested_root: suggested_root() })
}

#[tauri::command]
pub async fn onboarding_set(app: AppHandle, onboarding: Onboarding) -> Result<(), String> {
    let st = app.state::<AppState>();
    let json = serde_json::to_string(&onboarding).map_err(|e| e.to_string())?;
    kv::set(&st.db.lock(), KEY, &json, now_ms()).map_err(|e| e.to_string())?;
    // Le choix de l'écran 2 règle aussi le réglage ; l'écriture dans Run attend la fin de l'accueil.
    let mut s = st.settings.read().clone();
    if let Some(c) = onboarding.autostart_consent.filter(|c| *c != s.autostart) {
        s.autostart = c;
        crate::commands::apply_settings(&app, s)?;
    }
    let enabled = st.settings.read().autostart;
    crate::tray::sync_autostart(&app, enabled);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ob(done: bool, consent: Option<bool>) -> Onboarding {
        Onboarding { step: 5, done, autostart_consent: consent }
    }
    #[test]
    fn rien_n_est_ecrit_sans_fin_d_accueil_ni_consentement() {
        assert_eq!(autostart_target(&ob(false, Some(true)), true), None);
        assert_eq!(autostart_target(&ob(true, None), true), None);
        assert_eq!(autostart_target(&Onboarding::default(), true), None);
    }
    #[test]
    fn oui_ecrit_non_retire() {
        assert_eq!(autostart_target(&ob(true, Some(true)), true), Some(true));
        assert_eq!(autostart_target(&ob(true, Some(false)), false), Some(false));
        assert_eq!(autostart_target(&ob(true, Some(true)), false), Some(false)); // réglage décoché plus tard
        assert_eq!(autostart_target(&ob(true, Some(false)), true), Some(true)); // « Non » puis coché plus tard
    }
    #[test]
    fn etat_absent_ou_abime_vaut_defaut() {
        let mut c = rusqlite::Connection::open_in_memory().unwrap();
        oniris_core::migrate(&mut c, oniris_core::CORE_MIGRATIONS).unwrap();
        assert_eq!(load(&c), Onboarding::default());
        kv::set(&c, KEY, "{\"step\":3,\"done\":false,\"autostart_consent\":false,\"x\":1}", 1).unwrap();
        assert_eq!(load(&c), Onboarding { step: 3, done: false, autostart_consent: Some(false) });
        kv::set(&c, KEY, "pas du json", 2).unwrap();
        assert_eq!(load(&c), Onboarding::default());
    }
}
