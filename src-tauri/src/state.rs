use crate::hotkey::HotkeyConfig;
use crate::scheduler::Scheduler;
use oniris_core::events::Events;
use oniris_core::tools::{Tool, ToolBroker};
use oniris_core::{migrate, open_db, load_settings, Settings, CORE_MIGRATIONS};
use parking_lot::{Mutex, RwLock};
use rusqlite::Connection;
use std::sync::atomic::{AtomicBool, AtomicIsize};
use std::sync::{Arc, OnceLock};
use tauri::AppHandle;

pub struct AppState {
    pub panel_open: AtomicBool,
    /// Fenêtre au premier plan avant l'ouverture du panneau (HWND en entier, 0 si inconnue ou à nous) :
    /// cible du collage depuis l'historique du presse-papiers.
    pub prev_foreground: AtomicIsize,
    pub hide_pill_in_fullscreen: AtomicBool,
    /// État de « Mettre la voix en pause » (sans effet avant le plan 6).
    pub voice_paused: AtomicBool,
    /// Seule connexion du processus.
    pub db: Arc<Mutex<Connection>>,
    /// Client HTTP partagé (météo) : délai 8 s, agent générique.
    pub http: reqwest::Client,
    pub settings: RwLock<Settings>,
    /// Seul point de passage des actions d'IRIS ; ses outils sont fixés dans `AppState::open`.
    pub broker: Arc<ToolBroker>,
    /// Relu par le crochet clavier à chaque touche : n'écrire que brièvement, sans E/S.
    pub hotkey: Arc<RwLock<HotkeyConfig>>,
    /// Notifie panneau et pilule qu'un sujet de données a changé.
    #[allow(dead_code)] // les outils en gardent un clone ; le champ servira aux commandes qui notifient hors outil
    pub events: Arc<dyn Events>,
    /// Rempli dans `setup` après `manage` (il a besoin de l'`AppHandle`).
    pub scheduler: OnceLock<Arc<Scheduler>>,
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
        let db = Arc::new(Mutex::new(conn));
        let clock = Arc::new(|| std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64));
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
            http: oniris_core::weather::client()?,
            _log_guard: None,
            db,
            broker,
            settings: RwLock::new(s),
        })
    }
}
