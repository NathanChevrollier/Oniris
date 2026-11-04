//! Météo : lecture du cache (instantanée) et actualisation en tâche de fond. Jamais attendue à l'ouverture du panneau.
//! Journal : ni ville ni coordonnées, seulement les erreurs (sans URL).
use crate::state::AppState;
use oniris_core::events::Topic;
use oniris_core::kv;
use oniris_core::weather::tools::{LAST_KEY, PLACE_KEY};
use oniris_core::weather::{self, Place, Weather};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use tauri::{AppHandle, Manager};

/// Dernier échec (ms), 0 si aucun : en mémoire seulement, un redémarrage permet de réessayer.
static LAST_FAIL_MS: AtomicI64 = AtomicI64::new(0);
/// Une seule actualisation à la fois.
static BUSY: AtomicBool = AtomicBool::new(false);

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

/// Ville changée : le lieu en cache ne vaut plus ; ville vidée : plus de donnée. Puis actualisation forcée.
pub fn city_changed(app: &AppHandle, city: &str) {
    let state = app.state::<AppState>();
    {
        let db = state.db.lock();
        let _ = kv::delete(&db, PLACE_KEY);
        if city.is_empty() {
            let _ = kv::delete(&db, LAST_KEY);
        }
    }
    LAST_FAIL_MS.store(0, Ordering::SeqCst);
    state.events.changed(Topic::Weather);
    if !city.is_empty() {
        spawn_refresh(app);
    }
}

async fn refresh(app: &AppHandle) -> Result<Option<Weather>, String> {
    if BUSY.swap(true, Ordering::SeqCst) {
        return Ok(None);
    }
    let r = do_refresh(app).await;
    BUSY.store(false, Ordering::SeqCst);
    if r.is_err() {
        LAST_FAIL_MS.store(now_ms(), Ordering::SeqCst);
    }
    r
}

async fn do_refresh(app: &AppHandle) -> Result<Option<Weather>, String> {
    let state = app.state::<AppState>();
    let city = state.settings.read().weather_city.clone();
    if city.is_empty() {
        return Ok(None);
    }
    let cached: Option<Place> = kv::get(&state.db.lock(), PLACE_KEY)
        .ok()
        .flatten()
        .and_then(|(j, _)| serde_json::from_str(&j).ok());
    let (place, w) = weather::fetch(&state.http, &city, cached).await.map_err(|e| e.to_string())?;
    if state.settings.read().weather_city != city {
        return Ok(None); // ville changée pendant la requête : résultat périmé
    }
    {
        let db = state.db.lock();
        let (now, p, w_json) = (now_ms(), serde_json::to_string(&place), serde_json::to_string(&w));
        // Lieu enregistré seulement après un géocodage réussi (la prévision l'a aussi été ici).
        kv::set(&db, PLACE_KEY, &p.map_err(|e| e.to_string())?, now).map_err(|e| e.to_string())?;
        kv::set(&db, LAST_KEY, &w_json.map_err(|e| e.to_string())?, now).map_err(|e| e.to_string())?;
    }
    LAST_FAIL_MS.store(0, Ordering::SeqCst);
    state.events.changed(Topic::Weather);
    Ok(Some(w))
}

/// Cache uniquement : aucune requête.
#[tauri::command]
pub fn weather_get(state: tauri::State<AppState>) -> Result<Option<Weather>, String> {
    let Some((json, _)) = kv::get(&state.db.lock(), LAST_KEY).map_err(|e| e.to_string())? else { return Ok(None) };
    serde_json::from_str(&json).map(Some).map_err(|e| e.to_string())
}

/// Actualisation manuelle, sans tenir compte des 30 min.
#[tauri::command]
pub async fn weather_refresh(app: AppHandle) -> Result<Option<Weather>, String> {
    refresh(&app).await
}
