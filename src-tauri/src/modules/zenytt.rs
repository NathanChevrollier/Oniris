//! Charge du VPS via le serveur MCP en lecture seule de Zenytt (`Zenytt.exe --mcp`, stdio). Zenytt garde la
//! connexion et les secrets SSH : Oniris n'en voit aucun. Processus lancé à la première ouverture du panneau
//! (tâche de fond, jamais sur le chemin d'ouverture), relevé toutes les 30 s panneau ouvert et hors plein écran,
//! arrêté 10 min après la fermeture et à la sortie d'Oniris. Journal : genres d'erreur seulement (les textes
//! de Zenytt peuvent contenir des noms de serveur ou d'hôte).
use crate::state::AppState;
use oniris_core::events::Topic;
use oniris_core::sites::{VpsStatus, VpsView};
use oniris_core::time_util::now_ms;
use parking_lot::Mutex;
use rmcp::model::CallToolRequestParams;
use rmcp::service::{RunningService, ServiceError};
use rmcp::{RoleClient, ServiceExt};
use serde_json::{json, Value};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Weak};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use tokio::sync::Notify;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const MCP_ARGS: &[&str] = &["--mcp"];
/// Par appel : la première connexion SSH de Zenytt peut prendre 20 s.
const CALL_TIMEOUT: Duration = Duration::from_secs(25);
const POLL: Duration = Duration::from_secs(30);
const IDLE_STOP: Duration = Duration::from_secs(10 * 60);

/// Où chercher Zenytt : le réglage s'il est rempli, puis les dossiers d'installation connus. L'installeur NSIS
/// de Zenytt (`installMode: currentUser`) installe dans `%LOCALAPPDATA%\Zenytt`, et l'exécutable porte le nom
/// du paquet Cargo (`zenytt-desktop.exe`), pas celui du produit.
fn candidates(setting: &str, local: Option<PathBuf>) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = Vec::new();
    if !setting.is_empty() {
        v.push(setting.into());
    }
    if let Some(l) = local {
        v.push(l.join(r"Programs\Zenytt\Zenytt.exe"));
        v.push(l.join(r"Zenytt\Zenytt.exe"));
        v.push(l.join(r"Zenytt\zenytt-desktop.exe"));
    }
    v
}

/// Premier exécutable présent (jamais lancé ici) ; `None` : ligne VPS masquée.
pub fn zenytt_exe(setting: &str) -> Option<PathBuf> {
    candidates(setting, std::env::var_os("LOCALAPPDATA").map(PathBuf::from)).into_iter().find(|p| p.is_file())
}

/// Échec d'un relevé. Seul `kind` est journalisé ; `text` est montré, raccourci et nettoyé.
#[derive(Debug)]
pub enum Fail {
    Spawn,
    Timeout,
    Closed,
    /// Refus de Zenytt (serveur non autorisé, clé jamais approuvée…) : le processus reste vivant.
    Tool(String),
}

impl Fail {
    fn kind(&self) -> &'static str {
        match self {
            Fail::Spawn => "lancement impossible",
            Fail::Timeout => "délai dépassé",
            Fail::Closed => "processus arrêté",
            Fail::Tool(_) => "refus de Zenytt",
        }
    }
    pub fn text(&self) -> String {
        match self {
            Fail::Tool(t) => short(t),
            Fail::Spawn => "Zenytt ne démarre pas".into(),
            Fail::Timeout => "Zenytt ne répond pas".into(),
            Fail::Closed => "Zenytt s'est arrêté".into(),
        }
    }
    /// Le processus ne répond plus : il est arrêté et relancé au tour suivant.
    fn fatal(&self) -> bool {
        !matches!(self, Fail::Tool(_))
    }
}

/// Texte d'erreur de Zenytt pour l'interface : sans caractère de contrôle, espaces resserrés, 90 caractères au plus.
fn short(s: &str) -> String {
    let clean = s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>();
    let words = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    if words.chars().count() > 90 {
        format!("{}…", words.chars().take(89).collect::<String>().trim_end())
    } else if words.is_empty() {
        "erreur de Zenytt".into()
    } else {
        words
    }
}

/// Réponse de `server_status` → vue. Disque : le plus rempli hors lecture seule et montages écartés,
/// comme `Metrics::disk_percent` de Zenytt (en pratique `/`).
fn view_from_status(server: &str, v: &Value, at_ms: i64) -> Option<VpsView> {
    let m = v.get("metrics")?;
    let disk_pct = m["disks"]
        .as_array()?
        .iter()
        .filter(|d| !d["readOnly"].as_bool().unwrap_or(false) && !d["ignored"].as_bool().unwrap_or(false))
        .filter_map(|d| Some((d["used"].as_f64()?, d["total"].as_f64().filter(|t| *t > 0.0)?)))
        .map(|(u, t)| u / t * 100.0)
        .fold(0.0, f64::max);
    Some(VpsView {
        server: server.chars().take(40).collect(),
        cpu_pct: m["cpuPercent"].as_f64()?,
        mem_pct: v["memPercent"].as_f64()?,
        disk_pct,
        load1: m["load"][0].as_f64()?,
        alerts: v["activeAlerts"].as_array().map_or(0, |a| a.len() as u32),
        at_ms,
    })
}

/// Domaines servis en HTTPS par les sites nginx activés (`files`, pas `disabled`), sans nom par défaut (`_`),
/// joker, expression régulière ni `localhost` ; `www.x` écarté si `x` est présent.
fn https_domains(v: &Value) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for f in v["files"].as_array().into_iter().flatten().filter(|f| f["enabled"].as_bool().unwrap_or(true)) {
        for s in f["servers"].as_array().into_iter().flatten().filter(|s| s["ssl"].as_bool() == Some(true)) {
            for n in s["serverNames"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                let n = n.trim().trim_end_matches('.').to_ascii_lowercase();
                let plain = n.contains('.') && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
                if plain && !out.contains(&n) {
                    out.push(n);
                }
            }
        }
    }
    let all = out.clone();
    out.retain(|n| !n.strip_prefix("www.").is_some_and(|bare| all.iter().any(|o| o == bare)));
    out
}

/// Processus Zenytt vivants, tués à la sortie d'Oniris (`shutdown`).
static LIVE: Mutex<Vec<Weak<Mutex<Child>>>> = Mutex::new(Vec::new());

/// Tue le processus quand le client disparaît (arrêt, échec, remplacement).
struct Kill(Arc<Mutex<Child>>);

impl Drop for Kill {
    fn drop(&mut self) {
        let mut c = self.0.lock();
        let _ = c.kill();
        let _ = c.wait();
    }
}

pub struct Client {
    // Ordre de destruction : le service d'abord, puis le processus.
    svc: RunningService<RoleClient, ()>,
    _child: Kill,
    timeout: Duration,
}

impl Client {
    pub async fn spawn(exe: &Path, args: &[&str], timeout: Duration) -> Result<Self, Fail> {
        let mut cmd = std::process::Command::new(exe);
        cmd.args(args).creation_flags(CREATE_NO_WINDOW).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        let mut child = cmd.spawn().map_err(|_| Fail::Spawn)?;
        let (stdin, stdout) = (child.stdin.take(), child.stdout.take());
        let kill = Kill(Arc::new(Mutex::new(child)));
        let (Some(stdin), Some(stdout)) = (stdin, stdout) else { return Err(Fail::Spawn) };
        let stdin = tokio::process::ChildStdin::from_std(stdin).map_err(|_| Fail::Spawn)?;
        let stdout = tokio::process::ChildStdout::from_std(stdout).map_err(|_| Fail::Spawn)?;
        let svc = tokio::time::timeout(timeout, ().serve((stdout, stdin))).await.map_err(|_| Fail::Timeout)?.map_err(|_| Fail::Closed)?;
        let mut live = LIVE.lock();
        live.retain(|w| w.strong_count() > 0);
        live.push(Arc::downgrade(&kill.0));
        drop(live);
        Ok(Self { svc, _child: kill, timeout })
    }

    async fn call(&self, tool: &'static str, args: Value) -> Result<Value, Fail> {
        let mut p = CallToolRequestParams::new(tool);
        if let Value::Object(m) = args {
            p = p.with_arguments(m);
        }
        let r = match tokio::time::timeout(self.timeout, self.svc.call_tool(p)).await {
            Err(_) => return Err(Fail::Timeout),
            Ok(Err(ServiceError::McpError(e))) => return Err(Fail::Tool(e.message.into_owned())),
            Ok(Err(_)) => return Err(Fail::Closed),
            Ok(Ok(r)) => r,
        };
        let text = r.content.iter().filter_map(|c| c.as_text()).map(|t| t.text.as_str()).collect::<Vec<_>>().join("\n");
        if r.is_error == Some(true) {
            return Err(Fail::Tool(text));
        }
        // Réponse en texte libre (« Aucun serveur n'est autorisé… ») : c'est la raison à montrer.
        serde_json::from_str(&text).map_err(|_| Fail::Tool(text))
    }

    /// `(id, nom)` des serveurs que l'utilisateur a ouverts à l'IA dans Zenytt.
    async fn servers(&self) -> Result<Vec<(String, String)>, Fail> {
        let v = self.call("list_servers", json!({})).await?;
        let list: Vec<_> = v
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| Some((s["id"].as_str()?.to_string(), s["name"].as_str().unwrap_or("VPS").to_string())))
            .collect();
        if list.is_empty() {
            return Err(Fail::Tool("aucun serveur autorisé pour l'IA dans Zenytt".into()));
        }
        Ok(list)
    }

    #[cfg(test)]
    fn kill(&self) {
        let _ = self._child.0.lock().kill();
    }
}

/// Client et cycle de vie, sans Tauri (testé contre le faux serveur). Horloge injectée dans `idle`.
pub struct Poller {
    client: Option<Client>,
    exe: Option<PathBuf>,
    closed_at: Option<Instant>,
    args: &'static [&'static str],
    timeout: Duration,
}

impl Poller {
    pub const fn new(args: &'static [&'static str], timeout: Duration) -> Self {
        Self { client: None, exe: None, closed_at: None, args, timeout }
    }

    async fn client(&mut self, exe: &Path) -> Result<&Client, Fail> {
        if self.exe.as_deref() != Some(exe) {
            self.client = None; // réglage modifié : autre exécutable
        }
        if self.client.is_none() {
            self.client = Some(Client::spawn(exe, self.args, self.timeout).await?);
            self.exe = Some(exe.to_path_buf());
        }
        Ok(self.client.as_ref().expect("client"))
    }

    /// Processus mort ou muet : abandonné, relancé au prochain tour.
    fn settle<T>(&mut self, r: Result<T, Fail>) -> Result<T, Fail> {
        if let Err(e) = &r {
            tracing::warn!("vps : {}", e.kind());
            if e.fatal() {
                self.client = None;
            }
        }
        r
    }

    /// Un relevé : premier serveur de `list_servers` (l'utilisateur n'a qu'un VPS), puis `server_status`.
    pub async fn poll(&mut self, exe: &Path) -> Result<VpsView, Fail> {
        self.closed_at = None;
        let r = async {
            let c = self.client(exe).await?;
            let (id, name) = c.servers().await?.swap_remove(0);
            let v = c.call("server_status", json!({ "server": id })).await?;
            view_from_status(&name, &v, now_ms()).ok_or(Fail::Tool("réponse de Zenytt illisible".into()))
        }
        .await;
        self.settle(r)
    }

    /// Domaines HTTPS de tous les serveurs (`list_sites` sur chacun).
    pub async fn import(&mut self, exe: &Path) -> Result<Vec<String>, Fail> {
        let r = async {
            let c = self.client(exe).await?;
            let mut all = Vec::new();
            for (id, _) in c.servers().await? {
                for d in https_domains(&c.call("list_sites", json!({ "server": id })).await?) {
                    if !all.contains(&d) {
                        all.push(d);
                    }
                }
            }
            Ok(all)
        }
        .await;
        self.settle(r)
    }

    /// Panneau fermé à `now` : arrêt 10 min après la fermeture. Renvoie le temps restant si le processus vit encore.
    pub fn idle(&mut self, now: Instant) -> Option<Duration> {
        self.client.as_ref()?;
        let since = *self.closed_at.get_or_insert(now);
        let left = IDLE_STOP.saturating_sub(now.saturating_duration_since(since));
        if left.is_zero() {
            self.stop();
            return None;
        }
        Some(left)
    }

    /// Panneau ouvert (même sans relevé, plein écran par exemple) : le compte à rebours d'arrêt repart de zéro.
    pub fn reopen(&mut self) {
        self.closed_at = None;
    }

    pub fn stop(&mut self) {
        self.client = None;
        self.closed_at = None;
    }

    #[cfg(test)]
    pub fn running(&self) -> bool {
        self.client.is_some()
    }
}

/// Attente avant le prochain tour panneau ouvert. Relevé dû mais non fait (plein écran) : une période entière,
/// pas une seconde (sinon une boucle d'une seconde pendant tout le jeu).
fn open_wait(since_last: Option<Duration>, fullscreen: bool) -> Duration {
    match since_last {
        Some(t) if t < POLL => POLL.saturating_sub(t).max(Duration::from_secs(1)),
        _ if fullscreen => POLL,
        _ => Duration::from_secs(1),
    }
}

static POLLER: LazyLock<tokio::sync::Mutex<Poller>> = LazyLock::new(|| tokio::sync::Mutex::new(Poller::new(MCP_ARGS, CALL_TIMEOUT)));
/// Dernier état montré (sans `zenytt`, recalculé à la lecture).
static STATE: Mutex<VpsStatus> = Mutex::new(VpsStatus { zenytt: false, view: None, error: None });
static WAKE: LazyLock<Notify> = LazyLock::new(Notify::new);
static STARTED: AtomicBool = AtomicBool::new(false);

/// Ne bloque pas : démarre la tâche de fond la première fois, puis la réveille.
pub fn on_panel_open(app: &AppHandle) {
    if !STARTED.swap(true, Ordering::SeqCst) {
        let app = app.clone();
        tauri::async_runtime::spawn(async move { run(app).await });
    }
    WAKE.notify_one();
}

pub fn on_panel_close() {
    WAKE.notify_one();
}

/// Sortie d'Oniris : tue Zenytt même si un relevé est en cours.
pub fn shutdown() {
    for c in LIVE.lock().drain(..).filter_map(|w| w.upgrade()) {
        let _ = c.lock().kill();
    }
}

fn exe_of(app: &AppHandle) -> Option<PathBuf> {
    let setting = app.state::<AppState>().settings.read().zenytt_path.clone();
    zenytt_exe(&setting)
}

async fn run(app: AppHandle) {
    let mut last: Option<Instant> = None;
    loop {
        let open = app.state::<AppState>().panel_open.load(Ordering::SeqCst);
        let exe = exe_of(&app);
        let mut p = POLLER.lock().await;
        let wait = match &exe {
            None => {
                p.stop();
                None
            }
            Some(exe) if open => {
                p.reopen();
                let due = last.is_none_or(|t| t.elapsed() >= POLL);
                if due && !crate::fullscreen::active() {
                    let r = p.poll(exe).await;
                    last = Some(Instant::now());
                    let mut st = STATE.lock();
                    match r {
                        Ok(v) => (st.view, st.error) = (Some(v), None),
                        Err(e) => st.error = Some(e.text()), // dernier relevé gardé, marqué indisponible
                    }
                    drop(st);
                    app.state::<AppState>().events.changed(Topic::Sites);
                }
                Some(open_wait(last.map(|t| t.elapsed()), crate::fullscreen::active()))
            }
            Some(_) => p.idle(Instant::now()),
        };
        drop(p);
        match wait {
            Some(d) => tokio::select! {
                _ = tokio::time::sleep(d) => {}
                _ = WAKE.notified() => {}
            },
            None => WAKE.notified().await,
        }
    }
}

pub fn status(app: &AppHandle) -> VpsStatus {
    match exe_of(app) {
        Some(_) => VpsStatus { zenytt: true, ..STATE.lock().clone() },
        None => VpsStatus::default(),
    }
}

#[tauri::command(async)]
pub fn vps_status(app: AppHandle) -> VpsStatus {
    status(&app)
}

/// Ajoute aux sites surveillés les domaines HTTPS absents, lus dans Zenytt. Renvoie le nombre ajouté.
#[tauri::command]
pub async fn sites_import_from_zenytt(app: AppHandle) -> Result<usize, String> {
    let exe = exe_of(&app).ok_or("Zenytt introuvable sur ce PC")?;
    let domains = POLLER.lock().await.import(&exe).await.map_err(|e| e.text())?;
    let mut s = app.state::<AppState>().settings.read().clone();
    let added = oniris_core::merge_domains(&mut s.sites, &domains);
    if added > 0 {
        tauri::async_runtime::spawn_blocking(move || crate::commands::apply_settings(&app, s)).await.map_err(|e| e.to_string())??;
    }
    Ok(added)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `examples/fake_zenytt.rs`, compilé par `cargo test` à côté du binaire de test.
    fn fake() -> PathBuf {
        let deps = std::env::current_exe().unwrap();
        let p = deps.parent().unwrap().parent().unwrap().join("examples").join("fake_zenytt.exe");
        assert!(p.is_file(), "faux serveur absent : lancer `cargo test` (qui compile les exemples), pas `--lib` seul");
        p
    }

    #[test]
    fn ordre_de_recherche_de_l_executable() {
        let l = PathBuf::from(r"C:\L");
        assert_eq!(
            candidates(r"D:\z\Z.exe", Some(l.clone())),
            vec![PathBuf::from(r"D:\z\Z.exe"), l.join(r"Programs\Zenytt\Zenytt.exe"), l.join(r"Zenytt\Zenytt.exe"), l.join(r"Zenytt\zenytt-desktop.exe")]
        );
        assert_eq!(candidates("", None), Vec::<PathBuf>::new());
        assert_eq!(zenytt_exe(r"C:\nulle\part\absent.exe").is_some(), zenytt_exe("").is_some());
    }

    #[test]
    fn erreur_courte_et_nettoyee() {
        assert_eq!(short("clé\u{1b}[31m jamais\n\napprouvée\u{0}"), "clé [31m jamais approuvée");
        let long = short(&"x".repeat(500));
        assert_eq!(long.chars().count(), 90);
        assert!(long.ends_with('…'));
        assert_eq!(short(" \n"), "erreur de Zenytt");
    }

    #[test]
    fn domaines_https_actives_seulement() {
        let v = json!({ "files": [{ "enabled": true, "servers": [
            { "serverNames": ["Blog.Example.com", "www.blog.example.com", "www.solo.example.com"], "ssl": true },
            { "serverNames": ["plain.example.com"], "ssl": false },
            { "serverNames": ["_", "*.example.com", "~^a\\d+$", "localhost", "a b.com"], "ssl": true }
        ]}], "disabled": [{ "enabled": false, "servers": [{ "serverNames": ["old.example.com"], "ssl": true }] }] });
        assert_eq!(https_domains(&v), vec!["blog.example.com", "www.solo.example.com"]);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn releve_des_metriques_et_import() {
        let exe = fake();
        let mut p = Poller::new(MCP_ARGS, Duration::from_secs(10));
        let v = p.poll(&exe).await.unwrap();
        assert_eq!((v.server.as_str(), v.cpu_pct, v.mem_pct, v.load1, v.alerts), ("vps-test", 12.5, 40.0, 0.42, 1));
        assert_eq!(v.disk_pct, 61.0, "disque le plus rempli, hors lecture seule et écartés");
        let domains = p.import(&exe).await.unwrap();
        assert_eq!(domains, vec!["blog.example.com", "shop.example.net"]);
        let mut sites = vec![oniris_core::Site { name: "Blog".into(), url: "https://BLOG.example.com/".into(), slow_ms: 1500 }];
        assert_eq!(oniris_core::merge_domains(&mut sites, &domains), 1);
        assert_eq!(sites[1].url, "https://shop.example.net");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn delai_depasse_puis_processus_mort_relance() {
        static HANG: &[&str] = &["--mcp", "hang"];
        let exe = fake();
        let mut p = Poller::new(HANG, Duration::from_millis(1500));
        assert!(matches!(p.poll(&exe).await, Err(Fail::Timeout)));
        assert!(!p.running(), "délai dépassé : processus abandonné");

        let mut p = Poller::new(MCP_ARGS, Duration::from_secs(10));
        p.poll(&exe).await.unwrap();
        p.client.as_ref().unwrap().kill();
        let r = p.poll(&exe).await;
        assert!(r.as_ref().is_err_and(Fail::fatal), "{r:?}");
        assert_eq!(r.unwrap_err().text(), "Zenytt s'est arrêté");
        assert!(!p.running());
        assert!(p.poll(&exe).await.is_ok(), "relancé au tour suivant");
    }

    #[test]
    fn attente_panneau_ouvert_et_plein_ecran() {
        let s = Duration::from_secs;
        assert_eq!(open_wait(None, true), POLL, "relevé dû mais plein écran : une période entière");
        assert_eq!(open_wait(Some(s(45)), true), POLL);
        assert_eq!(open_wait(Some(s(10)), true), s(20));
        assert_eq!(open_wait(Some(Duration::ZERO), false), POLL, "juste après un relevé");
        assert_eq!(open_wait(Some(Duration::from_millis(29_900)), false), s(1), "au moins une seconde");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn ouverture_sans_releve_remet_le_compte_a_zero() {
        let exe = fake();
        let mut p = Poller::new(MCP_ARGS, Duration::from_secs(10));
        p.poll(&exe).await.unwrap();
        let t0 = Instant::now();
        assert_eq!(p.idle(t0), Some(IDLE_STOP));
        p.reopen(); // panneau rouvert en plein écran : aucun relevé
        assert_eq!(p.idle(t0 + Duration::from_secs(9 * 60)), Some(IDLE_STOP), "le compte repart à la prochaine fermeture");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn arret_dix_minutes_apres_la_fermeture() {
        let exe = fake();
        let mut p = Poller::new(MCP_ARGS, Duration::from_secs(10));
        p.poll(&exe).await.unwrap();
        let t0 = Instant::now();
        assert_eq!(p.idle(t0), Some(IDLE_STOP));
        assert_eq!(p.idle(t0 + Duration::from_secs(9 * 60)), Some(Duration::from_secs(60)));
        p.poll(&exe).await.unwrap(); // réouverture : le compte repart
        assert!(p.idle(t0 + Duration::from_secs(10 * 60)).is_some());
        assert_eq!(p.idle(t0 + Duration::from_secs(20 * 60)), None);
        assert!(!p.running());
        assert_eq!(p.idle(t0 + Duration::from_secs(30 * 60)), None, "rien à arrêter");
    }
}
