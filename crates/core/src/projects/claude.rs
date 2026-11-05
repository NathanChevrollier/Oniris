//! Sessions Claude Code d'un projet : `~/.claude/projects/<chemin encodé>/<uuid>.jsonl`, lues par la fin.
use serde::Serialize;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Au plus 256 Ko lus (d'un seul tenant, depuis la fin), jamais le fichier entier.
const TAIL_BYTES: u64 = 256 * 1024;
const TITLE_CHARS: usize = 80;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SessionInfo {
    /// Nom du `.jsonl` s'il a la forme d'un UUID (sinon `None` : jamais passé tel quel à une commande).
    pub id: Option<String>,
    pub updated_ms: i64,
    pub title: Option<String>,
}

/// Comme Claude Code : tout caractère non alphanumérique ASCII devient `-`.
pub fn encode(project: &str) -> String {
    project.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

/// Dossier attendu sous `base` (`~/.claude/projects`), casse du lecteur telle que donnée.
pub fn sessions_dir(base: &Path, project: &Path) -> PathBuf {
    base.join(encode(&project.to_string_lossy()))
}

/// Dossiers existants sous `base` qui correspondent sans tenir compte de la casse (`c--` et `C--`).
fn matching_dirs(base: &Path, project: &Path) -> Vec<PathBuf> {
    let want = encode(&project.to_string_lossy());
    let Ok(rd) = std::fs::read_dir(base) else { return vec![] };
    rd.flatten().filter(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(&want)).map(|e| e.path()).collect()
}

pub(crate) fn is_uuid(s: &str) -> bool {
    s.len() == 36 && s.bytes().enumerate().all(|(i, b)| if matches!(i, 8 | 13 | 18 | 23) { b == b'-' } else { b.is_ascii_hexdigit() })
}

fn clip(s: &str) -> Option<String> {
    let flat = s.split_whitespace().collect::<Vec<_>>().join(" ");
    (!flat.is_empty()).then(|| flat.chars().take(TITLE_CHARS).collect())
}

fn user_text(v: &serde_json::Value) -> Option<String> {
    if v["isMeta"].as_bool() == Some(true) {
        return None;
    }
    let ok = |t: &str| !t.trim_start().starts_with("<system-reminder>");
    match &v["message"]["content"] {
        serde_json::Value::String(s) => ok(s).then(|| s.clone()),
        serde_json::Value::Array(parts) => parts.iter().filter(|p| p["type"] == "text").filter_map(|p| p["text"].as_str()).find(|t| ok(t)).map(str::to_string),
        _ => None,
    }
}

/// Titre d'après la fin d'un journal : dernier résumé, sinon dernier titre automatique (`ai-title`), sinon premier
/// message de l'utilisateur de la fenêtre lue. Les lignes qui ne s'analysent pas (début coupé) sont ignorées.
pub fn title_from_lines<'a>(lines: impl Iterator<Item = &'a str>) -> Option<String> {
    let (mut summary, mut ai, mut first_user) = (None, None, None);
    for line in lines {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
        match v["type"].as_str() {
            Some("summary") => summary = v["summary"].as_str().and_then(clip).or(summary),
            Some("ai-title") => ai = v["aiTitle"].as_str().and_then(clip).or(ai),
            Some("user") if first_user.is_none() => first_user = user_text(&v).as_deref().and_then(clip),
            _ => {}
        }
    }
    summary.or(ai).or(first_user)
}

/// Dernier `TAIL_BYTES` du lecteur ; si on a sauté le début, la première ligne (probablement coupée) est retirée.
pub fn read_tail<R: Read + Seek>(r: &mut R) -> std::io::Result<String> {
    let len = r.seek(SeekFrom::End(0))?;
    let start = len.saturating_sub(TAIL_BYTES);
    r.seek(SeekFrom::Start(start))?;
    let mut buf = Vec::with_capacity((len - start) as usize);
    r.read_to_end(&mut buf)?;
    let text = String::from_utf8_lossy(&buf);
    Ok(if start > 0 { text.split_once('\n').map_or(String::new(), |(_, rest)| rest.to_string()) } else { text.into_owned() })
}

/// Session la plus récente du projet (tous les dossiers qui correspondent confondus).
pub fn latest(base: &Path, project: &Path) -> Option<SessionInfo> {
    let (path, ms) = matching_dirs(base, project)
        .into_iter()
        .filter_map(|d| std::fs::read_dir(d).ok())
        .flat_map(|rd| rd.flatten())
        .filter(|e| e.path().extension().is_some_and(|x| x.eq_ignore_ascii_case("jsonl")))
        .filter_map(|e| Some((e.path(), super::mtime_ms(&e.path())?)))
        .max_by_key(|(_, ms)| *ms)?;
    let title = std::fs::File::open(&path).and_then(|mut f| read_tail(&mut f)).ok().and_then(|t| title_from_lines(t.lines()));
    let id = path.file_stem()?.to_str().filter(|s| is_uuid(s)).map(str::to_string);
    Some(SessionInfo { id, updated_ms: ms, title })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    const UUID: &str = "fd1036ff-2ad0-43f6-abdc-f1b13d44b544";

    #[test]
    fn encodage_comme_claude_code() {
        assert_eq!(encode(r"c:\Users\natha\Desktop\Utilitaires\Projets\MacroWorld"), "c--Users-natha-Desktop-Utilitaires-Projets-MacroWorld");
        assert_eq!(encode(r"C:\a b_c.d/e"), "C--a-b-c-d-e");
    }

    #[test]
    fn dossier_attendu_et_casse_du_lecteur() {
        let base = tempfile::tempdir().unwrap();
        std::fs::create_dir(base.path().join("c--Users-x-Projets-Oniris")).unwrap();
        let p = Path::new(r"C:\Users\x\Projets\Oniris");
        assert_eq!(sessions_dir(base.path(), p), base.path().join("C--Users-x-Projets-Oniris"));
        assert_eq!(matching_dirs(base.path(), p), vec![base.path().join("c--Users-x-Projets-Oniris")]);
        assert_eq!(matching_dirs(base.path(), Path::new(r"c:\Users\x\Projets\Oniris")).len(), 1);
        assert!(matching_dirs(base.path(), Path::new(r"c:\autre")).is_empty());
    }

    fn write(dir: &Path, name: &str, body: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(name), body).unwrap();
    }

    #[test]
    fn titre_resume_sinon_premier_message() {
        let user = |c: &str| format!("{{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":{c}}}}}");
        let sys = user("[{\"type\":\"text\",\"text\":\"<system-reminder>x</system-reminder>\"}]");
        let tool = user("[{\"type\":\"tool_result\",\"content\":\"zzz\"}]");
        let real = user("[{\"type\":\"text\",\"text\":\"Corrige   le\\nbug\"}]");
        let lines = [sys.as_str(), tool.as_str(), real.as_str(), "{\"type\":\"assistant\"}", "pas du json"];
        assert_eq!(title_from_lines(lines.into_iter()).as_deref(), Some("Corrige le bug"));
        let with = [real.as_str(), "{\"type\":\"summary\",\"summary\":\"Premier\"}", "{\"type\":\"summary\",\"summary\":\"Dernier\"}"];
        assert_eq!(title_from_lines(with.into_iter()).as_deref(), Some("Dernier"));
        assert_eq!(title_from_lines(std::iter::empty()), None);
        let long = user(&format!("\"{}\"", "é".repeat(200)));
        assert_eq!(title_from_lines([long.as_str()].into_iter()).unwrap().chars().count(), 80);
    }

    #[test]
    fn session_la_plus_recente_sur_deux_dossiers() {
        let base = tempfile::tempdir().unwrap();
        let p = Path::new(r"c:\Users\x\Oniris");
        write(&base.path().join("c--Users-x-Oniris"), "vieux.jsonl", "{\"type\":\"summary\",\"summary\":\"Vieux\"}\n");
        std::thread::sleep(std::time::Duration::from_millis(30));
        write(&base.path().join("C--Users-x-Oniris"), &format!("{UUID}.jsonl"), "{\"type\":\"summary\",\"summary\":\"Récent\"}\n");
        let s = latest(base.path(), p).unwrap();
        assert_eq!((s.id.as_deref(), s.title.as_deref()), (Some(UUID), Some("Récent")));
        assert!(s.updated_ms > 0);
        write(&base.path().join("c--Users-x-Autre"), "pas-un-uuid.jsonl", "");
        let s = latest(base.path(), Path::new(r"c:\Users\x\Autre")).unwrap();
        assert_eq!((s.id, s.title), (None, None));
        assert!(latest(base.path(), Path::new(r"c:\rien")).is_none());
    }

    /// Compte les octets réellement lus.
    struct Counting(Cursor<Vec<u8>>, u64);
    impl Read for Counting {
        fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
            let n = self.0.read(b)?;
            self.1 += n as u64;
            Ok(n)
        }
    }
    impl Seek for Counting {
        fn seek(&mut self, p: SeekFrom) -> std::io::Result<u64> {
            self.0.seek(p)
        }
    }

    #[test]
    fn fichier_de_10_mo_seule_la_fin_est_lue() {
        let mut data = vec![b'x'; 10 * 1024 * 1024];
        data.extend_from_slice(b"\n{\"type\":\"summary\",\"summary\":\"Fin\"}\n");
        let mut r = Counting(Cursor::new(data), 0);
        let text = read_tail(&mut r).unwrap();
        assert!(r.1 <= TAIL_BYTES, "lu : {}", r.1);
        assert_eq!(title_from_lines(text.lines()).as_deref(), Some("Fin"));
    }

    #[test]
    fn petit_fichier_lu_en_entier() {
        let text = read_tail(&mut Cursor::new(b"{\"type\":\"summary\",\"summary\":\"A\"}\n".to_vec())).unwrap();
        assert_eq!(title_from_lines(text.lines()).as_deref(), Some("A"));
    }
}
