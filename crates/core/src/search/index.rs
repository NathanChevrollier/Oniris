use crate::CoreError;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub(crate) path: Box<str>,
    pub(crate) name_start: u16,
    pub(crate) is_dir: bool,
}

impl Entry {
    /// Nom : tranche finale du chemin (pas de copie).
    pub fn name(&self) -> &str {
        &self.path[self.name_start as usize..]
    }
}

#[derive(Debug, Default, Clone)]
pub struct FileIndex {
    pub(crate) entries: Vec<Entry>,
}

fn is_sep(c: char) -> bool {
    c == '/' || c == '\\'
}

/// Clé de comparaison d'un chemin Windows : minuscules, `\` partout, sans séparateur final.
pub fn norm_key(p: &str) -> String {
    p.trim_end_matches(is_sep).to_lowercase().replace('/', "\\")
}

/// `key` (déjà passée par `norm_key`) est-elle l'un des `prefixes` ou dans leur sous-arbre ?
pub fn under_any(key: &str, prefixes: &HashSet<String>) -> bool {
    prefixes.contains(key) || key.match_indices('\\').any(|(i, _)| prefixes.contains(&key[..i]))
}

impl FileIndex {
    /// Chemins et indicateur dossier, dans l'ordre d'insertion.
    pub fn iter(&self) -> impl Iterator<Item = (&str, bool)> {
        self.entries.iter().map(|e| (e.path.as_ref(), e.is_dir))
    }
    /// Comme `remove_prefix` pour plusieurs préfixes (clés `norm_key`), en un seul passage.
    pub fn remove_prefixes(&mut self, prefixes: &HashSet<String>) -> usize {
        if prefixes.is_empty() {
            return 0;
        }
        let before = self.entries.len();
        self.entries.retain(|e| !under_any(&norm_key(&e.path), prefixes));
        before - self.entries.len()
    }
    pub fn new() -> Self {
        Self::default()
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    /// Pas de dédoublonnage : l'appelant n'insère un chemin qu'une fois.
    /// Chemin vide ou trop long pour `u16` : ignoré (renvoie false).
    pub fn insert(&mut self, path: &str, is_dir: bool) -> bool {
        let path = path.trim_end_matches(is_sep);
        if path.is_empty() || path.len() > u16::MAX as usize {
            return false;
        }
        let name_start = path.rfind(is_sep).map_or(0, |i| i + 1) as u16;
        self.entries.push(Entry { path: path.into(), name_start, is_dir });
        true
    }
    /// Retire `prefix` et tout son sous-arbre ; renvoie le nombre d'entrées retirées.
    pub fn remove_prefix(&mut self, prefix: &str) -> usize {
        let prefix = prefix.trim_end_matches(is_sep);
        let before = self.entries.len();
        self.entries.retain(|e| {
            // Windows ignore la casse : comparaison ASCII sans allocation.
            let p = e.path.as_bytes();
            let n = prefix.len();
            let under = p.len() >= n
                && p[..n].eq_ignore_ascii_case(prefix.as_bytes())
                && (p.len() == n || p[n] == b'/' || p[n] == b'\\');
            !under
        });
        before - self.entries.len()
    }
    pub fn snapshot(&self) -> Result<Vec<u8>, CoreError> {
        postcard::to_stdvec(&self.entries).map_err(|e| CoreError::Invalid(format!("snapshot : {e}")))
    }
    pub fn restore(bytes: &[u8]) -> Result<Self, CoreError> {
        let entries: Vec<Entry> =
            postcard::from_bytes(bytes).map_err(|e| CoreError::Invalid(format!("restauration : {e}")))?;
        // Un snapshot corrompu ne doit pas faire paniquer `name()`.
        if entries.iter().any(|e| e.name_start as usize > e.path.len() || !e.path.is_char_boundary(e.name_start as usize)) {
            return Err(CoreError::Invalid("snapshot incohérent".into()));
        }
        Ok(Self { entries })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remove_prefix_retire_le_sous_arbre_seulement() {
        let mut i = FileIndex::new();
        for (p, d) in [("C:\\a", true), ("C:\\a\\b.txt", false), ("C:\\a\\c", true), ("C:\\ab", true), ("C:\\a\\c\\d", false)] {
            i.insert(p, d);
        }
        assert_eq!(i.remove_prefix("C:\\a"), 4);
        assert_eq!(i.len(), 1);
        assert_eq!(i.entries[0].path.as_ref(), "C:\\ab");
    }

    #[test]
    fn remove_prefixes_en_un_passage() {
        let mut i = FileIndex::new();
        for p in ["C:\\a", "C:\\a\\b.txt", "C:\\ab", "D:\\X\\y", "D:\\x\\y\\z", "D:\\keep"] {
            i.insert(p, false);
        }
        let set: HashSet<String> = ["c:\\a/", "D:\\x\\Y"].iter().map(|p| norm_key(p)).collect();
        assert_eq!(i.remove_prefixes(&set), 4);
        let left: Vec<_> = i.iter().map(|(p, _)| p).collect();
        assert_eq!(left, ["C:\\ab", "D:\\keep"]);
    }

    #[test]
    fn remove_prefix_insensible_a_la_casse() {
        let mut i = FileIndex::new();
        i.insert("C:\\Users\\Natha\\x.txt", false);
        i.insert("C:\\Users\\NathaN", true);
        assert_eq!(i.remove_prefix("c:\\users\\natha"), 1);
        assert_eq!(i.len(), 1);
    }

    #[test]
    fn snapshot_restore_fidele() {
        let mut i = FileIndex::new();
        i.insert("C:\\x\\é.txt", false);
        i.insert("C:\\x", true);
        let r = FileIndex::restore(&i.snapshot().unwrap()).unwrap();
        assert_eq!(r.entries, i.entries);
        assert_eq!(r.entries[0].name(), "é.txt");
        assert!(FileIndex::restore(&[1, 2, 3]).is_err());
    }

    #[test]
    fn memoire_par_entree_sous_150_octets() {
        let p = "C:\\Users\\natha\\Documents\\Projets\\Client-12\\src\\components\\widget\\fichier_12345.tsx";
        let per = std::mem::size_of::<Entry>() + p.len();
        assert!(per <= 150, "{per} octets/entrée");
    }
}
