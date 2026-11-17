//! Commandes Tauri de l'agenda. Lecture directe de la base ; création et suppression passent par le courtier.
//! Calendriers : affichage par calendrier, import de fichiers .ics et abonnements en ligne (https, adresses publiques,
//! 5 Mo, relevés au plus toutes les 6 h et seulement quand la carte est affichée). Journal : des nombres, jamais d'URL.
use crate::commands::dispatch_as_user;
use crate::modules::favicons::{fetch, Limits};
use crate::modules::mail;
use crate::state::AppState;
use chrono::{Local, NaiveDate, TimeZone};
use oniris_core::agenda::calendars::{self, Calendar, Kind};
use oniris_core::agenda::ics::{self, Parsed};
use oniris_core::agenda::{self, Event};
use oniris_core::events::Topic;
use oniris_core::time_util::now_ms;
use oniris_core::tools::ToolOutcome;
use oniris_core::CoreError;
use serde_json::json;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Manager, State};
use url::Url;

const SUBSCRIPTION_MAX_AGE_MS: i64 = 6 * 3_600_000;
static REFRESHING: AtomicBool = AtomicBool::new(false);

#[tauri::command(async)]
pub fn agenda_between(state: State<AppState>, from_ms: i64, to_ms: i64) -> Result<Vec<Event>, String> {
    agenda::between(&state.db.lock(), from_ms, to_ms).map_err(|e| e.to_string())
}

/// Début du mois (heure locale) en ms.
fn month_start_ms(year: i32, month: u32) -> Option<i64> {
    let d = NaiveDate::from_ymd_opt(year, month, 1)?.and_hms_opt(0, 0, 0)?;
    Some(Local.from_local_datetime(&d).earliest()?.timestamp_millis())
}

#[tauri::command(async)]
pub fn agenda_month_days(state: State<AppState>, year: i32, month: u32) -> Result<Vec<u32>, String> {
    let (ny, nm) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
    let (Some(from), Some(to)) = (month_start_ms(year, month), month_start_ms(ny, nm)) else {
        return Err("mois invalide".into());
    };
    agenda::days_with_events(&state.db.lock(), from, to).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn agenda_create(app: AppHandle, text: String) -> ToolOutcome {
    dispatch_as_user(&app, "agenda.create_event", json!({ "text": text })).await
}

/// Passe par la confirmation : la suppression n'a lieu qu'après l'accord dans la carte du panneau.
#[tauri::command]
pub async fn agenda_delete(app: AppHandle, id: i64) -> ToolOutcome {
    dispatch_as_user(&app, "agenda.delete_event", json!({ "id": id })).await
}

fn msg(e: CoreError) -> String {
    match e {
        CoreError::Invalid(m) | CoreError::NotFound(m) => m,
        e => e.to_string(),
    }
}

#[derive(serde::Serialize)]
pub struct ImportReport { name: String, events: usize, truncated: bool }

#[tauri::command(async)]
pub fn agenda_calendars(state: State<AppState>) -> Result<Vec<Calendar>, String> {
    calendars::list(&state.db.lock()).map_err(msg)
}

/// Un agenda Google réaffiché est relevé tout de suite ; masqué, il disparaît de l'affichage et n'est plus relevé.
#[tauri::command(async)]
pub fn agenda_calendar_set_visible(state: State<AppState>, id: i64, visible: bool) -> Result<(), String> {
    let kind = calendars::set_visible(&state.db.lock(), id, visible).map_err(msg)?;
    state.events.changed(Topic::Agenda);
    if kind == Kind::Google && visible {
        mail::sync_now();
    }
    Ok(())
}

/// Calendrier importé ou abonnement ; un agenda Google se masque, il ne se supprime pas.
#[tauri::command(async)]
pub fn agenda_calendar_delete(state: State<AppState>, id: i64) -> Result<(), String> {
    calendars::delete(&state.db.lock(), id).map_err(msg)?;
    state.events.changed(Topic::Agenda);
    Ok(())
}

fn parse_now(text: &str) -> Result<Parsed, String> {
    let (from, to) = ics::window(now_ms());
    ics::parse(text, from, to).map_err(msg)
}

fn store(state: &AppState, kind: Kind, name: String, url: Option<&str>, parsed: Parsed) -> Result<ImportReport, String> {
    calendars::add_local(&state.db.lock(), kind, &name, url, &parsed.events, now_ms()).map_err(msg)?;
    tracing::info!(events = parsed.events.len(), truncated = parsed.truncated, "agenda : calendrier {} ajouté", if url.is_some() { "en ligne" } else { "importé" });
    state.events.changed(Topic::Agenda);
    Ok(ImportReport { name, events: parsed.events.len(), truncated: parsed.truncated })
}

/// Fichier .ics choisi dans la boîte de dialogue ou déposé sur la carte : devient un calendrier local, jamais envoyé à Google.
#[tauri::command]
pub async fn agenda_import_ics(state: State<'_, AppState>, path: String) -> Result<ImportReport, String> {
    let p = std::path::PathBuf::from(path);
    if p.extension().is_none_or(|e| !e.eq_ignore_ascii_case("ics")) {
        return Err("choisis un fichier .ics".into());
    }
    let stem: Option<String> = p.file_stem().map(|s| s.to_string_lossy().chars().take(80).collect());
    let parsed = tauri::async_runtime::spawn_blocking(move || {
        if std::fs::metadata(&p).map_err(|_| "fichier illisible".to_string())?.len() > ics::MAX_BYTES as u64 {
            return Err("fichier trop volumineux (5 Mo au maximum)".to_string());
        }
        let bytes = std::fs::read(&p).map_err(|_| "fichier illisible".to_string())?;
        parse_now(&String::from_utf8_lossy(&bytes))
    }).await.map_err(|e| e.to_string())??;
    let name = parsed.name.clone().or(stem).unwrap_or_else(|| "Calendrier importé".into());
    store(&state, Kind::File, name, None, parsed)
}

/// Lien https (ou webcal://, réécrit en https) sans identifiants ; le reste (IP privée, redirections) est vérifié au téléchargement.
fn subscription_url(s: &str) -> Option<Url> {
    let s = s.trim();
    let s = match s.get(..9) { Some(p) if p.eq_ignore_ascii_case("webcal://") => format!("https://{}", &s[9..]), _ => s.to_string() };
    let u = Url::parse(&oniris_core::launch::safe_url(&s)?).ok()?;
    (u.scheme() == "https" && u.host_str().is_some()).then_some(u)
}

/// Seule requête envoyée : un GET vers l'adresse donnée (et ses redirections https publiques), sans cookie.
async fn download(url: &Url) -> Result<Parsed, String> {
    let lim = Limits { max_bytes: ics::MAX_BYTES, timeout: Duration::from_secs(30), https_only: true };
    let (_, body) = fetch(url, "", &AtomicU32::new(4), &lim).await
        .ok_or_else(|| "téléchargement impossible (adresse injoignable, non publique ou fichier de plus de 5 Mo)".to_string())?;
    tauri::async_runtime::spawn_blocking(move || parse_now(&String::from_utf8_lossy(&body))).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn agenda_subscribe(state: State<'_, AppState>, url: String) -> Result<ImportReport, String> {
    let u = subscription_url(&url).ok_or_else(|| "adresse invalide : colle un lien https:// ou webcal:// vers un calendrier .ics".to_string())?;
    let parsed = download(&u).await?;
    let name = parsed.name.clone().unwrap_or_else(|| u.host_str().unwrap_or("Calendrier en ligne").to_string());
    store(&state, Kind::Url, name, Some(u.as_str()), parsed)
}

struct RefreshGuard;
impl Drop for RefreshGuard {
    fn drop(&mut self) { REFRESHING.store(false, Ordering::SeqCst); }
}

/// Appelé par la carte quand le panneau s'ouvre : relève les abonnements visibles vieux de plus de 6 h.
/// Un échec est noté (les anciens événements restent) et réessayé 6 h plus tard.
#[tauri::command]
pub async fn agenda_refresh(app: AppHandle) -> Result<(), String> {
    if REFRESHING.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    let _guard = RefreshGuard;
    let st = app.state::<AppState>();
    let due = calendars::stale_subscriptions(&st.db.lock(), now_ms(), SUBSCRIPTION_MAX_AGE_MS).map_err(msg)?;
    for (id, url) in &due {
        let r = match Url::parse(url) {
            Ok(u) => download(&u).await,
            Err(_) => Err("adresse invalide".into()),
        };
        let stored = calendars::store_fetch(&st.db.lock(), *id, r.as_ref().map(|p| p.events.as_slice()).map_err(String::as_str), now_ms());
        if let Err(e) = stored {
            tracing::warn!("agenda : abonnement {id} : relève non enregistrée : {e}");
        }
    }
    if !due.is_empty() {
        tracing::info!(count = due.len(), "agenda : abonnements relevés");
        st.events.changed(Topic::Agenda);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn adresse_d_abonnement() {
        assert_eq!(subscription_url(" webcal://ecole.example.fr/cal.ics ").unwrap().as_str(), "https://ecole.example.fr/cal.ics");
        assert!(subscription_url("https://calendar.google.com/calendar/ical/x/basic.ics").is_some());
        for bad in ["http://ecole.example.fr/cal.ics", "file:///C:/x.ics", "https://user:pw@x.fr/a.ics", "mailto:a@b.fr", "javascript:alert(1)", "webcal:x", ""] {
            assert!(subscription_url(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn debut_de_mois() {
        assert!(month_start_ms(2026, 11).unwrap() < month_start_ms(2026, 12).unwrap());
        assert!(month_start_ms(2026, 13).is_none());
    }
}
