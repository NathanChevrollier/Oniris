use crate::CoreError;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

/// `key` : "Space", "A".."Z" ou "F1".."F12".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HotkeySetting {
    pub alt: bool,
    pub ctrl: bool,
    pub shift: bool,
    pub win: bool,
    pub key: String,
}

impl Default for HotkeySetting {
    fn default() -> Self {
        Self { alt: true, ctrl: false, shift: false, win: false, key: "Space".into() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub user_name: String,
    pub hotkey: HotkeySetting,
    pub hold_threshold_ms: u32,
    pub hover_open: bool,
    pub hover_delay_ms: u32,
    pub autostart: bool,
    pub hide_pill_in_fullscreen: bool,
    pub timeline_start_hour: u8,
    pub timeline_end_hour: u8,
    pub reduce_motion: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            user_name: String::new(),
            hotkey: HotkeySetting::default(),
            hold_threshold_ms: 250,
            hover_open: true,
            hover_delay_ms: 350,
            autostart: true,
            hide_pill_in_fullscreen: true,
            timeline_start_hour: 8,
            timeline_end_hour: 23,
            reduce_motion: false,
        }
    }
}

/// Ligne absente : défauts insérés. JSON illisible : copié dans `broken_json`, remplacé par les défauts.
pub fn load_settings(conn: &Connection) -> Result<Settings, CoreError> {
    let row: Option<String> =
        conn.query_row("SELECT json FROM settings WHERE id = 1", [], |r| r.get(0)).optional()?;
    match row {
        Some(json) => match serde_json::from_str(&json) {
            Ok(s) => Ok(s),
            Err(_) => {
                let d = Settings::default();
                conn.execute(
                    "UPDATE settings SET broken_json = ?1, json = ?2 WHERE id = 1",
                    params![json, serde_json::to_string(&d)?],
                )?;
                Ok(d)
            }
        },
        None => {
            let d = Settings::default();
            save_settings(conn, &d)?;
            Ok(d)
        }
    }
}

pub fn save_settings(conn: &Connection, s: &Settings) -> Result<(), CoreError> {
    conn.execute(
        "INSERT INTO settings(id, json) VALUES (1, ?1) ON CONFLICT(id) DO UPDATE SET json = excluded.json",
        params![serde_json::to_string(s)?],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{migrate, open_db, CORE_MIGRATIONS};
    #[test] fn champs_manquants_remplis_par_defaut() {
        let s: Settings = serde_json::from_str(r#"{"user_name":"Nathan"}"#).unwrap();
        assert_eq!(s.user_name, "Nathan");
        assert_eq!(s.hold_threshold_ms, 250);
        assert_eq!(s.timeline_end_hour, 23);
    }
    #[test] fn raccourci_partiel_garde_le_reste() {
        let s: Settings = serde_json::from_str(r#"{"user_name":"Nathan","hotkey":{"alt":true}}"#).unwrap();
        assert_eq!(s.user_name, "Nathan");
        assert!(s.hotkey.alt);
        assert_eq!(s.hotkey.key, "Space");
    }
    #[test] fn json_corrompu_donne_les_defauts_et_garde_une_copie() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = open_db(&dir.path().join("t.db")).unwrap();
        migrate(&mut c, CORE_MIGRATIONS).unwrap();
        c.execute("INSERT INTO settings(id, json) VALUES (1, '{pas du json')", []).unwrap();
        assert_eq!(load_settings(&c).unwrap(), Settings::default());
        let (json, broken): (String, Option<String>) =
            c.query_row("SELECT json, broken_json FROM settings WHERE id = 1", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(broken.as_deref(), Some("{pas du json"));
        assert_eq!(json, serde_json::to_string(&Settings::default()).unwrap());
    }
    #[test] fn absent_donne_les_defauts_et_les_insere() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = open_db(&dir.path().join("t.db")).unwrap();
        migrate(&mut c, CORE_MIGRATIONS).unwrap();
        assert_eq!(load_settings(&c).unwrap(), Settings::default());
        let n: i64 = c.query_row("SELECT count(*) FROM settings", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
    }
    #[test] fn aller_retour() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = open_db(&dir.path().join("t.db")).unwrap();
        migrate(&mut c, CORE_MIGRATIONS).unwrap();
        let s = Settings { user_name: "Zoé".into(), hold_threshold_ms: 400, ..Settings::default() };
        save_settings(&c, &s).unwrap();
        assert_eq!(load_settings(&c).unwrap(), s);
    }
}
