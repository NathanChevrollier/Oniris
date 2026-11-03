use crate::hotkey::HotkeyConfig;
use oniris_core::{migrate, open_db, load_settings, Settings, CORE_MIGRATIONS};
use parking_lot::{Mutex, RwLock};
use rusqlite::Connection;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

pub struct AppState {
    pub panel_open: AtomicBool,
    pub hide_pill_in_fullscreen: AtomicBool,
    /// Seule connexion du processus.
    pub db: Arc<Mutex<Connection>>,
    pub settings: RwLock<Settings>,
    /// Relu par le crochet clavier à chaque touche : n'écrire que brièvement, sans E/S.
    pub hotkey: Arc<RwLock<HotkeyConfig>>,
}

impl AppState {
    /// Ouvre `%APPDATA%\Oniris\oniris.db`, migre et charge les réglages.
    pub fn open() -> Result<Self, Box<dyn std::error::Error>> {
        let appdata = std::env::var_os("APPDATA").ok_or("variable APPDATA absente")?;
        let dir = std::path::Path::new(&appdata).join("Oniris");
        std::fs::create_dir_all(&dir)?;
        let mut conn = open_db(&dir.join("oniris.db"))?;
        migrate(&mut conn, CORE_MIGRATIONS)?;
        let s = load_settings(&conn)?;
        Ok(Self {
            panel_open: AtomicBool::new(false),
            hide_pill_in_fullscreen: AtomicBool::new(s.hide_pill_in_fullscreen),
            hotkey: Arc::new(RwLock::new(HotkeyConfig::from_setting(&s.hotkey, s.hold_threshold_ms as u64))),
            db: Arc::new(Mutex::new(conn)),
            settings: RwLock::new(s),
        })
    }
}
