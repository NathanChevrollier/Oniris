//! Météo : lecture du cache (instantanée) et actualisation en tâche de fond. Jamais attendue à l'ouverture du panneau.
//! Journal : ni ville ni coordonnées, seulement les erreurs (sans URL).
use crate::state::AppState;
use oniris_core::events::Topic;
use oniris_core::kv;
use oniris_core::weather::tools::{LAST_KEY, PLACE_KEY};
use oniris_core::weather::{self, Place, Weather};
use oniris_core::CoreError;
use parking_lot::Mutex;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use tauri::{AppHandle, Manager};

/// Dernier échec (ms), 0 si aucun : en mémoire seulement, un redémarrage permet de réessayer.
static LAST_FAIL_MS: AtomicI64 = AtomicI64::new(0);
/// Une seule actualisation à la fois.
static BUSY: AtomicBool = AtomicBool::new(false);
/// Ville changée pendant une actualisation : elle doit être refaite une fois pour la nouvelle ville.
static PENDING: AtomicBool = AtomicBool::new(false);
/// Nature du dernier échec (`not_found`, `offline`, `invalid`), effacée au premier succès.
static LAST_ERR: Mutex<Option<&'static str>> = Mutex::new(None);

/// Prend le verrou `busy` ; il se relâche au `drop`, même si l'actualisation panique.
struct BusyGuard<'a>(&'a AtomicBool);
impl<'a> BusyGuard<'a> {
    fn acquire(busy: &'a AtomicBool) -> Option<Self> { (!busy.swap(true, Ordering::SeqCst)).then(|| Self(busy)) }
}
impl Drop for BusyGuard<'_> {
    fn drop(&mut self) { self.0.store(false, Ordering::SeqCst); }
}

/// Demande une actualisation pour la nouvelle ville : `true` = lancer maintenant ; `false` = une actualisation
/// tourne, elle se relancera seule (`PENDING`).
fn request_rerun(busy: &AtomicBool, pending: &AtomicBool) -> bool {
    pending.store(true, Ordering::SeqCst);
    !busy.load(Ordering::SeqCst)
}

fn error_kind(e: &CoreError) -> &'static str {
    match e {
        CoreError::NotFound(_) => "not_found",
        CoreError::Invalid(m) if m.starts_with("réseau") => "offline",
        _ => "invalid",
    }
}

/// Réponse de `weather_get` : la dernière météo et, sans donnée, la nature du dernier échec.
#[derive(Serialize)]
pub struct WeatherState { pub weather: Option<Weather>, pub error: Option<&'static str> }

fn now_ms() -> i64 { chrono::Utc::now().timestamp_millis() }

/// Appelé par `windows::open_panel` : lance une actualisation si la ville est réglée et les données trop vieilles.
pub fn on_panel_open(app: &AppHandle) {
    let state = app.state::<AppState>();
    if state.settings.read().weather_city.is_empty() {
        return;
    }
    let last_ok = kv::get(&state.db.lock(), LAST_KEY).ok().flatten().map(|(_, ms)| ms);
    let fail = LAST_FAIL_MS.load(Ordering::SeqCst);
    if weather::should_refresh(last_ok, (fail != 0).then_some(fail), now_ms()) {
        spawn_refresh(app);
    }
}

/// Lance l'actualisation sans l'attendre.
pub fn spawn_refresh(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = refresh(&app).await {
            tracing::warn!("météo : {e}");
        }
    });
}

/// Ville changée : le lieu et la météo en cache ne valent plus. Puis actualisation forcée (relancée si une tourne déjà).
pub fn city_changed(app: &AppHandle, city: &str) {
    let state = app.state::<AppState>();
    {
        let db = state.db.lock();
        let _ = kv::delete(&db, PLACE_KEY);
        let _ = kv::delete(&db, LAST_KEY);
    }
    LAST_FAIL_MS.store(0, Ordering::SeqCst);
    *LAST_ERR.lock() = None;
    state.events.changed(Topic::Weather);
    if !city.is_empty() && request_rerun(&BUSY, &PENDING) {
        spawn_refresh(app);
    }
}

async fn refresh(app: &AppHandle) -> Result<Option<Weather>, String> {
    let mut out = Ok(None);
    loop {
        let Some(guard) = BusyGuard::acquire(&BUSY) else { return out };
        PENDING.store(false, Ordering::SeqCst);
        let r = do_refresh(app).await;
        drop(guard);
        match &r {
            Ok(Some(_)) => { LAST_FAIL_MS.store(0, Ordering::SeqCst); *LAST_ERR.lock() = None; }
            Ok(None) => {}
            Err(e) => {
                LAST_FAIL_MS.store(now_ms(), Ordering::SeqCst);
                *LAST_ERR.lock() = Some(error_kind(e));
                app.state::<AppState>().events.changed(Topic::Weather); // l'écran affiche la cause
            }
        }
        out = r.map_err(|e| e.to_string());
        if !PENDING.swap(false, Ordering::SeqCst) {
            return out;
        }
    }
}

async fn do_refresh(app: &AppHandle) -> Result<Option<Weather>, CoreError> {
    let state = app.state::<AppState>();
    let city = state.settings.read().weather_city.clone();
    if city.is_empty() {
        return Ok(None);
    }
    let cached: Option<Place> = kv::get(&state.db.lock(), PLACE_KEY)
        .ok()
        .flatten()
        .and_then(|(j, _)| serde_json::from_str(&j).ok());
    let (place, w) = weather::fetch(&state.http, &city, cached).await?;
    if state.settings.read().weather_city != city {
        return Ok(None); // ville changée pendant la requête : résultat périmé
    }
    {
        let db = state.db.lock();
        let now = now_ms();
        kv::set(&db, PLACE_KEY, &serde_json::to_string(&place)?, now)?;
        kv::set(&db, LAST_KEY, &serde_json::to_string(&w)?, now)?;
    }
    state.events.changed(Topic::Weather);
    Ok(Some(w))
}

/// Cache uniquement : aucune requête.
#[tauri::command]
pub fn weather_get(state: tauri::State<AppState>) -> Result<WeatherState, String> {
    let weather = match kv::get(&state.db.lock(), LAST_KEY).map_err(|e| e.to_string())? {
        Some((json, _)) => Some(serde_json::from_str(&json).map_err(|e| e.to_string())?),
        None => None,
    };
    Ok(WeatherState { weather, error: *LAST_ERR.lock() })
}

/// Actualisation manuelle, sans tenir compte des 30 min.
#[tauri::command]
pub async fn weather_refresh(app: AppHandle) -> Result<Option<Weather>, String> {
    refresh(&app).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ville_changee_pendant_une_actualisation_la_relance_une_fois() {
        let (busy, pending) = (AtomicBool::new(false), AtomicBool::new(false));
        assert!(request_rerun(&busy, &pending), "rien en cours : lancer");
        let g = BusyGuard::acquire(&busy).unwrap();
        assert!(BusyGuard::acquire(&busy).is_none(), "une seule à la fois");
        pending.store(false, Ordering::SeqCst);
        assert!(!request_rerun(&busy, &pending), "en cours : pas de seconde tâche");
        assert!(pending.load(Ordering::SeqCst), "mais relance demandée");
        drop(g);
        assert!(pending.swap(false, Ordering::SeqCst), "la fin de l'actualisation relance");
        assert!(!pending.swap(false, Ordering::SeqCst), "une seule fois");
    }

    #[test]
    fn un_panic_ne_bloque_pas_le_verrou() {
        let busy = AtomicBool::new(false);
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _g = BusyGuard::acquire(&busy).unwrap();
            panic!("échec simulé");
        }));
        assert!(r.is_err());
        assert!(BusyGuard::acquire(&busy).is_some());
    }

    #[test]
    fn nature_des_echecs() {
        assert_eq!(error_kind(&CoreError::NotFound("ville introuvable".into())), "not_found");
        assert_eq!(error_kind(&CoreError::Invalid("réseau : timeout".into())), "offline");
        assert_eq!(error_kind(&CoreError::Invalid("json".into())), "invalid");
    }
}
