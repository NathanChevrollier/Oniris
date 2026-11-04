pub mod agenda;
pub mod clipboard;
mod db;
mod error;
pub mod events;
pub mod notes;
pub mod reminders;
pub mod timers;
mod settings;
pub mod time_util;
pub mod tools;
pub mod when;

pub use db::{migrate, open_db, Migration, CORE_MIGRATIONS};
pub use error::CoreError;
pub use settings::{key_to_vk, load_settings, save_settings, HotkeySetting, Settings};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_error_displays_message() {
        assert!(CoreError::Invalid("x".into()).to_string().contains("x"));
    }
}
