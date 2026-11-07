//! Corbeille : envoi injecté (`Recycler`, le vrai vit dans src-tauri) et restauration depuis les fichiers `$I`/`$R`.
use std::path::{Path, PathBuf};

pub trait Recycler: Send + Sync {
    fn recycle(&self, p: &Path) -> Result<(), String>;
    /// Remet `original` à sa place depuis sa plus récente suppression faite après `deleted_after_ms`.
    fn restore(&self, original: &Path, deleted_after_ms: i64) -> Result<(), String>;
}

/// Écart entre l'époque FILETIME (1601) et l'époque Unix, en millisecondes.
const FILETIME_UNIX_MS: i64 = 11_644_473_600_000;
const MAX_INFO_CHARS: usize = 32_768;

/// Fichier `$I` version 2 : version (8), taille (8), FILETIME (8), longueur en caractères (4), chemin UTF-16.
pub fn parse_info(b: &[u8]) -> Option<(PathBuf, i64)> {
    let u64_at = |i: usize| Some(u64::from_le_bytes(b.get(i..i + 8)?.try_into().ok()?));
    if u64_at(0)? != 2 {
        return None;
    }
    let ms = (u64_at(16)? / 10_000) as i64 - FILETIME_UNIX_MS;
    let n = u32::from_le_bytes(b.get(24..28)?.try_into().ok()?) as usize;
    if n > MAX_INFO_CHARS {
        return None;
    }
    let units: Vec<u16> = b.get(28..28 + 2 * n)?.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
    let s = String::from_utf16(&units).ok()?;
    Some((PathBuf::from(s.trim_end_matches('\0')), ms))
}

/// Cherche dans `bin_dir` le `$I` le plus récent de `original` (après `after_ms`), renomme son `$R` jumeau vers
/// `original` (refusé si la cible existe), puis supprime le `$I`.
pub fn restore_from(bin_dir: &Path, original: &Path, after_ms: i64) -> Result<(), String> {
    let want = original.to_string_lossy().to_lowercase();
    let mut best: Option<(i64, PathBuf)> = None;
    for e in std::fs::read_dir(bin_dir).map_err(|e| format!("Corbeille illisible : {e}"))?.flatten() {
        if !e.file_name().to_string_lossy().starts_with("$I") {
            continue;
        }
        let Some((orig, ms)) = std::fs::read(e.path()).ok().and_then(|b| parse_info(&b)) else { continue };
        if ms >= after_ms && orig.to_string_lossy().to_lowercase() == want && best.as_ref().is_none_or(|(b, _)| ms > *b) {
            best = Some((ms, e.path()));
        }
    }
    let (_, info) = best.ok_or("élément introuvable dans la Corbeille")?;
    let suffix = &info.file_name().unwrap_or_default().to_string_lossy()[2..];
    let data = info.with_file_name(format!("$R{suffix}"));
    // `symlink_metadata` : un lien cassé occupe aussi la place (et `rename` remplacerait un fichier existant).
    if std::fs::symlink_metadata(original).is_ok() {
        return Err("un élément occupe déjà cet emplacement".into());
    }
    std::fs::rename(&data, original).map_err(|e| format!("restauration impossible : {e}"))?;
    let _ = std::fs::remove_file(&info); // restauré : un `$I` orphelin ne ferait qu'une entrée fantôme
    Ok(())
}

#[cfg(test)]
pub(crate) fn info_bytes(original: &Path, deleted_ms: i64) -> Vec<u8> {
    let units: Vec<u16> = original.to_string_lossy().encode_utf16().chain([0]).collect();
    let ft = ((deleted_ms + FILETIME_UNIX_MS) * 10_000) as u64;
    let mut b = [2u64.to_le_bytes(), 42u64.to_le_bytes(), ft.to_le_bytes()].concat();
    b.extend((units.len() as u32).to_le_bytes());
    b.extend(units.iter().flat_map(|u| u.to_le_bytes()));
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lit_un_fichier_i() {
        let p = Path::new(r"C:\Users\x\Documents\é.txt");
        assert_eq!(parse_info(&info_bytes(p, 1_700_000_000_123)), Some((p.to_path_buf(), 1_700_000_000_123)));
        let mut v1 = info_bytes(p, 1);
        v1[0] = 1;
        assert_eq!(parse_info(&v1), None);
        assert_eq!(parse_info(&info_bytes(p, 1)[..30]), None);
    }

    #[test]
    fn restaure_depuis_une_fausse_corbeille() {
        let bin = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        let orig = dest.path().join("A.txt");
        for (id, ms, body) in [("AAA", 1_000, "vieux"), ("BBB", 5_000, "récent"), ("CCC", 9_000, "autre")] {
            let o = if id == "CCC" { dest.path().join("B.txt") } else { orig.clone() };
            std::fs::write(bin.path().join(format!("$I{id}.txt")), info_bytes(&o, ms)).unwrap();
            std::fs::write(bin.path().join(format!("$R{id}.txt")), body).unwrap();
        }
        assert!(restore_from(bin.path(), &orig, 6_000).is_err(), "aucune suppression après 6 s");
        // Casse différente : Windows l'ignore.
        let upper = PathBuf::from(orig.to_string_lossy().to_uppercase());
        restore_from(bin.path(), &upper, 0).unwrap();
        assert_eq!(std::fs::read_to_string(&upper).unwrap(), "récent");
        assert!(!bin.path().join("$IBBB.txt").exists() && !bin.path().join("$RBBB.txt").exists());
        // La cible existe désormais : le reste n'est pas restauré par-dessus.
        assert!(restore_from(bin.path(), &orig, 0).is_err());
        assert!(bin.path().join("$RAAA.txt").exists());
        assert_eq!(std::fs::read_to_string(&orig).unwrap(), "récent");
    }
}
