//! Titre en cours lu par Windows (SMTC). Abonné seulement panneau ouvert : un fil dédié reçoit les événements
//! du gestionnaire et de la session suivie, relit l'état, met à jour `NOW` et émet `Topic::Music` (4 fois par
//! seconde au plus). La fermeture retire tous les jetons et arrête le fil. Journal : codes d'erreur seulement.
use crate::state::AppState;
use oniris_core::events::Topic;
use oniris_core::music::{self, MediaControl, NowPlaying, Source, Throttle};
use oniris_core::time_util::now_ms;
use oniris_core::tools::ToolOutcome;
use parking_lot::Mutex;
use serde_json::json;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;
use tauri::{AppHandle, Manager};
use windows::Foundation::TypedEventHandler;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession as Session, GlobalSystemMediaTransportControlsSessionManager as Manager_,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as Status,
};
use windows::Storage::Streams::DataReader;

/// Dernier état connu (relu à chaque ouverture du panneau).
static NOW: Mutex<Option<NowPlaying>> = Mutex::new(None);
/// Canal du fil d'écoute ; `None` : panneau fermé, aucun abonnement.
static LISTENER: Mutex<Option<Sender<Msg>>> = Mutex::new(None);

#[derive(Clone, Copy)]
enum Msg { Changed, Sessions, Stop }

/// Gestionnaire d'événement WinRT : signale `m` au fil d'écoute (fil du pool WinRT : ne fait rien d'autre).
fn signal<S, A>(tx: &Sender<Msg>, m: Msg) -> TypedEventHandler<S, A>
where
    S: windows::core::RuntimeType + 'static,
    A: windows::core::RuntimeType + 'static,
{
    let tx = tx.clone();
    TypedEventHandler::new(move |_, _| {
        let _ = tx.send(m);
        Ok(())
    })
}

/// Appelé par `windows::{open_panel, close_panel}` : ne fait que lancer ou arrêter le fil (ouverture instantanée).
pub fn set_listening(app: &AppHandle, open: bool) {
    let mut l = LISTENER.lock();
    if !open {
        if let Some(tx) = l.take() {
            let _ = tx.send(Msg::Stop);
        }
        return;
    }
    if l.is_some() {
        return;
    }
    let (tx, rx) = channel();
    *l = Some(tx.clone());
    let app = app.clone();
    std::thread::spawn(move || {
        if let Err(e) = listen(&app, &tx, &rx) {
            tracing::warn!("SMTC : écoute impossible ({:#x})", e.code().0);
        }
    });
}

/// Jetons d'une session suivie, retirés par `unbind`.
struct Bound { session: Session, tokens: [i64; 3] }

fn listen(app: &AppHandle, tx: &Sender<Msg>, rx: &Receiver<Msg>) -> windows::core::Result<()> {
    let mgr = Manager_::RequestAsync()?.join()?;
    let t_sessions = mgr.SessionsChanged(&signal(tx, Msg::Sessions))?;
    let t_current = mgr.CurrentSessionChanged(&signal(tx, Msg::Sessions))?;
    let mut bound = bind(&mgr, tx);
    let mut throttle = Throttle::new(250);
    let mut rebind = false;
    let mut first = true;
    loop {
        let t0 = now_ms();
        let wait = throttle.wait_ms(t0);
        // Regroupe les événements arrivés pendant l'attente du limiteur.
        let deadline = std::time::Instant::now() + Duration::from_millis(wait as u64);
        let stop = loop {
            match rx.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now())) {
                Ok(Msg::Stop) | Err(RecvTimeoutError::Disconnected) => break true,
                Ok(Msg::Sessions) => rebind = true,
                Ok(Msg::Changed) => {}
                Err(RecvTimeoutError::Timeout) => break false,
            }
        };
        if stop {
            break;
        }
        if rebind {
            unbind(bound.take());
            bound = bind(&mgr, tx);
            rebind = false;
        }
        let prev = NOW.lock().clone(); // verrou relâché avant la lecture (bloquante) : `music_now` tourne sur le fil principal
        let next = bound.as_ref().and_then(|b| read(&b.session, prev.clone()).ok());
        let changed = first || prev != next;
        first = false;
        *NOW.lock() = next;
        throttle.mark(now_ms());
        // Un événement sans effet visible (position inchangée, même titre) ne réveille pas l'interface.
        if changed {
            app.state::<AppState>().events.changed(Topic::Music);
        }
        // Attend le prochain événement.
        match rx.recv() {
            Ok(Msg::Stop) | Err(_) => break,
            Ok(Msg::Sessions) => rebind = true,
            Ok(Msg::Changed) => {}
        }
    }
    unbind(bound);
    let _ = mgr.RemoveSessionsChanged(t_sessions);
    let _ = mgr.RemoveCurrentSessionChanged(t_current);
    Ok(())
}

/// Session suivie (Spotify d'abord, sinon la session courante) et ses trois abonnements.
fn bind(mgr: &Manager_, tx: &Sender<Msg>) -> Option<Bound> {
    let session = pick(mgr)?;
    let tokens = (|| Ok::<_, windows::core::Error>([
        session.MediaPropertiesChanged(&signal(tx, Msg::Changed))?,
        session.PlaybackInfoChanged(&signal(tx, Msg::Changed))?,
        session.TimelinePropertiesChanged(&signal(tx, Msg::Changed))?,
    ]))();
    match tokens {
        Ok(tokens) => Some(Bound { session, tokens }),
        Err(e) => {
            tracing::warn!("SMTC : abonnement à la session impossible ({:#x})", e.code().0);
            None
        }
    }
}

fn unbind(b: Option<Bound>) {
    if let Some(Bound { session, tokens: [m, p, t] }) = b {
        let _ = session.RemoveMediaPropertiesChanged(m);
        let _ = session.RemovePlaybackInfoChanged(p);
        let _ = session.RemoveTimelinePropertiesChanged(t);
    }
}

fn pick(mgr: &Manager_) -> Option<Session> {
    let sessions: Vec<Session> = mgr.GetSessions().ok()?.into_iter().collect();
    let ids: Vec<String> = sessions.iter().map(|s| s.SourceAppUserModelId().map(|h| h.to_string()).unwrap_or_default()).collect();
    match music::pick_session(ids.iter().map(String::as_str)) {
        Some(i) => sessions.into_iter().nth(i),
        None => mgr.GetCurrentSession().ok(),
    }
}

/// Unités WinRT (100 ns) → ms.
const TICKS_PER_MS: i64 = 10_000;
/// Écart entre l'époque Windows (1601) et l'époque Unix, en unités de 100 ns.
const UNIX_EPOCH_TICKS: i64 = 116_444_736_000_000_000;

/// Relit l'état de la session ; la pochette n'est relue que si le titre ou l'artiste change (ou si elle manquait).
fn read(session: &Session, prev: Option<NowPlaying>) -> windows::core::Result<NowPlaying> {
    let props = session.TryGetMediaPropertiesAsync()?.join()?;
    let (title, artist, album) = (props.Title()?.to_string(), props.Artist()?.to_string(), props.AlbumTitle().map(|h| h.to_string()).unwrap_or_default());
    let playing = session.GetPlaybackInfo().and_then(|p| p.PlaybackStatus()).is_ok_and(|s| s == Status::Playing);
    let tl = session.GetTimelineProperties()?;
    let updated = tl.LastUpdatedTime().map(|d| (d.UniversalTime - UNIX_EPOCH_TICKS) / TICKS_PER_MS).unwrap_or(0);
    let source = if session.SourceAppUserModelId().is_ok_and(|id| music::is_spotify(&id.to_string())) { Source::Spotify } else { Source::Other };
    let (cover, colors) = match prev {
        Some(p) if p.title == title && p.artist == artist && p.cover.is_some() => (p.cover, p.colors),
        _ => props.Thumbnail().ok().and_then(|r| thumbnail(&r).ok()).and_then(|b| music::cover(&b)).map_or((None, vec![]), |(c, k)| (Some(c), k)),
    };
    Ok(NowPlaying {
        title, artist, album, cover, colors, playing, source,
        position_ms: tl.Position()?.Duration / TICKS_PER_MS,
        duration_ms: (tl.EndTime()?.Duration - tl.StartTime()?.Duration) / TICKS_PER_MS,
        updated_ms: if updated > 0 { updated } else { now_ms() },
    })
}

/// Octets bruts de la pochette (8 Mo au plus).
fn thumbnail(r: &windows::Storage::Streams::IRandomAccessStreamReference) -> windows::core::Result<Vec<u8>> {
    let stream = r.OpenReadAsync()?.join()?;
    let size = stream.Size()?.min(8 << 20) as u32;
    let reader = DataReader::CreateDataReader(&stream)?;
    let n = reader.LoadAsync(size)?.join()?;
    let mut buf = vec![0u8; n as usize];
    reader.ReadBytes(&mut buf)?;
    Ok(buf)
}

/// Commandes de lecture par SMTC (aucun réseau).
pub struct Smtc;

impl Smtc {
    fn session() -> Result<Session, String> {
        let mgr = Manager_::RequestAsync().and_then(|op| op.join()).map_err(|_| "Windows ne donne pas accès aux lecteurs multimédias.".to_string())?;
        pick(&mgr).ok_or_else(|| "Aucun lecteur multimédia actif.".into())
    }
    fn run(op: impl FnOnce(&Session) -> windows::core::Result<bool>, refused: &str) -> Result<(), String> {
        match op(&Self::session()?) {
            Ok(true) => Ok(()),
            _ => Err(refused.into()),
        }
    }
}

impl MediaControl for Smtc {
    fn toggle(&self) -> Result<(), String> { Self::run(|s| s.TryTogglePlayPauseAsync()?.join(), "Le lecteur n'a pas accepté la lecture ou la pause.") }
    fn next(&self) -> Result<(), String> { Self::run(|s| s.TrySkipNextAsync()?.join(), "Le lecteur n'a pas pu passer au titre suivant.") }
    fn previous(&self) -> Result<(), String> { Self::run(|s| s.TrySkipPreviousAsync()?.join(), "Le lecteur n'a pas pu revenir au titre précédent.") }
    // shortcut: SMTC seul, la Web API Spotify prend le relais à la tâche 8.
    fn seek(&self, ms: i64) -> Result<(), String> {
        Self::run(|s| s.TryChangePlaybackPositionAsync(ms * TICKS_PER_MS)?.join(), "Ce lecteur ne permet pas de changer la position.")
    }
}

#[tauri::command]
pub fn music_now() -> Option<NowPlaying> { NOW.lock().clone() }

#[tauri::command]
pub async fn music_toggle(app: AppHandle) -> ToolOutcome { crate::commands::dispatch_as_user(&app, "music.toggle", json!({})).await }

#[tauri::command]
pub async fn music_next(app: AppHandle) -> ToolOutcome { crate::commands::dispatch_as_user(&app, "music.next", json!({})).await }

#[tauri::command]
pub async fn music_previous(app: AppHandle) -> ToolOutcome { crate::commands::dispatch_as_user(&app, "music.previous", json!({})).await }

#[tauri::command]
pub async fn music_seek(app: AppHandle, position_ms: u32) -> ToolOutcome {
    crate::commands::dispatch_as_user(&app, "music.seek", json!({ "position_ms": position_ms })).await
}
