//! Analyse pure des sorties de git (le lancement des processus est dans `src-tauri`).

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct GitStatus {
    pub branch: Option<String>,
    pub dirty: u32,
}

/// `git status --porcelain=v2 --branch` : branche (`(detached)` si HEAD détaché) et nombre de fichiers modifiés
/// (suivis ou non). Dépôt sans commit : la branche est celle de `branch.head`.
pub fn parse_status(out: &str) -> GitStatus {
    let mut s = GitStatus::default();
    for line in out.lines() {
        if let Some(b) = line.strip_prefix("# branch.head ") {
            s.branch = Some(b.trim().to_string());
        } else if !line.starts_with('#') && !line.trim().is_empty() {
            s.dirty += 1;
        }
    }
    s
}

/// Commits par jour sur 14 jours à partir des dates `%ct` (secondes) : l'indice 13 est « aujourd'hui »,
/// 0 le plus ancien. Les dates futures comptent pour aujourd'hui, les plus vieilles que 14 jours sont ignorées.
pub fn bucket_activity(commits: &[i64], now_secs: i64) -> [u8; 14] {
    let mut b = [0u8; 14];
    for &t in commits {
        let days = (now_secs - t).max(0) / 86_400;
        if days < 14 {
            let i = 13 - days as usize;
            b[i] = b[i].saturating_add(1);
        }
    }
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depot_propre() {
        let s = parse_status("# branch.oid abc\n# branch.head main\n# branch.upstream origin/main\n# branch.ab +0 -0\n");
        assert_eq!(s, GitStatus { branch: Some("main".into()), dirty: 0 });
    }

    #[test]
    fn fichiers_modifies_et_non_suivis() {
        let s = parse_status("# branch.oid abc\n# branch.head feat/x\n1 .M N... 100644 100644 100644 a b src/a.rs\n2 R. N... 100644 100644 100644 a b R100 n\to\n? new.txt\nu UU N... 1 1 1 1 a b c m.rs\n");
        assert_eq!(s, GitStatus { branch: Some("feat/x".into()), dirty: 4 });
    }

    #[test]
    fn head_detache_et_aucun_commit() {
        assert_eq!(parse_status("# branch.oid abc\n# branch.head (detached)\n").branch.as_deref(), Some("(detached)"));
        let s = parse_status("# branch.oid (initial)\n# branch.head master\n? a\n");
        assert_eq!((s.branch.as_deref(), s.dirty), (Some("master"), 1));
        assert_eq!(parse_status(""), GitStatus::default());
    }

    #[test]
    fn jours_et_bornes() {
        let now = 100 * 86_400;
        let b = bucket_activity(&[now, now - 10, now - 86_400, now - 13 * 86_400, now - 14 * 86_400, now + 500], now);
        assert_eq!(b[13], 3);
        assert_eq!(b[12], 1);
        assert_eq!(b[0], 1);
        assert_eq!(b.iter().map(|&x| x as u32).sum::<u32>(), 5);
    }
}
