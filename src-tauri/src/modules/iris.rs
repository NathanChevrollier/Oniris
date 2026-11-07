//! IRIS : tours de conversation (`iris_ask`, événements `oniris://iris`), état du cerveau local (Ollama),
//! téléchargement de son modèle et lancement de Claude Code sans outils natifs. Le journal ne porte aucun texte utilisateur.
use crate::job;
use crate::state::AppState;
use async_trait::async_trait;
use crate::commands::{dispatch_as_user, show_confirm};
use oniris_core::events::Topic;
use oniris_core::iris::memory::{self, HistoryItem, Memory};
use oniris_core::iris::{prompt, Agent, IrisEventKind, LocalBrain, PromptCtx, Quarantine, Segment, TurnInput, Vault};
use oniris_core::time_util::now_ms;
use oniris_core::tools::{Brain, Origin, PendingAction, ToolOutcome};
use parking_lot::Mutex;
use serde_json::json;
use oniris_core::iris::{classify, invocation, locate, parse_line, ClaudeBrain, ClaudeError, ClaudeEvent, Executor, LocalStatus, McpConfig, OnDelta, Ollama};
use oniris_core::Settings;
use serde::Serialize;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
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
pub struct RealClaude { pub program: PathBuf, pub cwd: PathBuf, pub model: String, pub timeout: Duration }

impl RealClaude {
    pub fn new(program: PathBuf, cwd: PathBuf, model: String) -> Self {
        Self { program, cwd, model, timeout: Duration::from_secs(180) }
    }

    /// claude.exe des réglages (ou détecté), dossier de travail `%APPDATA%\Oniris\claude` ; `None` sans claude.exe.
    pub fn from_settings(s: &Settings) -> Option<Self> {
        let cwd = dirs::data_dir()?.join("Oniris").join("claude");
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

/// claude.exe introuvable : chaque appel répond `NotInstalled`.
struct NoClaude;

#[async_trait]
impl ClaudeBrain for NoClaude {
    async fn complete(&self, _: &str, _: String, _: OnDelta<'_>) -> Result<String, ClaudeError> { Err(ClaudeError::NotInstalled) }
    async fn plan(&self, _: &str, _: String, _: Arc<Executor>, _: OnDelta<'_>) -> Result<String, ClaudeError> { Err(ClaudeError::NotInstalled) }
}

/// Événement d'un tour sur `oniris://iris`.
#[derive(Serialize, Clone)]
struct IrisEvent { turn: u32, #[serde(flatten)] kind: IrisEventKind }

/// Messages gardés pour le contexte : 6 échanges utilisateur / IRIS.
const CONTEXT_MESSAGES: usize = 12;
/// Octets d'un message, au plus.
const ASK_MAX: usize = 8 * 1024;

static NEXT_TURN: AtomicU32 = AtomicU32::new(0);
/// Le tour en cours (un seul à la fois).
static TURN: Mutex<Option<(u32, tauri::async_runtime::JoinHandle<()>)>> = parking_lot::const_mutex(None);
/// Coffre de la conversation en cours : une nouvelle conversation repart d'un coffre vide.
static VAULT: Mutex<Option<(i64, Arc<Mutex<Vault>>)>> = parking_lot::const_mutex(None);

fn vault_for(conv: i64) -> Arc<Mutex<Vault>> {
    let mut v = VAULT.lock();
    match &*v {
        Some((c, vault)) if *c == conv => vault.clone(),
        _ => {
            let fresh: Arc<Mutex<Vault>> = Default::default();
            *v = Some((conv, fresh.clone()));
            fresh
        }
    }
}

/// Lance un tour et rend son numéro ; le tour précédent, s'il court encore, est abandonné.
#[tauri::command]
pub async fn iris_ask(app: AppHandle, text: String, force_claude: bool) -> Result<u32, String> {
    let text = text.trim().to_string();
    if text.is_empty() || text.len() > ASK_MAX {
        return Err("message vide ou trop long".into());
    }
    let turn = NEXT_TURN.fetch_add(1, Ordering::SeqCst) + 1;
    let mut cur = TURN.lock();
    if let Some((_, old)) = cur.take() {
        old.abort();
    }
    let task = tauri::async_runtime::spawn(async move {
        if let Err(e) = run_turn(&app, turn, text, force_claude).await {
            tracing::warn!("oniris: tour d'IRIS interrompu ({e})");
            let kind = IrisEventKind::Error { kind: "failed", message: "IRIS n'a pas pu traiter ce message.".into(), offer_local: false };
            let _ = app.emit("oniris://iris", IrisEvent { turn, kind });
        }
    });
    *cur = Some((turn, task));
    Ok(turn)
}

/// Abandonne le tour s'il est encore le tour en cours (Claude Code et son tube MCP s'arrêtent avec lui).
#[tauri::command]
pub async fn iris_cancel(turn: u32) {
    let mut cur = TURN.lock();
    if cur.as_ref().is_some_and(|(t, _)| *t == turn) {
        if let Some((_, task)) = cur.take() {
            task.abort();
        }
    }
}

/// Cerveaux construits d'après les réglages du moment ; exécuteur sur le coffre de la conversation.
async fn run_turn(app: &AppHandle, turn: u32, text: String, force_claude: bool) -> Result<(), String> {
    let st = app.state::<AppState>();
    let s = st.settings.read().clone();
    let now = now_ms();
    let (conv, history, memories) = {
        let c = st.db.lock();
        let conv = memory::conversation(&c, now).map_err(|e| e.to_string())?;
        let history = memory::context(&c, conv, CONTEXT_MESSAGES).map_err(|e| e.to_string())?;
        let memories: Vec<String> = memory::list(&c).map_err(|e| e.to_string())?.into_iter().map(|m| m.text).collect();
        (conv, history, memories)
    };
    let vault = vault_for(conv);
    let local: Arc<dyn LocalBrain> = Arc::new(Ollama::new(&s.iris_local_model, s.iris_unload_minutes));
    let claude: Arc<dyn ClaudeBrain> = match RealClaude::from_settings(&s) {
        Some(c) => Arc::new(c),
        None => Arc::new(NoClaude),
    };
    let handle = app.clone();
    let exec = Arc::new(Executor {
        broker: st.broker.clone(),
        vault: vault.clone(),
        origin: Origin::Assistant { brain: Brain::Local },
        on_pending: Arc::new(move |p: &PendingAction| show_confirm(&handle, p)),
        virtuals: Some(Arc::new(Quarantine { local: local.clone(), claude: claude.clone(), vault, now_ms })),
    });
    let system = prompt::system(&s.iris_personality, &s.user_name, chrono::Local::now(), &memories, Brain::Local);
    let agent = Agent { local, claude, model: s.iris_local_model.clone(), keep_alive_min: s.iris_unload_minutes };
    let emitter = app.clone();
    let sink = move |kind: IrisEventKind| {
        let _ = emitter.emit("oniris://iris", IrisEvent { turn, kind });
    };
    let end = agent.turn(TurnInput { text: text.clone(), force_claude }, exec, PromptCtx { system, history }, &sink).await;
    let shown: String = end.shown.iter().map(|s| match s { Segment::Text { text } | Segment::Untrusted { text, .. } => text.as_str() }).collect();
    {
        let c = st.db.lock();
        memory::record(&c, conv, "user", None, &text, &text, now).map_err(|e| e.to_string())?;
        memory::record(&c, conv, "iris", Some(end.brain.as_str()), &end.planner_text, &shown, now_ms()).map_err(|e| e.to_string())?;
    }
    st.events.changed(Topic::Iris);
    Ok(())
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

#[tauri::command]
pub async fn iris_history(state: tauri::State<'_, AppState>, limit: usize) -> Result<Vec<HistoryItem>, String> {
    memory::history(&state.db.lock(), limit.min(500)).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn iris_history_clear(state: tauri::State<'_, AppState>) -> Result<(), String> {
    memory::clear(&state.db.lock()).map_err(|e| e.to_string())?;
    state.events.changed(Topic::Iris);
    Ok(())
}

#[tauri::command]
pub async fn iris_memories(state: tauri::State<'_, AppState>) -> Result<Vec<Memory>, String> {
    memory::list(&state.db.lock()).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn iris_memory_delete(app: AppHandle, id: i64) -> ToolOutcome {
    dispatch_as_user(&app, "memory.forget", json!({ "id": id })).await
}

/// Ferme la conversation en cours : le prochain message ouvre un nouveau fil.
#[tauri::command]
pub async fn iris_conversation_new(state: tauri::State<'_, AppState>) -> Result<i64, String> {
    let id = memory::new_conversation(&state.db.lock(), now_ms()).map_err(|e| e.to_string())?;
    state.events.changed(Topic::Iris);
    Ok(id)
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
    async fn plan_sert_le_tube_pendant_le_tour() {
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
        // L'arrêt du tube au drop est couvert par `iris_mcp::tests::le_drop_arrete_le_tube`.
        assert_eq!(o["args"][2], "connecté", "le tube était joignable pendant le tour");
    }

    #[test]
    fn from_settings_travaille_dans_appdata_oniris_claude() {
        let d = tempfile::tempdir().unwrap();
        let exe = fake(d.path(), 1).program;
        let s = Settings { claude_path: exe.to_string_lossy().into_owned(), iris_claude_model: "sonnet".into(), ..Settings::default() };
        let c = RealClaude::from_settings(&s).unwrap();
        assert_eq!((c.program, c.model), (exe, "sonnet".to_string()));
        assert_eq!(c.cwd, dirs::data_dir().unwrap().join("Oniris").join("claude"));
    }

    #[tokio::test]
    async fn programme_absent_donne_not_installed() {
        let d = tempfile::tempdir().unwrap();
        let brain = RealClaude::new(d.path().join("absent.exe"), d.path().join("cwd"), String::new());
        assert_eq!(brain.complete("s", "p".into(), &mut |_: &str| {}).await, Err(ClaudeError::NotInstalled));
    }

    /// Source jamais appelée : seuls les noms des outils sont lus.
    fn never<T>() -> Arc<dyn Fn() -> T + Send + Sync> { Arc::new(|| unreachable!("jamais appelé")) }

    /// Noms d'outils enregistrés : mêmes constructeurs que `AppState::open` (qui exige une `AppHandle`), plus les outils virtuels.
    fn registered_names(dir: &Path) -> Vec<String> {
        use oniris_core::{accounts, agenda, apps, calendar, clipboard, events, favorites, general, mail, music, notes, projects, reminders, secrets, sites, system, timers, weather};
        use oniris_core::iris::VirtualTools;
        use oniris_core::tools::Tool;
        let mut c = oniris_core::open_db(&dir.join("t.db")).unwrap();
        oniris_core::migrate(&mut c, oniris_core::CORE_MIGRATIONS).unwrap();
        let db = Arc::new(Mutex::new(c));
        let ev: Arc<dyn events::Events> = Arc::new(events::NoEvents);
        let opener: Arc<dyn oniris_core::launch::Opener> = Arc::new(crate::launch::RealOpener);
        let inventory = Arc::new(apps::Inventory::default());
        let http = reqwest::Client::new();
        let secrets: Arc<dyn secrets::SecretStore> = Arc::new(crate::secrets_win::WinCredentials);
        let tokens = Arc::new(accounts::TokenCache::new(db.clone(), secrets, ev.clone(), http.clone(), Arc::new(|_| None)));
        let media: Arc<dyn music::MediaControl> = Arc::new(crate::modules::smtc::Smtc);
        let web = music::tools::Web::new(db.clone(), ev.clone(), tokens.clone(), music::spotify::Client::new(http.clone()));
        let shell: Arc<dyn general::shell::ShellExec> = Arc::new(crate::modules::general::RealShell);
        let recycler: Arc<dyn general::recycle::Recycler> = Arc::new(crate::modules::general::ShellRecycler);
        let win: Arc<dyn general::windows::WindowControl> = Arc::new(crate::modules::general::Win32Windows);
        let source: projects::tools::Source = never();
        let mut tools: Vec<Arc<dyn Tool>> = vec![];
        tools.extend(agenda::tools::all(&db, &ev));
        tools.extend(reminders::tools::all(&db, &ev));
        tools.extend(notes::tools::all(&db, &ev));
        tools.extend(timers::tools::all(&db, &ev));
        tools.extend(clipboard::tools::all(&db));
        tools.extend(system::tools::all(&never()));
        tools.extend(weather::tools::all(&db));
        tools.extend(apps::tools::all(&opener, never(), &inventory));
        tools.extend(favorites::tools::all(&db, &ev, &opener));
        tools.extend(memory::all(&db, &ev));
        tools.extend(projects::tools::all(&source));
        tools.push(Arc::new(projects::tools::Resume { source, env: never(), opener: opener.clone() }));
        tools.extend(accounts::tools::all(&db));
        tools.extend(mail::tools::all(&db, &ev, &tokens, mail::gmail::Client::new(http), &opener));
        tools.extend(calendar::all(&db));
        tools.extend(music::tools::all(&media, &web));
        tools.push(Arc::new(sites::tools::Status { source: never() }));
        tools.push(Arc::new(sites::tools::Vps { source: never() }));
        tools.extend(general::shell::all(&shell, never(), dir.to_path_buf(), Arc::new(|| false)));
        tools.extend(general::files::all(&recycler, never(), general::files::protected_dirs()));
        tools.extend(general::windows::all(&win, &inventory));
        let local: Arc<dyn LocalBrain> = Arc::new(Ollama::new("m", 5));
        let q = Quarantine { local, claude: Arc::new(NoClaude), vault: Default::default(), now_ms };
        tools.iter().map(|t| t.spec().name).chain(q.specs().into_iter().map(|s| s.name)).collect()
    }

    #[test]
    fn noms_reels_aller_retour_et_outils_du_raccourci_enregistres() {
        let d = tempfile::tempdir().unwrap();
        let names = registered_names(d.path());
        assert!(names.len() > 60, "{}", names.len());
        for n in &names {
            assert!(!n.contains("__") && !n.contains("_.") && !n.contains("._"), "{n}");
            let w = oniris_core::iris::wire(n);
            assert!(w.len() <= 64 && w.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'), "{w}");
            assert_eq!(oniris_core::iris::unwire(&w), *n);
        }
        for t in oniris_core::iris::fastpath::TOOLS {
            assert!(names.iter().any(|n| n == t), "{t} n'est pas enregistré");
        }
    }
}
