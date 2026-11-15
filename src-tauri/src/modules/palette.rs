//! Palette : taux de change (BCE via frankfurter.app, demandés seulement quand une conversion de devise est tapée,
//! gardés 12 h en mémoire) et « Rechercher sur le web » (navigateur par défaut, rien n'est envoyé avant le clic).
use crate::state::AppState;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tauri::{AppHandle, State};

const RATES_URL: &str = "https://api.frankfurter.app/latest?from=EUR";
const RATES_TTL: Duration = Duration::from_secs(12 * 3600);
// 300 caractères encodés (6 octets par accent au pire) tiennent sous la limite de `safe_url`.
const MAX_QUERY: usize = 300;

static RATES: Mutex<Option<(Instant, HashMap<String, f64>)>> = Mutex::new(None);

#[derive(serde::Deserialize)]
struct Latest { rates: HashMap<String, f64> }

/// Taux pour 1 EUR (EUR compris), codes ISO à 3 lettres et valeurs finies positives seulement.
fn parse_rates(body: &str) -> Result<HashMap<String, f64>, String> {
    let l: Latest = serde_json::from_str(body).map_err(|_| "réponse des taux illisible".to_string())?;
    let mut r: HashMap<String, f64> = l.rates.into_iter()
        .filter(|(k, v)| k.len() == 3 && k.bytes().all(|b| b.is_ascii_uppercase()) && v.is_finite() && *v > 0.0)
        .collect();
    if r.is_empty() {
        return Err("aucun taux reçu".into());
    }
    r.insert("EUR".into(), 1.0);
    Ok(r)
}

/// Taux de la BCE pour 1 EUR ; une seule requête toutes les 12 h au plus (la requête ne contient rien de ce qui est tapé).
#[tauri::command]
pub async fn palette_rates(state: State<'_, AppState>) -> Result<HashMap<String, f64>, String> {
    if let Some((at, r)) = RATES.lock().as_ref() {
        if at.elapsed() < RATES_TTL {
            return Ok(r.clone());
        }
    }
    let http = state.http.as_ref().ok_or("client HTTP indisponible")?;
    let body = http.get(RATES_URL).send().await.and_then(|r| r.error_for_status())
        .map_err(|_| "taux de change indisponibles (connexion ?)".to_string())?
        .text().await.map_err(|_| "taux de change indisponibles".to_string())?;
    let r = parse_rates(&body)?;
    *RATES.lock() = Some((Instant::now(), r.clone()));
    Ok(r)
}

/// URL de recherche du moteur choisi (`google` par défaut), requête encodée.
fn search_url(engine: &str, q: &str) -> Option<String> {
    let base = match engine {
        "duckduckgo" => "https://duckduckgo.com/",
        "bing" => "https://www.bing.com/search",
        _ => "https://www.google.com/search",
    };
    let q: String = q.trim().chars().take(MAX_QUERY).collect();
    if q.is_empty() {
        return None;
    }
    let u = reqwest::Url::parse_with_params(base, &[("q", q)]).ok()?;
    oniris_core::launch::safe_url(u.as_str())
}

/// Ouvre la recherche dans le navigateur par défaut, puis ferme le panneau.
#[tauri::command]
pub fn palette_web_search(app: AppHandle, state: State<'_, AppState>, q: String) -> Result<(), String> {
    let engine = state.settings.read().web_search_engine.clone();
    let url = search_url(&engine, &q).ok_or("recherche invalide")?;
    crate::launch::open_url(&url);
    crate::windows::close_panel(&app).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn taux_filtres_et_eur_ajoute() {
        let r = parse_rates(r#"{"amount":1.0,"base":"EUR","date":"2026-11-13","rates":{"USD":1.08,"GBP":0.85,"bad":2.0,"JPY":-1}}"#).unwrap();
        assert_eq!(r.get("USD"), Some(&1.08));
        assert_eq!(r.get("EUR"), Some(&1.0));
        assert!(!r.contains_key("bad") && !r.contains_key("JPY"));
        assert!(parse_rates("<html>").is_err());
        assert!(parse_rates(r#"{"rates":{}}"#).is_err());
    }

    #[test]
    fn url_de_recherche_encodee_par_moteur() {
        assert_eq!(search_url("google", "chat & chien").unwrap(), "https://www.google.com/search?q=chat+%26+chien");
        assert!(search_url("duckduckgo", "x").unwrap().starts_with("https://duckduckgo.com/?q=x"));
        assert!(search_url("bing", "x").unwrap().starts_with("https://www.bing.com/search?q=x"));
        assert!(search_url("inconnu", "x").unwrap().starts_with("https://www.google.com/"));
        assert_eq!(search_url("google", "   "), None);
        assert!(search_url("google", &"é".repeat(5000)).is_some(), "requête tronquée, pas refusée");
    }
}
