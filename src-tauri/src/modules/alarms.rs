//! Alarmes : commandes Tauri, ordonnanceur et sonnerie.
//!
//! La sonnerie passe par la sortie audio de la voix (cpal, sortie par défaut), sur son propre fil, même panneau
//! fermé et même en plein écran : aucune fenêtre n'est mise au premier plan (notification + pilule + carte).
//! Elle boucle jusqu'à « Arrêter » ou « Répéter dans 5 min », 10 min au plus. Journal : ids seulement.
use crate::commands::dispatch_as_user;
use crate::modules::voice::playback::{AudioOut, CpalOut};
use crate::state::AppState;
use crate::tray;
use chrono::Local;
use oniris_core::alarms::{self, Alarm};
use oniris_core::events::Topic;
use oniris_core::tools::ToolOutcome;
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::json;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, State};

pub const KEY: &str = "alarms";
const RATE: u32 = 22_050;
const MAX_RING: Duration = Duration::from_secs(10 * 60);

/// Alarme en train de sonner.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Rung {
    id: i64,
    label: String,
    due_ms: i64,
}

struct Ringing {
    list: Vec<Rung>,
    stop: mpsc::Sender<()>,
}

static RINGING: Mutex<Option<Ringing>> = Mutex::new(None);

#[derive(Serialize)]
pub struct AlarmsState {
    alarms: Vec<Alarm>,
    ringing: Vec<Rung>,
}

fn ringing() -> Vec<Rung> {
    RINGING.lock().as_ref().map(|r| r.list.clone()).unwrap_or_default()
}

#[tauri::command(async)]
pub fn alarms_state(state: State<AppState>) -> Result<AlarmsState, String> {
    let alarms = alarms::list(&state.db.lock()).map_err(|e| e.to_string())?;
    Ok(AlarmsState { alarms, ringing: ringing() })
}

#[tauri::command]
pub async fn alarms_set(app: AppHandle, text: String) -> ToolOutcome {
    dispatch_as_user(&app, "alarm.set", json!({ "text": text })).await
}

#[tauri::command]
pub async fn alarms_cancel(app: AppHandle, id: i64) -> ToolOutcome {
    dispatch_as_user(&app, "alarm.cancel", json!({ "id": id })).await
}

/// Coupe la sonnerie ; rend les alarmes qui sonnaient.
fn silence() -> Vec<Rung> {
    let r = RINGING.lock().take();
    r.map(|r| {
        let _ = r.stop.send(());
        r.list
    })
    .unwrap_or_default()
}

#[tauri::command(async)]
pub fn alarms_stop(state: State<AppState>) {
    if !silence().is_empty() {
        tracing::info!("alarme arrêtée");
        state.events.changed(Topic::Timers);
    }
}

#[tauri::command(async)]
pub fn alarms_snooze(state: State<AppState>) -> Result<(), String> {
    let now = Local::now().timestamp_millis();
    for r in silence() {
        alarms::snooze(&state.db.lock(), &r.label, now).map_err(|e| e.to_string())?;
        tracing::info!("alarme {} répétée dans {} min", r.id, alarms::SNOOZE_MIN);
    }
    state.events.changed(Topic::Timers); // reprogramme et rafraîchit
    Ok(())
}

/// Trois bips (880 Hz, fondus de 5 ms pour éviter les clics) puis un silence : 1,35 s à `gain` (0 à 1).
fn chime(gain: f32) -> Vec<i16> {
    let ms = |n: u32| (RATE * n / 1000) as usize;
    let mut v = Vec::with_capacity(ms(1350));
    for _ in 0..3 {
        let n = ms(150);
        let fade = ms(5) as f32;
        v.extend((0..n).map(|i| {
            let env = (i as f32 / fade).min((n - i) as f32 / fade).min(1.0);
            ((i as f32 * 2.0 * std::f32::consts::PI * 880.0 / RATE as f32).sin() * env * gain * 32767.0) as i16
        }));
        v.extend(std::iter::repeat_n(0, ms(100)));
    }
    v.extend(std::iter::repeat_n(0, ms(600)));
    v
}

/// Volume raisonnable : doux au début, plus fort après 30 s.
fn gain(elapsed: Duration) -> f32 {
    (0.12 + elapsed.as_secs_f32() * 0.006).min(0.3)
}

/// Joue la sonnerie en boucle jusqu'à un ordre d'arrêt (vrai) ou `max` (faux). Sans sortie audio : attend quand même,
/// la carte et la pilule montrent l'alarme.
fn ring(out: &dyn AudioOut, stop: &mpsc::Receiver<()>, max: Duration) -> bool {
    let mut sink = out.open(RATE).map_err(|e| tracing::warn!("alarme : sortie audio indisponible : {e}")).ok();
    let t0 = Instant::now();
    loop {
        if let Some(s) = sink.as_mut() {
            if s.pending() < RATE as usize / 2 {
                s.push(&chime(gain(t0.elapsed())));
            }
        }
        match stop.recv_timeout(Duration::from_millis(100)) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => return true,
            Err(mpsc::RecvTimeoutError::Timeout) if t0.elapsed() >= max => return false,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
}

fn start_ringing(app: &AppHandle, rungs: Vec<Rung>, out: Arc<dyn AudioOut>) {
    let mut g = RINGING.lock();
    if let Some(r) = g.as_mut() {
        r.list.extend(rungs); // une seule sonnerie à la fois
        return;
    }
    let (tx, rx) = mpsc::channel();
    *g = Some(Ringing { list: rungs, stop: tx });
    drop(g);
    let app = app.clone();
    let spawned = std::thread::Builder::new().name("oniris-alarm".into()).spawn(move || {
        if !ring(out.as_ref(), &rx, MAX_RING) && RINGING.lock().take().is_some() {
            tracing::info!("alarme arrêtée au bout de 10 min");
            tray::notify(&app, "Alarme", "Alarme arrêtée au bout de 10 minutes sans réponse.");
            if let Some(st) = app.try_state::<AppState>() {
                st.events.changed(Topic::Timers);
            }
        }
    });
    if let Err(e) = spawned {
        tracing::error!("alarme : fil de sonnerie impossible : {e}");
    }
}

fn hhmm(a: &Alarm) -> String { format!("{:02}:{:02}", a.hour, a.minute) }

/// Fait sonner les alarmes échues ; trop en retard (PC éteint ou en veille) : signalées sans sonner.
fn fire_due(app: &AppHandle, grace_ms: i64) {
    let Some(state) = app.try_state::<AppState>() else { return };
    let now = Local::now().timestamp_millis();
    // `let` d'abord : verrou de la base relâché avant les notifications et `changed`.
    let due = alarms::take_due(&state.db.lock(), now);
    let due = match due {
        Ok(d) => d,
        Err(e) => { tracing::error!("alarmes : lecture des échues impossible : {e}"); return; }
    };
    if due.is_empty() {
        state.events.changed(Topic::Timers);
        return;
    }
    let (ring, missed) = alarms::split_late(due, now, grace_ms);
    for a in &missed {
        tracing::info!("alarme {} manquée", a.id);
        tray::notify(app, "Alarme manquée", &format!("{} — {} (PC éteint ou en veille)", a.label, hhmm(a)));
    }
    if !ring.is_empty() {
        for a in &ring {
            tracing::info!("alarme {} sonne", a.id);
        }
        let body = ring.iter().map(|a| format!("{} — {}", a.label, hhmm(a))).collect::<Vec<_>>().join("\n");
        tray::notify(app, "Alarme", &body);
        start_ringing(app, ring.into_iter().map(|a| Rung { id: a.id, label: a.label, due_ms: a.next_ms }).collect(), Arc::new(CpalOut));
    }
    state.events.changed(Topic::Timers); // reprogramme et rafraîchit l'UI
}

/// À appeler dans `setup`, ordonnanceur en place ; au démarrage, une alarme passée est seulement signalée.
pub fn start(app: &AppHandle) {
    let Some(scheduler) = app.state::<AppState>().scheduler.get().cloned() else { return };
    let h = app.clone();
    scheduler.on_due(KEY, Arc::new(move |_| fire_due(&h, alarms::LATE_GRACE_MS)));
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || fire_due(&app, alarms::STARTUP_GRACE_MS));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::voice::piper::TtsError;
    use crate::modules::voice::playback::Sink;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Fausse sortie : compte les échantillons, ne joue rien.
    struct Fake(Arc<AtomicUsize>);
    struct FakeSink(Arc<AtomicUsize>);
    impl AudioOut for Fake {
        fn open(&self, _: u32) -> Result<Box<dyn Sink>, TtsError> { Ok(Box::new(FakeSink(self.0.clone()))) }
    }
    impl Sink for FakeSink {
        fn push(&mut self, pcm: &[i16]) { self.0.fetch_add(pcm.len(), Ordering::SeqCst); }
        fn pending(&self) -> usize { 0 }
    }
    struct NoOut;
    impl AudioOut for NoOut {
        fn open(&self, _: u32) -> Result<Box<dyn Sink>, TtsError> { Err(TtsError::Audio("aucune".into())) }
    }

    #[test]
    fn sonne_jusqu_a_l_arret_ou_au_plafond() {
        let n = Arc::new(AtomicUsize::new(0));
        let (tx, rx) = mpsc::channel();
        let h = { let n = n.clone(); std::thread::spawn(move || ring(&Fake(n), &rx, Duration::from_secs(30))) };
        std::thread::sleep(Duration::from_millis(250));
        tx.send(()).unwrap();
        assert!(h.join().unwrap(), "arrêtée par l'utilisateur");
        assert!(n.load(Ordering::SeqCst) >= chime(0.1).len());
        let (_tx, rx) = mpsc::channel();
        assert!(!ring(&NoOut, &rx, Duration::from_millis(150)), "plafond atteint, même sans sortie audio");
    }

    #[test]
    fn bips_bornes_et_volume_raisonnable() {
        let c = chime(gain(Duration::from_secs(3600)));
        assert_eq!(c.len(), 3 * (RATE as usize * 250 / 1000) + RATE as usize * 600 / 1000);
        let peak = c.iter().map(|s| s.unsigned_abs()).max().unwrap();
        assert!(peak > 9000 && peak <= (0.3 * 32767.0) as u16 + 1, "{peak}");
        assert!(gain(Duration::ZERO) < gain(Duration::from_secs(30)));
    }
}
