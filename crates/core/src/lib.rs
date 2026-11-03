mod db;
mod error;
mod settings;

pub use db::{migrate, open_db, Migration, CORE_MIGRATIONS};
pub use error::CoreError;
pub use settings::{load_settings, save_settings, HotkeySetting, Settings};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn core_error_displays_message() {
        assert!(CoreError::Invalid("x".into()).to_string().contains("x"));
    }
}
