use crate::hotkey::HotkeyConfig;
use oniris_core::tools::{Tool, ToolBroker};
use oniris_core::{migrate, open_db, load_settings, Settings, CORE_MIGRATIONS};
use parking_lot::{Mutex, RwLock};
use rusqlite::Connection;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tauri::AppHandle;

pub struct AppState {
    pub panel_open: AtomicBool,
    pub hide_pill_in_fullscreen: AtomicBool,
    /// État de « Mettre la voix en pause » (sans effet avant le plan 6).
    pub voice_paused: AtomicBool,
    /// Seule connexion du processus.
    pub db: Arc<Mutex<Connection>>,
    pub settings: RwLock<Settings>,
    /// Seul point de passage des actions d'IRIS ; ses outils sont fixés dans `AppState::open`.
    pub broker: Arc<ToolBroker>,
    /// Relu par le crochet clavier à chaque touche : n'écrire que brièvement, sans E/S.
    pub hotkey: Arc<RwLock<HotkeyConfig>>,
    /// Garde le journal fichier vivant (vidage à la fermeture).
    pub _log_guard: Option<tracing_appender::non_blocking::WorkerGuard>,
}

impl AppState {
    /// Ouvre `%APPDATA%\Oniris\oniris.db`, migre, charge les réglages et construit le courtier.
    /// `_app` : pour les outils qui en auront besoin (notifications…, plans suivants).
    pub fn open(_app: &AppHandle) -> Result<Self, Box<dyn std::error::Error>> {
        let appdata = std::env::var_os("APPDATA").ok_or("variable APPDATA absente")?;
        let dir = std::path::Path::new(&appdata).join("Oniris");
        std::fs::create_dir_all(&dir)?;
        let mut conn = open_db(&dir.join("oniris.db"))?;
        migrate(&mut conn, CORE_MIGRATIONS)?;
        let s = load_settings(&conn)?;
        let db = Arc::new(Mutex::new(conn));
        let clock = Arc::new(|| std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64));
        let tools: Vec<Arc<dyn Tool>> = vec![]; // les plans 2 à 5 ajoutent leurs outils ici
        let broker = Arc::new(ToolBroker::new(db.clone(), clock, tools)?);
        broker.mark_interrupted();
        broker.expire_stale();
        Ok(Self {
            panel_open: AtomicBool::new(false),
            voice_paused: AtomicBool::new(false),
            hide_pill_in_fullscreen: AtomicBool::new(s.hide_pill_in_fullscreen),
            hotkey: Arc::new(RwLock::new(HotkeyConfig::from_setting(&s.hotkey, s.hold_threshold_ms as u64))),
            _log_guard: None,
            db,
            broker,
            settings: RwLock::new(s),
        })
    }
}
