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
    if s.len() > MAX_URL || s.chars().any(char::is_control) {
        return None;
    }
    let u = url::Url::parse(s).ok()?;
    if !matches!(u.scheme(), "http" | "https" | "mailto") || !u.username().is_empty() || u.password().is_some() {
        return None;
    }
    // mailto : ni pièce jointe ni en-têtes arbitraires.
    if u.scheme() == "mailto" && u.query_pairs().any(|(k, _)| !["subject", "body", "cc", "bcc"].contains(&k.to_lowercase().as_str())) {
        return None;
    }
    Some(u.into())
}

const MAX_URL: usize = 2048;

const VSCODE_CLAUDE: &str = "vscode://anthropic.claude-code/open?";

/// Seul lien `vscode://` admis : le préfixe exact de l'extension Claude Code, requête ASCII, sans fragment.
pub fn safe_vscode_claude_uri(s: &str) -> Option<String> {
    let q = s.strip_prefix(VSCODE_CLAUDE)?;
    q.chars().all(|c| c.is_ascii_graphic() && c != '#').then(|| s.to_string())
}

/// Extensions qui exécutent du code : réservées à `files.run` (confirmation).
pub(crate) const EXECUTABLE: &[&str] = &[
    "exe", "bat", "cmd", "com", "ps1", "psm1", "vbs", "vbe", "js", "jse", "wsf", "wsh", "hta", "msi", "msp", "scr", "cpl", "jar", "reg", "lnk", "url", "pif", "application",
    "appref-ms", "msc", "gadget", "inf", "scf", "chm", "py", "pyw", "ps1xml", "wsc", "xll", "sh", "jnlp", "diagcab", "search-ms", "library-ms", "settingcontent-ms", "docm",
    "xlsm", "pptm", "dotm", "xltm", "xlam", "ppam",
];

/// Le fichier (nom canonique ; tout nom contenant `:` est un flux ADS, refusé) a-t-il une extension exécutable ?
pub fn is_executable(p: &Path) -> bool {
    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if name.contains(':') {
        return true; // flux ADS : jamais ouvert par files.open
    }
    name.rsplit_once('.').is_some_and(|(_, ext)| EXECUTABLE.contains(&ext.trim().to_lowercase().as_str()))
}

fn plain(p: &Path) -> String {
    let s = p.to_string_lossy();
    s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
}

/// Chemin absolu existant, une fois canonique (sans `..` ni lien) dans l'une des racines (insensible à la casse).
/// Renvoie ce chemin canonique (préfixe `\\?\` ôté) : c'est lui qu'il faut ouvrir, jamais l'entrée.
pub fn safe_path(p: &Path, allowed_roots: &[PathBuf]) -> Option<PathBuf> {
    if !p.is_absolute() {
        return None;
    }
    let c = PathBuf::from(plain(&std::fs::canonicalize(p).ok()?));
    let lc = PathBuf::from(c.to_string_lossy().to_lowercase());
    allowed_roots
        .iter()
        .filter_map(|r| std::fs::canonicalize(r).ok())
        .any(|r| lc.starts_with(plain(&r).to_lowercase()))
        .then_some(c)
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
        assert_eq!(safe_path(&root.join("sub").join("a.txt"), &roots).unwrap().file_name().unwrap(), "a.txt");
        assert!(safe_path(Path::new("a.txt"), &roots).is_none(), "relatif");
        assert!(safe_path(&root.join("SUB").join("A.TXT"), &roots).is_some(), "insensible à la casse");
        assert!(safe_path(&other.join("b.txt"), &roots).is_none(), "hors racine");
        assert!(safe_path(&root.join("sub").join("..").join("..").join("autre").join("b.txt"), &roots).is_none(), "évasion par ..");
        assert!(safe_path(&root.join("absent.txt"), &roots).is_none(), "inexistant");
        assert!(safe_path(&root.join("sub").join("a.txt"), &[]).is_none());
    }

    #[test]
    fn extensions_executables() {
        for n in ["a.exe", "A.EXE", "a.Lnk", "x.appref-ms", "a.chm", "a.exe:stream", "a.txt:s.exe", "a.txt:stream", "a.ps1", "a.py", "a.SH", "a.docm", "a.xlsm", "a.pptm", "a.dotm", "a.xltm", "a.xlam", "a.ppam", "a.pyw", "a.ps1xml", "a.wsc", "a.xll", "a.jnlp", "a.diagcab", "a.search-ms", "a.library-ms", "a.settingcontent-ms"] {
            assert!(is_executable(Path::new(n)), "{n}");
        }
        for n in ["a.txt", "exe", "a.exe.txt", "a.docx"] {
            assert!(!is_executable(Path::new(n)), "{n}");
        }
    }

    #[test]
    fn mailto_et_longueur() {
        assert!(safe_url("mailto:a@b.fr?subject=Salut&body=x&cc=c@d.fr").is_some());
        assert!(safe_url("mailto:a@b.fr?attach=c:/secret.txt").is_none());
        assert!(safe_url("mailto:a@b.fr?Subject=x&headers=y").is_none());
        assert!(safe_url(&format!("https://a.com/{}", "x".repeat(2048))).is_none());
    }
}
