//! Push-to-talk : micro → Whisper → tour d'IRIS → toast de la pilule (ou panneau) et voix de Piper.
//! La logique vit dans `Pipeline`, derrière des traits, testable sans Tauri. Journal : durées et longueurs seulement.
use super::capture::{CpalMic, Mic, Recording};
use super::install::voice_dir;
use super::piper::Speaker;
use super::whisper::WhisperProcess;
use crate::modules::iris::TurnResult;
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
#[derive(Debug, Clone, PartialEq)]
pub enum Ui { Phase(VoicePhase, Option<f32>), Toast(String), OpenPanel, Timing(Timing) }

/// Réglages et état relus à chaque appui.
#[derive(Debug, Clone, Default)]
pub struct Conf { pub paused: bool, pub installed: bool, pub fullscreen: bool, pub device: String, pub tts: bool, pub volume: u8, pub user_name: String }

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
    /// Même tour qu'`iris_ask(text, false, false)` ; `None` s'il a échoué ou a été abandonné.
    async fn run(&self, text: String) -> Option<TurnResult>;
}

/// Appui en cours : son numéro (un nouvel appui rend les suites de l'ancien muettes) et l'enregistrement.
#[derive(Default)]
struct State { press: u64, rec: Option<Box<dyn Recording>> }

type Recorded = (u64, Vec<f32>);

pub struct Pipeline {
    mic: Arc<dyn Mic>,
    stt: Arc<dyn Stt>,
    speaker: Arc<dyn Speaker>,
    turns: Arc<dyn Turns>,
    conf: Arc<dyn Fn() -> Conf + Send + Sync>,
    ui: Arc<dyn Fn(Ui) + Send + Sync>,
    state: Mutex<State>,
}

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

impl Pipeline {
    fn current(&self, press: u64) -> bool { self.state.lock().press == press }

    fn phase(&self, press: u64, p: VoicePhase) {
        if self.current(press) {
            (self.ui)(Ui::Phase(p, None));
        }
    }

    fn toast(&self, text: impl Into<String>) { (self.ui)(Ui::Toast(text.into())) }

    /// Début d'appui : coupe IRIS si elle parle, puis ouvre le micro et préchauffe Whisper.
    pub async fn start(&self) {
        let press = {
            let mut s = self.state.lock();
            s.press += 1;
            s.rec = None;
            s.press
        };
        self.speaker.stop(); // barge-in : avant d'ouvrir le micro
        let conf = (self.conf)();
        if conf.paused {
            return self.toast("Voix en pause");
        }
        if !conf.installed {
            return self.toast("Voix non installée");
        }
        self.phase(press, VoicePhase::Listening);
        let (mic, ui) = (self.mic.clone(), self.ui.clone());
        // `Mic::start` bloque le temps d'ouvrir le périphérique.
        let opened = tokio::task::spawn_blocking(move || mic.start(&conf.device, Box::new(move |l| ui(Ui::Phase(VoicePhase::Listening, Some(l)))))).await;
        match opened {
            Ok(Ok(rec)) => {
                self.state.lock().rec = Some(rec);
                let stt = self.stt.clone();
                tokio::spawn(async move { stt.warm().await });
            }
            Ok(Err(e)) => {
                self.phase(press, VoicePhase::Idle);
                self.toast(e.to_string());
            }
            Err(_) => {
                self.phase(press, VoicePhase::Idle);
                self.toast("Le micro n'a pas pu démarrer.");
            }
        }
    }

    /// Fin d'appui : ferme le micro ; `None` si rien n'était enregistré (pause, micro absent).
    pub async fn end(&self) -> Option<Recorded> {
        let (press, rec) = {
            let mut s = self.state.lock();
            (s.press, s.rec.take()?)
        };
        let samples = tokio::task::spawn_blocking(move || rec.stop()).await.ok()?;
        Some((press, samples))
    }

    /// Transcription, tour d'IRIS, puis toast et voix (ou panneau).
    pub async fn process(self: Arc<Self>, (press, samples): Recorded) {
        let t0 = Instant::now();
        let speech = audio::speech_ms(&samples, 0.01);
        if speech < MIN_SPEECH_MS {
            return self.phase(press, VoicePhase::Idle);
        }
        self.phase(press, VoicePhase::Transcribing);
        let raw = self.stt.transcribe(audio::wav_16k_mono(&samples)).await;
        self.stt.finish((self.conf)().fullscreen).await;
        self.phase(press, VoicePhase::Idle);
        let text = match raw {
            Ok(raw) => stt::clean(&raw),
            Err(e) => return self.toast(format!("Je n'ai pas pu t'entendre : {e}.")),
        };
        let Some(text) = text else { return };
        let t1 = Instant::now();
        tracing::info!(speech_ms = speech, ms = ms(t1 - t0), chars = text.chars().count(), "voix transcrite");
        let Some(r) = self.turns.run(text).await else { return };
        let timing = Timing {
            speech_to_text_ms: ms(t1 - t0),
            text_to_tool_ms: r.first_tool.map(|t| ms(t.saturating_duration_since(t1))),
            total_ms: ms(t0.elapsed()),
        };
        (self.ui)(Ui::Timing(timing));
        if !self.current(press) {
            return; // un nouvel appui a pris la main
        }
        let conf = (self.conf)();
        let short = speakable(&r.shown, &conf.user_name).filter(|s| !r.pending && !s.starts_with(TO_PANEL));
        let Some(say) = short else { return (self.ui)(Ui::OpenPanel) };
        self.toast(toast_text(&r.shown));
        if !conf.tts {
            return;
        }
        match self.speaker.speak(&say, conf.volume) {
            Ok(h) => {
                self.phase(press, VoicePhase::Speaking);
                h.done().await;
                self.phase(press, VoicePhase::Idle);
            }
            Err(e) => self.toast(format!("IRIS ne peut pas parler : {e}.")),
        }
    }
}

// --- Branchement Tauri ---

pub enum Input { Start, End, Escape }

/// Lu par le crochet clavier : Échap n'est observé que pendant que IRIS parle.
pub static SPEAKING: AtomicBool = AtomicBool::new(false);

type ConfFn = Arc<dyn Fn() -> Conf + Send + Sync>;
static VOICE: OnceLock<(tokio::sync::mpsc::UnboundedSender<Input>, ConfFn)> = OnceLock::new();

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
    VOICE.get().map_or(NOT_INSTALLED, |(_, conf)| label(&conf()))
}

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
    async fn run(&self, text: String) -> Option<TurnResult> {
        crate::modules::iris::voice_turn(&self.0, text).await
    }
}

#[derive(Serialize, Clone)]
struct PhaseEvent { phase: VoicePhase, #[serde(skip_serializing_if = "Option::is_none")] level: Option<f32> }

#[derive(Serialize, Clone)]
struct Toast { text: String }

fn emit_ui(app: &AppHandle, u: Ui) {
    match u {
        Ui::Phase(phase, level) => {
            SPEAKING.store(phase == VoicePhase::Speaking, Ordering::SeqCst);
            let _ = app.emit("oniris://voice", PhaseEvent { phase, level });
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
    let conf: ConfFn = Arc::new(move || {
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
        }
    });
    let h = app.clone();
    let p = Arc::new(Pipeline {
        mic: Arc::new(CpalMic),
        stt: Arc::new(Whisper { proc: Arc::default(), http: reqwest::Client::new(), app: app.clone() }),
        speaker: app.state::<AppState>().speaker.clone(),
        turns: Arc::new(VoiceTurns(app.clone())),
        conf: conf.clone(),
        ui: Arc::new(move |u| emit_ui(&h, u)),
        state: Mutex::default(),
    });
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    if VOICE.set((tx, conf)).is_err() {
        return;
    }
    tauri::async_runtime::spawn(async move {
        while let Some(i) = rx.recv().await {
            match i {
                Input::Start => p.start().await,
                Input::End => {
                    if let Some(rec) = p.end().await {
                        tauri::async_runtime::spawn(p.clone().process(rec));
                    }
                }
                Input::Escape => p.speaker.stop(),
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::voice::capture::MicError;
    use crate::modules::voice::piper::{SpeakHandle, TtsError};
    use oniris_core::iris::{Agent, BrainError, ChatReply, ChatRequest, Executor, LocalBrain, PromptCtx, ToolCallMsg, TurnInput};
    use oniris_core::tools::helpers::{low, spec};
    use oniris_core::tools::{Brain, ConfirmationText, Origin, Permission, Provenance, Tool, ToolBroker, ToolError, ToolResult, ToolSpec};
    use serde_json::{json, Value};
    use std::sync::atomic::AtomicUsize;

    type Log = Arc<Mutex<Vec<String>>>;

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
    impl Speaker for FakeSpeaker {
        fn speak(&self, _: &str, _: u8) -> Result<SpeakHandle, TtsError> {
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
        async fn run(&self, text: String) -> Option<TurnResult> {
            self.log.lock().push(format!("turn:{text}"));
            Some(TurnResult { shown: vec![Segment::Text { text: self.reply.into() }], pending: self.pending, first_tool: None })
        }
    }

    struct Fx { p: Arc<Pipeline>, log: Log, ui: Arc<Mutex<Vec<Ui>>>, _dir: Option<tempfile::TempDir> }

    impl Fx {
        fn toasts(&self) -> Vec<String> {
            self.ui.lock().iter().filter_map(|u| match u { Ui::Toast(t) => Some(t.clone()), _ => None }).collect()
        }
        fn has(&self, entry: &str) -> bool { self.log.lock().iter().any(|l| l == entry) }
        fn last_phase(&self) -> Option<VoicePhase> {
            self.ui.lock().iter().rev().find_map(|u| match u { Ui::Phase(p, None) => Some(*p), _ => None })
        }
        /// Un appui complet, suite comprise.
        async fn press(&self) {
            self.p.start().await;
            if let Some(rec) = self.p.end().await {
                self.p.clone().process(rec).await;
            }
        }
    }

    struct Opts { conf: Conf, secs: f32, mic_fails: bool, heard: &'static str, long_speech: bool, turns: Option<Arc<dyn Turns>> }

    impl Default for Opts {
        fn default() -> Self {
            let conf = Conf { installed: true, tts: true, volume: 80, user_name: "Nathan".into(), ..Conf::default() };
            Self { conf, secs: 1.0, mic_fails: false, heard: "Quel temps fait-il ?", long_speech: false, turns: None }
        }
    }

    fn fx(o: Opts, log: Log) -> Fx {
        let ui: Arc<Mutex<Vec<Ui>>> = Arc::default();
        let u = ui.clone();
        let conf = o.conf;
        let turns = o.turns.unwrap_or_else(|| Arc::new(FakeTurns { log: log.clone(), reply: "Il fait beau.", pending: false }));
        let p = Arc::new(Pipeline {
            mic: Arc::new(FakeMic { log: log.clone(), secs: o.secs, fail: o.mic_fails }),
            stt: Arc::new(FakeStt { log: log.clone(), text: o.heard.into() }),
            speaker: Arc::new(FakeSpeaker { log: log.clone(), long: o.long_speech, playing: Mutex::default() }),
            turns,
            conf: Arc::new(move || conf.clone()),
            ui: Arc::new(move |x| u.lock().push(x)),
            state: Mutex::default(),
        });
        Fx { p, log, ui, _dir: None }
    }

    fn simple(o: Opts) -> Fx { fx(o, Log::default()) }

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
        f.p.start().await;
        let rec = f.p.end().await.unwrap();
        let task = tokio::spawn(f.p.clone().process(rec));
        for _ in 0..100 {
            if f.last_phase() == Some(VoicePhase::Speaking) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(f.last_phase(), Some(VoicePhase::Speaking));
        f.p.start().await;
        tokio::time::timeout(Duration::from_secs(2), task).await.expect("la voix coupée termine la suite").unwrap();
        let log = f.log.lock().clone();
        assert_eq!(&log[log.len() - 3..], ["speak", "stop", "mic"]);
        assert_eq!(f.last_phase(), Some(VoicePhase::Listening), "l'ancien appui ne repasse pas en Idle");
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
        async fn run(&self, text: String) -> Option<TurnResult> {
            self.log.lock().push(format!("turn:{text}"));
            let ctx = PromptCtx { system: String::new(), claude_system: String::new(), history: vec![] };
            let input = TurnInput { text, force_claude: false, local_only: false };
            let (end, pending, first_tool) = crate::modules::iris::play(&self.agent, input, self.exec.clone(), ctx, |_| {}).await;
            Some(TurnResult { shown: end.shown, pending, first_tool })
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
        let agent = Agent { local: local.clone(), claude: Arc::new(crate::modules::iris::NoClaude), model: "m".into(), keep_alive_min: 5 };
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
