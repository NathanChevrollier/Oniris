use crate::CoreError;
use rusqlite::Connection;
use std::path::Path;

pub struct Migration {
    pub version: u32,
    pub sql: &'static str,
}

pub const CORE_MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    sql: "CREATE TABLE settings (
        id INTEGER PRIMARY KEY CHECK (id = 1),
        json TEXT NOT NULL,
        broken_json TEXT
    ) STRICT;",
},
Migration {
    version: 2,
    sql: "CREATE TABLE actions (
        id INTEGER PRIMARY KEY,
        ts_ms INTEGER NOT NULL,
        tool TEXT NOT NULL,
        args TEXT NOT NULL,
        origin TEXT NOT NULL,
        provenance TEXT NOT NULL,
        status TEXT NOT NULL CHECK (status IN ('pending','running','done','failed','rejected','expired','undone','interrupted')),
        result TEXT,
        undo TEXT,
        expires_at_ms INTEGER
    ) STRICT;",
}];

pub fn open_db(path: &Path) -> Result<Connection, CoreError> {
    let c = Connection::open(path)?;
    c.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000; PRAGMA synchronous=NORMAL;",
    )?;
    Ok(c)
}

/// Applique, dans une transaction, les migrations dont la version dépasse `user_version`.
pub fn migrate(conn: &mut Connection, migrations: &[Migration]) -> Result<u32, CoreError> {
    let tx = conn.transaction()?;
    let start: u32 = tx.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let mut sorted: Vec<&Migration> = migrations.iter().collect();
    sorted.sort_by_key(|m| m.version);
    let mut current = start;
    for m in sorted.into_iter().filter(|m| m.version > start) {
        tx.execute_batch(m.sql)?;
        current = m.version;
        tx.execute_batch(&format!("PRAGMA user_version = {current}"))?;
    }
    tx.commit()?;
    Ok(current)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn base_en_wal_et_strict() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = open_db(&dir.path().join("t.db")).unwrap();
        let mode: String = c.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
        assert_eq!(mode, "wal");
        migrate(&mut c, CORE_MIGRATIONS).unwrap();
        assert!(c.execute("INSERT INTO settings(id, json) VALUES ('a', 1)", []).is_err());
    }
    #[test] fn migrations_idempotentes() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = open_db(&dir.path().join("t.db")).unwrap();
        let v1 = migrate(&mut c, CORE_MIGRATIONS).unwrap();
        let v2 = migrate(&mut c, CORE_MIGRATIONS).unwrap();
        assert_eq!(v1, v2);
    }
}
