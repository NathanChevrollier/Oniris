//! IRIS : état du cerveau local (Ollama), téléchargement de son modèle et lancement de Claude Code sans outils natifs.
//! Le journal ne porte aucun texte utilisateur.
use crate::job;
use crate::state::AppState;
use async_trait::async_trait;
use oniris_core::iris::{classify, invocation, locate, parse_line, ClaudeBrain, ClaudeError, ClaudeEvent, Executor, LocalStatus, McpConfig, OnDelta, Ollama};
use oniris_core::Settings;
use serde::Serialize;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

/// Claude Code : `claude.exe` trouvé (réglage, npm, installeur natif) ou absent.
#[derive(Serialize, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ClaudeStatus { Found { path: String }, Missing }

fn claude_exe(setting: &str) -> Option<PathBuf> {
    locate(setting, dirs::data_dir().as_deref(), dirs::home_dir().as_deref())
}

/// Lance `claude -p` ; fermer le `Job` (fin, erreur ou abandon du futur) tue le processus et ses descendants.
#[allow(dead_code)] // câblé par l'agent (tâche ultérieure)
pub struct RealClaude { pub program: PathBuf, pub cwd: PathBuf, pub model: String, pub timeout: Duration }

#[allow(dead_code)] // idem
impl RealClaude {
    pub fn new(program: PathBuf, cwd: PathBuf, model: String) -> Self {
        Self { program, cwd, model, timeout: Duration::from_secs(180) }
    }

    /// claude.exe des réglages (ou détecté), dossier de travail `%APPDATA%\Oniris\claude` ; `None` sans claude.exe.
    pub fn from_settings(s: &Settings) -> Option<Self> {
        let cwd = PathBuf::from(std::env::var_os("APPDATA")?).join("Oniris").join("claude");
        Some(Self::new(claude_exe(&s.claude_path)?, cwd, s.iris_claude_model.clone()))
    }

    async fn run(&self, system: &str, prompt: String, mcp: Option<&McpConfig>, on_delta: OnDelta<'_>) -> Result<String, ClaudeError> {
        std::fs::create_dir_all(&self.cwd).map_err(|_| ClaudeError::Failed("dossier de travail illisible".into()))?;
        let inv = invocation(self.program.clone(), self.cwd.clone(), system, &self.model, mcp, prompt);
        let mut cmd = Command::new(&inv.program);
        cmd.args(&inv.args).current_dir(&inv.cwd).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let (mut child, _job) = job::spawn(cmd).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => ClaudeError::NotInstalled,
            _ => ClaudeError::Failed("lancement impossible".into()),
        })?;
        let mut stdin = child.stdin.take().ok_or_else(|| ClaudeError::Failed("entrée indisponible".into()))?;
        let mut err = child.stderr.take().ok_or_else(|| ClaudeError::Failed("erreur indisponible".into()))?;
        let stderr = tokio::spawn(async move { let mut s = String::new(); let _ = err.read_to_string(&mut s).await; s });
        let mut lines = BufReader::new(child.stdout.take().ok_or_else(|| ClaudeError::Failed("sortie indisponible".into()))?).lines();

        let read = async {
            // Si le processus meurt avant d'avoir tout lu, l'erreur d'écriture est sans importance : la sortie le dira.
            let _ = stdin.write_all(inv.stdin.as_bytes()).await;
            drop(stdin);
            let (mut result, mut text, mut inited) = (None, String::new(), false);
            while let Ok(Some(line)) = lines.next_line().await {
                let ev = parse_line(&line);
                // Échec fermé : rien n'est accepté avant un Init valide qui annonce les outils.
                // Une erreur avant l'Init (limite, non connecté) reste classée : aucun outil n'a pu tourner.
                if let (false, Some(e @ ClaudeEvent::Result { is_error: true, .. })) = (inited, &ev) {
                    return Err(classify("", Some(e), None));
                }
                if !inited && matches!(ev, Some(ClaudeEvent::Delta(_) | ClaudeEvent::Result { .. })) {
                    return Err(ClaudeError::Failed("Claude a démarré sans annonce d'outils".into()));
                }
                match ev {
                    Some(ClaudeEvent::Init { tools }) => {
                        inited = true;
                        let bad: Vec<String> = tools.into_iter().filter(|t| !t.starts_with("mcp__oniris__")).collect();
                        if !bad.is_empty() {
                            return Err(ClaudeError::ForbiddenTools(bad));
                        }
                    }
                    Some(ClaudeEvent::Delta(d)) => { on_delta(&d); text.push_str(&d); }
                    Some(r @ ClaudeEvent::Result { .. }) => result = Some(r),
                    None => {}
                }
            }
            Ok((result, text))
        };
        let (result, streamed) = tokio::time::timeout(self.timeout, read).await.map_err(|_| ClaudeError::Failed("délai dépassé".into()))??;
        let exit = tokio::time::timeout(Duration::from_secs(2), child.wait()).await.ok().and_then(|r| r.ok()).and_then(|s| s.code());
        let stderr = tokio::time::timeout(Duration::from_secs(1), stderr).await.ok().and_then(|r| r.ok()).unwrap_or_default();
        match result {
            Some(ClaudeEvent::Result { text, is_error: false, .. }) => Ok(if text.is_empty() { streamed } else { text }),
            other => Err(classify(&stderr, other.as_ref(), exit)),
        }
    }
}

#[async_trait]
impl ClaudeBrain for RealClaude {
    async fn complete(&self, system: &str, prompt: String, on_delta: OnDelta<'_>) -> Result<String, ClaudeError> {
        self.run(system, prompt, None, on_delta).await
    }

    /// Le tube vit le temps du tour : `session` est lâchée au retour.
    async fn plan(&self, system: &str, prompt: String, exec: Arc<Executor>, on_delta: OnDelta<'_>) -> Result<String, ClaudeError> {
        let session = crate::modules::iris_mcp::serve(exec).map_err(|_| ClaudeError::Failed("serveur MCP indisponible".into()))?;
        let relay_exe = std::env::current_exe().map_err(|_| ClaudeError::Failed("exécutable introuvable".into()))?;
        let cfg = McpConfig { pipe: session.pipe.clone(), token: session.token.clone(), relay_exe };
        self.run(system, prompt, Some(&cfg), on_delta).await
    }
}

#[derive(Serialize)]
pub struct IrisStatus { local: LocalStatus, model: String, claude: ClaudeStatus }

#[derive(Serialize, Clone)]
struct PullEvent { done: Option<u64>, total: Option<u64>, error: Option<String> }

static PULLING: AtomicBool = AtomicBool::new(false);

fn model(app: &AppHandle) -> (String, u8) {
    let st = app.state::<AppState>();
    let s = st.settings.read();
    (s.iris_local_model.clone(), s.iris_unload_minutes)
}

/// Libère `PULLING` même si la tâche est abandonnée ou panique.
struct PullGuard;
impl Drop for PullGuard { fn drop(&mut self) { PULLING.store(false, Ordering::SeqCst); } }

#[tauri::command]
pub async fn iris_status(app: AppHandle) -> IrisStatus {
    let (model, keep) = model(&app);
    let setting = app.state::<AppState>().settings.read().claude_path.clone();
    let claude = claude_exe(&setting).map_or(ClaudeStatus::Missing, |p| ClaudeStatus::Found { path: p.to_string_lossy().into_owned() });
    IrisStatus { local: Ollama::new(&model, keep).status(&model).await, model, claude }
}

/// Télécharge le modèle configuré ; progression sur `oniris://iris-pull`, puis `{done: total}` ou `{error}`.
#[tauri::command]
pub async fn iris_pull_model(app: AppHandle) -> Result<(), String> {
    if PULLING.swap(true, Ordering::SeqCst) {
        return Err("téléchargement déjà en cours".into());
    }
    let _guard = PullGuard;
    let (model, keep) = model(&app);
    let emit = |done, total, error| { let _ = app.emit("oniris://iris-pull", PullEvent { done, total, error }); };
    let mut last = (0, 0);
    let r = Ollama::new(&model, keep).pull(&model, |d, t| { last = (d, t); emit(Some(d), Some(t), None); }).await;
    match r {
        Ok(()) => { emit(Some(last.1), Some(last.1), None); Ok(()) }
        Err(e) => { tracing::warn!("oniris: téléchargement du modèle local échoué ({e})"); emit(None, None, Some(e.to_string())); Err(e.to_string()) }
    }
}

/// Entrée en plein écran : libère la mémoire du modèle, hors du fil du détecteur.
pub fn unload_on_fullscreen(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let (model, keep) = model(&app);
        Ollama::new(&model, keep).unload(&model).await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// `examples/fake_claude.rs`, compilé par `cargo test` à côté du binaire de test.
    fn fake(dir: &Path, timeout_ms: u64) -> RealClaude {
        let deps = std::env::current_exe().unwrap();
        let p = deps.parent().unwrap().parent().unwrap().join("examples").join("fake_claude.exe");
        assert!(p.is_file(), "faux claude absent : lancer `cargo test` (qui compile les exemples), pas `--lib` seul");
        RealClaude { timeout: Duration::from_millis(timeout_ms), ..RealClaude::new(p, dir.join("cwd"), String::new()) }
    }

    async fn pid_of(file: &Path) -> u32 {
        for _ in 0..100 {
            if let Ok(s) = std::fs::read_to_string(file) { if let Ok(n) = s.parse() { return n; } }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("le faux claude n'a pas démarré");
    }

    async fn gone_within_2s(pid: u32) -> bool {
        let p = sysinfo::Pid::from_u32(pid);
        let mut sys = sysinfo::System::new();
        for _ in 0..40 {
            sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[p]), true);
            if sys.process(p).is_none() { return true; }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        false
    }

    #[tokio::test]
    async fn scenario_normal_donne_le_texte_complet() {
        let d = tempfile::tempdir().unwrap();
        let mut got = String::new();
        let r = fake(d.path(), 20_000).complete("sys", "normal".into(), &mut |t: &str| got.push_str(t)).await;
        assert_eq!(r, Ok("Bonjour".into()));
        assert_eq!(got, "Bonjour");
        assert!(d.path().join("cwd").is_dir());
    }

    #[tokio::test]
    async fn limite_d_usage_donne_unavailable() {
        let d = tempfile::tempdir().unwrap();
        let r = fake(d.path(), 20_000).complete("sys", "limite".into(), &mut |_: &str| {}).await;
        assert!(matches!(r, Err(ClaudeError::Unavailable(_))), "{r:?}");
    }

    #[tokio::test]
    async fn outil_interdit_tue_le_processus() {
        let d = tempfile::tempdir().unwrap();
        let pidf = d.path().join("pid");
        let r = fake(d.path(), 20_000).complete("sys", format!("bash:{}", pidf.display()), &mut |_: &str| {}).await;
        assert_eq!(r, Err(ClaudeError::ForbiddenTools(vec!["Bash".into()])));
        assert!(gone_within_2s(pid_of(&pidf).await).await);
    }

    #[tokio::test]
    async fn sans_annonce_d_outils_echec_ferme() {
        for sc in ["sansoutils", "sansinit"] {
            let d = tempfile::tempdir().unwrap();
            let pidf = d.path().join("pid");
            let r = fake(d.path(), 20_000).complete("sys", format!("{sc}:{}", pidf.display()), &mut |_: &str| {}).await;
            assert_eq!(r, Err(ClaudeError::Failed("Claude a démarré sans annonce d'outils".into())), "{sc}");
            assert!(gone_within_2s(pid_of(&pidf).await).await);
        }
    }

    #[tokio::test]
    async fn erreur_de_limite_avant_init_reste_unavailable() {
        let d = tempfile::tempdir().unwrap();
        let pidf = d.path().join("pid");
        let r = fake(d.path(), 20_000).complete("sys", format!("limitesansinit:{}", pidf.display()), &mut |_: &str| {}).await;
        assert!(matches!(r, Err(ClaudeError::Unavailable(_))), "{r:?}");
        assert!(gone_within_2s(pid_of(&pidf).await).await);
    }

    #[tokio::test]
    async fn silence_au_dela_du_delai() {
        let d = tempfile::tempdir().unwrap();
        let pidf = d.path().join("pid");
        let r = fake(d.path(), 500).complete("sys", format!("silence:{}", pidf.display()), &mut |_: &str| {}).await;
        assert_eq!(r, Err(ClaudeError::Failed("délai dépassé".into())));
        assert!(gone_within_2s(pid_of(&pidf).await).await);
    }

    #[tokio::test]
    async fn abandon_du_futur_tue_le_processus() {
        let d = tempfile::tempdir().unwrap();
        let pidf = d.path().join("pid");
        let brain = fake(d.path(), 60_000);
        let arg = format!("silence:{}", pidf.display());
        let task = tokio::spawn(async move { brain.complete("sys", arg, &mut |_: &str| {}).await });
        let pid = pid_of(&pidf).await;
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        assert!(gone_within_2s(pid).await);
    }

    #[tokio::test]
    async fn plan_sert_le_tube_pendant_le_tour_seulement() {
        let d = tempfile::tempdir().unwrap();
        let cfgf = d.path().join("cfg");
        let exec = crate::modules::iris_mcp::tests::exec(d.path());
        let r = fake(d.path(), 20_000).plan("sys", format!("mcp:{}", cfgf.display()), exec, &mut |_: &str| {}).await;
        assert_eq!(r, Ok("Bonjour".into()));
        let cfg: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&cfgf).unwrap()).unwrap();
        let o = &cfg["mcpServers"]["oniris"];
        assert_eq!(o["command"], std::env::current_exe().unwrap().to_string_lossy().as_ref());
        assert_eq!(o["args"][0], "--mcp-relay");
        assert_eq!(o["env"]["ONIRIS_MCP_TOKEN"].as_str().unwrap().len(), 32);
        assert_eq!(o["args"][2], "connecté", "le tube était joignable pendant le tour");
        tokio::time::sleep(Duration::from_millis(50)).await;
        let pipe = o["args"][1].as_str().unwrap();
        assert!(tokio::net::windows::named_pipe::ClientOptions::new().open(pipe).is_err(), "tube arrêté après le tour");
    }

    #[tokio::test]
    async fn programme_absent_donne_not_installed() {
        let d = tempfile::tempdir().unwrap();
        let brain = RealClaude::new(d.path().join("absent.exe"), d.path().join("cwd"), String::new());
        assert_eq!(brain.complete("s", "p".into(), &mut |_: &str| {}).await, Err(ClaudeError::NotInstalled));
    }
}
