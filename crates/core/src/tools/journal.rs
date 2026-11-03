//! Accès SQL à la table `actions`. Les verrous sont pris par l'appelant, jamais tenus pendant un `.await`.
use super::{JournalEntry, Origin, Provenance};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

/// Résultat provisoire d'une action réclamée : si le processus meurt en cours d'exécution, elle reste « failed ».
pub const INTERRUPTED: &str = r#"{"error":"interrompue avant la fin"}"#;

pub struct NewRow<'a> {
    pub ts: u64,
    pub tool: &'a str,
    pub args: &'a Value,
    pub origin: &'a Origin,
    pub prov: &'a Provenance,
    pub status: &'a str,
    pub result: Option<&'a Value>,
    pub undo: Option<&'a Value>,
    pub expires: Option<u64>,
}

pub fn insert(c: &Connection, r: NewRow) -> rusqlite::Result<i64> {
    c.execute(
        "INSERT INTO actions(ts_ms, tool, args, origin, provenance, status, result, undo, expires_at_ms)
         VALUES (?,?,?,?,?,?,?,?,?)",
        params![
            r.ts as i64,
            r.tool,
            r.args.to_string(),
            serde_json::to_string(r.origin).unwrap_or_default(),
            serde_json::to_string(r.prov).unwrap_or_default(),
            r.status,
            r.result.map(Value::to_string),
            r.undo.map(Value::to_string),
            r.expires.map(|e| e as i64)
        ],
    )?;
    Ok(c.last_insert_rowid())
}

/// Transition atomique : une seule réussite possible par ligne.
pub fn transition(c: &Connection, id: i64, from: &str, to: &str) -> rusqlite::Result<bool> {
    Ok(c.execute("UPDATE actions SET status=? WHERE id=? AND status=?", params![to, id, from])? == 1)
}

/// Réclame une action en attente avant de l'exécuter (statut provisoire « failed » + marqueur).
pub fn claim(c: &Connection, id: i64) -> rusqlite::Result<bool> {
    Ok(c.execute(
        "UPDATE actions SET status='failed', result=? WHERE id=? AND status='pending'",
        params![INTERRUPTED, id],
    )? == 1)
}

/// Clôture une action réclamée.
pub fn finish(c: &Connection, id: i64, status: &str, result: &Value, undo: Option<&Value>) -> rusqlite::Result<()> {
    c.execute(
        "UPDATE actions SET status=?, result=?, undo=? WHERE id=? AND status='failed' AND result=?",
        params![status, result.to_string(), undo.map(Value::to_string), id, INTERRUPTED],
    )?;
    Ok(())
}

pub fn status(c: &Connection, id: i64) -> Option<String> {
    c.query_row("SELECT status FROM actions WHERE id=?", [id], |r| r.get(0)).optional().ok().flatten()
}

pub struct Row {
    pub tool: String,
    pub args: Value,
}

pub fn load(c: &Connection, id: i64) -> Option<Row> {
    c.query_row("SELECT tool, args FROM actions WHERE id=?", [id], |r| {
        Ok(Row {
            tool: r.get(0)?,
            args: serde_json::from_str(&r.get::<_, String>(1)?).unwrap_or(Value::Null),
        })
    })
    .optional()
    .ok()
    .flatten()
}

pub fn load_undo(c: &Connection, id: i64) -> Option<(String, Value)> {
    c.query_row("SELECT tool, undo FROM actions WHERE id=? AND status='done' AND undo IS NOT NULL", [id], |r| {
        Ok((r.get::<_, String>(0)?, serde_json::from_str(&r.get::<_, String>(1)?).unwrap_or(Value::Null)))
    })
    .optional()
    .ok()
    .flatten()
}

pub fn expire(c: &Connection, now: u64) {
    let _ = c.execute(
        "UPDATE actions SET status='expired' WHERE status='pending' AND expires_at_ms < ?",
        [now as i64],
    );
}

pub fn list(c: &Connection, limit: u32) -> Vec<JournalEntry> {
    let Ok(mut st) = c.prepare(
        "SELECT id, ts_ms, tool, args, origin, provenance, status, result, undo FROM actions ORDER BY id DESC LIMIT ?",
    ) else {
        return vec![];
    };
    let rows = st.query_map([limit], |r| {
        let status: String = r.get(6)?;
        let undo: Option<String> = r.get(8)?;
        Ok(JournalEntry {
            id: r.get(0)?,
            ts_ms: r.get::<_, i64>(1)? as u64,
            tool: r.get(2)?,
            args: serde_json::from_str(&r.get::<_, String>(3)?).unwrap_or(Value::Null),
            origin: serde_json::from_str(&r.get::<_, String>(4)?).unwrap_or(Origin::User),
            provenance: serde_json::from_str(&r.get::<_, String>(5)?)
                .unwrap_or(Provenance::Untrusted { source: "?".into() }),
            undoable: status == "done" && undo.is_some(),
            status,
            result: r.get::<_, Option<String>>(7)?.and_then(|s| serde_json::from_str(&s).ok()),
        })
    });
    rows.map(|it| it.flatten().collect()).unwrap_or_default()
}
