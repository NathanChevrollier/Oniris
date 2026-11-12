//! Push-to-talk : micro → Whisper → tour d'IRIS → toast de la pilule (ou panneau) et voix de Piper.
//! La logique vit dans `Pipeline`, derrière des traits, testable sans Tauri. Journal : durées et longueurs seulement.
use super::capture::{CpalMic, Mic, Recording};
use super::install::voice_dir;
use super::piper::Speaker;
use super::whisper::WhisperProcess;
use crate::modules::iris::{TurnError, TurnResult};
use crate::state::AppState;
use async_trait::async_trait;
use oniris_core::iris::Segment;
use oniris_core::system::NOT_INSTALLED;
use oniris_core::voice::assets::{self, AssetKind};
use oniris_core::voice::speech::{speakable, TO_PANEL};
use oniris_core::voice::stt::{self, SttError};
use oniris_core::voice::audio;
use parking_lot::Mutex;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

/// En deçà de cette durée de parole, l'appui est ignoré.
const MIN_SPEECH_MS: u32 = 300;
const TOAST_MAX: usize = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum VoicePhase { Idle, Listening, Transcribing, Speaking }

/// Mesures d'un tour vocal (sans contenu).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Timing { pub speech_to_text_ms: u64, pub text_to_tool_ms: Option<u64>, pub total_ms: u64 }

/// Ce que le pipeline montre : phase (et niveau du micro), toast de la pilule, ouverture du panneau, mesures.
/// `WakeListening` : écoute ouverte par le mot d'activation (`by_wake` côté interface).
#[derive(Debug, Clone, PartialEq)]
pub enum Ui { Phase(VoicePhase, Option<f32>), WakeListening, Toast(String), OpenPanel, Timing(Timing) }

/// Réglages et état relus à chaque appui.
#[derive(Debug, Clone, Default)]
pub struct Conf { pub paused: bool, pub installed: bool, pub fullscreen: bool, pub device: String, pub tts: bool, pub volume: u8, pub user_name: String, pub wake: bool, pub learning: bool }

#[async_trait]
pub trait Stt: Send + Sync {
    /// Démarre Whisper s'il ne tourne pas (préchauffage pendant l'écoute).
    async fn warm(&self);
    async fn transcribe(&self, wav: Vec<u8>) -> Result<String, SttError>;
    /// Fin d'une transcription : en plein écran, Whisper s'arrête tout de suite.
    async fn finish(&self, fullscreen: bool);
}

#[async_trait]
pub trait Turns: Send + Sync {
    /// Même tour qu'`iris_ask(text, false, false)`.
    async fn run(&self, text: String) -> Result<TurnResult, TurnError>;
}

/// Appui en cours : son numéro (un nouvel appui rend les suites de l'ancien muettes), son tour en cours
/// (de l'appui jusqu'à la fin de `process`, voix comprise) et l'enregistrement.
#[derive(Default)]
struct State { press: u64, busy: bool, rec: Option<Box<dyn Recording>> }

/// Fin du tour de `press` quand `process` se termine, quel que soit le chemin de sortie.
struct TurnGuard(Arc<Pipeline>, u64);
impl Drop for TurnGuard {
    fn drop(&mut self) { self.0.done(self.1) }
}

type Recorded = (u64, Vec<f32>);

pub struct Pipeline {
    mic: Arc<dyn Mic>,
    stt: Arc<dyn Stt>,
    speaker: Arc<dyn Speaker>,
    turns: Arc<dyn Turns>,
    pub(super) conf: Arc<dyn Fn() -> Conf + Send + Sync>,
    /// Appelée sous le verrou d'état pour les phases : ne doit jamais rappeler le pipeline.
    ui: Arc<dyn Fn(Ui) + Send + Sync>,
    /// Vrai pendant Speaking (lu par le crochet clavier pour Échap).
    pub(super) speaking: &'static AtomicBool,
    /// Vrai tant que l'écouteur du mot d'activation tient le micro.
    pub(super) wake_open: Arc<AtomicBool>,
    /// Micro fermé d'office au-delà (PttEnd perdu) : jamais d'écoute sans touche tenue.
    max_hold: Duration,
    state: Mutex<State>,
    /// Vidé par l'écouteur à chaque pas.
    pub(super) marks: Mutex<Vec<Mark>>,
}

/// Maintien maximal de la touche.
const MAX_HOLD: Duration = Duration::from_secs(30);
/// Attente maximale de la fermeture du flux de l'écouteur avant d'ouvrir le micro du push-to-talk.
const WAKE_CLOSE_WAIT: Duration = Duration::from_millis(300);

fn ms(d: Duration) -> u64 { d.as_millis() as u64 }

/// Texte rendu de la réponse, 80 caractères au plus.
fn toast_text(shown: &[Segment]) -> String {
    let text: String = shown.iter().map(|s| match s { Segment::Text { text } | Segment::Untrusted { text, .. } => text.as_str() }).collect();
    let text = text.trim();
    if text.chars().count() <= TOAST_MAX {
        return text.to_string();
    }
    format!("{}…", text.chars().take(TOAST_MAX - 1).collect::<String>())
}

pub enum Input {
    Start,
    End,
    Escape,
    /// Mot d'activation détecté : la réponse porte le numéro d'appui, ou `None` si le pipeline refuse.
    Wake(tokio::sync::oneshot::Sender<Option<u64>>),
    /// Demande enregistrée après le mot d'activation.
    WakeEnd(u64, Vec<f32>),
    /// Boutons du mode apprentissage : « Je vais dire Iris » et « Faux réveil ».
    ExpectWake,
    FalseWake,
}

/// Entrées vues par le mode apprentissage, relevées par l'écouteur à chaque pas.
pub enum Mark { Start, Escape, ExpectWake, FalseWake }

impl Pipeline {
    fn current(&self, press: u64) -> bool { self.state.lock().press == press }

    /// Phase émise seulement si `press` est toujours l'appui courant (vérifié et émis sous le même verrou).
    fn phase(&self, press: u64, p: VoicePhase) {
        let s = self.state.lock();
        if s.press == press {
            self.speaking.store(p == VoicePhase::Speaking, Ordering::SeqCst);
            (self.ui)(Ui::Phase(p, None));
        }
    }

    /// Appui courant et tour en cours (relus par l'écouteur à chaque pas).
    pub(super) fn snapshot(&self) -> (u64, bool) {
        let s = self.state.lock();
        (s.press, s.busy)
    }

    /// Le tour de `press` est fini (s'il est toujours le courant).
    fn done(&self, press: u64) {
        let mut s = self.state.lock();
        if s.press == press {
            s.busy = false;
        }
    }

    pub(super) fn toast(&self, text: impl Into<String>) { (self.ui)(Ui::Toast(text.into())) }

    /// Une entrée du crochet, dans l'ordre d'arrivée ; la suite d'un appui part en tâche de fond.
    pub async fn handle(self: &Arc<Self>, i: Input) {
        let mark = match i {
            Input::Start => Some(Mark::Start),
            Input::Escape => Some(Mark::Escape),
            Input::ExpectWake => Some(Mark::ExpectWake),
            Input::FalseWake => Some(Mark::FalseWake),
            _ => None,
        };
        if let Some(m) = mark {
            self.marks.lock().push(m);
        }
        match i {
            Input::Start => self.start().await,
            Input::End => {
                if let Some(rec) = self.end().await {
                    tokio::spawn(self.clone().process(rec));
                }
            }
            Input::Escape => self.speaker.stop(),
            Input::Wake(reply) => {
                let _ = reply.send(self.wake_begin());
            }
            Input::WakeEnd(press, samples) => {
                if self.current(press) {
                    tokio::spawn(self.clone().process((press, samples)));
                }
            }
            Input::ExpectWake | Input::FalseWake => {}
        }
    }

    /// Début d'une demande par le mot d'activation : comme `start`, sans ouvrir le micro (l'écouteur enregistre).
    /// `None` si la voix est en pause, non installée, ou si un tour est en cours (push-to-talk ou réponse d'IRIS).
    pub fn wake_begin(self: &Arc<Self>) -> Option<u64> {
        let conf = (self.conf)();
        if conf.paused || !conf.installed {
            return None;
        }
        let press = {
            let mut s = self.state.lock();
            if s.busy {
                return None;
            }
            s.busy = true;
            s.press += 1;
            s.rec = None;
            s.press
        };
        self.speaker.stop();
        self.phase(press, VoicePhase::Listening);
        (self.ui)(Ui::WakeListening);
        let stt = self.stt.clone();
        tokio::spawn(async move { stt.warm().await });
        Some(press)
    }

    /// Demande par le mot abandonnée (rien dit, réglage coupé) : la pilule revient au repos, sans toast.
    pub(super) fn wake_abort(&self, press: u64) {
        self.phase(press, VoicePhase::Idle);
        self.done(press);
    }

    /// Début d'appui : coupe IRIS si elle parle, puis ouvre le micro et préchauffe Whisper.
    pub async fn start(self: &Arc<Self>) {
        let press = {
            let mut s = self.state.lock();
            s.press += 1;
            s.busy = true;
            s.rec = None;
            s.press
        };
        self.speaker.stop(); // barge-in : avant d'ouvrir le micro
        let conf = (self.conf)();
        if conf.paused || !conf.installed {
            self.phase(press, VoicePhase::Idle); // une voix coupée ne laisse pas la pilule en Speaking
            self.done(press);
            return self.toast(if conf.paused { "Voix en pause" } else { "Voix non installée" });
        }
        self.phase(press, VoicePhase::Listening);
        // L'écouteur voit la phase changer et ferme son flux : un seul micro ouvert à la fois.
        let t = tokio::time::Instant::now();
        while self.wake_open.load(Ordering::SeqCst) && t.elapsed() < WAKE_CLOSE_WAIT {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let (mic, ui) = (self.mic.clone(), self.ui.clone());
        // `Mic::start` bloque le temps d'ouvrir le périphérique.
        let opened = tokio::task::spawn_blocking(move || mic.start(&conf.device, Box::new(move |l| ui(Ui::Phase(VoicePhase::Listening, Some(l)))))).await;
        match opened {
            Ok(Ok(rec)) => {
                self.state.lock().rec = Some(rec);
                let stt = self.stt.clone();
                tokio::spawn(async move { stt.warm().await });
                let p = self.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(p.max_hold).await;
                    if let Some(rec) = p.take(Some(press)).await {
                        tracing::warn!("voix : maintien trop long, micro fermé");
                        p.process(rec).await;
                    }
                });
            }
            Ok(Err(e)) => {
                self.phase(press, VoicePhase::Idle);
                self.done(press);
                self.toast(e.to_string());
            }
            Err(_) => {
                self.phase(press, VoicePhase::Idle);
                self.done(press);
                self.toast("Le micro n'a pas pu démarrer.");
            }
        }
    }

    /// Fin d'appui : ferme le micro ; `None` si rien n'était enregistré (pause, micro absent, délai dépassé).
    pub async fn end(&self) -> Option<Recorded> {
        self.take(None).await
    }

    /// Ferme le micro de l'appui courant (de `only` seulement, s'il est donné).
    async fn take(&self, only: Option<u64>) -> Option<Recorded> {
        let (press, rec) = {
            let mut s = self.state.lock();
            if only.is_some_and(|p| p != s.press) {
                return None;
            }
            (s.press, s.rec.take()?)
        };
        let samples = tokio::task::spawn_blocking(move || rec.stop()).await.ok()?;
        Some((press, samples))
    }

    /// Texte entendu, ou `None` (trop court, rien de net, erreur déjà signalée).
    async fn hear(&self, press: u64, samples: &[f32], speech: u32) -> Option<String> {
        if speech < MIN_SPEECH_MS {
            self.phase(press, VoicePhase::Idle);
            return None;
        }
        self.phase(press, VoicePhase::Transcribing);
        let raw = self.stt.transcribe(audio::wav_16k_mono(samples)).await;
        self.phase(press, VoicePhase::Idle);
        match raw {
            Ok(raw) => stt::clean(&raw),
            Err(e) => {
                self.toast(format!("Je n'ai pas pu t'entendre : {e}."));
                None
            }
        }
    }

    /// Transcription, tour d'IRIS, puis toast et voix (ou panneau).
    pub async fn process(self: Arc<Self>, (press, samples): Recorded) {
        let _turn = TurnGuard(self.clone(), press);
        let t0 = Instant::now();
        let speech = audio::speech_ms(&samples, 0.01);
        let text = self.hear(press, &samples, speech).await;
        // Whisper a été préchauffé : en plein écran, il s'arrête quelle que soit l'issue.
        self.stt.finish((self.conf)().fullscreen).await;
        let Some(text) = text else { return };
        let t1 = Instant::now();
        tracing::info!(speech_ms = speech, ms = ms(t1 - t0), chars = text.chars().count(), "voix transcrite");
        if !self.current(press) {
            return; // un nouvel appui a pris la main
        }
        let r = match self.turns.run(text).await {
            Ok(r) => r,
            Err(TurnError::Aborted) => return, // Échap du panneau ou demande tapée : silence
            Err(TurnError::Failed) => {
                if self.current(press) {
                    self.toast("Je n'ai pas pu traiter ta demande.");
                }
                return;
            }
        };
        let timing = Timing {
            speech_to_text_ms: ms(t1 - t0),
            text_to_tool_ms: r.first_tool.map(|t| ms(t.saturating_duration_since(t1))),
            total_ms: ms(t0.elapsed()),
        };
        (self.ui)(Ui::Timing(timing));
        if !self.current(press) {
            return;
        }
        let conf = (self.conf)();
        // P6-R7 : jamais de panneau par-dessus un jeu plein écran.
        let to_panel = || if conf.fullscreen { self.toast("Réponse prête dans le panneau") } else { (self.ui)(Ui::OpenPanel) };
        if r.pending {
            return to_panel();
        }
        let Some(say) = speakable(&r.shown, &conf.user_name) else { return }; // réponse vide : rien à montrer
        if say.starts_with(TO_PANEL) {
            return to_panel();
        }
        self.toast(toast_text(&r.shown));
        if !conf.tts {
            return;
        }
        match self.speaker.speak(&say, conf.volume).await {
            Ok(h) => {
                // Un appui arrivé pendant l'ouverture de la sortie : IRIS ne parle pas par-dessus l'écoute.
                if !self.current(press) {
                    return h.stop();
                }
                self.phase(press, VoicePhase::Speaking);
                h.done().await;
                self.phase(press, VoicePhase::Idle);
            }
            Err(e) => self.toast(format!("IRIS ne peut pas parler : {e}.")),
        }
    }
}

// --- Branchement Tauri ---

/// Lu par le crochet clavier : Échap n'est observé que pendant que IRIS parle.
pub static SPEAKING: AtomicBool = AtomicBool::new(false);

/// Serveur Whisper unique, aussi arrêté à l'entrée en plein écran.
static WHISPER: std::sync::LazyLock<Arc<WhisperProcess>> = std::sync::LazyLock::new(Arc::default);

/// Entrée en plein écran : Whisper libère sa mémoire tout de suite.
pub fn unload_on_fullscreen() {
    tauri::async_runtime::spawn(async { WHISPER.stop().await });
}
type Voice = (tokio::sync::mpsc::UnboundedSender<Input>, Arc<Pipeline>);
static VOICE: OnceLock<Voice> = OnceLock::new();

/// Entrée et pipeline, pour l'écouteur du mot d'activation.
pub(super) fn voice() -> Option<&'static Voice> { VOICE.get() }

/// Sans attente : appelable depuis le crochet clavier.
pub fn send(i: Input) {
    if let Some((tx, _)) = VOICE.get() {
        let _ = tx.send(i);
    }
}

fn label(c: &Conf) -> &'static str {
    if !c.installed {
        NOT_INSTALLED
    } else if c.paused {
        "en pause"
    } else {
        "prête"
    }
}

/// Statut « voix » de l'état du PC.
pub fn status_label() -> &'static str {
    VOICE.get().map_or(NOT_INSTALLED, |(_, p)| label(&(p.conf)()))
}

/// État de l'écoute du mot d'activation, pour la feuille IRIS.
#[tauri::command]
pub fn voice_wake_state() -> String { super::wake::wake_state().into() }

/// Boutons de la pilule en mode apprentissage : « Je vais dire Iris » et « Faux réveil ».
#[tauri::command]
pub fn voice_expect_wake() { send(Input::ExpectWake) }

#[tauri::command]
pub fn voice_false_wake() { send(Input::FalseWake) }

/// (positifs, négatifs) gardés par le mode apprentissage.
#[tauri::command]
pub fn voice_learn_count() -> (u32, u32) { super::learn::learn_dir().map_or((0, 0), |d| super::learn::count_in(&d)) }

/// Supprime les extraits du mode apprentissage ; renvoie leur nombre.
#[tauri::command]
pub fn voice_learn_clear() -> u32 { super::learn::learn_dir().map_or(0, |d| super::learn::clear_in(&d)) }

/// Modèle Whisper choisi dans les réglages.
fn model_path(dir: &std::path::Path, stt_model: &str) -> PathBuf {
    let dest = assets::needed(stt_model).into_iter().find_map(|a| match a.kind {
        AssetKind::File { dest } if a.key.starts_with("stt-") => Some(dest),
        _ => None,
    });
    dest.map(|d| dir.join(d)).unwrap_or_default()
}

struct Whisper { proc: Arc<WhisperProcess>, http: reqwest::Client, app: AppHandle }

impl Whisper {
    async fn base(&self) -> Result<String, SttError> {
        let (model, idle) = {
            let st = self.app.state::<AppState>();
            let s = st.settings.read();
            (s.voice_stt_model.clone(), Duration::from_secs(s.voice_unload_minutes as u64 * 60))
        };
        let dir = voice_dir();
        self.proc.ensure(&dir, &model_path(&dir, &model), idle).await
    }
}

#[async_trait]
impl Stt for Whisper {
    async fn warm(&self) {
        let _ = self.base().await;
    }
    async fn transcribe(&self, wav: Vec<u8>) -> Result<String, SttError> {
        stt::transcribe(&self.http, &self.base().await?, wav).await
    }
    async fn finish(&self, fullscreen: bool) {
        self.proc.finish(fullscreen).await
    }
}

struct VoiceTurns(AppHandle);

#[async_trait]
impl Turns for VoiceTurns {
    async fn run(&self, text: String) -> Result<TurnResult, TurnError> {
        crate::modules::iris::voice_turn(&self.0, text).await
    }
}

#[derive(Serialize, Clone)]
struct PhaseEvent { phase: VoicePhase, #[serde(skip_serializing_if = "Option::is_none")] level: Option<f32>, by_wake: bool }

#[derive(Serialize, Clone)]
struct Toast { text: String }

fn emit_ui(app: &AppHandle, u: Ui) {
    match u {
        Ui::Phase(phase, level) => {
            let _ = app.emit("oniris://voice", PhaseEvent { phase, level, by_wake: false });
        }
        Ui::WakeListening => {
            let _ = app.emit("oniris://voice", PhaseEvent { phase: VoicePhase::Listening, level: None, by_wake: true });
        }
        Ui::Toast(text) => {
            let _ = app.emit_to("pill", "oniris://toast", Toast { text });
        }
        Ui::OpenPanel => {
            let a = app.clone();
            let _ = app.run_on_main_thread(move || {
                if let Err(e) = crate::windows::open_panel(&a) {
                    tracing::error!("voix : ouverture du panneau impossible : {e}");
                }
            });
        }
        Ui::Timing(t) => {
            tracing::info!(speech_to_text_ms = t.speech_to_text_ms, text_to_tool_ms = ?t.text_to_tool_ms, total_ms = t.total_ms, "voix : mesures");
            #[cfg(any(debug_assertions, feature = "perf"))]
            let _ = app.emit("oniris://voice-timing", t);
        }
    }
}

/// Lance la boucle du pipeline : les appuis sont traités dans l'ordre, la suite d'un appui en tâche de fond
/// (un nouvel appui peut ainsi couper IRIS pendant qu'elle parle).
pub fn start(app: &AppHandle) {
    let h = app.clone();
    let conf = Arc::new(move || {
        let st = h.state::<AppState>();
        let s = st.settings.read().clone();
        Conf {
            paused: st.voice_paused.load(Ordering::SeqCst),
            installed: assets::status(&voice_dir(), &s.voice_stt_model).installed,
            fullscreen: crate::fullscreen::active(),
            device: s.voice_input_device,
            tts: s.voice_tts,
            volume: s.voice_volume,
            user_name: s.user_name,
            wake: s.voice_wake,
            learning: s.voice_wake_learning,
        }
    });
    let h = app.clone();
    let p = Arc::new(Pipeline {
        mic: Arc::new(CpalMic),
        // Jamais de proxy : l'audio ne quitte pas 127.0.0.1.
        stt: Arc::new(Whisper { proc: WHISPER.clone(), http: reqwest::Client::builder().no_proxy().build().expect("client HTTP local"), app: app.clone() }),
        speaker: app.state::<AppState>().speaker.clone(),
        turns: Arc::new(VoiceTurns(app.clone())),
        conf,
        ui: Arc::new(move |u| emit_ui(&h, u)),
        speaking: &SPEAKING,
        wake_open: Arc::default(),
        max_hold: MAX_HOLD,
        state: Mutex::default(),
        marks: Mutex::default(),
    });
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    if VOICE.set((tx, p.clone())).is_err() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        while let Some(i) = rx.recv().await {
            p.handle(i).await;
        }
    });
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::modules::voice::capture::MicError;
    use crate::modules::voice::piper::{SpeakHandle, TtsError};
    use oniris_core::iris::{Agent, BrainError, ChatReply, ChatRequest, Executor, LocalBrain, PromptCtx, ToolCallMsg, TurnInput};
    use oniris_core::tools::helpers::{low, spec};
    use oniris_core::tools::{Brain, ConfirmationText, Origin, Permission, Provenance, Tool, ToolBroker, ToolError, ToolResult, ToolSpec};
    use serde_json::{json, Value};
    use std::sync::atomic::AtomicUsize;

    pub(crate) type Log = Arc<Mutex<Vec<String>>>;

    struct FakeMic { log: Log, secs: f32, fail: bool }
    struct FakeRec(Vec<f32>);
    impl Recording for FakeRec {
        fn stop(self: Box<Self>) -> Vec<f32> { self.0 }
    }
    impl Mic for FakeMic {
        fn start(&self, _: &str, _: Box<dyn Fn(f32) + Send>) -> Result<Box<dyn Recording>, MicError> {
            self.log.lock().push("mic".into());
            if self.fail {
                return Err(MicError::NoDevice);
            }
            Ok(Box::new(FakeRec(vec![0.5; (16_000.0 * self.secs) as usize])))
        }
        fn listen(&self, _: &str, _: Box<dyn FnMut(&[f32]) + Send>) -> Result<Box<dyn crate::modules::voice::capture::Listening>, MicError> {
            Err(MicError::NoDevice)
        }
    }

    struct FakeStt { log: Log, text: String }
    #[async_trait]
    impl Stt for FakeStt {
        async fn warm(&self) {}
        async fn transcribe(&self, _: Vec<u8>) -> Result<String, SttError> {
            self.log.lock().push("stt".into());
            Ok(self.text.clone())
        }
        async fn finish(&self, fullscreen: bool) {
            self.log.lock().push(format!("finish:{fullscreen}"));
        }
    }

    /// Voix qui ne finit que sur `stop()` (`long`) ou aussitôt.
    struct FakeSpeaker { log: Log, long: bool, playing: Mutex<Option<tokio::sync::watch::Sender<bool>>> }
    #[async_trait::async_trait]
    impl Speaker for FakeSpeaker {
        async fn speak(&self, _: &str, _: u8) -> Result<SpeakHandle, TtsError> {
            self.log.lock().push("speak".into());
            let (h, done) = SpeakHandle::new();
            if self.long {
                *self.playing.lock() = Some(done);
            }
            Ok(h)
        }
        fn stop(&self) {
            if let Some(done) = self.playing.lock().take() {
                self.log.lock().push("stop".into());
                let _ = done.send(true);
            }
        }
    }

    struct FakeTurns { log: Log, reply: &'static str, pending: bool }
    #[async_trait]
    impl Turns for FakeTurns {
        async fn run(&self, text: String) -> Result<TurnResult, TurnError> {
            self.log.lock().push(format!("turn:{text}"));
            Ok(TurnResult { shown: vec![Segment::Text { text: self.reply.into() }], pending: self.pending, first_tool: None })
        }
    }

    pub(crate) struct Fx { pub(crate) p: Arc<Pipeline>, pub(crate) log: Log, pub(crate) ui: Arc<Mutex<Vec<Ui>>>, pub(crate) speaking: &'static AtomicBool, _dir: Option<tempfile::TempDir> }

    impl Fx {
        pub(crate) fn toasts(&self) -> Vec<String> {
            self.ui.lock().iter().filter_map(|u| match u { Ui::Toast(t) => Some(t.clone()), _ => None }).collect()
        }
        pub(crate) fn has(&self, entry: &str) -> bool { self.log.lock().iter().any(|l| l == entry) }
        pub(crate) fn last_phase(&self) -> Option<VoicePhase> {
            self.ui.lock().iter().rev().find_map(|u| match u { Ui::Phase(p, None) => Some(*p), _ => None })
        }
        /// Lance un appui dont la voix dure jusqu'à `stop()` ; rend la tâche de sa suite une fois en Speaking.
        async fn speaking_press(&self) -> tokio::task::JoinHandle<()> {
            self.p.start().await;
            let rec = self.p.end().await.unwrap();
            let task = tokio::spawn(self.p.clone().process(rec));
            for _ in 0..100 {
                if self.last_phase() == Some(VoicePhase::Speaking) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            assert_eq!(self.last_phase(), Some(VoicePhase::Speaking));
            assert!(self.speaking.load(Ordering::SeqCst));
            task
        }
        /// Un appui complet, suite comprise.
        async fn press(&self) {
            self.p.start().await;
            if let Some(rec) = self.p.end().await {
                self.p.clone().process(rec).await;
            }
        }
    }

    struct Opts { conf: Conf, secs: f32, mic_fails: bool, heard: &'static str, long_speech: bool, turns: Option<Arc<dyn Turns>>, max_hold: Duration }

    impl Default for Opts {
        fn default() -> Self {
            let conf = Conf { installed: true, tts: true, volume: 80, user_name: "Nathan".into(), ..Conf::default() };
            Self { conf, secs: 1.0, mic_fails: false, heard: "Quel temps fait-il ?", long_speech: false, turns: None, max_hold: Duration::from_secs(30) }
        }
    }

    fn fx(o: Opts, log: Log) -> Fx {
        let ui: Arc<Mutex<Vec<Ui>>> = Arc::default();
        let u = ui.clone();
        let conf = o.conf;
        let speaking: &'static AtomicBool = Box::leak(Box::new(AtomicBool::new(false)));
        let turns = o.turns.unwrap_or_else(|| Arc::new(FakeTurns { log: log.clone(), reply: "Il fait beau.", pending: false }));
        let p = Arc::new(Pipeline {
            mic: Arc::new(FakeMic { log: log.clone(), secs: o.secs, fail: o.mic_fails }),
            stt: Arc::new(FakeStt { log: log.clone(), text: o.heard.into() }),
            speaker: Arc::new(FakeSpeaker { log: log.clone(), long: o.long_speech, playing: Mutex::default() }),
            turns,
            conf: Arc::new(move || conf.clone()),
            ui: Arc::new(move |x| u.lock().push(x)),
            speaking,
            wake_open: Arc::default(),
            max_hold: o.max_hold,
            state: Mutex::default(),
            marks: Mutex::default(),
        });
        Fx { p, log, ui, speaking, _dir: None }
    }

    fn simple(o: Opts) -> Fx { fx(o, Log::default()) }

    /// Pipeline de l'écouteur : micro et réglages fournis, même journal que le micro.
    pub(crate) fn for_wake(mic: Arc<dyn Mic>, conf: Arc<Mutex<Conf>>, log: Log, turns: Option<Arc<dyn Turns>>) -> Fx {
        let mut f = fx(Opts { turns, ..Opts::default() }, log);
        let p = Arc::get_mut(&mut f.p).unwrap();
        p.mic = mic;
        p.conf = Arc::new(move || conf.lock().clone());
        f
    }

    #[tokio::test]
    async fn wake_turn_goes_through_process() {
        let f = simple(Opts::default());
        let press = f.p.wake_begin().expect("appui par le mot");
        f.p.clone().process((press, vec![0.5; 16_000])).await;
        assert_eq!(*f.log.lock(), ["stt", "finish:false", "turn:Quel temps fait-il ?", "speak"]);
        assert_eq!(f.toasts(), ["Il fait beau."]);
        let phases: Vec<VoicePhase> = f.ui.lock().iter().filter_map(|u| match u { Ui::Phase(p, None) => Some(*p), _ => None }).collect();
        assert_eq!(phases, [VoicePhase::Listening, VoicePhase::Transcribing, VoicePhase::Idle, VoicePhase::Speaking, VoicePhase::Idle]);
        assert!(!f.has("mic"), "le mot n'ouvre pas le micro du push-to-talk");
    }

    #[tokio::test]
    async fn voix_en_pause_ou_non_installee_le_micro_n_est_jamais_ouvert() {
        let f = simple(Opts { conf: Conf { paused: true, ..Opts::default().conf }, ..Opts::default() });
        f.press().await;
        assert!(!f.has("mic"));
        assert_eq!(f.toasts(), ["Voix en pause"]);
        let f = simple(Opts { conf: Conf { installed: false, ..Opts::default().conf }, ..Opts::default() });
        f.press().await;
        assert!(!f.has("mic"));
        assert_eq!(f.toasts(), ["Voix non installée"]);
        assert_eq!(label(&Conf { installed: true, paused: true, ..Conf::default() }), "en pause");
        assert_eq!(label(&Conf { installed: true, ..Conf::default() }), "prête");
        assert_eq!(label(&Conf::default()), NOT_INSTALLED);
    }

    #[tokio::test]
    async fn appui_de_deux_dixiemes_aucune_transcription() {
        let f = simple(Opts { secs: 0.2, ..Opts::default() });
        f.press().await;
        assert!(f.has("mic") && !f.has("stt"));
        assert_eq!(f.last_phase(), Some(VoicePhase::Idle));
    }

    #[tokio::test]
    async fn hallucination_aucun_tour() {
        let f = simple(Opts { heard: "Sous-titres réalisés par la communauté d'Amara.org", ..Opts::default() });
        f.press().await;
        assert!(f.has("stt"));
        assert!(!f.log.lock().iter().any(|l| l.starts_with("turn:")));
        assert_eq!(f.last_phase(), Some(VoicePhase::Idle));
    }

    #[tokio::test]
    async fn reponse_courte_toast_puis_voix() {
        let f = simple(Opts::default());
        f.press().await;
        assert_eq!(*f.log.lock(), ["mic", "stt", "finish:false", "turn:Quel temps fait-il ?", "speak"]);
        assert_eq!(f.toasts(), ["Il fait beau."]);
        let phases: Vec<VoicePhase> = f.ui.lock().iter().filter_map(|u| match u { Ui::Phase(p, None) => Some(*p), _ => None }).collect();
        assert_eq!(phases, [VoicePhase::Listening, VoicePhase::Transcribing, VoicePhase::Idle, VoicePhase::Speaking, VoicePhase::Idle]);
        assert!(f.ui.lock().iter().any(|u| matches!(u, Ui::Timing(_))));
        assert!(!f.ui.lock().contains(&Ui::OpenPanel));
    }

    #[tokio::test]
    async fn sans_voix_le_toast_seul() {
        let f = simple(Opts { conf: Conf { tts: false, ..Opts::default().conf }, ..Opts::default() });
        f.press().await;
        assert_eq!(f.toasts(), ["Il fait beau."]);
        assert!(!f.has("speak"));
    }

    #[tokio::test]
    async fn barge_in_coupe_la_voix_avant_d_ouvrir_le_micro() {
        let f = simple(Opts { long_speech: true, ..Opts::default() });
        let task = f.speaking_press().await;
        f.p.start().await;
        tokio::time::timeout(Duration::from_secs(2), task).await.expect("la voix coupée termine la suite").unwrap();
        let log = f.log.lock().clone();
        assert_eq!(&log[log.len() - 3..], ["speak", "stop", "mic"]);
        assert_eq!(f.last_phase(), Some(VoicePhase::Listening), "l'ancien appui ne repasse pas en Idle");
        assert!(!f.speaking.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn appui_en_pause_pendant_la_voix_repasse_en_idle() {
        let paused = Arc::new(AtomicBool::new(false));
        let mut f = simple(Opts { long_speech: true, ..Opts::default() });
        let (c, base) = (paused.clone(), Opts::default().conf);
        Arc::get_mut(&mut f.p).unwrap().conf = Arc::new(move || Conf { paused: c.load(Ordering::SeqCst), ..base.clone() });
        let task = f.speaking_press().await;
        paused.store(true, Ordering::SeqCst);
        f.p.start().await;
        tokio::time::timeout(Duration::from_secs(2), task).await.unwrap().unwrap();
        assert_eq!(f.last_phase(), Some(VoicePhase::Idle));
        assert!(!f.speaking.load(Ordering::SeqCst));
        assert_eq!(f.toasts(), ["Il fait beau.", "Voix en pause"]);
    }

    #[tokio::test]
    async fn echap_coupe_la_voix() {
        let f = simple(Opts { long_speech: true, ..Opts::default() });
        let task = f.speaking_press().await;
        f.p.handle(Input::Escape).await;
        tokio::time::timeout(Duration::from_secs(2), task).await.expect("la voix coupée termine la suite").unwrap();
        assert!(f.has("stop"));
        assert_eq!(f.last_phase(), Some(VoicePhase::Idle));
        assert!(!f.speaking.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn sans_fin_d_appui_le_micro_se_ferme_seul() {
        let f = simple(Opts { max_hold: Duration::from_millis(50), ..Opts::default() });
        f.p.handle(Input::Start).await;
        for _ in 0..200 {
            if f.has("speak") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(f.has("stt") && f.has("speak"), "traité comme relâché : {:?}", f.log.lock());
        assert!(f.p.end().await.is_none(), "un PttEnd tardif ne trouve plus de micro ouvert");
    }

    #[tokio::test]
    async fn plein_ecran_appui_court_arrete_aussi_whisper() {
        let f = simple(Opts { secs: 0.2, conf: Conf { fullscreen: true, ..Opts::default().conf }, ..Opts::default() });
        f.press().await;
        assert!(!f.has("stt") && f.has("finish:true"));
    }

    struct NoTurn(fn() -> TurnError);
    #[async_trait]
    impl Turns for NoTurn {
        async fn run(&self, _: String) -> Result<TurnResult, TurnError> { Err(self.0()) }
    }

    #[tokio::test]
    async fn tour_abandonne_aucun_toast_ni_voix() {
        let f = simple(Opts { turns: Some(Arc::new(NoTurn(|| TurnError::Aborted))), ..Opts::default() });
        f.press().await;
        assert!(f.toasts().is_empty() && !f.has("speak"));
        assert_eq!(f.last_phase(), Some(VoicePhase::Idle));
    }

    #[tokio::test]
    async fn tour_echoue_un_toast_et_reponse_vide_rien() {
        let f = simple(Opts { turns: Some(Arc::new(NoTurn(|| TurnError::Failed))), ..Opts::default() });
        f.press().await;
        assert_eq!(f.toasts(), ["Je n'ai pas pu traiter ta demande."]);
        let log = Log::default();
        let f = fx(Opts { turns: Some(Arc::new(FakeTurns { log: log.clone(), reply: " ", pending: false })), ..Opts::default() }, log);
        f.press().await;
        assert!(f.toasts().is_empty() && !f.has("speak"));
        assert!(!f.ui.lock().contains(&Ui::OpenPanel));
        assert_eq!(f.last_phase(), Some(VoicePhase::Idle));
    }

    #[tokio::test]
    async fn micro_absent_un_seul_toast_par_pression() {
        let f = simple(Opts { mic_fails: true, ..Opts::default() });
        f.press().await;
        assert_eq!(f.toasts(), [MicError::NoDevice.to_string()]);
        assert_eq!(f.last_phase(), Some(VoicePhase::Idle));
        f.press().await;
        assert_eq!(f.toasts().len(), 2);
    }

    #[tokio::test]
    async fn plein_ecran_arrete_whisper_apres_la_transcription() {
        let f = simple(Opts { conf: Conf { fullscreen: true, ..Opts::default().conf }, ..Opts::default() });
        f.press().await;
        let log = f.log.lock().clone();
        assert_eq!(&log[1..3], ["stt", "finish:true"]);
    }

    #[tokio::test]
    async fn reponse_pending_ouvre_le_panneau_sans_parler() {
        let log = Log::default();
        let turns = Arc::new(FakeTurns { log: log.clone(), reply: "Je t'ai préparé la confirmation.", pending: true });
        let f = fx(Opts { turns: Some(turns), ..Opts::default() }, log);
        f.press().await;
        assert!(f.ui.lock().contains(&Ui::OpenPanel));
        assert!(!f.has("speak") && f.toasts().is_empty());
    }

    #[tokio::test]
    async fn plein_ecran_reponse_pending_ou_longue_un_toast_sans_panneau() {
        for (reply, pending) in [("Je t'ai préparé la confirmation.", true), ("a ".repeat(200).leak() as &'static str, false)] {
            let log = Log::default();
            let turns = Arc::new(FakeTurns { log: log.clone(), reply, pending });
            let f = fx(Opts { turns: Some(turns), conf: Conf { fullscreen: true, ..Opts::default().conf }, ..Opts::default() }, log);
            f.press().await;
            assert!(!f.ui.lock().contains(&Ui::OpenPanel), "pending={pending}");
            assert_eq!(f.toasts(), ["Réponse prête dans le panneau"]);
            assert!(!f.has("speak"));
        }
    }

    /// Voix dont l'ouverture bloque jusqu'au feu vert du test (périphérique lent).
    struct SlowSpeaker { entered: std::sync::mpsc::Sender<()>, go: Mutex<std::sync::mpsc::Receiver<()>>, handle: Arc<Mutex<Option<SpeakHandle>>>, done: Mutex<Vec<tokio::sync::watch::Sender<bool>>> }
    #[async_trait::async_trait]
    impl Speaker for SlowSpeaker {
        async fn speak(&self, _: &str, _: u8) -> Result<SpeakHandle, TtsError> {
            let _ = self.entered.send(());
            // Multi-thread uniquement (le test le déclare) : le fil bloqué cède sa file de tâches au runtime.
            tokio::task::block_in_place(|| { let _ = self.go.lock().recv(); });
            let (h, done) = SpeakHandle::new();
            self.done.lock().push(done); // jamais finie d'elle-même
            *self.handle.lock() = Some(h.clone());
            Ok(h)
        }
        fn stop(&self) {}
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn appui_pendant_l_ouverture_de_la_voix_la_coupe() {
        let (entered, entered_rx) = std::sync::mpsc::channel();
        let (go_tx, go) = std::sync::mpsc::channel();
        let handle: Arc<Mutex<Option<SpeakHandle>>> = Arc::default();
        let mut f = simple(Opts::default());
        Arc::get_mut(&mut f.p).unwrap().speaker = Arc::new(SlowSpeaker { entered, go: Mutex::new(go), handle: handle.clone(), done: Mutex::default() });
        f.p.start().await;
        let rec = f.p.end().await.unwrap();
        let task = tokio::spawn(f.p.clone().process(rec));
        tokio::task::spawn_blocking(move || entered_rx.recv()).await.unwrap().unwrap();
        f.p.start().await; // nouvel appui pendant que `speak` ouvre la sortie
        go_tx.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(2), task).await.expect("la suite de l'ancien appui se termine").unwrap();
        assert!(handle.lock().as_ref().unwrap().stopped(), "la voix ouverte trop tard est coupée");
        assert_eq!(f.last_phase(), Some(VoicePhase::Listening));
        assert!(!f.speaking.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn reponse_longue_ouvre_le_panneau() {
        let log = Log::default();
        let reply: &'static str = "a ".repeat(200).leak();
        let f = fx(Opts { turns: Some(Arc::new(FakeTurns { log: log.clone(), reply, pending: false })), ..Opts::default() }, log);
        f.press().await;
        assert!(f.ui.lock().contains(&Ui::OpenPanel));
        assert!(!f.has("speak"));
    }

    #[test]
    fn le_toast_tient_en_80_caracteres() {
        let t = toast_text(&[Segment::Text { text: "é".repeat(100) }]);
        assert_eq!(t.chars().count(), 80);
        assert!(t.ends_with('…'));
        assert_eq!(toast_text(&[Segment::Text { text: " Salut ".into() }]), "Salut");
    }

    // --- Vrai agent d'IRIS (cerveaux et outils factices) : provenance et raccourci ---

    struct Local { chats: AtomicUsize }
    #[async_trait]
    impl LocalBrain for Local {
        /// Note d'abord, puis répond.
        async fn chat(&self, _: ChatRequest, _: &mut (dyn for<'a> FnMut(&'a str) + Send)) -> Result<ChatReply, BrainError> {
            Ok(if self.chats.fetch_add(1, Ordering::SeqCst) == 0 {
                ChatReply { text: String::new(), tool_calls: vec![ToolCallMsg { name: "notes__create".into(), args: json!({"text": "pain"}) }] }
            } else {
                ChatReply { text: "C'est noté.".into(), tool_calls: vec![] }
            })
        }
        async fn structured(&self, _: &str, _: &str, _: &Value) -> Result<Value, BrainError> { Err(BrainError::Unavailable) }
    }

    struct FakeTool(&'static str);
    #[async_trait]
    impl Tool for FakeTool {
        fn spec(&self) -> ToolSpec { spec(self.0, "fake", json!({"type": "object"}), Permission::Free) }
        fn describe(&self, _: &Value) -> ConfirmationText { low("fake") }
        async fn run(&self, _: &Value) -> Result<ToolResult, ToolError> { Ok(ToolResult { output: json!({}), undo: None }) }
    }

    struct AgentTurns { log: Log, agent: Agent, exec: Arc<Executor> }
    #[async_trait]
    impl Turns for AgentTurns {
        async fn run(&self, text: String) -> Result<TurnResult, TurnError> {
            self.log.lock().push(format!("turn:{text}"));
            let ctx = PromptCtx { system: String::new(), claude_system: String::new(), history: vec![] };
            let input = TurnInput { text, force_claude: false, local_only: false };
            let (end, pending, first_tool) = crate::modules::iris::play(&self.agent, input, self.exec.clone(), ctx, |_| {}).await;
            Ok(TurnResult { shown: end.shown, pending, first_tool })
        }
    }

    fn with_agent(heard: &'static str) -> (Fx, Arc<Local>, Arc<ToolBroker>) {
        let dir = tempfile::tempdir().unwrap();
        let mut c = oniris_core::open_db(&dir.path().join("t.db")).unwrap();
        oniris_core::migrate(&mut c, oniris_core::CORE_MIGRATIONS).unwrap();
        let tools: Vec<Arc<dyn Tool>> = vec![Arc::new(FakeTool("notes.create")), Arc::new(FakeTool("music.next"))];
        let broker = Arc::new(ToolBroker::new(Arc::new(Mutex::new(c)), Arc::new(|| 0), tools).unwrap());
        let exec = Arc::new(Executor {
            broker: broker.clone(),
            vault: Default::default(),
            origin: Origin::Assistant { brain: Brain::Local },
            on_pending: Arc::new(|_| {}),
            virtuals: None,
        });
        let local = Arc::new(Local { chats: AtomicUsize::new(0) });
        let agent = Agent { local: local.clone(), claude: Arc::new(crate::modules::iris::NoClaude), model: "m".into(), keep_alive_min: 5, claude_auto: true };
        let log = Log::default();
        let turns = Arc::new(AgentTurns { log: log.clone(), agent, exec });
        let mut f = fx(Opts { heard, turns: Some(turns), ..Opts::default() }, log);
        f._dir = Some(dir);
        (f, local, broker)
    }

    #[tokio::test]
    async fn phrase_normale_un_tour_en_provenance_user() {
        let (f, local, broker) = with_agent("Ajoute une note : acheter du pain");
        f.press().await;
        assert_eq!(f.log.lock().iter().filter(|l| l.starts_with("turn:")).count(), 1);
        assert_eq!(local.chats.load(Ordering::SeqCst), 2);
        let j = broker.journal(5);
        assert_eq!(j.len(), 1);
        assert_eq!((j[0].tool.as_str(), &j[0].provenance), ("notes.create", &Provenance::User));
        assert_eq!(f.toasts(), ["C'est noté."]);
        assert!(f.has("speak"));
        let timing = f.ui.lock().iter().find_map(|u| match u { Ui::Timing(t) => Some(t.clone()), _ => None }).unwrap();
        assert!(timing.text_to_tool_ms.is_some(), "premier appel d'outil mesuré");
    }

    #[tokio::test]
    async fn suivant_passe_par_le_raccourci_sans_cerveau_local() {
        let (f, local, broker) = with_agent("Suivant.");
        f.press().await;
        assert_eq!(local.chats.load(Ordering::SeqCst), 0);
        let j = broker.journal(5);
        assert_eq!((j[0].tool.as_str(), &j[0].provenance), ("music.next", &Provenance::User));
        assert_eq!(f.toasts(), ["C'est fait."]);
    }
}
