//! Sondes des sites : un tour toutes les 60 s, panneau fermé ou non, plein écran compris (requêtes légères).
//! Sans site, la tâche n'a ni minuteur ni travail : elle dort jusqu'à `wake`. Journal : des nombres seulement,
//! jamais d'URL ni de nom de site.
use crate::state::AppState;
use oniris_core::events::Topic;
use oniris_core::sites::{self, Probe, SiteView};
use oniris_core::time_util::now_ms;
use oniris_core::Site;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};
use tokio::sync::{Notify, Semaphore};
use tokio::time::MissedTickBehavior;

const ROUND: Duration = Duration::from_secs(60);
const PARALLEL: usize = 4;
/// Écart minimal entre deux tours, même si les réglages changent souvent.
const MIN_GAP: Duration = Duration::from_secs(30);

/// Réveille la boucle : liste de sites modifiée.
static WAKE: LazyLock<Notify> = LazyLock::new(Notify::new);
/// Dernier tour hors ligne (échecs réseau partout et Windows sans Internet) : la carte le dit.
static OFFLINE: AtomicBool = AtomicBool::new(false);

pub fn wake() { WAKE.notify_one(); }

/// À appeler dans `setup`, état géré.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move { run(app).await });
}

/// Vues de la carte (base seule, aucune requête réseau).
pub fn list(st: &AppState) -> Result<Vec<SiteView>, String> {
    let sites = st.settings.read().sites.clone();
    sites::views(&st.db.lock(), &sites, OFFLINE.load(Ordering::SeqCst)).map_err(|e| e.to_string())
}

async fn run(app: AppHandle) {
    let http = match sites::client() {
        Ok(h) => h,
        Err(e) => return tracing::error!("sites : {e}"),
    };
    // Période d'environ 60 s ; un retard ne déclenche pas de rafale de rattrapage.
    let mut tick = tokio::time::interval(ROUND);
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    tick.tick().await; // le premier tick est immédiat
    let mut last: Option<Instant> = None;
    loop {
        let list = app.state::<AppState>().settings.read().sites.clone();
        if list.is_empty() {
            OFFLINE.store(false, Ordering::SeqCst);
            WAKE.notified().await;
            continue;
        }
        // Réglages modifiés en rafale : 30 s au moins entre deux tours.
        if let Some(t) = last.filter(|t| t.elapsed() < MIN_GAP) {
            tokio::time::sleep(MIN_GAP - t.elapsed()).await;
        }
        let list = app.state::<AppState>().settings.read().sites.clone(); // peut avoir changé pendant l'attente
        if !list.is_empty() {
            round(&app, &http, &list).await;
            last = Some(Instant::now());
        }
        tokio::select! {
            _ = tick.tick() => {}
            _ = WAKE.notified() => {}
        }
    }
}

/// Windows voit-il Internet ? (Network List Manager, sans trafic réseau.) En cas de doute : oui, pour que
/// de vraies pannes ne soient jamais masquées.
fn os_online() -> bool {
    use windows::Win32::Networking::NetworkListManager::{INetworkListManager, NetworkListManager, NLM_CONNECTIVITY_IPV4_INTERNET, NLM_CONNECTIVITY_IPV6_INTERNET};
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED};
    unsafe {
        let init = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
        let online = CoCreateInstance::<_, INetworkListManager>(&NetworkListManager, None, CLSCTX_ALL)
            .and_then(|m| m.GetConnectivity())
            .map(|c| c.0 & (NLM_CONNECTIVITY_IPV4_INTERNET.0 | NLM_CONNECTIVITY_IPV6_INTERNET.0) != 0)
            .unwrap_or(true);
        if init {
            CoUninitialize();
        }
        online
    }
}

/// Sonde les sites (4 à la fois), puis note les mesures et notifie les transitions.
async fn round(app: &AppHandle, http: &reqwest::Client, list: &[Site]) {
    let gate = Arc::new(Semaphore::new(PARALLEL));
    let mut set = tokio::task::JoinSet::new();
    for (i, s) in list.iter().enumerate() {
        let (http, gate, url) = (http.clone(), gate.clone(), s.url.clone());
        set.spawn(async move {
            let _slot = gate.acquire_owned().await;
            (i, sites::probe(&http, &url, sites::TIMEOUT).await)
        });
    }
    let mut probes = vec![Probe { ms: None, status: None, ok: false }; list.len()];
    while let Some(done) = set.join_next().await {
        match done {
            Ok((i, p)) => probes[i] = p,
            Err(_) => tracing::error!("sites : une sonde a planté"), // reste un échec
        }
    }
    // Windows n'est interrogé que si tout a échoué sans réponse HTTP : le cas courant n'y touche pas.
    let all_silent = probes.iter().all(|p| p.status.is_none());
    let online = if all_silent { tokio::task::spawn_blocking(os_online).await.unwrap_or(true) } else { true };
    let offline = sites::offline(&probes, online);
    OFFLINE.store(offline, Ordering::SeqCst);
    let st = app.state::<AppState>();
    let notices = sites::apply_round(&st.db, list, &probes, offline, now_ms());
    for n in notices {
        let (title, body) = sites::notice_text(n.kind, &list[n.site].name);
        crate::tray::notify(app, title, &body);
    }
    st.events.changed(Topic::Sites);
}

#[tauri::command(async)]
pub fn sites_list(state: tauri::State<AppState>) -> Result<Vec<SiteView>, String> {
    list(&state)
}

/// Remplace la liste : validation, enregistrement et réveil de la boucle passent par `apply_settings`.
#[tauri::command(async)]
pub fn sites_set(app: AppHandle, sites: Vec<Site>) -> Result<(), String> {
    let mut s = app.state::<AppState>().settings.read().clone();
    s.sites = sites;
    crate::commands::apply_settings(&app, s)
}
