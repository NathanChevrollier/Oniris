pub mod accounts;
pub mod agenda;
pub mod apps;
pub mod calendar;
pub mod launch;
pub mod mail;
pub mod music;
pub mod clipboard;
mod db;
mod error;
pub mod events;
pub mod favorites;
pub mod iris;
pub mod kv;
pub mod notes;
pub mod oauth;
pub mod projects;
pub mod reminders;
pub mod search;
pub mod sites;
pub mod secrets;
pub mod system;
pub mod timers;
mod settings;
pub mod time_util;
pub mod tools;
pub mod weather;
pub mod when;

pub use db::{migrate, open_db, Migration, CORE_MIGRATIONS};
pub use error::CoreError;
pub use settings::{key_to_vk, load_settings, merge_domains, save_settings, HotkeySetting, Settings, Site};

/// La connexion unique du processus, partagée par le courtier et les outils.
pub type Db = std::sync::Arc<parking_lot::Mutex<rusqlite::Connection>>;

/// Aides des tests de modules.
#[cfg(test)]
pub mod test_util {
    /// Base en mémoire, mêmes réglages qu'`open_db` (hors WAL, sans objet en mémoire), migrée.
    pub fn db() -> rusqlite::Connection {
        let mut c = rusqlite::Connection::open_in_memory().unwrap();
        c.execute_batch("PRAGMA secure_delete=ON; PRAGMA foreign_keys=ON;").unwrap();
        crate::migrate(&mut c, crate::CORE_MIGRATIONS).unwrap();
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_error_displays_message() {
        assert!(CoreError::Invalid("x".into()).to_string().contains("x"));
    }
}
