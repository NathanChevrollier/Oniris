//! Météo Open-Meteo : analyse pure des réponses (`parse_*`), politique d'actualisation et appel réseau (`fetch`).
pub mod codes;
pub mod tools;

use crate::CoreError;
use chrono::Timelike;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Place { pub name: String, pub lat: f64, pub lon: f64 }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Weather {
    pub place: String,
    pub temp_c: f32,
    pub code: u8,
    pub label: String,
    pub icon: String,
    pub today_min: f32,
    pub today_max: f32,
    pub rain_from_hour: Option<u8>,
    pub fetched_ms: i64,
}

const MIN_MS: i64 = 60_000;
const FRESH_MS: i64 = 30 * MIN_MS;
const RETRY_MS: i64 = 10 * MIN_MS;
const RAIN_PCT: f64 = 60.0;
const RAIN_WINDOW_H: i64 = 12;
const GEOCODING_URL: &str = "https://geocoding-api.open-meteo.com/v1/search";
const FORECAST_URL: &str = "https://api.open-meteo.com/v1/forecast";

#[derive(Deserialize)]
struct GeoResponse { #[serde(default)] results: Vec<GeoResult> }
#[derive(Deserialize)]
struct GeoResult { name: String, latitude: f64, longitude: f64 }

pub fn parse_geocoding(json: &str) -> Result<Option<Place>, CoreError> {
    let r: GeoResponse = serde_json::from_str(json)?;
    Ok(r.results.into_iter().next().map(|g| Place { name: g.name, lat: g.latitude, lon: g.longitude }))
}

#[derive(Deserialize)]
struct Forecast { utc_offset_seconds: i64, current: Current, hourly: Hourly, daily: Daily }
#[derive(Deserialize)]
struct Current { temperature_2m: f32, weather_code: u8 }
#[derive(Deserialize)]
struct Hourly { time: Vec<String>, precipitation_probability: Vec<Option<f64>> }
#[derive(Deserialize)]
struct Daily { temperature_2m_min: Vec<f32>, temperature_2m_max: Vec<f32> }

/// `now_ms` : instant UTC ; les heures de la réponse sont locales (`timezone=auto`, sans décalage), d'où `utc_offset_seconds`.
pub fn parse_forecast(json: &str, place: &str, now_ms: i64) -> Result<Weather, CoreError> {
    let f: Forecast = serde_json::from_str(json)?;
    let (Some(&today_min), Some(&today_max)) = (f.daily.temperature_2m_min.first(), f.daily.temperature_2m_max.first()) else {
        return Err(CoreError::Invalid("prévision sans températures du jour".into()));
    };
    let local = chrono::DateTime::from_timestamp_millis(now_ms + f.utc_offset_seconds * 1000)
        .ok_or_else(|| CoreError::Invalid("date hors limites".into()))?
        .naive_utc();
    let hour_start = local.date().and_hms_opt(local.time().hour(), 0, 0).unwrap_or(local);
    let end = hour_start + chrono::Duration::hours(RAIN_WINDOW_H);
    let rain_from_hour = f.hourly.time.iter().zip(&f.hourly.precipitation_probability).find_map(|(t, p)| {
        let t = chrono::NaiveDateTime::parse_from_str(t, "%Y-%m-%dT%H:%M").ok()?;
        (t >= hour_start && t < end && p.is_some_and(|p| p >= RAIN_PCT)).then(|| t.time().hour() as u8)
    });
    let (label, icon) = codes::label_icon(f.current.weather_code);
    Ok(Weather {
        place: place.into(),
        temp_c: f.current.temperature_2m,
        code: f.current.weather_code,
        label: label.into(),
        icon: icon.into(),
        today_min,
        today_max,
        rain_from_hour,
        fetched_ms: now_ms,
    })
}

/// Faux : un échec il y a moins de 10 min. Vrai : aucune donnée, ou plus de 30 min. Sinon faux.
pub fn should_refresh(last_ok_ms: Option<i64>, last_fail_ms: Option<i64>, now_ms: i64) -> bool {
    if last_fail_ms.is_some_and(|f| now_ms - f < RETRY_MS) {
        return false;
    }
    last_ok_ms.is_none_or(|ok| now_ms - ok > FRESH_MS)
}

/// Client partagé : délai de 8 s, agent générique (jamais de donnée personnelle dedans).
pub fn client() -> Result<reqwest::Client, CoreError> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .user_agent("Oniris/0.1")
        .build()
        .map_err(net_err)
}

/// Sans l'URL : elle contient la ville et les coordonnées.
fn net_err(e: reqwest::Error) -> CoreError { CoreError::Invalid(format!("réseau : {}", e.without_url())) }

async fn get_text(client: &reqwest::Client, url: &str, query: &[(&str, String)]) -> Result<String, CoreError> {
    client.get(url).query(query).send().await.map_err(net_err)?.error_for_status().map_err(net_err)?.text().await.map_err(net_err)
}

/// Géocode seulement sans lieu en cache (le cache est vidé quand la ville change). Renvoie aussi le lieu, à mettre en cache.
pub async fn fetch(client: &reqwest::Client, city: &str, cached_place: Option<Place>) -> Result<(Place, Weather), CoreError> {
    let place = match cached_place {
        Some(p) => p,
        None => {
            let q = [("name", city.to_string()), ("count", "1".into()), ("language", "fr".into())];
            parse_geocoding(&get_text(client, GEOCODING_URL, &q).await?)?.ok_or_else(|| CoreError::NotFound("ville introuvable".into()))?
        }
    };
    let q = [
        ("latitude", place.lat.to_string()),
        ("longitude", place.lon.to_string()),
        ("current", "temperature_2m,weather_code".into()),
        ("hourly", "precipitation_probability".into()),
        ("daily", "temperature_2m_min,temperature_2m_max".into()),
        ("timezone", "auto".into()),
        ("forecast_days", "2".into()),
    ];
    let now = chrono::Utc::now().timestamp_millis();
    let w = parse_forecast(&get_text(client, FORECAST_URL, &q).await?, &place.name, now)?;
    Ok((place, w))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GEO: &str = include_str!("../../tests/fixtures/weather/geocoding_paris.json");
    const GEO_EMPTY: &str = include_str!("../../tests/fixtures/weather/geocoding_empty.json");
    const FORECAST: &str = include_str!("../../tests/fixtures/weather/forecast_paris.json");
    const MIDNIGHT: &str = include_str!("../../tests/fixtures/weather/forecast_midnight.json");

    /// 2026-11-04 07:45 UTC = 08:45 à Paris (UTC+1).
    const NOW_MORNING: i64 = 1_793_778_300_000;
    /// 2026-11-04 21:30 UTC = 22:30 à Paris.
    const NOW_NIGHT: i64 = 1_793_827_800_000;
    const MIN: i64 = 60_000;

    #[test]
    fn geocodage_ville_trouvee() {
        let p = parse_geocoding(GEO).unwrap().unwrap();
        assert_eq!(p, Place { name: "Paris".into(), lat: 48.85341, lon: 2.3488 });
    }

    #[test]
    fn geocodage_sans_resultat_donne_none() {
        assert_eq!(parse_geocoding(GEO_EMPTY).unwrap(), None);
        assert_eq!(parse_geocoding(r#"{"results":[]}"#).unwrap(), None);
    }

    #[test]
    fn json_malforme_donne_une_erreur() {
        assert!(parse_geocoding("{pas du json").is_err());
        assert!(parse_forecast("", "Paris", 0).is_err());
        assert!(parse_forecast(r#"{"current":{}}"#, "Paris", 0).is_err());
        assert!(parse_forecast(r#"{"utc_offset_seconds":0}"#, "Paris", 0).is_err());
    }

    #[test]
    fn prevision_temperature_code_et_pluie() {
        let w = parse_forecast(FORECAST, "Paris", NOW_MORNING).unwrap();
        assert_eq!(w.place, "Paris");
        assert_eq!(w.temp_c, 11.6);
        assert_eq!((w.code, w.label.as_str(), w.icon.as_str()), (61, "Pluie faible", "CloudRain"));
        assert_eq!((w.today_min, w.today_max), (8.1, 12.4));
        // 07 h (80 %) est passé ; 09 h n'atteint que 30 % ; la première heure à >= 60 % est 18 h.
        assert_eq!(w.rain_from_hour, Some(18));
        assert_eq!(w.fetched_ms, NOW_MORNING);
    }

    #[test]
    fn pluie_commence_a_l_heure_courante() {
        // 18 h 15 locale : l'heure courante (18 h, 70 %) compte.
        let now = NOW_MORNING + (9 * 60 + 30) * MIN;
        assert_eq!(parse_forecast(FORECAST, "Paris", now).unwrap().rain_from_hour, Some(18));
    }

    #[test]
    fn pluie_passee_ignoree_et_pas_de_donnees_donne_none() {
        // 03 h 30 locale le 5 : 01 h et 02 h sont passées, la première pluie à venir est 12 h.
        assert_eq!(parse_forecast(MIDNIGHT, "Paris", 1_793_845_800_000).unwrap().rain_from_hour, Some(12));
        // 07 h 45 le lendemain : la réponse ne couvre que le 4, rien dans la fenêtre.
        let now = NOW_MORNING + (23 * 60) * MIN;
        assert_eq!(parse_forecast(FORECAST, "Paris", now).unwrap().rain_from_hour, None);
    }

    #[test]
    fn pluie_apres_minuit_utilise_le_decalage_de_la_reponse() {
        // 22 h 30 locale : 21 h (95 %) est passé, 23 h (10 %) trop faible, puis 01 h du lendemain (65 %) est dans les 12 h.
        let w = parse_forecast(MIDNIGHT, "Paris", NOW_NIGHT).unwrap();
        assert_eq!(w.rain_from_hour, Some(1));
        assert_eq!(w.code, 3);
    }

    #[test]
    fn politique_d_actualisation() {
        let now = 1_000 * 60 * MIN;
        assert!(should_refresh(None, None, now), "aucune donnée");
        assert!(!should_refresh(Some(now - 30 * MIN), None, now), "exactement 30 min : encore valable");
        assert!(should_refresh(Some(now - 30 * MIN - 1), None, now), "plus de 30 min");
        assert!(!should_refresh(Some(now - 5 * MIN), None, now), "données récentes");
        assert!(!should_refresh(None, Some(now - 10 * MIN + 1), now), "échec il y a moins de 10 min");
        assert!(should_refresh(None, Some(now - 10 * MIN), now), "échec il y a 10 min pile : on réessaie");
        assert!(!should_refresh(Some(now - 2 * 60 * MIN), Some(now - 3 * MIN), now), "ancien mais échec récent");
        assert!(should_refresh(Some(now - 2 * 60 * MIN), Some(now - 11 * MIN), now), "ancien et échec ancien");
    }
}
