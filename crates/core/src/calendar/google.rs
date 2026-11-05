//! Client de l'API Google Calendar v3 (lecture seule). Erreurs sans URL ni contenu : seulement le statut HTTP.
use crate::time_util::local_day_bounds;
use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, TimeZone};
use serde::Deserialize;

pub const BASE: &str = "https://www.googleapis.com";

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum GcalError {
    #[error("jeton refusé")]
    Unauthorized,
    /// Transport, 429 ou 5xx : réessayer plus tard.
    #[error("réseau indisponible")]
    Network,
    #[error("Google Agenda refuse (HTTP {0})")]
    Status(u16),
    #[error("réponse Google Agenda illisible")]
    Parse,
}

#[derive(Clone)]
pub struct Client {
    pub http: reqwest::Client,
    /// `https://www.googleapis.com` ; les tests le pointent sur un faux serveur.
    pub base: String,
}

/// Événement prêt à ranger. Titre et lieu : contenu utilisateur, jamais journalisés.
#[derive(Clone, PartialEq)]
pub struct ExtEvent {
    pub id: String,
    pub title: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub all_day: bool,
    pub location: Option<String>,
}

impl std::fmt::Debug for ExtEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExtEvent").field("id", &self.id).finish_non_exhaustive()
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct EventsPage {
    /// Événements à ajouter ou mettre à jour (les `cancelled` n'y sont pas).
    pub items: Vec<ExtEvent>,
    /// Ids des événements annulés : à retirer.
    pub cancelled: Vec<String>,
    pub next_sync_token: Option<String>,
    /// 410 : le syncToken est périmé, resynchronisation complète.
    pub gone: bool,
}

/// Ce qu'on demande : la suite depuis un syncToken, ou la fenêtre complète (ms).
pub enum Query<'a> {
    Sync(&'a str),
    Window { time_min_ms: i64, time_max_ms: i64 },
}

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct CalList { items: Vec<CalItem>, next_page_token: Option<String> }
#[derive(Deserialize, Default)]
#[serde(default)]
struct CalItem { id: String, selected: bool }

#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct EventList { items: Vec<RawEvent>, next_page_token: Option<String>, next_sync_token: Option<String> }
#[derive(Deserialize, Default)]
#[serde(default)]
struct RawEvent { id: String, status: String, summary: Option<String>, location: Option<String>, start: When, end: When }
#[derive(Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct When { date: Option<String>, date_time: Option<String>, time_zone: Option<String> }

/// Minuit local de `d`.
fn local_midnight(d: NaiveDate) -> Option<i64> {
    let noon = Local.from_local_datetime(&d.and_hms_opt(12, 0, 0)?).earliest()?.timestamp_millis();
    Some(local_day_bounds(noon).0)
}

/// Instant d'un `dateTime` : avec décalage (RFC 3339) tel quel ; sans décalage, dans le fuseau `timeZone`.
fn instant(w: &When) -> Option<i64> {
    let s = w.date_time.as_deref()?;
    if let Ok(t) = DateTime::parse_from_rfc3339(s) {
        return Some(t.timestamp_millis());
    }
    let n = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f").ok()?;
    let tz: chrono_tz::Tz = w.time_zone.as_deref()?.parse().ok()?;
    Some(tz.from_local_datetime(&n).earliest()?.timestamp_millis())
}

fn day(w: &When) -> Option<i64> { local_midnight(NaiveDate::parse_from_str(w.date.as_deref()?, "%Y-%m-%d").ok()?) }

/// `None` : événement sans heure lisible (ignoré). Journée entière : minuit local → minuit local de la date de fin (exclue).
fn convert(r: RawEvent) -> Option<ExtEvent> {
    let (start_ms, end_ms, all_day) = if r.start.date.is_some() {
        let s = day(&r.start)?;
        let e = day(&r.end).filter(|e| *e > s).unwrap_or_else(|| local_day_bounds(s).1);
        (s, e, true)
    } else {
        let s = instant(&r.start)?;
        (s, instant(&r.end).filter(|e| *e >= s).unwrap_or(s), false)
    };
    Some(ExtEvent {
        id: r.id,
        title: r.summary.filter(|t| !t.trim().is_empty()).unwrap_or_else(|| "(Sans titre)".into()),
        location: r.location.filter(|l| !l.trim().is_empty()),
        start_ms, end_ms, all_day,
    })
}

pub(super) fn rfc3339(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms).unwrap_or_default().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

impl Client {
    pub fn new(http: reqwest::Client) -> Self { Self { http, base: BASE.into() } }

    /// `Ok(None)` : 410.
    async fn get<T: serde::de::DeserializeOwned>(&self, token: &str, segments: &[&str], query: &[(&str, &str)]) -> Result<Option<T>, GcalError> {
        let mut url = reqwest::Url::parse(&self.base).map_err(|_| GcalError::Parse)?;
        // Les ids d'agenda (adresses, `#holiday@…`) sont encodés comme segments de chemin.
        url.path_segments_mut().map_err(|_| GcalError::Parse)?.extend(["calendar", "v3"]).extend(segments);
        let resp = self.http.get(url).bearer_auth(token).query(query).send().await.map_err(|_| GcalError::Network)?;
        let s = resp.status().as_u16();
        match s {
            410 => return Ok(None),
            401 => return Err(GcalError::Unauthorized),
            429 | 500..=599 => return Err(GcalError::Network),
            _ if !resp.status().is_success() => return Err(GcalError::Status(s)),
            _ => {}
        }
        resp.json().await.map(Some).map_err(|_| GcalError::Parse)
    }

    /// Ids des agendas cochés (`selected`), toutes pages lues.
    pub async fn list_calendars(&self, token: &str) -> Result<Vec<String>, GcalError> {
        let (mut ids, mut page) = (vec![], None::<String>);
        loop {
            let mut q = vec![("maxResults", "250")];
            if let Some(p) = &page {
                q.push(("pageToken", p));
            }
            let l: CalList = self.get(token, &["users", "me", "calendarList"], &q).await?.ok_or(GcalError::Status(410))?;
            ids.extend(l.items.into_iter().filter(|c| c.selected).map(|c| c.id));
            match l.next_page_token {
                Some(p) => page = Some(p),
                None => return Ok(ids),
            }
        }
    }

    /// Événements d'un agenda, toutes pages lues (`pageToken`). 410 : `gone`.
    pub async fn events(&self, token: &str, cal: &str, query: Query<'_>) -> Result<EventsPage, GcalError> {
        let mut out = EventsPage::default();
        let mut page: Option<String> = None;
        loop {
            let mut q: Vec<(&str, String)> = vec![("singleEvents", "true".into()), ("maxResults", "250".into())];
            match &query {
                Query::Sync(t) => q.push(("syncToken", (*t).into())),
                Query::Window { time_min_ms, time_max_ms } => {
                    q.push(("timeMin", rfc3339(*time_min_ms)));
                    q.push(("timeMax", rfc3339(*time_max_ms)));
                }
            }
            if let Some(p) = &page {
                q.push(("pageToken", p.clone()));
            }
            let q: Vec<(&str, &str)> = q.iter().map(|(k, v)| (*k, v.as_str())).collect();
            let Some(l) = self.get::<EventList>(token, &["calendars", cal, "events"], &q).await? else {
                return Ok(EventsPage { gone: true, ..Default::default() });
            };
            for r in l.items {
                if r.status == "cancelled" {
                    out.cancelled.push(r.id);
                } else if let Some(e) = convert(r) {
                    out.items.push(e);
                }
            }
            out.next_sync_token = l.next_sync_token;
            match l.next_page_token {
                Some(p) => page = Some(p),
                None => return Ok(out),
            }
        }
    }
}
