//! « Reprendre » un projet : VS Code sur le dossier, puis la dernière session Claude Code dans l'extension (lien
//! `vscode://anthropic.claude-code/open`) ; à défaut Windows Terminal + `claude --continue`.
//! Planification et encodage purs ; l'exécution passe par `Opener`. Journal : types d'étapes seulement.
use super::{claude::is_uuid, Project};
use crate::launch::{Opener, MAX_URL};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Normal,
    Orchestrator,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchStep {
    Code { exe: PathBuf, args: Vec<String> },
    Wait { ms: u64 },
    Uri { uri: String },
    Terminal { program: String, args: Vec<String> },
}

impl LaunchStep {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Code { .. } => "code",
            Self::Wait { .. } => "wait",
            Self::Uri { .. } => "uri",
            Self::Terminal { .. } => "terminal",
        }
    }
}

pub struct Prompts {
    pub normal: String,
    pub orchestrator: String,
}

impl Prompts {
    fn get(&self, mode: Mode) -> &str {
        match mode {
            Mode::Normal => &self.normal,
            Mode::Orchestrator => &self.orchestrator,
        }
    }
}

/// Ce que la machine offre : extension Claude capable d'`/open`, `Code.exe`, Windows Terminal.
#[derive(Debug, Clone, Default)]
pub struct Env {
    pub vscode_ok: bool,
    pub code: Option<PathBuf>,
    pub wt: Option<String>,
}

pub const MANUAL: &str = "Lance Claude manuellement : claude --continue";
/// Laisse à VS Code le temps d'ouvrir la fenêtre du projet avant le lien.
const WAIT_MS: u64 = 1500;
const OPEN_MARKER: &[u8] = br#"case"/open""#;
const MAX_EXTENSION_JS: u64 = 50 * 1024 * 1024;

pub const NO_PROMPT: &str = "La consigne de reprise n'est transmise qu'avec l'extension VS Code ; Claude reprend la dernière conversation.";
/// UUID factice : mesure de la longueur d'un lien par les réglages.
pub(crate) const DUMMY_UUID: &str = "00000000-0000-0000-0000-000000000000";

/// Valeur encodée deux fois : VS Code décode la requête (`URI.parse`), puis l'extension la relit avec
/// `URLSearchParams`. Encodée une seule fois, un `&` ou un `%41` de la consigne serait réinterprété.
fn encode2(v: &str) -> String {
    use url::form_urlencoded::byte_serialize;
    let once: String = byte_serialize(v.as_bytes()).collect();
    byte_serialize(once.as_bytes()).collect()
}

/// `vscode://anthropic.claude-code/open?prompt=…&session=…` ; la session n'y figure que si c'est un UUID
/// (l'extension ignore tout le lien sinon). Valeurs encodées par `encode2`, jamais concaténées brutes.
pub fn vscode_uri(prompt: &str, session: Option<&str>) -> String {
    let mut uri = format!("vscode://anthropic.claude-code/open?prompt={}", encode2(prompt));
    if let Some(s) = session.filter(|s| is_uuid(s)) {
        uri.push_str(&format!("&session={}", encode2(s)));
    }
    uri
}

/// wt lit `;` comme séparateur de commandes, même au milieu d'un argument : `\;` le rend littéral.
fn wt_escape(s: &str) -> String {
    s.replace(';', r"\;")
}

/// Étapes de reprise, et un message à afficher le cas échéant. Chaque chemin reste un argument séparé.
/// Repli terminal : `claude` est souvent un `claude.cmd` (npm), donc lu par cmd ; seuls des arguments constants
/// lui parviennent (la consigne, issue des réglages, n'y figure pas).
pub fn plan(project: &Project, mode: Mode, prompts: &Prompts, env: &Env) -> (Vec<LaunchStep>, Option<&'static str>) {
    let mut steps: Vec<LaunchStep> = env.code.iter().map(|exe| LaunchStep::Code { exe: exe.clone(), args: vec![project.path.clone()] }).collect();
    let session = project.last_session.as_ref().and_then(|s| s.id.as_deref());
    let uri = vscode_uri(prompts.get(mode), session);
    // Sans Code.exe, le lien seul ouvrirait VS Code hors du projet ; trop long, il serait refusé : repli terminal.
    if env.vscode_ok && !steps.is_empty() && uri.len() <= MAX_URL {
        steps.push(LaunchStep::Wait { ms: WAIT_MS });
        steps.push(LaunchStep::Uri { uri });
        return (steps, None);
    }
    let Some(wt) = &env.wt else { return (steps, Some(MANUAL)) };
    let args = ["-d", &wt_escape(&project.path), "cmd", "/k", "claude", "--continue"].map(String::from).to_vec();
    steps.push(LaunchStep::Terminal { program: wt.clone(), args });
    (steps, Some(NO_PROMPT))
}

/// `2.1.318` dans `anthropic.claude-code-2.1.318-win32-x64` (suffixe de plateforme ignoré).
fn extension_version(name: &str) -> Option<Vec<u64>> {
    let v = name.strip_prefix("anthropic.claude-code-")?.split('-').next()?;
    v.split('.').map(|n| n.parse().ok()).collect()
}

/// L'extension Claude Code la plus récente de `extensions` sait-elle traiter `/open` ? (50 Mo de `extension.js` au plus.)
pub fn vscode_ok(extensions: &Path) -> bool {
    let Ok(rd) = std::fs::read_dir(extensions) else { return false };
    let latest = rd.flatten().filter_map(|e| Some((extension_version(&e.file_name().to_string_lossy())?, e.path()))).max_by(|a, b| a.0.cmp(&b.0));
    let Some((_, dir)) = latest else { return false };
    let mut buf = Vec::new();
    let read = std::fs::File::open(dir.join("extension.js")).and_then(|f| f.take(MAX_EXTENSION_JS).read_to_end(&mut buf));
    read.is_ok() && buf.windows(OPEN_MARKER.len()).any(|w| w == OPEN_MARKER)
}

/// `Code.exe` : installation utilisateur, puis machine, puis `where code` (`…\bin\code.cmd` → `…\Code.exe`).
/// Lancé directement plutôt que `code.cmd` : pas de `cmd /C`, donc aucune injection de métacaractères possible.
pub fn vscode_exe(env: &impl Fn(&str) -> Option<String>, exists: &impl Fn(&Path) -> bool, where_code: impl FnOnce() -> Option<String>) -> Option<PathBuf> {
    [("LOCALAPPDATA", r"Programs\Microsoft VS Code\Code.exe"), ("ProgramFiles", r"Microsoft VS Code\Code.exe")]
        .iter()
        .filter_map(|(var, rel)| env(var).map(|base| Path::new(&base).join(rel)))
        .find(|p| exists(p))
        .or_else(|| {
            where_code()?.lines().map(str::trim).filter(|l| l.to_lowercase().ends_with(".cmd")).find_map(|l| {
                let exe = Path::new(l).parent()?.parent()?.join("Code.exe");
                exists(&exe).then_some(exe)
            })
        })
}

/// Exécute les étapes dans l'ordre (à appeler hors de l'interface : `Wait` dort) ; s'arrête au premier échec.
pub fn execute(opener: &dyn Opener, steps: &[LaunchStep]) {
    for s in steps {
        tracing::info!("reprise : étape {}", s.kind());
        let r = match s {
            LaunchStep::Code { exe, args } => opener.spawn_detached(&exe.to_string_lossy(), args, None),
            LaunchStep::Terminal { program, args } => opener.spawn_detached(program, args, None),
            LaunchStep::Uri { uri } => {
                opener.open_url(uri);
                Ok(())
            }
            LaunchStep::Wait { ms } => {
                std::thread::sleep(Duration::from_millis(*ms));
                Ok(())
            }
        };
        if let Err(e) = r {
            tracing::warn!("reprise : étape {} impossible ({:?})", s.kind(), e.kind());
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projects::claude::SessionInfo;

    const UUID: &str = "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0";

    fn prompts() -> Prompts {
        Prompts { normal: "Reprends ; vas-y".into(), orchestrator: "Mode orchestrateur".into() }
    }

    fn proj(path: &str, session: Option<&str>) -> Project {
        Project {
            path: path.into(),
            name: "P".into(),
            last_activity_ms: 0,
            branch: None,
            dirty: 0,
            activity_14d: [0; 14],
            plan: None,
            last_session: Some(SessionInfo { id: session.map(str::to_string), updated_ms: 0, title: None }),
            pinned: false,
            hidden: false,
        }
    }

    /// Ce que reçoit l'extension : VS Code décode la requête (`URI.parse`), puis `URLSearchParams` la relit.
    fn decode(uri: &str) -> Vec<(String, String)> {
        let q = uri.split_once('?').unwrap().1;
        let once = percent_encoding::percent_decode_str(q).decode_utf8().unwrap();
        url::form_urlencoded::parse(once.as_bytes()).into_owned().collect()
    }

    fn pairs(prompt: &str, session: Option<&str>) -> Vec<(String, String)> {
        let mut v = vec![("prompt".to_string(), prompt.to_string())];
        v.extend(session.map(|s| ("session".to_string(), s.to_string())));
        v
    }

    #[test]
    fn uri_doublement_encodee_et_aller_retour() {
        let uri = vscode_uri("Dis \"oui\" & 100% #1 + é", Some(UUID));
        assert!(uri.starts_with("vscode://anthropic.claude-code/open?prompt="), "{uri}");
        assert!(uri.contains("%2526") && uri.contains("%2B") && !uri.contains('#'), "{uri}");
        for prompt in [
            "Dis \"oui\" & 100% #1 + é à\nligne 2 🚀",
            "a&b",
            "1+1",
            "%41",
            "50%",
            "x&session=evil",
            "x&session=0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f1",
        ] {
            let uri = vscode_uri(prompt, Some(UUID));
            assert_eq!(crate::launch::safe_vscode_claude_uri(&uri).as_deref(), Some(uri.as_str()), "{prompt}");
            assert_eq!(decode(&uri), pairs(prompt, Some(UUID)), "{prompt} : intact, et notre session l'emporte");
            assert_eq!(decode(&vscode_uri(prompt, None)), pairs(prompt, None), "{prompt} : aucune session injectée");
        }
    }

    #[test]
    fn session_invalide_ecartee() {
        for bad in ["", "abc", "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1fZ", "0f1e2d3c-4b5a-6978-8796-a5b4c3d2e1f0&x=1"] {
            assert_eq!(decode(&vscode_uri("x", Some(bad))), pairs("x", None), "{bad}");
        }
    }

    fn env(vscode_ok: bool, wt: bool) -> Env {
        Env { vscode_ok, code: Some(PathBuf::from(r"C:\VS\Code.exe")), wt: wt.then(|| "wt".to_string()) }
    }

    const WT_ARGS: [&str; 6] = ["-d", r"C:\p\Oniris", "cmd", "/k", "claude", "--continue"];

    #[test]
    fn plan_dans_les_trois_environnements() {
        let p = proj(r"C:\p\Oniris", Some(UUID));
        let code = LaunchStep::Code { exe: PathBuf::from(r"C:\VS\Code.exe"), args: vec![r"C:\p\Oniris".into()] };

        let (steps, info) = plan(&p, Mode::Orchestrator, &prompts(), &env(true, true));
        assert_eq!(steps, [code.clone(), LaunchStep::Wait { ms: 1500 }, LaunchStep::Uri { uri: vscode_uri("Mode orchestrateur", Some(UUID)) }]);
        assert_eq!(info, None);
        assert_eq!(plan(&p, Mode::Normal, &prompts(), &env(true, false)).0[2], LaunchStep::Uri { uri: vscode_uri("Reprends ; vas-y", Some(UUID)) });

        let (steps, info) = plan(&p, Mode::Normal, &prompts(), &env(false, true));
        let wt = LaunchStep::Terminal { program: "wt".into(), args: WT_ARGS.map(String::from).to_vec() };
        assert_eq!(steps, [code.clone(), wt]);
        assert_eq!(info, Some(NO_PROMPT));

        assert_eq!(plan(&p, Mode::Normal, &prompts(), &env(false, false)), (vec![code], Some(MANUAL)));
        // Sans Code.exe, le lien seul ouvrirait VS Code hors du projet : repli terminal.
        let no_code = Env { code: None, ..env(true, true) };
        assert_eq!(plan(&p, Mode::Normal, &prompts(), &no_code).0.iter().map(LaunchStep::kind).collect::<Vec<_>>(), ["terminal"]);
    }

    #[test]
    fn terminal_sans_rien_des_reglages() {
        // `claude.cmd` passe par cmd : ni la consigne ni rien qui en dérive n'atteint la ligne de commande.
        let hostile = Prompts { normal: "x\" & calc & %COMSPEC% ^| y".into(), orchestrator: "orchestrateur & calc".into() };
        for mode in [Mode::Normal, Mode::Orchestrator] {
            let (steps, _) = plan(&proj(r"C:\p\Oniris", Some(UUID)), mode, &hostile, &env(false, true));
            let LaunchStep::Terminal { args, .. } = &steps[1] else { panic!() };
            assert_eq!(*args, WT_ARGS.map(String::from));
        }
    }

    #[test]
    fn lien_trop_long_repli_terminal() {
        let p = proj(r"C:\p\Oniris", Some(UUID));
        let long = Prompts { normal: "é".repeat(300), orchestrator: "x".into() };
        assert!(vscode_uri(&long.normal, Some(UUID)).len() > MAX_URL);
        let (steps, info) = plan(&p, Mode::Normal, &long, &env(true, true));
        assert_eq!(steps.iter().map(LaunchStep::kind).collect::<Vec<_>>(), ["code", "terminal"]);
        assert_eq!(info, Some(NO_PROMPT));
        assert_eq!(plan(&p, Mode::Normal, &long, &env(true, false)).1, Some(MANUAL));
    }

    #[test]
    fn chemin_hostile_un_seul_argument() {
        let path = r#"C:\p\a&b "c" %PATH% ^x;y"#;
        let p = proj(path, None);
        let (steps, _) = plan(&p, Mode::Normal, &prompts(), &env(false, true));
        assert_eq!(steps[0], LaunchStep::Code { exe: PathBuf::from(r"C:\VS\Code.exe"), args: vec![path.to_string()] });
        let LaunchStep::Terminal { args, .. } = &steps[1] else { panic!() };
        assert_eq!(args.len(), 6);
        assert_eq!(args[1], r#"C:\p\a&b "c" %PATH% ^x\;y"#);
        // Lien : le chemin n'y figure pas ; sans session valide, pas de `session=`.
        let (steps, _) = plan(&p, Mode::Normal, &prompts(), &env(true, true));
        let LaunchStep::Uri { uri } = &steps[2] else { panic!() };
        assert!(!uri.contains("session") && !uri.contains("PATH"));
    }

    fn ext(root: &Path, name: &str, js: &str) {
        std::fs::create_dir_all(root.join(name)).unwrap();
        std::fs::write(root.join(name).join("extension.js"), js).unwrap();
    }

    #[test]
    fn detection_extension_version_la_plus_haute() {
        let t = tempfile::tempdir().unwrap();
        let r = t.path();
        assert!(!vscode_ok(&r.join("absent")));
        assert!(!vscode_ok(r), "aucune extension");
        ext(r, "anthropic.claude-code-2.1.32-win32-x64", r#"x;case"/open":{}"#);
        assert!(vscode_ok(r));
        // 2.1.318 > 2.1.32 (numérique, pas lexical) : sans le marqueur, la plus récente l'emporte.
        ext(r, "anthropic.claude-code-2.1.318-win32-x64", "case\"/close\":{}");
        assert!(!vscode_ok(r));
        ext(r, "anthropic.claude-code-2.2.0", r#"switch(p){case"/open":{let A}}"#);
        assert!(vscode_ok(r));
        ext(r, "autre.claude-code-9.9.9", "");
        ext(r, "anthropic.claude-code-x.y", "");
        assert!(vscode_ok(r), "noms étrangers ou mal formés ignorés");
    }

    #[test]
    fn executable_vscode_ordre_de_resolution() {
        let vars = |k: &str| match k {
            "LOCALAPPDATA" => Some(r"C:\L".to_string()),
            "ProgramFiles" => Some(r"C:\PF".to_string()),
            _ => None,
        };
        let local = PathBuf::from(r"C:\L\Programs\Microsoft VS Code\Code.exe");
        let pf = PathBuf::from(r"C:\PF\Microsoft VS Code\Code.exe");
        let other = PathBuf::from(r"D:\VS\Code.exe");
        let where_out = || Some("D:\\VS\\bin\\code\r\nD:\\VS\\bin\\code.cmd\r\n".to_string());
        let has = |list: Vec<PathBuf>| move |p: &Path| list.iter().any(|x| x == p);
        let never = || -> Option<String> { panic!("where inutile") };

        assert_eq!(vscode_exe(&vars, &has(vec![local.clone(), pf.clone(), other.clone()]), never), Some(local.clone()));
        assert_eq!(vscode_exe(&vars, &has(vec![pf.clone(), other.clone()]), never), Some(pf));
        assert_eq!(vscode_exe(&vars, &has(vec![other.clone()]), where_out), Some(other.clone()));
        assert_eq!(vscode_exe(&|_| None, &has(vec![other.clone()]), where_out), Some(other));
        assert_eq!(vscode_exe(&vars, &has(vec![]), where_out), None, "Code.exe absent à côté de bin");
        assert_eq!(vscode_exe(&vars, &has(vec![]), || None), None);
    }

    #[derive(Default)]
    struct Fake(parking_lot::Mutex<Vec<String>>);
    impl Opener for Fake {
        fn open_path(&self, _: &Path) {
            self.0.lock().push("path".into());
        }
        fn open_url(&self, u: &str) {
            self.0.lock().push(format!("url {u}"));
        }
        fn spawn_detached(&self, program: &str, args: &[String], _: Option<&Path>) -> std::io::Result<()> {
            self.0.lock().push(format!("spawn {program} {args:?}"));
            Ok(())
        }
    }

    #[test]
    fn execution_par_l_opener() {
        let f = Fake::default();
        let steps = [
            LaunchStep::Code { exe: PathBuf::from(r"C:\VS\Code.exe"), args: vec!["C:\\p".into()] },
            LaunchStep::Wait { ms: 1 },
            LaunchStep::Uri { uri: "vscode://anthropic.claude-code/open?prompt=x".into() },
            LaunchStep::Terminal { program: "wt".into(), args: vec!["-d".into()] },
        ];
        execute(&f, &steps);
        assert_eq!(
            *f.0.lock(),
            [r#"spawn C:\VS\Code.exe ["C:\\p"]"#, "url vscode://anthropic.claude-code/open?prompt=x", r#"spawn wt ["-d"]"#]
        );
        assert_eq!(steps.iter().map(LaunchStep::kind).collect::<Vec<_>>(), ["code", "wait", "uri", "terminal"]);
    }
}
