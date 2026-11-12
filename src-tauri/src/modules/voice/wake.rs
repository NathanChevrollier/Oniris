//! Écoute du mot d'activation « Dis Iris ». Un thread à part possède un flux micro continu et fait tourner le
//! détecteur ; après la détection, ce même flux enregistre la demande jusqu'à la fin de parole, puis le pipeline la
//! traite comme un push-to-talk. Avant la détection, l'audio ne vit qu'en mémoire (canal et tampons du détecteur) :
//! jamais écrit, transcrit ni journalisé.
use super::capture::{CpalMic, Listening, Mic};
use super::learn::{Collector, Event, Label};
use super::pipeline::{self, Input, Mark, Pipeline};
use oniris_core::voice::endpoint::{Endpoint, Endpointer};
use oniris_core::voice::wake::{verify, WakeDetector, WakeError, WakeModels};
use parking_lot::Mutex;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::mpsc::{sync_channel, Receiver};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tokio::time::Instant;

/// Seuil de détection (scores saturés près de 1 : analyse fine du 12/11).
pub const WAKE_THRESHOLD: f32 = 0.998;
/// Presque-réveil : utilisé seulement par le mode apprentissage.
pub const NEAR_MISS: f32 = 0.3;
/// Après une détection, les suivantes sont ignorées.
pub const REFRACTORY: Duration = Duration::from_secs(2);
/// Suspension prolongée après la voix d'IRIS : elle ne se réveille pas elle-même.
pub const AFTER_SPEECH: Duration = Duration::from_millis(500);
/// Un pas du détecteur (1 280 échantillons à 16 kHz) ; l'état est relu à chaque pas.
const STEP: Duration = Duration::from_millis(80);
const OFF_STEP: Duration = Duration::from_secs(1);
const MAX_REC: usize = 30 * 16_000;
const MAX_BACKOFF: Duration = Duration::from_secs(60);
/// Une panne du micro n'est close qu'après ce temps d'écoute sans erreur : un micro instable ne multiplie pas les toasts.
const HEALTHY: Duration = Duration::from_secs(10);

pub trait Detector: Send {
    fn push(&mut self, s: &[f32]) -> Vec<f32>;
    fn reset(&mut self);
}

impl Detector for WakeDetector {
    fn push(&mut self, s: &[f32]) -> Vec<f32> { WakeDetector::push(self, s) }
    fn reset(&mut self) { WakeDetector::reset(self) }
}

pub type Load = Box<dyn Fn() -> Result<Box<dyn Detector>, WakeError> + Send>;

/// Empreintes vérifiées avant le chargement : un modèle absent ou altéré n'est jamais exécuté.
pub fn load_models(dir: &Path) -> Result<Box<dyn Detector>, WakeError> {
    verify(dir)?;
    Ok(Box::new(WakeDetector::load(&WakeModels::in_dir(dir))?))
}

enum State {
    /// Réglage désactivé.
    Off,
    Armed,
    /// Demande en cours d'enregistrement, pour l'appui `press` du pipeline.
    Recording { press: u64, rec: Vec<f32>, ep: Endpointer },
    /// Pause, voix non installée, plein écran, IRIS qui parle (et 500 ms après) ou tour en cours.
    Suspended,
    /// Micro indisponible : nouvel essai à `retry_at`.
    Failed { retry_at: Instant },
    /// Modèle absent, corrompu ou en panne : plus d'écoute jusqu'au prochain lancement.
    Broken,
}

type Stream = (Box<dyn Listening>, Receiver<Vec<f32>>, Instant);

pub struct WakeListener {
    p: Arc<Pipeline>,
    mic: Arc<dyn Mic>,
    load: Load,
    send: Arc<dyn Fn(Input) + Send + Sync>,
    label: Arc<Mutex<&'static str>>,
    state: State,
    det: Option<Box<dyn Detector>>,
    stream: Option<Stream>,
    /// Attente de la panne en cours (`None` : pas de panne, le prochain échec fera un toast).
    outage: Option<Duration>,
    last_wake: Option<Instant>,
    spoke: Option<Instant>,
    /// Dossier `learn` du mode apprentissage (`None` : pas de dossier personnel, jamais d'extrait).
    learn_dir: Option<PathBuf>,
    /// Collecteur du mode apprentissage : n'existe (anneau compris) que si l'écoute et le mode sont actifs.
    learn: Option<Collector>,
}

static LABEL: OnceLock<Arc<Mutex<&'static str>>> = OnceLock::new();

/// État de l'écoute, pour la feuille IRIS.
pub fn wake_state() -> &'static str { LABEL.get().map_or("désactivé", |l| *l.lock()) }

impl WakeListener {
    fn new(p: Arc<Pipeline>, mic: Arc<dyn Mic>, load: Load, send: Arc<dyn Fn(Input) + Send + Sync>, learn_dir: Option<PathBuf>) -> Self {
        let label = Arc::new(Mutex::new("désactivé"));
        Self { p, mic, load, send, label, state: State::Off, det: None, stream: None, outage: None, last_wake: None, spoke: None, learn_dir, learn: None }
    }

    /// Lance l'écouteur sur son propre thread : le détecteur peut y paniquer et `Mic::listen` y bloquer sans gêner l'app.
    pub fn start(app: &AppHandle) {
        let Some((tx, p)) = pipeline::voice() else { return };
        let dir = app.path().resolve("resources/wake", tauri::path::BaseDirectory::Resource).unwrap_or_default();
        let tx = tx.clone();
        let learn_dir = super::learn::learn_dir();
        let l = Self::new(p.clone(), Arc::new(CpalMic), Box::new(move || load_models(&dir)), Arc::new(move |i| { let _ = tx.send(i); }), learn_dir);
        if LABEL.set(l.label.clone()).is_err() {
            return;
        }
        let spawned = std::thread::Builder::new().name("oniris-wake".into()).spawn(move || {
            match tokio::runtime::Builder::new_current_thread().enable_time().build() {
                Ok(rt) => rt.block_on(l.run()),
                Err(e) => tracing::error!("mot d'activation : démarrage impossible : {e}"),
            }
        });
        if let Err(e) = spawned {
            tracing::error!("mot d'activation : thread impossible : {e}");
        }
    }

    async fn run(mut self) {
        loop {
            self.step().await;
            *self.label.lock() = match self.state {
                State::Off => "désactivé",
                State::Armed | State::Recording { .. } => "à l'écoute",
                State::Suspended => "en pause",
                State::Failed { .. } => "micro indisponible",
                State::Broken => "modèle absent",
            };
            // Désactivé : on ne relit la conf qu'une fois par seconde.
            tokio::time::sleep(if matches!(self.state, State::Off) { OFF_STEP } else { STEP }).await;
        }
    }

    async fn step(&mut self) {
        let marks = std::mem::take(&mut *self.p.marks.lock());
        if matches!(self.state, State::Broken) {
            self.learn = None;
            return;
        }
        let now = Instant::now();
        let c = (self.p.conf)();
        if !(c.wake && c.learning) {
            self.learn = None;
        } else if self.learn.is_none() {
            self.learn = self.learn_dir.clone().map(Collector::new);
        }
        for m in marks {
            let at_ms = self.learn.as_ref().map_or(0, Collector::now_ms);
            self.learn(match m {
                Mark::Start => Event::ManualStart { at_ms },
                Mark::Escape if matches!(self.state, State::Recording { .. }) => Event::Cancelled,
                Mark::Escape => continue,
                Mark::ExpectWake => Event::ExpectWake { at_ms },
                Mark::FalseWake => Event::FalseWake { at_ms },
            });
        }
        if self.p.speaking.load(Ordering::SeqCst) {
            self.spoke = Some(now);
        }
        let (press, busy) = self.p.snapshot();
        let hold = if !c.wake {
            Some(State::Off)
        } else if c.paused || !c.installed || c.fullscreen || self.spoke.is_some_and(|t| now - t < AFTER_SPEECH) {
            Some(State::Suspended)
        } else if let State::Recording { press: mine, .. } = self.state {
            (mine != press).then_some(State::Suspended) // push-to-talk : il prend la main, sans tour pour le mot
        } else {
            busy.then_some(State::Suspended) // push-to-talk, ou tour d'IRIS jusqu'à la fin de sa réponse
        };
        if let Some(next) = hold {
            self.abort_recording(press);
            self.close();
            if matches!(next, State::Off) {
                self.outage = None;
            }
            self.state = next;
            return;
        }
        if let State::Failed { retry_at } = self.state {
            if now < retry_at {
                return;
            }
        }
        if self.stream.is_none() && !self.open(&c.device, now) {
            return;
        }
        let Some((l, rx, opened)) = &self.stream else { return };
        if l.failed() {
            self.abort_recording(press);
            self.close();
            return self.fail(now, "Mot d'activation : le micro ne répond plus.".into());
        }
        if now - *opened >= HEALTHY {
            self.outage = None;
        }
        let samples: Vec<f32> = rx.try_iter().flatten().collect();
        if let Some(c) = self.learn.as_mut() {
            c.push_audio(&samples);
        }
        let ended = match &mut self.state {
            State::Recording { press, rec, ep } => {
                let room = MAX_REC - rec.len();
                rec.extend_from_slice(&samples[..samples.len().min(room)]);
                match ep.push(&samples) {
                    Endpoint::Continue => None,
                    Endpoint::NoSpeech => {
                        self.p.wake_abort(*press);
                        Some(Event::NoSpeech)
                    }
                    Endpoint::End | Endpoint::TooLong => {
                        (self.send)(Input::WakeEnd(*press, std::mem::take(rec)));
                        Some(Event::Command)
                    }
                }
            }
            _ => return self.detect(&samples, now).await,
        };
        if let Some(e) = ended {
            self.state = State::Armed;
            self.learn(e);
        }
    }

    /// Un événement pour le mode apprentissage ; l'extrait décidé est écrit hors de la boucle d'écoute.
    fn learn(&mut self, e: Event) {
        let Some(c) = self.learn.as_mut() else { return };
        let Some((label, clip)) = c.on(e) else { return };
        let dir = c.dir().clone();
        // Collecteur jetable : `save` n'a besoin que du dossier, l'anneau reste ici.
        tokio::task::spawn_blocking(move || match Collector::new(dir).save(label, &clip) {
            Ok(_) => tracing::info!("extrait d'apprentissage gardé : {}", if label == Label::Pos { "positif" } else { "négatif" }),
            Err(e) => tracing::warn!("mode apprentissage : extrait non écrit : {}", e.kind()),
        });
    }

    /// Les échantillons de ce pas qui suivent le pic (fin du mot) sont jetés : seule la suite va dans la demande.
    async fn detect(&mut self, samples: &[f32], now: Instant) {
        let Some(det) = self.det.as_mut() else { return };
        // `WakeDetector` panique si l'inférence échoue : l'écoute s'arrête, l'app continue.
        let scores = match catch_unwind(AssertUnwindSafe(|| det.push(samples))) {
            Ok(s) => s,
            Err(_) => {
                tracing::error!("mot d'activation : le détecteur a planté, écoute arrêtée");
                self.close();
                self.state = State::Broken;
                return;
            }
        };
        // Un score par pas de 80 ms, le dernier finissant avec l'audio reçu.
        let end_ms = self.learn.as_ref().map_or(0, Collector::now_ms);
        let at = |i: usize| end_ms.saturating_sub((scores.len() - 1 - i) as u64 * 80);
        for (i, score) in scores.iter().enumerate() {
            self.learn(Event::Score { at_ms: at(i), score: *score });
        }
        let peak = scores.iter().enumerate().filter(|(_, s)| **s >= WAKE_THRESHOLD).max_by(|a, b| a.1.total_cmp(b.1));
        let Some((peak, _)) = peak else { return };
        if self.last_wake.is_some_and(|t| now - t < REFRACTORY) {
            return;
        }
        tracing::info!("mot d'activation détecté");
        self.last_wake = Some(now);
        if let Some(d) = self.det.as_mut() {
            d.reset();
        }
        let (tx, rx) = tokio::sync::oneshot::channel();
        (self.send)(Input::Wake(tx));
        // Toujours une réponse (ou le pipeline n'existe plus) : pas de délai, qui laisserait la pilule en écoute.
        if let Ok(Some(press)) = rx.await {
            // Endpointer neuf : il ne voit que l'audio arrivé après la détection.
            self.state = State::Recording { press, rec: Vec::new(), ep: Endpointer::new() };
            self.learn(Event::Woke { at_ms: at(peak) });
        }
    }

    /// Ouvre le flux (et charge le détecteur la première fois) ; faux si l'écoute est impossible pour l'instant.
    fn open(&mut self, device: &str, now: Instant) -> bool {
        if self.det.is_none() {
            match (self.load)() {
                Ok(d) => self.det = Some(d),
                Err(e) => {
                    tracing::error!("mot d'activation désactivé : {e}");
                    self.state = State::Broken;
                    return false;
                }
            }
        }
        // Thread audio : copie et envoi sans attente ; canal plein (écouteur en retard) = morceau perdu.
        let (tx, rx) = sync_channel::<Vec<f32>>(64);
        match self.mic.listen(device, Box::new(move |s| { let _ = tx.try_send(s.to_vec()); })) {
            Ok(l) => {
                self.p.wake_open.store(true, Ordering::SeqCst);
                if let Some(d) = self.det.as_mut() {
                    d.reset();
                }
                self.stream = Some((l, rx, now));
                self.state = State::Armed;
                true
            }
            Err(e) => {
                self.fail(now, format!("Mot d'activation : {e}"));
                false
            }
        }
    }

    fn close(&mut self) {
        if let Some(c) = self.learn.as_mut() {
            c.gap();
        }
        if let Some((l, ..)) = self.stream.take() {
            l.stop();
            self.p.wake_open.store(false, Ordering::SeqCst);
        }
    }

    /// Demande en cours jetée ; la pilule revient au repos si l'appui est toujours le sien.
    fn abort_recording(&mut self, current: u64) {
        if let State::Recording { press, .. } = self.state {
            if press == current {
                self.p.wake_abort(press);
            }
            self.state = State::Armed;
        }
    }

    /// Attente doublée à chaque échec (1 s, 2 s… 60 s) ; un seul toast par panne.
    fn fail(&mut self, now: Instant, toast: String) {
        let backoff = self.outage.map_or(Duration::from_secs(1), |b| (b * 2).min(MAX_BACKOFF));
        if self.outage.is_none() {
            tracing::warn!("mot d'activation : micro indisponible");
            self.p.toast(toast);
        }
        self.outage = Some(backoff);
        self.state = State::Failed { retry_at: now + backoff };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oniris_core::voice::wake::WAKE_CHUNK;
    use crate::modules::voice::capture::{MicError, Recording};
    use crate::modules::voice::pipeline::tests::{for_wake, Fx, Log};
    use crate::modules::voice::pipeline::{Conf, Turns, VoicePhase};
    use crate::modules::iris::{TurnError, TurnResult};
    use oniris_core::iris::Segment;
    use std::sync::atomic::{AtomicBool, AtomicUsize};
    use tokio::time::sleep;

    type Sink = Arc<Mutex<Option<Box<dyn FnMut(&[f32]) + Send>>>>;

    /// Micro factice : `listen` garde le sink, que le test appelle comme le ferait le thread audio.
    struct FakeMic { log: Log, sink: Sink, fails: AtomicUsize, attempts: Mutex<Vec<Instant>>, broken: Arc<AtomicBool> }
    struct FakeRec;
    impl Recording for FakeRec {
        fn stop(self: Box<Self>) -> Vec<f32> { vec![0.5; 16_000] }
    }
    struct FakeListening { log: Log, sink: Sink, broken: Arc<AtomicBool> }
    impl Listening for FakeListening {
        fn stop(self: Box<Self>) {
            *self.sink.lock() = None;
            self.log.lock().push("unlisten".into());
        }
        fn failed(&self) -> bool { self.broken.load(Ordering::SeqCst) }
    }
    impl Mic for FakeMic {
        fn start(&self, _: &str, _: Box<dyn Fn(f32) + Send>) -> Result<Box<dyn Recording>, MicError> {
            self.log.lock().push("mic".into());
            Ok(Box::new(FakeRec))
        }
        fn listen(&self, _: &str, sink: Box<dyn FnMut(&[f32]) + Send>) -> Result<Box<dyn Listening>, MicError> {
            self.attempts.lock().push(Instant::now());
            if self.fails.load(Ordering::SeqCst) > 0 {
                self.fails.fetch_sub(1, Ordering::SeqCst);
                return Err(MicError::NoDevice);
            }
            self.log.lock().push("listen".into());
            *self.sink.lock() = Some(sink);
            Ok(Box::new(FakeListening { log: self.log.clone(), sink: self.sink.clone(), broken: self.broken.clone() }))
        }
    }

    /// Faux détecteur : le score d'un pas est la valeur du premier échantillon du pas ; une valeur négative le fait paniquer.
    #[derive(Default)]
    struct FakeDet { buf: Vec<f32> }
    impl Detector for FakeDet {
        fn push(&mut self, s: &[f32]) -> Vec<f32> {
            self.buf.extend_from_slice(s);
            let n = self.buf.len() / WAKE_CHUNK;
            let scores: Vec<f32> = (0..n).map(|i| self.buf[i * WAKE_CHUNK]).collect();
            self.buf.drain(..n * WAKE_CHUNK);
            assert!(scores.iter().all(|s| *s >= 0.0), "inférence en panne");
            scores
        }
        fn reset(&mut self) { self.buf.clear() }
    }

    struct H { f: Fx, conf: Arc<Mutex<Conf>>, mic: Arc<FakeMic>, inputs: Log, ends: Arc<Mutex<Vec<Vec<f32>>>>, label: Arc<Mutex<&'static str>>, send: Arc<dyn Fn(Input) + Send + Sync>, learn: tempfile::TempDir }

    fn base() -> Conf { Conf { installed: true, tts: true, volume: 80, wake: true, user_name: "Nathan".into(), ..Conf::default() } }

    /// Écouteur lancé sur un pipeline réel (faux micro, faux détecteur) ; `refuse` : les `Wake` reçoivent `None`.
    fn h_with(conf: Conf, fails: usize, refuse: bool, load: Option<Load>, turns: Option<Arc<dyn Turns>>) -> H {
        let log = Log::default();
        let mic = Arc::new(FakeMic { log: log.clone(), sink: Sink::default(), fails: AtomicUsize::new(fails), attempts: Mutex::default(), broken: Arc::default() });
        let conf = Arc::new(Mutex::new(conf));
        let f = for_wake(mic.clone(), conf.clone(), log.clone(), turns);
        let (inputs, ends): (Log, Arc<Mutex<Vec<Vec<f32>>>>) = Default::default();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Input>();
        let (p, i2, e2) = (f.p.clone(), inputs.clone(), ends.clone());
        tokio::spawn(async move {
            while let Some(i) = rx.recv().await {
                match i {
                    Input::Wake(reply) if refuse => {
                        i2.lock().push("wake".into());
                        let _ = reply.send(None);
                        continue;
                    }
                    Input::Wake(_) => i2.lock().push("wake".into()),
                    Input::WakeEnd(press, ref s) => {
                        i2.lock().push(format!("wake_end:{press}"));
                        e2.lock().push(s.clone());
                    }
                    Input::Start => i2.lock().push("start".into()),
                    Input::End | Input::Escape | Input::ExpectWake | Input::FalseWake => {}
                }
                p.handle(i).await;
            }
        });
        let send: Arc<dyn Fn(Input) + Send + Sync> = Arc::new(move |i| { let _ = tx.send(i); });
        let load = load.unwrap_or_else(|| Box::new(|| Ok(Box::new(FakeDet::default()) as Box<dyn Detector>)));
        let learn = tempfile::tempdir().unwrap();
        let l = WakeListener::new(f.p.clone(), mic.clone(), load, send.clone(), Some(learn.path().to_path_buf()));
        let label = l.label.clone();
        tokio::spawn(l.run());
        H { f, conf, mic, inputs, ends, label, send, learn }
    }

    fn h(conf: Conf) -> H { h_with(conf, 0, false, None, None) }

    impl H {
        /// `secs` d'audio de valeur `v`, par pas de 80 ms en temps réel simulé (perdu si le micro est fermé).
        async fn feed(&self, v: f32, secs: f32) {
            for _ in 0..(secs / 0.08).round() as usize {
                if let Some(sink) = self.mic.sink.lock().as_mut() {
                    sink(&[v; WAKE_CHUNK]);
                }
                sleep(Duration::from_millis(80)).await;
            }
        }
        fn inputs(&self) -> Vec<String> { self.inputs.lock().clone() }
        fn count(&self, entry: &str) -> usize { self.f.log.lock().iter().filter(|l| *l == entry).count() }
        fn label(&self) -> &'static str { *self.label.lock() }
        fn files(&self, d: &str) -> Vec<std::path::PathBuf> {
            std::fs::read_dir(self.learn.path().join(d)).map_or(Vec::new(), |r| r.flatten().map(|e| e.path()).collect())
        }
        /// Extraits écrits dans `learn/<d>`, une fois finies les écritures lancées : le temps simulé n'avance pas
        /// tant qu'une tâche `spawn_blocking` tourne, donc ce `sleep` attend la fin des écritures en cours.
        async fn clips(&self, d: &str) -> usize {
            sleep(Duration::from_millis(1)).await;
            self.files(d).len()
        }
        /// Attend (2 s au plus, en temps réel) que `learn/<d>` compte `n` extraits.
        async fn wait_clips(&self, d: &str, n: usize) {
            let t = std::time::Instant::now();
            while self.clips(d).await < n && t.elapsed() < std::time::Duration::from_secs(2) {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            assert_eq!(self.clips(d).await, n, "{d}");
        }
        /// « Dis Iris », puis une demande parlée.
        async fn command(&self) {
            self.wake().await;
            self.feed(0.001, 0.4).await;
            self.feed(0.2, 0.5).await;
            self.feed(0.001, 1.2).await;
        }
        /// Armé, puis « Dis Iris » : l'écouteur enregistre la suite.
        async fn wake(&self) {
            self.feed(0.1, 1.0).await;
            assert_eq!(self.label(), "à l'écoute");
            self.feed(0.999, 0.08).await;
        }
    }

    #[tokio::test(start_paused = true)]
    async fn off_by_default_never_opens_mic() {
        let h = h(Conf { wake: false, ..base() });
        h.feed(0.999, 3.0).await;
        assert_eq!(h.count("listen"), 0);
        assert!(h.mic.attempts.lock().is_empty());
        assert_eq!(h.label(), "désactivé");
    }

    #[tokio::test(start_paused = true)]
    async fn nothing_reaches_stt_before_detection() {
        let h = h(base());
        h.feed(0.1, 30.0).await;
        assert_eq!(h.count("listen"), 1);
        assert!(h.inputs().is_empty());
        assert!(!h.f.has("stt"));
    }

    #[tokio::test(start_paused = true)]
    async fn detection_records_then_processes() {
        let h = h(base());
        h.wake().await;
        h.feed(0.001, 0.4).await;
        h.feed(0.2, 0.5).await;
        h.feed(0.001, 1.2).await;
        sleep(Duration::from_secs(1)).await;
        assert_eq!(h.inputs(), ["wake", "wake_end:1"]);
        let s = h.ends.lock()[0].clone();
        assert!(!s.is_empty());
        assert!(s.iter().all(|v| *v != 0.1 && *v != 0.999), "rien d'avant la détection ni du mot");
        assert!(h.f.has("stt") && h.f.has("speak"), "{:?}", h.f.log.lock());
        assert!(!h.f.has("mic"));
    }

    #[tokio::test(start_paused = true)]
    async fn refractory_two_seconds() {
        let h = h_with(base(), 0, true, None, None);
        h.wake().await;
        h.feed(0.1, 0.92).await;
        h.feed(0.999, 0.08).await; // 1 s après le premier pic
        h.feed(0.1, 0.5).await;
        assert_eq!(h.inputs(), ["wake"]);
        h.feed(0.1, 1.0).await;
        h.feed(0.999, 0.08).await; // 2,6 s après
        h.feed(0.1, 0.2).await;
        assert_eq!(h.inputs(), ["wake", "wake"]);
    }

    #[tokio::test(start_paused = true)]
    async fn suspended_while_speaking_and_after() {
        let h = h(base());
        h.feed(0.1, 1.0).await;
        h.f.speaking.store(true, Ordering::SeqCst);
        h.feed(0.1, 1.0).await;
        assert_eq!(h.count("unlisten"), 1, "IRIS parle : micro fermé");
        assert_eq!(h.label(), "en pause");
        h.f.speaking.store(false, Ordering::SeqCst);
        h.feed(0.1, 0.22).await;
        h.feed(0.999, 0.08).await; // +300 ms
        assert!(h.inputs().is_empty());
        h.feed(0.1, 0.32).await;
        h.feed(0.999, 0.08).await; // +700 ms
        h.feed(0.1, 0.16).await;
        assert_eq!(h.inputs(), ["wake"]);
    }

    #[tokio::test(start_paused = true)]
    async fn disabling_mid_recording_discards() {
        for off in [Conf { wake: false, ..base() }, Conf { paused: true, ..base() }] {
            let h = h(base());
            h.wake().await;
            h.feed(0.2, 0.4).await;
            *h.conf.lock() = off;
            h.feed(0.2, 0.16).await;
            assert_eq!(h.count("unlisten"), 1, "micro fermé tout de suite");
            h.feed(0.001, 2.0).await;
            assert_eq!(h.inputs(), ["wake"]);
            assert_eq!(h.f.last_phase(), Some(VoicePhase::Idle));
            assert!(!h.f.has("stt"));
        }
    }

    #[tokio::test(start_paused = true)]
    async fn push_to_talk_preempts_recording() {
        let h = h(base());
        h.wake().await;
        h.feed(0.2, 0.4).await;
        (h.send)(Input::Start);
        h.feed(0.2, 0.4).await;
        h.feed(0.001, 2.0).await;
        let log = h.f.log.lock().clone();
        let at = |e: &str| log.iter().position(|l| l == e).unwrap_or_else(|| panic!("{e} absent : {log:?}"));
        assert!(at("unlisten") < at("mic"), "{log:?}");
        assert_eq!(h.inputs(), ["wake", "start"]);
        assert!(h.ends.lock().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn no_speech_after_wake_is_silent() {
        let h = h(base());
        h.wake().await;
        h.feed(0.001, 5.0).await;
        assert_eq!(h.inputs(), ["wake"]);
        assert!(h.f.toasts().is_empty() && !h.f.has("stt"));
        assert_eq!(h.f.last_phase(), Some(VoicePhase::Idle));
        assert_eq!(h.label(), "à l'écoute");
    }

    #[tokio::test(start_paused = true)]
    async fn mic_failure_backs_off_with_one_toast() {
        let h = h_with(base(), 5, false, None, None);
        sleep(Duration::from_secs(2)).await;
        assert_eq!(h.label(), "micro indisponible");
        sleep(Duration::from_secs(30)).await;
        let at = h.mic.attempts.lock().clone();
        assert_eq!(at.len(), 6, "5 échecs puis l'ouverture");
        for (i, w) in at.windows(2).enumerate() {
            let gap = (w[1] - w[0]).as_secs_f32();
            assert!((gap - (1 << i) as f32).abs() <= 0.1, "attente {i} : {gap}");
        }
        assert_eq!(h.f.toasts().len(), 1);
        assert_eq!(h.label(), "à l'écoute");
        // Micro perdu pendant l'écoute, après 10 s sans souci : nouvelle panne, un toast de plus, nouvel essai 1 s après.
        sleep(Duration::from_secs(10)).await;
        h.mic.broken.store(true, Ordering::SeqCst);
        sleep(Duration::from_millis(200)).await;
        assert_eq!(h.count("unlisten"), 1);
        assert_eq!(h.f.toasts().len(), 2);
        h.mic.broken.store(false, Ordering::SeqCst);
        sleep(Duration::from_millis(1100)).await;
        assert_eq!(h.count("listen"), 2);
    }

    #[tokio::test(start_paused = true)]
    async fn fullscreen_or_pause_suspends() {
        let h = h(base());
        h.feed(0.1, 0.4).await;
        for (n, c) in [Conf { fullscreen: true, ..base() }, Conf { paused: true, ..base() }].into_iter().enumerate() {
            *h.conf.lock() = c;
            h.feed(0.999, 0.4).await;
            assert_eq!(h.count("unlisten"), n + 1);
            assert_eq!(h.label(), "en pause");
            *h.conf.lock() = base();
            h.feed(0.1, 0.4).await;
            assert_eq!(h.count("listen"), n + 2, "réarmé");
        }
        assert!(h.inputs().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn missing_models_disable_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().to_path_buf();
        let h = h_with(base(), 0, false, Some(Box::new(move || load_models(&p))), None);
        sleep(Duration::from_secs(2)).await;
        assert_eq!(h.label(), "modèle absent");
        assert!(h.mic.attempts.lock().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn detector_panic_disables_without_crash() {
        let h = h(base());
        h.feed(0.1, 0.4).await;
        h.feed(-1.0, 0.4).await;
        assert_eq!(h.label(), "modèle absent");
        assert_eq!(h.count("unlisten"), 1);
        h.feed(0.999, 1.0).await;
        assert!(h.inputs().is_empty());
    }

    /// Tour qui dure 3 s (cerveau lent) puis répond.
    struct SlowTurns(Log);
    #[async_trait::async_trait]
    impl Turns for SlowTurns {
        async fn run(&self, text: String) -> Result<TurnResult, TurnError> {
            sleep(Duration::from_secs(3)).await;
            self.0.lock().push(format!("turn:{text}"));
            Ok(TurnResult { shown: vec![Segment::Text { text: "Il fait beau.".into() }], pending: false, first_tool: None })
        }
    }

    #[tokio::test(start_paused = true)]
    async fn no_wake_during_turn() {
        let log = Log::default();
        let h = h_with(base(), 0, false, None, Some(Arc::new(SlowTurns(log.clone()))));
        h.wake().await;
        h.feed(0.001, 0.4).await;
        h.feed(0.2, 0.5).await;
        h.feed(0.001, 1.0).await;
        assert_eq!(h.inputs(), ["wake", "wake_end:1"]);
        // Tour en cours (Whisper fini, phase Idle) : un « Dis Iris » n'est pas entendu.
        h.feed(0.1, 0.5).await;
        h.feed(0.999, 0.08).await;
        h.feed(0.1, 0.5).await;
        assert!(log.lock().is_empty(), "le tour n'est pas fini");
        assert_eq!(h.inputs(), ["wake", "wake_end:1"]);
        assert_eq!(h.count("listen"), 1, "micro pas rouvert pendant le tour");
        sleep(Duration::from_secs(3)).await;
        assert_eq!(*log.lock(), ["turn:Quel temps fait-il ?"]);
        assert_eq!(h.f.toasts(), ["Il fait beau."]);
        assert!(h.f.has("speak"));
        assert_eq!(h.count("listen"), 2, "réarmé après le tour");
        assert_eq!(h.label(), "à l'écoute");
    }

    #[tokio::test(start_paused = true)]
    async fn nothing_written_when_learning_off() {
        let h = h(base());
        h.command().await;
        assert_eq!(h.inputs(), ["wake", "wake_end:1"]);
        h.feed(0.1, 3.0).await;
        (h.send)(Input::FalseWake);
        h.feed(0.1, 11.0).await;
        assert_eq!(h.clips("").await, 0, "le dossier reste vide");
        // Les écritures marchent bien : le mode allumé, le même scénario écrit son positif, et lui seul.
        h.conf.lock().learning = true;
        h.command().await;
        h.feed(0.1, 11.0).await;
        h.wait_clips("pos", 1).await;
        assert_eq!(h.clips("neg").await, 0);
    }

    #[tokio::test(start_paused = true)]
    async fn learning_keeps_positive_after_command() {
        let h = h(Conf { learning: true, ..base() });
        h.command().await;
        assert_eq!(h.inputs(), ["wake", "wake_end:1"]);
        h.feed(0.1, 5.0).await;
        assert_eq!(h.clips("pos").await, 0, "un faux réveil peut encore être signalé");
        h.feed(0.1, 6.0).await;
        h.wait_clips("pos", 1).await;
        assert_eq!(h.clips("neg").await, 0);
        // Mode coupé : collecteur et anneau détruits, plus rien n'est écrit.
        *h.conf.lock() = base();
        h.command().await;
        h.feed(0.1, 11.0).await;
        assert_eq!((h.clips("pos").await, h.clips("neg").await), (1, 0));
    }

    #[tokio::test(start_paused = true)]
    async fn false_wake_button_writes_negative() {
        let h = h_with(Conf { learning: true, ..base() }, 0, false, None, Some(Arc::new(SlowTurns(Log::default()))));
        h.command().await;
        h.feed(0.1, 4.0).await; // tour de 3 s, micro fermé, puis réarmé
        assert!(h.count("unlisten") >= 1, "le tour d'IRIS a fermé le micro");
        (h.send)(Input::FalseWake);
        h.feed(0.1, 0.4).await;
        h.wait_clips("neg", 1).await;
        // Pris au réveil : rien de la demande (0,2) ni d'après.
        let wav = std::fs::read(&h.files("neg")[0]).unwrap();
        let cmd = (0.2f32 * 32767.0) as i16;
        assert!(wav[44..].chunks(2).all(|b| i16::from_le_bytes([b[0], b[1]]) != cmd));
        h.feed(0.1, 11.0).await;
        assert_eq!((h.clips("pos").await, h.clips("neg").await), (0, 1), "aucun positif pour ce réveil");
    }

    #[tokio::test(start_paused = true)]
    async fn expect_wake_button_keeps_best_score() {
        let h = h(Conf { learning: true, ..base() });
        h.feed(0.01, 1.0).await;
        (h.send)(Input::ExpectWake);
        h.feed(0.01, 0.4).await;
        h.feed(0.2, 0.08).await; // « Dis Iris » pas reconnu
        h.feed(0.01, 6.0).await;
        assert!(h.inputs().is_empty());
        h.wait_clips("pos", 1).await;
        assert_eq!(h.clips("neg").await, 0);
    }
}
