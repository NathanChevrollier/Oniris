use crate::hotkey::HotkeyConfig;
use crate::scheduler::Scheduler;
use oniris_core::accounts::TokenCache;
use oniris_core::events::Events;
use oniris_core::oauth::{providers, Provider, ProviderConfig};
use oniris_core::secrets::{SecretStore, GOOGLE_CLIENT_SECRET};
use oniris_core::tools::{Tool, ToolBroker};
use oniris_core::{migrate, open_db, load_settings, Db, Settings, CORE_MIGRATIONS};
use parking_lot::{Mutex, RwLock};
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32};
use std::sync::{Arc, OnceLock};
use tauri::{AppHandle, Manager};

pub struct AppState {
    pub panel_open: AtomicBool,
    /// Fenêtre au premier plan avant l'ouverture du panneau (HWND en entier, 0 si inconnue ou à nous) :
    /// cible du collage depuis l'historique du presse-papiers.
    pub prev_foreground: AtomicIsize,
    pub hide_pill_in_fullscreen: AtomicBool,
    /// État de « Mettre la voix en pause » (sans effet avant le plan 6).
    pub voice_paused: AtomicBool,
    /// Seule connexion du processus.
    pub db: Db,
    /// Client HTTP partagé (météo) : délai 8 s, agent générique. `None` s'il n'a pas pu être construit :
    /// la météo est alors indisponible, mais l'app démarre.
    pub http: Option<reqwest::Client>,
    /// Client des API de comptes en ligne : délai 15 s.
    pub account_http: reqwest::Client,
    /// Coffre Windows : secret client Google et jetons d'actualisation.
    pub secrets: Arc<dyn SecretStore>,
    /// Jetons d'accès en mémoire, actualisés à la demande.
    pub accounts: Arc<TokenCache>,
    /// Connexion OAuth en cours (une seule, annulable).
    pub flight: crate::modules::accounts::Flight,
    pub settings: RwLock<Settings>,
    /// Seul point de passage des actions d'IRIS ; ses outils sont fixés dans `AppState::open`.
    pub broker: Arc<ToolBroker>,
    /// Relu par le crochet clavier à chaque touche : n'écrire que brièvement, sans E/S.
    pub hotkey: Arc<RwLock<HotkeyConfig>>,
    /// Notifie panneau et pilule qu'un sujet de données a changé.
    pub events: Arc<dyn Events>,
    /// Rempli dans `setup` après `manage` (il a besoin de l'`AppHandle`).
    pub scheduler: OnceLock<Arc<Scheduler>>,
    /// Rempli au démarrage par `modules::indexer::start` (sur un autre fil) ; `None` avant.
    pub indexer: OnceLock<crate::modules::indexer::Indexer>,
    /// Applications du menu Démarrer (rempli par `modules::apps`).
    pub apps: Arc<oniris_core::apps::Inventory>,
    /// Copie mémoire de la table `usage`, chargée à l'ouverture : la frappe ne lit jamais la base.
    pub usage: RwLock<oniris_core::search::UsageMap>,
    /// Entrées de recherche des favoris, reconstruites sur `Topic::Favorites` (la frappe ne lit jamais la base).
    pub favorites: RwLock<Arc<Vec<oniris_core::search::Item>>>,
    /// Inventaire des projets, recalculé en tâche de fond (`modules::projects`).
    pub projects: RwLock<oniris_core::projects::ProjectsCache>,
    /// Plus récent numéro de requête reçu par `search_query` (les plus anciennes abandonnent).
    pub search_req: AtomicU32,
    /// Garde le journal fichier vivant (vidage à la fermeture).
    pub _log_guard: Option<tracing_appender::non_blocking::WorkerGuard>,
}

impl AppState {
    /// Ouvre `%APPDATA%\Oniris\oniris.db`, migre, charge les réglages et construit le courtier.
    /// `app` : pour les événements et les outils qui en auront besoin.
    pub fn open(app: &AppHandle) -> Result<Self, Box<dyn std::error::Error>> {
        let appdata = std::env::var_os("APPDATA").ok_or("variable APPDATA absente")?;
        let dir = std::path::Path::new(&appdata).join("Oniris");
        std::fs::create_dir_all(&dir)?;
        let mut conn = open_db(&dir.join("oniris.db"))?;
        migrate(&mut conn, CORE_MIGRATIONS)?;
        let s = load_settings(&conn)?;
        let usage = oniris_core::search::load(&conn)?;
        let favorites = oniris_core::favorites::items(&conn);
        let db = Arc::new(Mutex::new(conn));
        let clock = Arc::new(|| oniris_core::time_util::now_ms() as u64);
        let events: Arc<dyn Events> = Arc::new(crate::events::TauriEvents(app.clone()));
        let mut tools: Vec<Arc<dyn Tool>> = vec![]; // les plans 2 à 5 ajoutent leurs outils ici
        tools.extend(oniris_core::agenda::tools::all(&db, &events));
        tools.extend(oniris_core::reminders::tools::all(&db, &events));
        tools.extend(oniris_core::notes::tools::all(&db, &events));
        tools.extend(oniris_core::timers::tools::all(&db, &events));
        tools.extend(oniris_core::clipboard::tools::all(&db));
        let snapshot: oniris_core::system::tools::Snapshot = Arc::new(crate::modules::system::snapshot);
        tools.extend(oniris_core::system::tools::all(&snapshot));
        tools.extend(oniris_core::weather::tools::all(&db));
        let apps = Arc::new(oniris_core::apps::Inventory::default());
        let opener: Arc<dyn oniris_core::launch::Opener> = Arc::new(crate::launch::RealOpener);
        let handle = app.clone();
        let roots: oniris_core::apps::tools::Roots = Arc::new(move || {
            let st = handle.state::<AppState>();
            let s = st.settings.read();
            crate::modules::indexer::effective_roots(&s.search_roots, &s.projects_roots, dirs::home_dir())
        });
        tools.extend(oniris_core::apps::tools::all(&opener, roots, &apps));
        tools.extend(oniris_core::favorites::tools::all(&db, &events, &opener));
        let handle = app.clone();
        let projects: oniris_core::projects::tools::Source = Arc::new(move || handle.state::<AppState>().projects.read().list.clone());
        tools.extend(oniris_core::projects::tools::all(&projects));
        let handle = app.clone();
        let env: oniris_core::projects::tools::ResumeEnv = Arc::new(move || crate::modules::projects::resume_env(&handle));
        tools.push(Arc::new(oniris_core::projects::tools::Resume { source: projects, env, opener: opener.clone() }));
        tools.extend(oniris_core::accounts::tools::all(&db));
        let secrets: Arc<dyn SecretStore> = Arc::new(crate::secrets_win::WinCredentials);
        let account_http = oniris_core::accounts::client()?;
        let handle = app.clone();
        let accounts = Arc::new(TokenCache::new(db.clone(), secrets.clone(), events.clone(), account_http.clone(), Arc::new(move |p| provider_config(&handle, p))));
        let gmail = oniris_core::mail::gmail::Client::new(account_http.clone());
        tools.extend(oniris_core::mail::tools::all(&db, &events, &accounts, gmail, &opener));
        let broker = Arc::new(ToolBroker::new(db.clone(), clock, tools)?);
        broker.mark_interrupted();
        broker.expire_stale();
        Ok(Self {
            panel_open: AtomicBool::new(false),
            prev_foreground: AtomicIsize::new(0),
            voice_paused: AtomicBool::new(false),
            hide_pill_in_fullscreen: AtomicBool::new(s.hide_pill_in_fullscreen),
            hotkey: Arc::new(RwLock::new(HotkeyConfig::from_setting(&s.hotkey, s.hold_threshold_ms as u64))),
            events,
            scheduler: OnceLock::new(),
            indexer: OnceLock::new(),
            apps,
            usage: RwLock::new(usage),
            favorites: RwLock::new(Arc::new(favorites)),
            projects: RwLock::default(),
            search_req: AtomicU32::new(0),
            http: oniris_core::weather::client()
                .inspect_err(|e| tracing::error!("météo : client HTTP impossible, météo indisponible : {e}"))
                .ok(),
            account_http,
            secrets,
            accounts,
            flight: Default::default(),
            _log_guard: None,
            db,
            broker,
            settings: RwLock::new(s),
        })
    }
}

/// Configuration OAuth courante : identifiant client des réglages, secret Google du coffre ; `None` si non configuré.
pub fn provider_config(app: &AppHandle, p: Provider) -> Option<ProviderConfig> {
    let st = app.state::<AppState>();
    let (google, spotify) = {
        let s = st.settings.read();
        (s.google_client_id.clone(), s.spotify_client_id.clone())
    };
    match p {
        Provider::Google if !google.is_empty() => st.secrets.get(GOOGLE_CLIENT_SECRET).map(|sec| providers::google(&google, Some(sec))),
        Provider::Spotify if !spotify.is_empty() => Some(providers::spotify(&spotify)),
        _ => None,
    }
}
