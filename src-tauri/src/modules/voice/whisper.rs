//! `whisper-server` local : démarrage paresseux, arrêt après inactivité ou tout de suite en plein écran.
//! Journal : durées seulement, jamais de texte transcrit ni de chemin.
#![allow(dead_code)] // utilisé par le pipeline vocal (tâche suivante)
use crate::job::{self, Job};
use oniris_core::voice::stt::SttError;
use parking_lot::Mutex;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::process::{Child, Command};

struct Running {
    child: Child,
    _job: Job,
    base: String,
    id: u64,
}

#[derive(Default)]
pub struct WhisperProcess {
    running: tokio::sync::Mutex<Option<Running>>,
    /// (dernière activité, délai d'inactivité) ; le minuteur relit cette valeur à chaque réveil.
    idle: Mutex<(Option<Instant>, Duration)>,
    next_id: Mutex<u64>,
}

impl WhisperProcess {
    /// Adresse du serveur vivant, sinon le lance (une seule fois même si plusieurs appels arrivent ensemble).
    /// `dir` = dossier de la voix, `idle` = délai avant l'arrêt pour inactivité.
    pub async fn ensure(self: &Arc<Self>, dir: &Path, model: &Path, idle: Duration) -> Result<String, SttError> {
        let mut slot = self.running.lock().await;
        *self.idle.lock() = (Some(Instant::now()), idle);
        if let Some(r) = slot.as_mut() {
            if r.child.try_wait().ok().flatten().is_none() {
                return Ok(r.base.clone());
            }
            *slot = None;
        }
        let exe = dir.join("whisper").join("whisper-server.exe");
        if !exe.is_file() || !model.is_file() {
            return Err(SttError::NotInstalled);
        }
        let t = Instant::now();
        let mut last = SttError::Timeout;
        for _ in 0..2 {
            match start(&exe, model).await {
                Ok((child, job, base)) => {
                    let id = {
                        let mut n = self.next_id.lock();
                        *n += 1;
                        *n
                    };
                    tracing::info!(ms = t.elapsed().as_millis() as u64, "whisper démarré");
                    *slot = Some(Running { child, _job: job, base: base.clone(), id });
                    self.touch(); // le délai court depuis que le serveur est prêt
                    tokio::spawn(self.clone().watch(id));
                    return Ok(base);
                }
                Err(e) => last = e,
            }
        }
        Err(last)
    }

    /// Réarme le minuteur d'inactivité.
    pub fn touch(&self) {
        self.idle.lock().0 = Some(Instant::now());
    }

    pub async fn stop(&self) {
        if let Some(mut r) = self.running.lock().await.take() {
            let _ = r.child.kill().await;
            tracing::info!("whisper arrêté");
        }
    }

    /// Fin d'une transcription : plein écran actif, on libère la mémoire tout de suite.
    pub async fn finish(&self) {
        self.finish_with(crate::fullscreen::active()).await;
    }

    async fn finish_with(&self, fullscreen: bool) {
        if fullscreen {
            self.stop().await
        } else {
            self.touch()
        }
    }

    /// Arrête le serveur `id` quand l'inactivité dépasse le délai ; s'éteint si un autre serveur a pris la place.
    async fn watch(self: Arc<Self>, id: u64) {
        loop {
            let (last, idle) = *self.idle.lock();
            let due = last.unwrap_or_else(Instant::now) + idle;
            tokio::time::sleep_until(due.into()).await;
            let mut slot = self.running.lock().await;
            if !slot.as_ref().is_some_and(|r| r.id == id) {
                return;
            }
            let (last, idle) = *self.idle.lock();
            if last.is_none_or(|l| l.elapsed() >= idle) {
                if let Some(mut r) = slot.take() {
                    let _ = r.child.kill().await;
                    tracing::info!("whisper arrêté après inactivité");
                }
                return;
            }
        }
    }
}

/// Lance le serveur sur un port libre et attend qu'il réponde (10 s au plus).
async fn start(exe: &Path, model: &Path) -> Result<(Child, Job, String), SttError> {
    // shortcut: le port peut être pris entre la libération et le lancement ; `ensure` réessaie une fois.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map_err(|e| SttError::Start(e.to_string()))?
        .port();
    let mut cmd = Command::new(exe);
    cmd.args(["--host", "127.0.0.1", "--port", &port.to_string(), "-m"]).arg(model).args(["-l", "fr", "-t", "8", "--no-timestamps"]);
    cmd.current_dir(exe.parent().unwrap_or(Path::new(".")))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let (mut child, job) = job::spawn(cmd).map_err(|e| SttError::Start(e.to_string()))?;
    let base = format!("http://127.0.0.1:{port}");
    let http = reqwest::Client::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if let Ok(Some(st)) = child.try_wait() {
            return Err(SttError::Start(format!("arrêt immédiat ({st})")));
        }
        if tokio::net::TcpStream::connect(("127.0.0.1", port)).await.is_ok()
            && http.get(&base).timeout(Duration::from_secs(2)).send().await.is_ok()
        {
            return Ok((child, job, base));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let _ = child.kill().await;
    Err(SttError::Start("pas de réponse en 10 s".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Copie le faux serveur (compilé par `cargo test`) en `whisper/whisper-server.exe`, avec un faux modèle.
    fn setup() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
        let src = std::env::current_exe().unwrap().parent().unwrap().parent().unwrap().join("examples").join("fake_whisper.exe");
        assert!(src.is_file(), "faux whisper absent : lancer `cargo test` (qui compile les exemples), pas `--lib` seul");
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("whisper")).unwrap();
        let exe = d.path().join("whisper").join("whisper-server.exe");
        std::fs::copy(src, &exe).unwrap();
        let model = d.path().join("m.bin");
        std::fs::write(&model, b"x").unwrap();
        let pid = exe.with_extension("pid");
        (d, model, pid)
    }

    fn pid_of(f: &Path) -> u32 {
        std::fs::read_to_string(f).unwrap().parse().unwrap()
    }

    fn gone_now(pid: u32) -> bool {
        let p = sysinfo::Pid::from_u32(pid);
        let mut sys = sysinfo::System::new();
        sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[p]), true);
        sys.process(p).is_none()
    }

    async fn gone(pid: u32) -> bool {
        for _ in 0..40 {
            if gone_now(pid) {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        false
    }

    #[tokio::test]
    async fn non_installe_sans_les_fichiers() {
        let d = tempfile::tempdir().unwrap();
        let w = Arc::new(WhisperProcess::default());
        assert!(matches!(w.ensure(d.path(), &d.path().join("m.bin"), Duration::from_secs(5)).await, Err(SttError::NotInstalled)));
    }

    #[tokio::test]
    async fn demarre_reutilise_puis_stop_tue_le_processus() {
        let (d, model, pidf) = setup();
        let w = Arc::new(WhisperProcess::default());
        let (a, b) = tokio::join!(w.ensure(d.path(), &model, Duration::from_secs(60)), w.ensure(d.path(), &model, Duration::from_secs(60)));
        let (a, b) = (a.unwrap(), b.unwrap());
        assert_eq!(a, b, "un seul démarrage");
        let pid = pid_of(&pidf);
        assert_eq!(w.ensure(d.path(), &model, Duration::from_secs(60)).await.unwrap(), a);
        assert_eq!(pid_of(&pidf), pid);
        let text = oniris_core::voice::stt::transcribe(&reqwest::Client::new(), &a, vec![0]).await.unwrap();
        assert_eq!(text, "ok");
        w.stop().await;
        assert!(gone(pid).await);
    }

    #[tokio::test]
    async fn l_inactivite_arrete_le_serveur() {
        let (d, model, pidf) = setup();
        let w = Arc::new(WhisperProcess::default());
        w.ensure(d.path(), &model, Duration::from_millis(800)).await.unwrap();
        let pid = pid_of(&pidf);
        tokio::time::sleep(Duration::from_millis(500)).await;
        w.touch(); // repousse l'échéance à ~1300 ms
        tokio::time::sleep(Duration::from_millis(500)).await; // t = 1000 ms : sans touch() il serait déjà arrêté
        assert!(!gone_now(pid), "touch() doit repousser l'arrêt");
        assert!(gone(pid).await);
        assert!(w.running.lock().await.is_none());
    }

    #[tokio::test]
    async fn le_plein_ecran_arrete_tout_de_suite() {
        let (d, model, pidf) = setup();
        let w = Arc::new(WhisperProcess::default());
        w.ensure(d.path(), &model, Duration::from_secs(60)).await.unwrap();
        let pid = pid_of(&pidf);
        w.finish_with(false).await;
        assert!(!gone_now(pid));
        w.finish_with(true).await;
        assert!(gone(pid).await);
    }
}
