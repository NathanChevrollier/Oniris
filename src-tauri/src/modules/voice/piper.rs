//! Voix d'IRIS : `piper.exe --output-raw` (une phrase par ligne sur stdin, PCM 16 bits mono sur stdout) joué par une
//! sortie injectable. Une seule voix à la fois ; `stop()` coupe en quelques ms. Journal : longueurs et durées, jamais le texte.
#![allow(dead_code)] // consommé par le pipeline vocal (tâche suivante) ; à retirer alors
use super::playback::{AudioOut, Sink};
use crate::job;
use oniris_core::voice::speech::sentences;
use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::sync::watch;

#[derive(Debug)]
pub enum TtsError {
    NotInstalled,
    Start(String),
    Audio(String),
}

impl std::fmt::Display for TtsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotInstalled => write!(f, "voix non installée"),
            Self::Start(e) => write!(f, "démarrage de Piper impossible : {e}"),
            Self::Audio(e) => write!(f, "sortie audio indisponible : {e}"),
        }
    }
}

/// Lecture en cours : `stop()` la coupe, `done()` attend sa fin (lue en entier ou coupée).
#[derive(Clone)]
pub struct SpeakHandle {
    cancel: Arc<watch::Sender<bool>>,
    done: watch::Receiver<bool>,
}

impl SpeakHandle {
    pub fn stop(&self) {
        self.cancel.send_replace(true);
    }

    pub async fn done(&self) {
        let _ = self.done.clone().wait_for(|d| *d).await;
    }
}

pub trait Speaker: Send + Sync {
    fn speak(&self, text: &str, volume: u8) -> Result<SpeakHandle, TtsError>;
}

pub struct PiperSpeaker {
    /// Dossier de la voix (`piper\piper.exe`, `models\…`).
    dir: PathBuf,
    out: Arc<dyn AudioOut>,
    // shortcut: un état par PiperSpeaker (et non un global) ; l'appelant n'en crée qu'un, upgrade : static si ça change.
    current: Mutex<Option<SpeakHandle>>,
}

impl PiperSpeaker {
    pub fn new(dir: PathBuf, out: Arc<dyn AudioOut>) -> Self {
        Self { dir, out, current: Mutex::new(None) }
    }

    /// Coupe la voix en cours, s'il y en a une.
    pub fn stop(&self) {
        if let Some(h) = self.current.lock().take() {
            h.stop();
        }
    }
}

const MODEL: &str = "fr_FR-siwis-medium.onnx";

/// Échantillon mis à l'échelle du volume (0..=100), logiciel.
fn scale(s: i16, volume: u8) -> i16 {
    (s as i32 * volume.min(100) as i32 / 100) as i16
}

fn sample_rate(json: &std::path::Path) -> u32 {
    std::fs::read_to_string(json)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v["audio"]["sample_rate"].as_u64())
        .map_or(22_050, |r| r as u32)
}

impl Speaker for PiperSpeaker {
    fn speak(&self, text: &str, volume: u8) -> Result<SpeakHandle, TtsError> {
        self.stop();
        let exe = self.dir.join("piper").join("piper.exe");
        let model = self.dir.join("models").join(MODEL);
        if !exe.is_file() || !model.is_file() {
            return Err(TtsError::NotInstalled);
        }
        let lines = sentences(text);
        let sink = self.out.open(sample_rate(&self.dir.join("models").join(format!("{MODEL}.json"))))?;
        let mut cmd = Command::new(&exe);
        cmd.arg("--model").arg(&model).arg("--output-raw");
        cmd.current_dir(exe.parent().unwrap_or(std::path::Path::new(".")))
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        let (child, job) = job::spawn(cmd).map_err(|e| TtsError::Start(e.to_string()))?;
        let (cancel_tx, mut cancel_rx) = watch::channel(false);
        let (done_tx, done_rx) = watch::channel(false);
        let handle = SpeakHandle { cancel: Arc::new(cancel_tx), done: done_rx };
        *self.current.lock() = Some(handle.clone());
        let chars = text.chars().count();
        tokio::spawn(async move {
            let t = Instant::now();
            // Annulé par `select!`, `play` est lâché : le Sink se ferme, puis le Job tue Piper.
            tokio::select! {
                _ = cancel_rx.wait_for(|c| *c) => tracing::info!(ms = t.elapsed().as_millis() as u64, "voix coupée"),
                _ = play(child, sink, lines, volume) => tracing::info!(chars, ms = t.elapsed().as_millis() as u64, "voix terminée"),
            }
            drop(job);
            let _ = done_tx.send(true);
        });
        Ok(handle)
    }
}

/// Écrit les phrases, lit le PCM, le joue, puis attend que la file soit vidée.
async fn play(mut child: tokio::process::Child, mut sink: Box<dyn Sink>, lines: Vec<String>, volume: u8) {
    let (Some(mut stdin), Some(mut stdout)) = (child.stdin.take(), child.stdout.take()) else { return };
    tokio::spawn(async move {
        for l in lines {
            if stdin.write_all(format!("{l}\n").as_bytes()).await.is_err() {
                return;
            }
        }
        // stdin fermé au drop : Piper finit sa dernière phrase puis s'arrête
    });
    let mut buf = [0u8; 8192];
    let mut odd: Option<u8> = None; // octet de poids faible en attente d'un échantillon complet
    while let Ok(n) = stdout.read(&mut buf).await {
        if n == 0 {
            break;
        }
        let mut bytes = Vec::with_capacity(n + 1);
        bytes.extend(odd.take());
        bytes.extend_from_slice(&buf[..n]);
        if bytes.len() % 2 == 1 {
            odd = bytes.pop();
        }
        let pcm: Vec<i16> = bytes.as_chunks::<2>().0.iter().map(|&b| scale(i16::from_le_bytes(b), volume)).collect();
        sink.push(&pcm);
    }
    while sink.pending() > 0 {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};

    /// Faux puits : compte les échantillons reçus et les puits fermés.
    #[derive(Default)]
    struct FakeOut {
        samples: Arc<AtomicUsize>,
        closed: Arc<AtomicUsize>,
        rate: Arc<AtomicUsize>,
    }
    struct FakeSink {
        samples: Arc<AtomicUsize>,
        closed: Arc<AtomicUsize>,
    }
    impl AudioOut for FakeOut {
        fn open(&self, rate: u32) -> Result<Box<dyn Sink>, TtsError> {
            self.rate.store(rate as usize, SeqCst);
            Ok(Box::new(FakeSink { samples: self.samples.clone(), closed: self.closed.clone() }))
        }
    }
    impl Sink for FakeSink {
        fn push(&mut self, pcm: &[i16]) {
            self.samples.fetch_add(pcm.len(), SeqCst);
        }
        fn pending(&self) -> usize {
            0
        }
    }
    impl Drop for FakeSink {
        fn drop(&mut self) {
            self.closed.fetch_add(1, SeqCst);
        }
    }

    /// Voix factice installée dans un dossier temporaire : `piper\piper.exe` (faux), modèle et JSON (rate 16000).
    fn setup() -> (tempfile::TempDir, PiperSpeaker, Arc<FakeOut>, PathBuf) {
        let src = std::env::current_exe().unwrap().parent().unwrap().parent().unwrap().join("examples").join("fake_piper.exe");
        assert!(src.is_file(), "faux piper absent : lancer `cargo test` (qui compile les exemples), pas `--lib` seul");
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("piper")).unwrap();
        std::fs::create_dir_all(d.path().join("models")).unwrap();
        let exe = d.path().join("piper").join("piper.exe");
        std::fs::copy(src, &exe).unwrap();
        std::fs::write(d.path().join("models").join(MODEL), b"x").unwrap();
        std::fs::write(d.path().join("models").join(format!("{MODEL}.json")), r#"{"audio":{"sample_rate":16000}}"#).unwrap();
        let out = Arc::new(FakeOut::default());
        let speaker = PiperSpeaker::new(d.path().to_path_buf(), out.clone());
        (d, speaker, out, exe.with_extension("pid"))
    }

    async fn pid_of(f: &std::path::Path) -> u32 {
        for _ in 0..60 {
            if let Some(p) = std::fs::read_to_string(f).ok().and_then(|t| t.parse().ok()) {
                return p;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("pas de PID");
    }

    fn gone_now(pid: u32) -> bool {
        let p = sysinfo::Pid::from_u32(pid);
        let mut sys = sysinfo::System::new();
        sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[p]), true);
        sys.process(p).is_none()
    }

    #[test]
    fn le_volume_est_applique_en_logiciel() {
        assert_eq!(scale(1000, 100), 1000);
        assert_eq!(scale(1000, 50), 500);
        assert_eq!(scale(-1000, 0), 0);
        assert_eq!(scale(i16::MIN, 100), i16::MIN);
    }

    #[tokio::test]
    async fn non_installe_sans_les_fichiers() {
        let d = tempfile::tempdir().unwrap();
        let s = PiperSpeaker::new(d.path().to_path_buf(), Arc::new(FakeOut::default()));
        assert!(matches!(s.speak("Bonjour", 80), Err(TtsError::NotInstalled)));
    }

    #[tokio::test]
    async fn les_octets_arrivent_au_taux_du_json() {
        let (_d, s, out, _pid) = setup();
        let h = s.speak("Bonjour. Salut !", 80).unwrap();
        tokio::time::timeout(Duration::from_secs(10), h.done()).await.expect("fin de lecture");
        assert_eq!(out.samples.load(SeqCst), 1000, "2 phrases x 1000 octets = 1000 échantillons");
        assert_eq!(out.rate.load(SeqCst), 16_000);
        assert_eq!(out.closed.load(SeqCst), 1);
    }

    #[tokio::test]
    async fn stop_coupe_en_moins_de_150_ms_et_tue_piper() {
        let (_d, s, out, pidf) = setup();
        let h = s.speak("LONG", 100).unwrap();
        let pid = pid_of(&pidf).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(out.samples.load(SeqCst) > 0 && !gone_now(pid));
        let t = Instant::now();
        h.stop();
        h.done().await;
        assert!(t.elapsed() < Duration::from_millis(150), "coupé en {:?}", t.elapsed());
        assert_eq!(out.closed.load(SeqCst), 1);
        for _ in 0..40 {
            if gone_now(pid) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("piper tourne encore");
    }

    #[tokio::test]
    async fn un_deuxieme_speak_coupe_le_premier() {
        let (_d, s, out, _pid) = setup();
        let first = s.speak("LONG", 100).unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        let second = s.speak("Bonjour", 100).unwrap();
        tokio::time::timeout(Duration::from_millis(500), first.done()).await.expect("le premier est coupé");
        tokio::time::timeout(Duration::from_secs(10), second.done()).await.expect("le second finit");
        assert_eq!(out.closed.load(SeqCst), 2);
    }
}
