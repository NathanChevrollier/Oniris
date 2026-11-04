use super::ItemKind;
use crate::CoreError;
use super::frecency;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;

/// (kind, key) -> (score amorti, dernier usage en ms). Le score est celui connu à `last_ms` :
/// la lecture applique `frecency(score, last_ms, now)`.
pub type UsageMap = HashMap<(ItemKind, String), (f64, i64)>;

/// Nouveau score après un usage à `now` : `score * decay(now - last_ms) + 1`.
pub fn bumped(score: f64, last_ms: i64, now: i64) -> f64 {
    frecency(score, last_ms, now) + 1.0
}

pub fn record(c: &Connection, kind: ItemKind, key: &str, now: i64) -> Result<(), CoreError> {
    // Lecture puis écriture dans une même transaction : pas d'incrément perdu.
    // `unchecked_transaction` car on ne reçoit que `&Connection` (la connexion est déjà sous verrou).
    let tx = c.unchecked_transaction()?;
    let prev: Option<(f64, i64)> = tx
        .query_row("SELECT score, last_ms FROM usage WHERE kind = ?1 AND key = ?2", params![kind.as_str(), key], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .optional()?;
    let score = prev.map_or(1.0, |(s, t)| bumped(s, t, now));
    tx.execute(
        "INSERT INTO usage(kind, key, score, last_ms) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(kind, key) DO UPDATE SET score = ?3, last_ms = ?4",
        params![kind.as_str(), key, score, now],
    )?;
    tx.commit()?;
    Ok(())
}

/// Même mise à jour sur la copie mémoire.
pub fn bump(map: &mut UsageMap, kind: ItemKind, key: &str, now: i64) {
    let e = map.entry((kind, key.to_string())).or_insert((0.0, now));
    *e = (bumped(e.0, e.1, now), now);
}

pub fn load(c: &Connection) -> Result<UsageMap, CoreError> {
    let mut st = c.prepare("SELECT kind, key, score, last_ms FROM usage")?;
    let rows = st.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, f64>(2)?, r.get::<_, i64>(3)?))
    })?;
    let mut m = UsageMap::new();
    for row in rows {
        let (k, key, n, t) = row?;
        if let Some(k) = ItemKind::parse(&k) {
            m.insert((k, key), (n, t));
        }
    }
    Ok(m)
}

/// Ne garde que les `keep` usages les plus récents ; renvoie le nombre supprimé.
pub fn prune(c: &Connection, keep: usize) -> Result<usize, CoreError> {
    Ok(c.execute(
        "DELETE FROM usage WHERE rowid NOT IN (SELECT rowid FROM usage ORDER BY last_ms DESC LIMIT ?1)",
        params![keep as i64],
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::db;

    #[test]
    fn record_load_prune() {
        let c = db();
        record(&c, ItemKind::File, "a", 10).unwrap();
        record(&c, ItemKind::File, "a", 20).unwrap();
        record(&c, ItemKind::App, "b", 30).unwrap();
        record(&c, ItemKind::Note, "c", 5).unwrap();
        let m = load(&c).unwrap();
        let (s, t) = m[&(ItemKind::File, "a".into())];
        assert_eq!(t, 20);
        assert!((s - 2.0).abs() < 1e-6, "{s}"); // 10 ms d'écart : décroissance négligeable
        assert_eq!(prune(&c, 2).unwrap(), 1);
        assert_eq!(load(&c).unwrap().len(), 2);
        let mut m = UsageMap::new();
        bump(&mut m, ItemKind::App, "b", 7);
        bump(&mut m, ItemKind::App, "b", 9);
        let (s, t) = m[&(ItemKind::App, "b".into())];
        assert_eq!(t, 9);
        assert!((s - 2.0).abs() < 1e-6);
    }
}
