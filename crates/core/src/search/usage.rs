use super::ItemKind;
use crate::CoreError;
use rusqlite::{params, Connection};
use std::collections::HashMap;

/// (kind, key) -> (nombre d'usages, dernier usage en ms).
pub type UsageMap = HashMap<(ItemKind, String), (u32, i64)>;

pub fn record(c: &Connection, kind: ItemKind, key: &str, now: i64) -> Result<(), CoreError> {
    c.execute(
        "INSERT INTO usage(kind, key, count, last_ms) VALUES (?1, ?2, 1, ?3)
         ON CONFLICT(kind, key) DO UPDATE SET count = count + 1, last_ms = ?3",
        params![kind.as_str(), key, now],
    )?;
    Ok(())
}

/// Même mise à jour sur la copie mémoire.
pub fn bump(map: &mut UsageMap, kind: ItemKind, key: &str, now: i64) {
    let e = map.entry((kind, key.to_string())).or_insert((0, now));
    e.0 = e.0.saturating_add(1);
    e.1 = now;
}

pub fn load(c: &Connection) -> Result<UsageMap, CoreError> {
    let mut st = c.prepare("SELECT kind, key, count, last_ms FROM usage")?;
    let rows = st.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, u32>(2)?, r.get::<_, i64>(3)?))
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
        assert_eq!(m[&(ItemKind::File, "a".into())], (2, 20));
        assert_eq!(prune(&c, 2).unwrap(), 1);
        assert_eq!(load(&c).unwrap().len(), 2);
        let mut m = UsageMap::new();
        bump(&mut m, ItemKind::App, "b", 7);
        bump(&mut m, ItemKind::App, "b", 9);
        assert_eq!(m[&(ItemKind::App, "b".into())], (2, 9));
    }
}
