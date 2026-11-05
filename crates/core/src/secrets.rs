//! Coffre des secrets (jetons d'actualisation, secret client Google).
//! En production : gestionnaire d'identifiants Windows (`src-tauri/src/secrets_win.rs`), cible `Oniris/<key>`.
//! Rien de ce qui passe par ici n'est journalisé, renvoyé à l'interface ni écrit en base.
use parking_lot::Mutex;
use std::collections::HashMap;

pub trait SecretStore: Send + Sync {
    fn get(&self, key: &str) -> Option<String>;
    fn set(&self, key: &str, value: &str) -> Result<(), String>;
    /// Sans erreur si la clé est absente.
    fn delete(&self, key: &str);
}

/// Clé du secret client Google.
pub const GOOGLE_CLIENT_SECRET: &str = "google-client-secret";

/// Clé du jeton d'actualisation d'un compte.
pub fn account_key(id: i64) -> String { format!("account/{id}") }

/// Coffre en mémoire, pour les tests.
#[derive(Default)]
pub struct MemorySecrets(Mutex<HashMap<String, String>>);

impl SecretStore for MemorySecrets {
    fn get(&self, key: &str) -> Option<String> { self.0.lock().get(key).cloned() }
    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        self.0.lock().insert(key.into(), value.into());
        Ok(())
    }
    fn delete(&self, key: &str) { self.0.lock().remove(key); }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn memoire_aller_retour() {
        let s = MemorySecrets::default();
        assert_eq!(s.get("a"), None);
        s.set("a", "x").unwrap();
        s.set("a", "y").unwrap();
        assert_eq!(s.get("a").as_deref(), Some("y"));
        s.delete("a");
        s.delete("a");
        assert_eq!(s.get("a"), None);
        assert_eq!(account_key(7), "account/7");
    }
}
