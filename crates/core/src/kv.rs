//! Petit magasin clé-valeur (table `kv`, migration 8) : caches et états internes des modules.
use crate::CoreError;
use rusqlite::{params, Connection, OptionalExtension};

/// Valeur et date de dernière écriture (ms).
pub fn get(c: &Connection, key: &str) -> Result<Option<(String, i64)>, CoreError> {
    Ok(c.query_row("SELECT value, updated_ms FROM kv WHERE key = ?1", [key], |r| Ok((r.get(0)?, r.get(1)?))).optional()?)
}

pub fn set(c: &Connection, key: &str, value: &str, now: i64) -> Result<(), CoreError> {
    c.execute(
        "INSERT INTO kv(key, value, updated_ms) VALUES (?1, ?2, ?3) ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_ms = excluded.updated_ms",
        params![key, value, now],
    )?;
    Ok(())
}

pub fn delete(c: &Connection, key: &str) -> Result<(), CoreError> {
    c.execute("DELETE FROM kv WHERE key = ?1", [key])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::db;

    #[test]
    fn set_get_ecrase_et_delete() {
        let c = db();
        assert_eq!(get(&c, "a").unwrap(), None);
        set(&c, "a", "1", 10).unwrap();
        set(&c, "a", "2", 20).unwrap();
        assert_eq!(get(&c, "a").unwrap(), Some(("2".into(), 20)));
        delete(&c, "a").unwrap();
        assert_eq!(get(&c, "a").unwrap(), None);
    }
}
