//! Sondes des sites : un tour toutes les 60 s, panneau fermé ou non, plein écran compris (requêtes légères).
//! Sans site, la tâche n'a ni minuteur ni travail : elle dort jusqu'à `wake`. Journal : des nombres seulement,
//! jamais d'URL ni de nom de site.
use crate::state::AppState;
use oniris_core::events::Topic;
use oniris_core::sites::{self, Probe, SiteState, SiteView};
use oniris_core::time_util::now_ms;
use oniris_core::Site;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tokio::sync::{Notify, Semaphore};

const ROUND: Duration = Duration::from_secs(60);
const PARALLEL: usize = 4;
/// Dernières mesures qui suffisent à `sites::state`.
const WINDOW: usize = 3;

/// Réveille la boucle : liste de sites modifiée.
static WAKE: LazyLock<Notify> = LazyLock::new(Notify::new);
/// Le dernier tour a échoué partout (au moins 2 sites) : le PC est hors ligne, la carte le dit.
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
    loop {
        let list = app.state::<AppState>().settings.read().sites.clone();
        if list.is_empty() {
            OFFLINE.store(false, Ordering::SeqCst);
            WAKE.notified().await;
            continue;
        }
        round(&app, &http, &list).await;
        tokio::select! {
            _ = tokio::time::sleep(ROUND) => {}
            _ = WAKE.notified() => {}
        }
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
    let st = app.state::<AppState>();
    let offline = sites::all_failed(&probes);
    OFFLINE.store(offline, Ordering::SeqCst);
    let mut notices = Vec::new();
    // Hors ligne : rien n'est noté, ces échecs ne disent rien des sites (pas de « panne » à retardement au retour du réseau).
    if !offline {
        let now = now_ms();
        let db = st.db.lock();
        let (mut prev, mut next) = (Vec::new(), Vec::new());
        for (s, p) in list.iter().zip(&probes) {
            let before = sites::history(&db, &s.url, WINDOW).map(|h| sites::state(&h, s.slow_ms));
            let after = sites::record(&db, &s.url, now, p).and_then(|_| sites::history(&db, &s.url, WINDOW)).map(|h| sites::state(&h, s.slow_ms));
            match (before, after) {
                (Ok(b), Ok(a)) => (prev.push(b), next.push(a)),
                _ => { tracing::warn!("sites : mesure non enregistrée"); (prev.push(SiteState::Unknown), next.push(SiteState::Unknown)) }
            };
        }
        if let Err(e) = sites::prune(&db, now) {
            tracing::warn!("sites : purge impossible : {e}");
        }
        notices = sites::transitions(&prev, &next, false);
    }
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
