//! Lancement sûr : validation pure des URL et chemins, et contrat `Opener` (réalisé côté Tauri).
use std::path::{Path, PathBuf};

/// Ouvre fichiers, dossiers, liens et processus. Réalisé par `src-tauri/src/launch.rs` ; faux dans les tests.
pub trait Opener: Send + Sync {
    fn open_path(&self, p: &Path);
    fn open_url(&self, u: &str);
    fn spawn_detached(&self, program: &str, args: &[String], cwd: Option<&Path>) -> std::io::Result<()>;
}

/// URL http, https ou mailto, sans caractère de contrôle ni identifiants (hameçonnage) ; schéma en minuscules.
pub fn safe_url(s: &str) -> Option<String> {
    let s = s.trim();
    if s.chars().any(char::is_control) {
        return None;
    }
    let u = url::Url::parse(s).ok()?;
    if !matches!(u.scheme(), "http" | "https" | "mailto") || !u.username().is_empty() || u.password().is_some() {
        return None;
    }
    Some(u.into())
}

const VSCODE_CLAUDE: &str = "vscode://anthropic.claude-code/open?";

/// Seul lien `vscode://` admis : le préfixe exact de l'extension Claude Code, requête ASCII, sans fragment.
pub fn safe_vscode_claude_uri(s: &str) -> Option<String> {
    let q = s.strip_prefix(VSCODE_CLAUDE)?;
    q.chars().all(|c| c.is_ascii_graphic() && c != '#').then(|| s.to_string())
}

fn plain(p: &Path) -> String {
    let s = p.to_string_lossy();
    s.strip_prefix(r"\\?\").unwrap_or(&s).to_lowercase()
}

/// Chemin existant, une fois canonique (sans `..` ni lien) dans l'une des racines (insensible à la casse).
pub fn safe_path(p: &Path, allowed_roots: &[PathBuf]) -> bool {
    let Ok(c) = std::fs::canonicalize(p) else { return false };
    let c = PathBuf::from(plain(&c));
    allowed_roots.iter().filter_map(|r| std::fs::canonicalize(r).ok()).any(|r| c.starts_with(plain(&r)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_dangereuses_refusees() {
        for u in ["javascript:alert(1)", "file:///c:/x", "vbscript:x", "data:text/html,<b>", " JAVASCRIPT:alert(1)", "https://u:p@evil.com", "https://a.com/\u{7}"] {
            assert_eq!(safe_url(u), None, "{u}");
        }
        assert_eq!(safe_url(" https://github.com ").as_deref(), Some("https://github.com/"));
        assert_eq!(safe_url("HTTPS://github.com/x").as_deref(), Some("https://github.com/x"));
        assert!(safe_url("mailto:a@b.fr").is_some());
    }

    #[test]
    fn uri_vscode_claude_exacte() {
        let ok = "vscode://anthropic.claude-code/open?prompt=salut%20x&session=1";
        assert_eq!(safe_vscode_claude_uri(ok).as_deref(), Some(ok));
        for u in [
            "vscode://anthropic.claude-code/open",
            "vscode://autre.ext/open?a=1",
            "vscode://anthropic.claude-code/open?a=1#f",
            "vscode://anthropic.claude-code/open?a=é",
            "vscode://anthropic.claude-code/open?a b",
            "VSCODE://anthropic.claude-code/open?a=1",
            " vscode://anthropic.claude-code/open?a=1",
        ] {
            assert_eq!(safe_vscode_claude_uri(u), None, "{u}");
        }
    }

    #[test]
    fn chemins_dans_les_racines_seulement() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("Racine");
        let other = t.path().join("autre");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(root.join("sub").join("a.txt"), "x").unwrap();
        std::fs::write(other.join("b.txt"), "x").unwrap();
        let roots = vec![root.clone()];
        assert!(safe_path(&root.join("sub").join("a.txt"), &roots));
        assert!(safe_path(&root.join("SUB").join("A.TXT"), &roots), "insensible à la casse");
        assert!(!safe_path(&other.join("b.txt"), &roots), "hors racine");
        assert!(!safe_path(&root.join("sub").join("..").join("..").join("autre").join("b.txt"), &roots), "évasion par ..");
        assert!(!safe_path(&root.join("absent.txt"), &roots), "inexistant");
        assert!(!safe_path(&root.join("sub").join("a.txt"), &[]));
    }
}
