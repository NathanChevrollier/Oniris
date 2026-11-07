use crate::CoreError;
use rusqlite::Connection;
use std::path::Path;

pub struct Migration {
    pub version: u32,
    pub sql: &'static str,
}

/// Liste globale unique, ordonnée, versions contiguës depuis 1 : chaque module (rappels, mails…) y ajoute
/// ses migrations à la suite, jamais dans une liste à part.
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
},
Migration {
    version: 3,
    sql: "CREATE TABLE events (
        id INTEGER PRIMARY KEY,
        title TEXT NOT NULL,
        start_ms INTEGER NOT NULL,
        end_ms INTEGER,
        all_day INTEGER NOT NULL DEFAULT 0,
        color TEXT,
        notes TEXT,
        deleted INTEGER NOT NULL DEFAULT 0,
        created_ms INTEGER NOT NULL
    ) STRICT;
    CREATE INDEX events_start ON events(start_ms) WHERE deleted = 0;",
},
Migration {
    version: 4,
    sql: "CREATE TABLE reminders (
        id INTEGER PRIMARY KEY,
        text TEXT NOT NULL,
        due_ms INTEGER,
        done INTEGER NOT NULL DEFAULT 0,
        notified INTEGER NOT NULL DEFAULT 0,
        deleted INTEGER NOT NULL DEFAULT 0,
        created_ms INTEGER NOT NULL
    ) STRICT;
    CREATE INDEX reminders_due ON reminders(due_ms) WHERE done = 0 AND deleted = 0 AND notified = 0;",
},
Migration {
    version: 5,
    sql: "CREATE TABLE notes (
        id INTEGER PRIMARY KEY,
        body TEXT NOT NULL,
        pinned INTEGER NOT NULL DEFAULT 0,
        deleted INTEGER NOT NULL DEFAULT 0,
        created_ms INTEGER NOT NULL,
        updated_ms INTEGER NOT NULL
    ) STRICT;
    CREATE VIRTUAL TABLE notes_fts USING fts5(body, content='notes', content_rowid='id', tokenize='unicode61 remove_diacritics 2');
    CREATE TRIGGER notes_ai AFTER INSERT ON notes BEGIN
        INSERT INTO notes_fts(rowid, body) VALUES (new.id, new.body);
    END;
    CREATE TRIGGER notes_ad AFTER DELETE ON notes BEGIN
        INSERT INTO notes_fts(notes_fts, rowid, body) VALUES ('delete', old.id, old.body);
    END;
    CREATE TRIGGER notes_au AFTER UPDATE OF body ON notes BEGIN
        INSERT INTO notes_fts(notes_fts, rowid, body) VALUES ('delete', old.id, old.body);
        INSERT INTO notes_fts(rowid, body) VALUES (new.id, new.body);
    END;",
},
Migration {
    version: 6,
    sql: "CREATE TABLE timers (
        id INTEGER PRIMARY KEY,
        label TEXT NOT NULL,
        kind TEXT NOT NULL CHECK (kind IN ('timer','focus','break')),
        duration_ms INTEGER NOT NULL,
        ends_ms INTEGER NOT NULL,
        state TEXT NOT NULL CHECK (state IN ('running','done','cancelled')),
        focus_cycle INTEGER,
        created_ms INTEGER NOT NULL
    ) STRICT;
    CREATE INDEX timers_running ON timers(ends_ms) WHERE state = 'running';",
},
Migration {
    version: 7,
    sql: "CREATE TABLE clipboard (
        id INTEGER PRIMARY KEY,
        kind TEXT NOT NULL CHECK (kind IN ('text','image')),
        text TEXT,
        truncated INTEGER NOT NULL DEFAULT 0,
        png BLOB,
        thumb BLOB,
        hash TEXT NOT NULL,
        pinned INTEGER NOT NULL DEFAULT 0,
        created_ms INTEGER NOT NULL
    ) STRICT;
    CREATE UNIQUE INDEX clipboard_hash ON clipboard(hash);",
},
Migration {
    version: 8,
    sql: "CREATE TABLE kv (
        key TEXT PRIMARY KEY,
        value TEXT NOT NULL,
        updated_ms INTEGER NOT NULL
    ) STRICT;",
},
Migration {
    version: 9,
    sql: "ALTER TABLE reminders ADD COLUMN done_ms INTEGER;",
},
Migration {
    version: 10,
    sql: "CREATE TABLE usage (
        kind TEXT NOT NULL,
        key TEXT NOT NULL,
        score REAL NOT NULL,
        last_ms INTEGER NOT NULL,
        PRIMARY KEY (kind, key)
    ) STRICT;",
},
Migration {
    version: 11,
    // icon_tried_ms : dernière tentative de récupération du favicon (fenêtre de nouvel essai, tâche 6).
    sql: "CREATE TABLE favorites (
        id INTEGER PRIMARY KEY,
        title TEXT NOT NULL,
        url TEXT NOT NULL,
        category TEXT,
        position INTEGER NOT NULL,
        icon TEXT,
        icon_tried_ms INTEGER,
        deleted INTEGER NOT NULL DEFAULT 0,
        created_ms INTEGER NOT NULL
    ) STRICT;
    CREATE UNIQUE INDEX favorites_url ON favorites(url) WHERE deleted = 0;",
},
Migration {
    version: 12,
    sql: "CREATE TABLE project_prefs (
        path TEXT PRIMARY KEY,
        pinned INTEGER NOT NULL DEFAULT 0,
        hidden INTEGER NOT NULL DEFAULT 0
    ) STRICT;",
},
Migration {
    version: 13,
    // label : adresse ou nom affiché ; status : ok | reauth. Les jetons sont dans le coffre Windows, jamais ici.
    sql: "CREATE TABLE accounts (
        id INTEGER PRIMARY KEY,
        provider TEXT NOT NULL,
        label TEXT NOT NULL,
        color TEXT NOT NULL,
        status TEXT NOT NULL,
        created_ms INTEGER NOT NULL
    ) STRICT;
    CREATE UNIQUE INDEX accounts_provider_label ON accounts(provider, label);",
},
Migration {
    version: 14,
    // Cache des en-têtes de la boîte de réception (jamais le corps). labels : JSON ; bucket : todo | read | filtered ;
    // bucket_src : heuristic | iris.
    sql: "CREATE TABLE mails (
        account_id INTEGER NOT NULL,
        id TEXT NOT NULL,
        thread_id TEXT NOT NULL,
        from_name TEXT NOT NULL,
        from_addr TEXT NOT NULL,
        subject TEXT NOT NULL,
        snippet TEXT NOT NULL,
        date_ms INTEGER NOT NULL,
        unread INTEGER NOT NULL,
        labels TEXT NOT NULL,
        bucket TEXT NOT NULL,
        bucket_src TEXT NOT NULL,
        summary TEXT,
        tag TEXT,
        PRIMARY KEY(account_id, id)
    ) STRICT;
    CREATE INDEX mails_date ON mails(date_ms);
    CREATE TABLE mail_sync (
        account_id INTEGER PRIMARY KEY,
        history_id TEXT,
        last_ok_ms INTEGER,
        backoff_until_ms INTEGER
    ) STRICT;",
},
Migration {
    version: 15,
    // Événements des agendas Google (lecture seule). Le syncToken de chaque agenda est dans `kv` : gcal_sync/<compte>/<agenda>.
    sql: "CREATE TABLE external_events (
        account_id INTEGER NOT NULL,
        id TEXT NOT NULL,
        calendar_id TEXT NOT NULL,
        title TEXT NOT NULL,
        start_ms INTEGER NOT NULL,
        end_ms INTEGER NOT NULL,
        all_day INTEGER NOT NULL,
        location TEXT,
        PRIMARY KEY(account_id, calendar_id, id)
    ) STRICT;
    CREATE INDEX external_events_start ON external_events(start_ms);",
},
Migration {
    version: 16,
    // Sondes des sites surveillés (`site` = URL), gardées 24 h. `ms` et `status` vides : pas de réponse.
    sql: "CREATE TABLE probes (
        site TEXT NOT NULL,
        at_ms INTEGER NOT NULL,
        ms INTEGER,
        status INTEGER,
        ok INTEGER NOT NULL
    ) STRICT;
    CREATE INDEX probes_site_at ON probes(site, at_ms);
    CREATE INDEX probes_at ON probes(at_ms);",
},
Migration {
    version: 17,
    // IRIS : conversations, messages (`planner_text` = ce que voit le planificateur, `shown_text` = ce que voit l'utilisateur)
    // et préférences retenues explicitement.
    sql: "CREATE TABLE conversations (
        id INTEGER PRIMARY KEY,
        started_ms INTEGER NOT NULL,
        last_ms INTEGER NOT NULL
    ) STRICT;
    CREATE TABLE messages (
        id INTEGER PRIMARY KEY,
        conversation_id INTEGER NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
        ts_ms INTEGER NOT NULL,
        role TEXT NOT NULL CHECK (role IN ('user', 'iris')),
        brain TEXT CHECK (brain IN ('local', 'claude', 'fast')),
        planner_text TEXT NOT NULL,
        shown_text TEXT NOT NULL
    ) STRICT;
    CREATE INDEX messages_conversation ON messages(conversation_id, id);
    CREATE TABLE memories (
        id INTEGER PRIMARY KEY,
        text TEXT NOT NULL,
        created_ms INTEGER NOT NULL
    ) STRICT;",
}];

pub fn open_db(path: &Path) -> Result<Connection, CoreError> {
    let c = Connection::open(path)?;
    c.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA secure_delete=ON; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000; PRAGMA synchronous=NORMAL;",
    )?;
    Ok(c)
}

/// Applique, dans une transaction, les migrations dont la version dépasse `user_version`.
/// Base créée par un build plus récent (version inconnue) : `CoreError::Invalid`, rien n'est touché.
pub fn migrate(conn: &mut Connection, migrations: &[Migration]) -> Result<u32, CoreError> {
    let tx = conn.transaction()?;
    let start: u32 = tx.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let known = migrations.iter().map(|m| m.version).max().unwrap_or(0);
    if start > known {
        return Err(CoreError::Invalid(format!(
            "base en version {start}, plus récente que ce build (version {known})"
        )));
    }
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
        let secure: i64 = c.query_row("PRAGMA secure_delete", [], |r| r.get(0)).unwrap();
        assert_eq!(secure, 1, "le contenu supprimé (presse-papiers) est écrasé sur disque");
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
    #[test] fn versions_uniques_et_contigues_depuis_1() {
        for (i, m) in CORE_MIGRATIONS.iter().enumerate() {
            assert_eq!(m.version, i as u32 + 1, "migration n°{i}");
        }
    }
    #[test] fn base_d_une_version_plus_recente_refusee() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = open_db(&dir.path().join("t.db")).unwrap();
        let last = migrate(&mut c, CORE_MIGRATIONS).unwrap();
        c.execute_batch(&format!("PRAGMA user_version = {}", last + 1)).unwrap();
        assert!(matches!(migrate(&mut c, CORE_MIGRATIONS), Err(CoreError::Invalid(_))));
    }
}
