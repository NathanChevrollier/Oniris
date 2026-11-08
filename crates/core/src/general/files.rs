//! Outils fichiers d'IRIS (créer, copier, renommer, déplacer, supprimer vers la Corbeille) sous garde-fous fixes.
use super::recycle::Recycler;
use crate::tools::helpers::{bad, failed, parse, spec};
use crate::tools::{ConfirmationText, Permission, Risk, Tool, ToolError, ToolResult, ToolSpec};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const MAX_CONTENT: usize = 1024 * 1024;
pub const MAX_PATHS: usize = 100;
/// Marge sous l'heure de suppression notée dans le `$I` (horloges et arrondis).
const DELETE_MARGIN_MS: i64 = 2_000;
/// Protégés sur tous les lecteurs (en plus de `protected_dirs`).
const SPECIAL: [&str; 2] = ["$Recycle.Bin", "System Volume Information"];
/// Noms de périphériques DOS (avec ou sans extension) : `C:\x\NUL.txt` ouvre le périphérique, pas un fichier.
const DEVICES: [&str; 24] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5",
    "lpt6", "lpt7", "lpt8", "lpt9", "conin$", "conout$",
];

/// Dossiers de travail d'IRIS (`iris_work_dirs`), relus à chaque appel.
pub type WorkDirs = Arc<dyn Fn() -> Vec<String> + Send + Sync>;

fn env_dir(k: &str) -> Option<PathBuf> {
    std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// Données d'Oniris (`%APPDATA%\Oniris` : base avec cache des mails et journal).
fn oniris_data() -> Option<PathBuf> {
    env_dir("APPDATA").map(|a| a.join("Oniris"))
}

/// Dossiers interdits en écriture (Global Constraints, plus les dossiers de démarrage et de profils PowerShell :
/// un `files.create` libre y gagnerait une exécution au prochain démarrage), d'après l'environnement.
/// `$Recycle.Bin` et `System Volume Information` sont refusés par nom sur tout lecteur par `writable`.
// shortcut: Documents pris dans %USERPROFILE% et %OneDrive% ; un Documents déplacé ailleurs (dossier connu) échappe.
pub fn protected_dirs() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = ["SystemRoot", "windir", "ProgramFiles", "ProgramFiles(x86)", "ProgramW6432", "ProgramData"].into_iter().filter_map(env_dir).collect();
    v.extend(oniris_data());
    v.extend(env_dir("APPDATA").map(|a| a.join(r"Microsoft\Windows\Start Menu\Programs\Startup")));
    v.extend(env_dir("ProgramData").map(|a| a.join(r"Microsoft\Windows\Start Menu\Programs\StartUp")));
    for root in ["USERPROFILE", "OneDrive"].into_iter().filter_map(env_dir) {
        v.extend(["WindowsPowerShell", "PowerShell"].map(|d| root.join("Documents").join(d)));
    }
    v
}

/// Forme lexicale sûre, sans aucun accès disque : `X:\…` (après `/` → `\`), donc ni relatif, ni UNC, ni `\\?\`,
/// ni `\\.\` ; pas de `:` après le lecteur (flux alternatif), ni `* ? < > | "` ; aucun composant `.`, `..`, finissant
/// par `.` ou une espace (Win32 les rognerait : `Windows.` vaut `Windows`) ni nom de périphérique DOS.
pub fn lexical(p: &Path) -> Result<PathBuf, String> {
    let s = p.to_string_lossy().replace('/', "\\");
    let b = s.as_bytes();
    if b.len() < 3 || !b[0].is_ascii_alphabetic() || b[1] != b':' || b[2] != b'\\' {
        return Err("chemin absolu local attendu (X:\\…), jamais réseau ni \\\\?\\".into());
    }
    if s[2..].contains(':') {
        return Err("flux alternatif refusé".into());
    }
    if s.contains(['*', '?', '<', '>', '|', '"']) {
        return Err("caractère interdit dans un chemin".into());
    }
    for c in s[3..].split('\\') {
        if c.ends_with('.') || c.ends_with(' ') {
            return Err("composant « . », « .. » ou finissant par un point ou une espace refusé".into());
        }
        let stem = c.split('.').next().unwrap_or_default().trim_end().to_lowercase();
        if DEVICES.contains(&stem.as_str()) {
            return Err("nom de périphérique refusé".into());
        }
    }
    Ok(PathBuf::from(s))
}

/// Canonique (jonctions, liens et noms 8.3 résolus) de l'ancêtre existant le plus profond, suivi du reste.
/// Seule l'absence fait remonter ; toute autre erreur (accès refusé…) refuse. À n'appeler qu'après `lexical`.
// shortcut: une jonction locale déjà existante vers un partage UNC est ouverte ici (identifiants NTLM envoyés) avant
// le refus « réseau » ; aucun outil d'IRIS ne crée de lien. À durcir en lisant les points d'analyse à la main.
fn resolve(p: &Path) -> Result<PathBuf, String> {
    let mut tail = vec![];
    let mut cur = p;
    loop {
        match std::fs::canonicalize(cur) {
            Ok(c) => {
                let mut c = PathBuf::from(crate::launch::unverbatim(&c.to_string_lossy()));
                if c.to_string_lossy().starts_with("\\\\") {
                    return Err("chemin réseau refusé".into());
                }
                c.extend(tail.iter().rev());
                return Ok(c);
            }
            Err(e) if e.kind() == ErrorKind::NotFound => {
                tail.push(cur.file_name().ok_or("lecteur introuvable")?);
                cur = cur.parent().ok_or("lecteur introuvable")?;
            }
            Err(_) => return Err("chemin inaccessible".into()),
        }
    }
}

fn lower(p: &Path) -> PathBuf {
    PathBuf::from(p.to_string_lossy().to_lowercase())
}

/// `c` est-il `d` ou en dessous, pour l'un des dossiers (composant par composant, sans casse) ?
fn under(c: &Path, dirs: &[PathBuf]) -> bool {
    let lc = lower(c);
    dirs.iter().any(|d| lc.starts_with(lower(d)))
}

/// `c` est-il strictement sous l'un des dossiers (le dossier lui-même exclu) ?
fn strictly_under(c: &Path, dirs: &[PathBuf]) -> bool {
    let lc = lower(c);
    dirs.iter().map(|d| lower(d)).any(|d| lc != d && lc.starts_with(&d))
}

/// Dossiers tels que donnés et canoniques (un dossier non local reste tel quel : jamais touché).
fn canonical_dirs(dirs: &[PathBuf]) -> Vec<PathBuf> {
    dirs.iter().flat_map(|d| [Some(d.clone()), lexical(d).and_then(|l| resolve(&l)).ok()]).flatten().collect()
}

fn forbidden(c: &Path, protected: &[PathBuf]) -> bool {
    let lc = lower(c);
    c.parent().and_then(Path::parent).is_none() // racine d'un lecteur, ou directement dedans
        || c.components().any(|k| SPECIAL.iter().any(|s| k.as_os_str().eq_ignore_ascii_case(s)))
        || under(c, protected)
        // Un ancêtre d'un protégé non plus : supprimer `AppData\Roaming` emporterait `%APPDATA%\Oniris`.
        || protected.iter().any(|d| lower(d).starts_with(&lc))
}

/// Chemin où IRIS peut écrire : forme `lexical`, parent canonisé, hors des dossiers protégés canoniques (ni dedans,
/// ni l'un de leurs ancêtres) et hors de la racine d'un lecteur. S'il existe déjà, sa propre cible (jonction, lien)
/// doit l'être aussi. Renvoie le parent canonique suivi du nom : c'est lui qu'il faut toucher (un lien est alors
/// visé lui-même, jamais sa cible).
pub fn writable(p: &Path, protected: &[PathBuf]) -> Result<PathBuf, String> {
    let p = lexical(p)?;
    let (Some(parent), Some(name)) = (p.parent(), p.file_name()) else {
        return Err("racine d'un lecteur refusée".into());
    };
    let c = resolve(parent)?.join(name);
    let prot = canonical_dirs(protected);
    let target = match std::fs::canonicalize(&p) {
        Ok(_) => Some(resolve(&p)?),
        Err(e) if e.kind() == ErrorKind::NotFound => None,
        Err(_) => return Err("chemin inaccessible".into()),
    };
    if forbidden(&c, &prot) || target.is_some_and(|t| forbidden(&t, &prot)) {
        return Err("dossier protégé : écriture refusée".into());
    }
    Ok(c)
}

/// Un dossier de travail est valable si IRIS peut écrire directement dedans.
pub fn work_dir_ok(d: &Path, protected: &[PathBuf]) -> Result<(), String> {
    writable(&d.join("x"), protected).map(|_| ())
}

pub struct Files {
    pub recycler: Arc<dyn Recycler>,
    pub work_dirs: WorkDirs,
    pub protected: Vec<PathBuf>,
}

/// Exécute `op` hors des fils de tokio (Corbeille du Shell, renommages et copies peuvent durer).
async fn blocking<T: Send + 'static>(f: &Arc<Files>, v: &Value, op: fn(&Files, &Value) -> Result<T, ToolError>) -> Result<T, ToolError> {
    let (f, v) = (f.clone(), v.clone());
    tokio::task::spawn_blocking(move || op(&f, &v)).await.map_err(failed)?
}

impl Files {
    fn w(&self, p: &str) -> Result<PathBuf, ToolError> {
        writable(Path::new(p), &self.protected).map_err(bad)
    }
    /// Vrai si le chemin (accepté par `writable`) est strictement dans un dossier de travail (pas sa racine).
    fn inside(&self, p: Option<&str>) -> bool {
        let dirs: Vec<PathBuf> = (self.work_dirs)().iter().filter_map(|d| lexical(Path::new(d)).ok()).collect();
        p.and_then(|p| self.w(p).ok()).is_some_and(|c| strictly_under(&c, &canonical_dirs(&dirs)))
    }
    /// Chemin canonique à montrer (l'entrée brute si elle est refusée : la carte ne s'affiche alors pas).
    fn shown(&self, p: &str) -> String {
        self.w(p).map(|c| c.display().to_string()).unwrap_or_else(|_| p.to_string())
    }
    /// Envoie à la Corbeille ce qui occupe `to` ; rend l'heure à donner à `restore`.
    fn clear(&self, to: &Path, overwrite: bool) -> Result<Option<i64>, ToolError> {
        if std::fs::symlink_metadata(to).is_err() {
            return Ok(None);
        }
        if !overwrite {
            return Err(failed("la cible existe déjà"));
        }
        let t = crate::time_util::now_ms() - DELETE_MARGIN_MS;
        self.recycler.recycle(to).map_err(failed)?;
        Ok(Some(t))
    }
    fn restore_opt(&self, p: &Path, after: Option<i64>) -> Result<(), ToolError> {
        after.map_or(Ok(()), |t| self.recycler.restore(p, t).map_err(failed))
    }
    /// Déplace `from` vers `to` (cible existante à la Corbeille si `overwrite`) ; undo = mouvement inverse.
    // shortcut: contrôle d'existence puis `rename` (qui remplace un fichier) : petite fenêtre de course ; à passer à
    // `MoveFileExW` sans `MOVEFILE_REPLACE_EXISTING` si elle gêne.
    fn mv(&self, from: &Path, to: &Path, overwrite: bool) -> Result<ToolResult, ToolError> {
        if std::fs::symlink_metadata(from).is_err() {
            return Err(failed("source introuvable"));
        }
        let after = self.clear(to, overwrite)?;
        if let Err(e) = std::fs::rename(from, to) {
            let _ = self.restore_opt(to, after); // on remet la cible écartée
            return Err(failed(e));
        }
        Ok(ToolResult { output: json!({"path": to}), undo: Some(json!({"from": to, "to": from, "restore_after_ms": after})) })
    }
    fn undo_move(&self, u: &Value) -> Result<(), ToolError> {
        let u: MoveUndo = parse(u)?;
        let (from, to) = (self.w(&u.from)?, self.w(&u.to)?);
        if std::fs::symlink_metadata(&to).is_ok() {
            return Err(failed("l'emplacement d'origine est occupé"));
        }
        std::fs::rename(&from, &to).map_err(failed)?;
        self.restore_opt(&from, u.restore_after_ms)
    }
}

#[derive(Deserialize)]
struct CreateArgs {
    path: String,
    content: Option<String>,
}
#[derive(Deserialize)]
struct MoveArgs {
    from: String,
    to: String,
    #[serde(default)]
    overwrite: bool,
}
#[derive(Deserialize)]
struct RenameArgs {
    path: String,
    new_name: String,
}
#[derive(Deserialize)]
struct DeleteArgs {
    paths: Vec<String>,
}
/// Undo de `files.create` et `files.copy` : la copie part à la Corbeille, l'ancienne cible revient.
#[derive(Deserialize)]
struct RecycleUndo {
    path: String,
    restore_after_ms: Option<i64>,
}
#[derive(Deserialize)]
struct MoveUndo {
    from: String,
    to: String,
    restore_after_ms: Option<i64>,
}
#[derive(Deserialize)]
struct DeleteUndo {
    paths: Vec<String>,
    after_ms: i64,
}

fn undo_recycle(f: &Files, u: &Value) -> Result<(), ToolError> {
    let u: RecycleUndo = parse(u)?;
    let p = f.w(&u.path)?;
    f.recycler.recycle(&p).map_err(failed)?;
    f.restore_opt(&p, u.restore_after_ms)
}

pub struct FilesCreate(pub Arc<Files>);
pub struct FilesCopy(pub Arc<Files>);
pub struct FilesRename(pub Arc<Files>);
pub struct FilesMove(pub Arc<Files>);
pub struct FilesDelete(pub Arc<Files>);

fn create_check(f: &Files, a: &Value) -> Result<(), ToolError> {
    let a: CreateArgs = parse(a)?;
    if a.content.as_ref().is_some_and(|c| c.len() > MAX_CONTENT) {
        return Err(bad("contenu : 1 Mo au plus"));
    }
    f.w(&a.path).map(|_| ())
}

fn create_run(f: &Files, a: &Value) -> Result<ToolResult, ToolError> {
    create_check(f, a)?;
    let a: CreateArgs = parse(a)?;
    let p = f.w(&a.path)?;
    // `create_new` : jamais d'écrasement, même en course.
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&p).map_err(failed)?;
    std::io::Write::write_all(&mut file, a.content.unwrap_or_default().as_bytes()).map_err(failed)?;
    Ok(ToolResult { output: json!({"path": p}), undo: Some(json!({"path": p})) })
}

#[async_trait]
impl Tool for FilesCreate {
    fn spec(&self) -> ToolSpec {
        spec(
            "files.create",
            "Crée un fichier texte (refusé s'il existe déjà).",
            json!({"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path"]}),
            Permission::Free,
        )
    }
    fn check(&self, a: &Value) -> Result<(), ToolError> {
        create_check(&self.0, a)
    }
    fn describe(&self, a: &Value) -> ConfirmationText {
        let mut details = self.0.shown(a["path"].as_str().unwrap_or_default());
        // La carte montre ce qui sera écrit : aperçu des 2000 premiers caractères.
        if let Some(c) = a["content"].as_str().filter(|c| !c.is_empty()) {
            let n = c.chars().count();
            details += &format!("

{}", c.chars().take(2000).collect::<String>());
            if n > 2000 {
                details += &format!(" … ({n} caractères)");
            }
        }
        ConfirmationText { summary: "Créer un fichier".into(), details, risk: Risk::Low }
    }
    async fn run(&self, a: &Value) -> Result<ToolResult, ToolError> {
        blocking(&self.0, a, create_run).await
    }
    async fn undo(&self, u: &Value) -> Result<(), ToolError> {
        blocking(&self.0, u, undo_recycle).await
    }
}

/// Source d'une copie : forme `lexical` (jamais de réseau), jamais les données d'Oniris (mails, journal).
fn copy_source(from: &str) -> Result<PathBuf, ToolError> {
    let l = lexical(Path::new(from)).map_err(bad)?;
    let c = resolve(&l).map_err(bad)?;
    if under(&c, &canonical_dirs(&oniris_data().into_iter().collect::<Vec<_>>())) {
        return Err(bad("données d'Oniris : copie refusée"));
    }
    Ok(c)
}

fn copy_check(f: &Files, a: &Value) -> Result<(), ToolError> {
    let a: MoveArgs = parse(a)?;
    copy_source(&a.from)?;
    f.w(&a.to).map(|_| ())
}

fn copy_run(f: &Files, a: &Value) -> Result<ToolResult, ToolError> {
    copy_check(f, a)?;
    let a: MoveArgs = parse(a)?;
    let (from, to) = (copy_source(&a.from)?, f.w(&a.to)?);
    let mut src = std::fs::File::open(&from).map_err(failed)?;
    if !src.metadata().map_err(failed)?.is_file() {
        return Err(bad("source : un fichier (pas un dossier)"));
    }
    let after = f.clear(&to, a.overwrite)?;
    // `create_new` : si quelqu'un a pris la place entre-temps, on ne touche pas à son fichier.
    let mut dst = std::fs::OpenOptions::new().write(true).create_new(true).open(&to).map_err(failed)?;
    if let Err(e) = std::io::copy(&mut src, &mut dst) {
        drop(dst);
        let _ = std::fs::remove_file(&to); // copie partielle, créée par nous
        let _ = f.restore_opt(&to, after);
        return Err(failed(e));
    }
    Ok(ToolResult { output: json!({"path": to}), undo: Some(json!({"path": to, "restore_after_ms": after})) })
}

#[async_trait]
impl Tool for FilesCopy {
    fn spec(&self) -> ToolSpec {
        spec(
            "files.copy",
            "Copie un fichier (écraser la cible demande confirmation ; elle part alors à la Corbeille).",
            json!({"type":"object","properties":{"from":{"type":"string"},"to":{"type":"string"},"overwrite":{"type":"boolean"}},"required":["from","to"]}),
            Permission::Free,
        )
    }
    fn check(&self, a: &Value) -> Result<(), ToolError> {
        copy_check(&self.0, a)
    }
    fn needs_confirm(&self, a: &Value) -> bool {
        a["overwrite"].as_bool().unwrap_or(false)
    }
    fn describe(&self, a: &Value) -> ConfirmationText {
        move_text(&self.0, "Copier un fichier", a)
    }
    async fn run(&self, a: &Value) -> Result<ToolResult, ToolError> {
        blocking(&self.0, a, copy_run).await
    }
    async fn undo(&self, u: &Value) -> Result<(), ToolError> {
        blocking(&self.0, u, undo_recycle).await
    }
}

fn move_text(f: &Files, summary: &str, a: &Value) -> ConfirmationText {
    let (from, to) = (a["from"].as_str().unwrap_or_default(), a["to"].as_str().unwrap_or_default());
    let over = a["overwrite"].as_bool().unwrap_or(false);
    let mut details = format!("{}\n→ {}", f.shown(from), f.shown(to));
    if over {
        details.push_str("\nUn élément déjà présent à la cible ira à la Corbeille.");
    }
    ConfirmationText { summary: summary.into(), details, risk: if over { Risk::High } else { Risk::Medium } }
}

fn move_run(f: &Files, a: &Value) -> Result<ToolResult, ToolError> {
    let a: MoveArgs = parse(a)?;
    f.mv(&f.w(&a.from)?, &f.w(&a.to)?, a.overwrite)
}

#[async_trait]
impl Tool for FilesMove {
    fn spec(&self) -> ToolSpec {
        spec(
            "files.move",
            "Déplace un fichier ou un dossier (confirmation hors des dossiers de travail ou pour écraser).",
            json!({"type":"object","properties":{"from":{"type":"string"},"to":{"type":"string"},"overwrite":{"type":"boolean"}},"required":["from","to"]}),
            Permission::Confirm,
        )
    }
    fn check(&self, a: &Value) -> Result<(), ToolError> {
        let a: MoveArgs = parse(a)?;
        self.0.w(&a.from)?;
        self.0.w(&a.to).map(|_| ())
    }
    fn needs_confirm(&self, a: &Value) -> bool {
        a["overwrite"].as_bool().unwrap_or(false) || !self.0.inside(a["from"].as_str()) || !self.0.inside(a["to"].as_str())
    }
    fn describe(&self, a: &Value) -> ConfirmationText {
        move_text(&self.0, "Déplacer", a)
    }
    async fn run(&self, a: &Value) -> Result<ToolResult, ToolError> {
        blocking(&self.0, a, move_run).await
    }
    async fn undo(&self, u: &Value) -> Result<(), ToolError> {
        blocking(&self.0, u, Files::undo_move).await
    }
}

fn rename_target(f: &Files, a: &RenameArgs) -> Result<(PathBuf, PathBuf), ToolError> {
    let n = &a.new_name;
    if n.is_empty() || n.chars().any(|c| matches!(c, '\\' | '/' | ':') || c.is_control()) {
        return Err(bad("nouveau nom : sans séparateur ni « : »"));
    }
    let from = f.w(&a.path)?;
    let to = f.w(&from.with_file_name(n).to_string_lossy())?;
    Ok((from, to))
}

fn rename_run(f: &Files, a: &Value) -> Result<ToolResult, ToolError> {
    let (from, to) = rename_target(f, &parse(a)?)?;
    f.mv(&from, &to, false)
}

#[async_trait]
impl Tool for FilesRename {
    fn spec(&self) -> ToolSpec {
        spec(
            "files.rename",
            "Renomme un fichier ou un dossier sur place (confirmation hors des dossiers de travail).",
            json!({"type":"object","properties":{"path":{"type":"string"},"new_name":{"type":"string"}},"required":["path","new_name"]}),
            Permission::Confirm,
        )
    }
    fn check(&self, a: &Value) -> Result<(), ToolError> {
        rename_target(&self.0, &parse(a)?).map(|_| ())
    }
    fn needs_confirm(&self, a: &Value) -> bool {
        !self.0.inside(a["path"].as_str())
    }
    fn describe(&self, a: &Value) -> ConfirmationText {
        let details = match parse::<RenameArgs>(a).and_then(|r| rename_target(&self.0, &r)) {
            Ok((from, to)) => format!("{}\n→ {}", from.display(), to.display()),
            Err(_) => a["path"].as_str().unwrap_or_default().to_string(),
        };
        ConfirmationText { summary: "Renommer".into(), details, risk: Risk::Medium }
    }
    async fn run(&self, a: &Value) -> Result<ToolResult, ToolError> {
        blocking(&self.0, a, rename_run).await
    }
    async fn undo(&self, u: &Value) -> Result<(), ToolError> {
        blocking(&self.0, u, Files::undo_move).await
    }
}

fn items(n: usize) -> String {
    if n == 1 { "1 élément ira".into() } else { format!("{n} éléments iront") }
}

fn delete_check(f: &Files, a: &Value) -> Result<(), ToolError> {
    let a: DeleteArgs = parse(a)?;
    if !(1..=MAX_PATHS).contains(&a.paths.len()) {
        return Err(bad(format!("1 à {MAX_PATHS} chemins")));
    }
    a.paths.iter().try_for_each(|p| f.w(p).map(|_| ()))
}

fn delete_run(f: &Files, a: &Value) -> Result<ToolResult, ToolError> {
    delete_check(f, a)?;
    let a: DeleteArgs = parse(a)?;
    let paths: Vec<PathBuf> = a.paths.iter().map(|p| f.w(p)).collect::<Result<_, _>>()?;
    let after = crate::time_util::now_ms() - DELETE_MARGIN_MS;
    let (mut done, mut missed) = (vec![], vec![]);
    for p in paths {
        match std::fs::symlink_metadata(&p).map_err(|e| e.to_string()).and_then(|_| f.recycler.recycle(&p)) {
            Ok(()) => done.push(p),
            Err(_) => missed.push(p),
        }
    }
    if done.is_empty() {
        return Err(failed("aucun élément n'a pu aller à la Corbeille"));
    }
    // Échec partiel : ce qui est parti garde son undo, le reste est signalé.
    Ok(ToolResult { output: json!({"recycled": done.len(), "failed": missed}), undo: Some(json!({"paths": done, "after_ms": after})) })
}

/// Rejouable après un échec partiel : un élément déjà restauré (place prise, entrée de Corbeille partie) est
/// reconnu par `restore_from` et compte comme fait.
fn delete_undo(f: &Files, u: &Value) -> Result<(), ToolError> {
    let u: DeleteUndo = parse(u)?;
    let mut errs = 0;
    for p in &u.paths {
        if f.w(p).and_then(|c| f.recycler.restore(&c, u.after_ms).map_err(failed)).is_err() {
            errs += 1;
        }
    }
    if errs == 0 { Ok(()) } else { Err(failed(format!("{errs} élément(s) non restauré(s)"))) }
}

#[async_trait]
impl Tool for FilesDelete {
    fn spec(&self) -> ToolSpec {
        spec(
            "files.delete",
            "Envoie des fichiers ou dossiers à la Corbeille (100 au plus ; confirmation hors des dossiers de travail).",
            json!({"type":"object","properties":{"paths":{"type":"array","items":{"type":"string"},"minItems":1,"maxItems":MAX_PATHS}},"required":["paths"]}),
            Permission::Confirm,
        )
    }
    fn check(&self, a: &Value) -> Result<(), ToolError> {
        delete_check(&self.0, a)
    }
    fn needs_confirm(&self, a: &Value) -> bool {
        a["paths"].as_array().is_none_or(|ps| ps.iter().any(|p| !self.0.inside(p.as_str())))
    }
    fn describe(&self, a: &Value) -> ConfirmationText {
        let ps: Vec<String> = a["paths"].as_array().into_iter().flatten().take(MAX_PATHS).map(|p| self.0.shown(p.as_str().unwrap_or_default())).collect();
        ConfirmationText {
            summary: "Mettre à la Corbeille".into(),
            details: format!("{}\n{} à la Corbeille.", ps.join("\n"), items(ps.len())),
            risk: Risk::Medium,
        }
    }
    async fn run(&self, a: &Value) -> Result<ToolResult, ToolError> {
        blocking(&self.0, a, delete_run).await
    }
    async fn undo(&self, u: &Value) -> Result<(), ToolError> {
        blocking(&self.0, u, delete_undo).await
    }
}

pub fn all(recycler: &Arc<dyn Recycler>, work_dirs: WorkDirs, protected: Vec<PathBuf>) -> Vec<Arc<dyn Tool>> {
    let f = Arc::new(Files { recycler: recycler.clone(), work_dirs, protected });
    vec![
        Arc::new(FilesCreate(f.clone())),
        Arc::new(FilesCopy(f.clone())),
        Arc::new(FilesRename(f.clone())),
        Arc::new(FilesMove(f.clone())),
        Arc::new(FilesDelete(f)),
    ]
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::general::recycle::{info_bytes, restore_from};
    use crate::test_util::db;
    use crate::tools::{Origin, Provenance, ToolBroker, ToolCall, ToolOutcome};
    use parking_lot::Mutex;

    fn refused(p: &str, protected: &[PathBuf]) {
        assert!(writable(Path::new(p), protected).is_err(), "{p}");
    }

    #[test]
    fn writable_refuse_les_chemins_proteges() {
        let prot = protected_dirs();
        for p in [r"C:\Windows\x", r"c:\WINDOWS\x", r"C:\PROGRA~1\x", r"C:\Program Files\x", r"C:\", "C:/", r"C:\x.txt", r"C:\a\..\Windows\x", r"C:\a:flux", r"C:\Windows.\x"] {
            refused(p, &prot);
        }
        for p in [r"\\srv\x", "//srv/x", r"/\srv\x", r"\/srv/x", r"\\?\C:\tmp\x", r"\\.\C:\x", r"\\?\UNC\srv\x", r"x\y", r"C:x", r"D:\$Recycle.Bin\x", r"D:\a\System Volume Information\x"] {
            refused(p, &prot);
        }
        let appdata = std::env::var("APPDATA").unwrap();
        refused(&format!(r"{appdata}\Oniris\oniris.db"), &prot);
        refused(&format!(r"{}\Oniris\x", appdata.to_uppercase()), &prot);
    }

    #[test]
    fn writable_accepte_un_dossier_temporaire() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("nouveau").join("a.txt");
        let c = writable(&p, &protected_dirs()).unwrap();
        assert_eq!(c.file_name().unwrap(), "a.txt");
        assert!(!c.to_string_lossy().starts_with(r"\\"));
        assert!(work_dir_ok(t.path(), &protected_dirs()).is_ok());
        assert!(work_dir_ok(Path::new(r"D:\"), &protected_dirs()).is_err());
        assert!(work_dir_ok(Path::new(r"C:\Windows"), &protected_dirs()).is_err());
    }

    #[test]
    fn writable_refuse_une_jonction_vers_un_dossier_protege() {
        let t = tempfile::tempdir().unwrap();
        let prot = t.path().join("Protege");
        std::fs::create_dir(&prot).unwrap();
        let link = t.path().join("lien");
        // `mklink /J` n'écrit que dans le dossier temporaire (sans droit administrateur).
        use std::os::windows::process::CommandExt;
        let ok = std::process::Command::new("cmd")
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .args(["/c", "mklink", "/J"])
            .arg(&link)
            .arg(&prot)
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap()
            .success();
        assert!(ok);
        let list = [prot.clone()];
        assert!(writable(&link.join("x"), &list).is_err());
        assert!(writable(&link.join("sous").join("x"), &list).is_err());
        assert!(writable(&link, &list).is_err(), "la jonction elle-même pointe dans le protégé");
        assert!(writable(&t.path().join("ailleurs"), &list).is_ok());
    }

    /// Fausse Corbeille : un dossier temporaire au format `$I`/`$R` ; `restore` passe par `restore_from`.
    struct Bin {
        dir: tempfile::TempDir,
        n: Mutex<u32>,
        restored: Mutex<Vec<PathBuf>>,
    }
    impl Recycler for Bin {
        fn recycle(&self, p: &Path) -> Result<(), String> {
            let mut n = self.n.lock();
            *n += 1;
            std::fs::write(self.dir.path().join(format!("$I{n}")), info_bytes(p, crate::time_util::now_ms())).unwrap();
            std::fs::rename(p, self.dir.path().join(format!("$R{n}"))).map_err(|e| e.to_string())
        }
        fn restore(&self, original: &Path, after: i64) -> Result<(), String> {
            self.restored.lock().push(original.to_path_buf());
            restore_from(self.dir.path(), original, after)
        }
    }

    struct Env {
        broker: ToolBroker,
        bin: Arc<Bin>,
        work: tempfile::TempDir,
        other: tempfile::TempDir,
    }

    fn setup() -> Env {
        let (work, other) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let bin = Arc::new(Bin { dir: tempfile::tempdir().unwrap(), n: Mutex::new(0), restored: Mutex::new(vec![]) });
        let r: Arc<dyn Recycler> = bin.clone();
        let w = work.path().to_string_lossy().into_owned();
        let tools = all(&r, Arc::new(move || vec![w.clone()]), protected_dirs());
        let broker = ToolBroker::new(Arc::new(Mutex::new(db())), Arc::new(|| 1_000), tools).unwrap();
        Env { broker, bin, work, other }
    }

    async fn call(b: &ToolBroker, tool: &str, args: Value) -> ToolOutcome {
        b.call(ToolCall { tool: tool.into(), args, provenance: Provenance::User, origin: Origin::User }).await
    }

    fn s(p: &Path) -> String {
        p.to_string_lossy().into_owned()
    }

    #[tokio::test]
    async fn delete_hors_des_dossiers_de_travail_montre_la_liste() {
        let e = setup();
        let (a, b) = (e.other.path().join("a.txt"), e.work.path().join("b.txt"));
        std::fs::write(&a, "a").unwrap();
        std::fs::write(&b, "b").unwrap();
        let ToolOutcome::NeedsConfirmation(p) = call(&e.broker, "files.delete", json!({"paths": [s(&a), s(&b)]})).await else { panic!() };
        let (ca, cb) = (std::fs::canonicalize(&a).unwrap(), std::fs::canonicalize(&b).unwrap());
        let plain = |c: &Path| crate::launch::unverbatim(&c.to_string_lossy());
        assert_eq!(p.text.details, format!("{}\n{}\n2 éléments iront à la Corbeille.", plain(&ca), plain(&cb)));
        assert!(a.exists() && b.exists());
    }

    #[tokio::test]
    async fn delete_dans_un_dossier_de_travail_puis_undo() {
        let e = setup();
        let a = e.work.path().join("a.txt");
        std::fs::write(&a, "a").unwrap();
        let ToolOutcome::Done { action_id, .. } = call(&e.broker, "files.delete", json!({"paths": [s(&a)]})).await else { panic!() };
        assert!(!a.exists());
        e.broker.undo(action_id).await.unwrap();
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "a");
        assert_eq!(e.bin.restored.lock().len(), 1);
        // Provenance non fiable : carte même dans un dossier de travail.
        let unt = ToolCall { tool: "files.delete".into(), args: json!({"paths": [s(&a)]}), provenance: Provenance::Untrusted { source: "mail".into() }, origin: Origin::User };
        assert!(matches!(e.broker.call(unt).await, ToolOutcome::NeedsConfirmation(_)));
    }

    #[tokio::test]
    async fn delete_refuse_avant_toute_carte() {
        let e = setup();
        for args in [json!({"paths": [r"C:\Windows\win.ini"]}), json!({"paths": [r"\\127.0.0.1\x\y"]}), json!({"paths": []}), json!({"paths": vec!["C:\\t\\x"; 101]})] {
            assert!(matches!(call(&e.broker, "files.delete", args).await, ToolOutcome::Failed { .. }));
        }
        assert_eq!(*e.bin.n.lock(), 0);
    }

    #[tokio::test]
    async fn move_puis_undo_revient_au_depart() {
        let e = setup();
        let (a, b) = (e.work.path().join("a.txt"), e.work.path().join("sous"));
        std::fs::write(&a, "a").unwrap();
        std::fs::create_dir(&b).unwrap();
        let to = b.join("a.txt");
        let ToolOutcome::Done { action_id, .. } = call(&e.broker, "files.move", json!({"from": s(&a), "to": s(&to)})).await else { panic!() };
        assert!(!a.exists() && to.exists());
        e.broker.undo(action_id).await.unwrap();
        assert!(a.exists() && !to.exists());
        // Hors des dossiers de travail, ou avec écrasement : carte.
        let out = e.other.path().join("a.txt");
        assert!(matches!(call(&e.broker, "files.move", json!({"from": s(&a), "to": s(&out)})).await, ToolOutcome::NeedsConfirmation(_)));
        assert!(matches!(call(&e.broker, "files.move", json!({"from": s(&a), "to": s(&to), "overwrite": true})).await, ToolOutcome::NeedsConfirmation(_)));
        // Cible existante sans écrasement : échec, rien ne bouge.
        std::fs::write(&to, "t").unwrap();
        assert!(matches!(call(&e.broker, "files.move", json!({"from": s(&a), "to": s(&to)})).await, ToolOutcome::Failed { .. }));
        assert_eq!(std::fs::read_to_string(&to).unwrap(), "t");
    }

    #[tokio::test]
    async fn move_avec_ecrasement_puis_undo_rend_la_cible() {
        let e = setup();
        let (a, b) = (e.work.path().join("a.txt"), e.work.path().join("b.txt"));
        std::fs::write(&a, "a").unwrap();
        std::fs::write(&b, "b").unwrap();
        let ToolOutcome::NeedsConfirmation(p) = call(&e.broker, "files.move", json!({"from": s(&a), "to": s(&b), "overwrite": true})).await else { panic!() };
        assert_eq!(p.text.risk, Risk::High);
        let ToolOutcome::Done { action_id, .. } = e.broker.resolve(p.action_id, true).await else { panic!() };
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "a");
        e.broker.undo(action_id).await.unwrap();
        assert_eq!((std::fs::read_to_string(&a).unwrap(), std::fs::read_to_string(&b).unwrap()), ("a".into(), "b".into()));
    }

    #[tokio::test]
    async fn rename_sur_place() {
        let e = setup();
        let a = e.work.path().join("a.txt");
        std::fs::write(&a, "a").unwrap();
        for bad_name in [r"..\x", "x/y", "x:y", "", "x."] {
            assert!(matches!(call(&e.broker, "files.rename", json!({"path": s(&a), "new_name": bad_name})).await, ToolOutcome::Failed { .. }), "{bad_name}");
        }
        let ToolOutcome::Done { action_id, .. } = call(&e.broker, "files.rename", json!({"path": s(&a), "new_name": "b.txt"})).await else { panic!() };
        assert!(e.work.path().join("b.txt").exists());
        e.broker.undo(action_id).await.unwrap();
        assert!(a.exists());
    }

    #[test]
    fn create_la_carte_montre_un_apercu_du_contenu() {
        let bin = Arc::new(Bin { dir: tempfile::tempdir().unwrap(), n: Mutex::new(0), restored: Mutex::new(vec![]) });
        let tool = FilesCreate(Arc::new(Files { recycler: bin, work_dirs: Arc::new(Vec::new), protected: vec![] }));
        let d = tool.describe(&json!({"path": "C:/x.txt", "content": "bonjour"})).details;
        assert!(d.ends_with("

bonjour"), "{d}");
        let d = tool.describe(&json!({"path": "C:/x.txt", "content": "é".repeat(2001)})).details;
        assert!(d.ends_with(&format!("{} … (2001 caractères)", "é".repeat(2000))), "{d}");
    }

    #[tokio::test]
    async fn create_refuse_un_fichier_existant_et_undo_recycle() {
        let e = setup();
        let p = e.other.path().join("n.txt");
        let ToolOutcome::Done { action_id, .. } = call(&e.broker, "files.create", json!({"path": s(&p), "content": "é"})).await else { panic!() };
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "é");
        assert!(matches!(call(&e.broker, "files.create", json!({"path": s(&p), "content": "x"})).await, ToolOutcome::Failed { .. }));
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "é");
        e.broker.undo(action_id).await.unwrap();
        assert!(!p.exists());
        let big = "a".repeat(MAX_CONTENT + 1);
        assert!(matches!(call(&e.broker, "files.create", json!({"path": s(&p), "content": big})).await, ToolOutcome::Failed { .. }));
        assert!(matches!(call(&e.broker, "files.create", json!({"path": r"C:\Windows\x.txt"})).await, ToolOutcome::Failed { .. }));
    }

    #[tokio::test]
    async fn copy_libre_sans_ecrasement() {
        let e = setup();
        let (a, b) = (e.other.path().join("a.txt"), e.other.path().join("b.txt"));
        std::fs::write(&a, "a").unwrap();
        let ToolOutcome::Done { action_id, .. } = call(&e.broker, "files.copy", json!({"from": s(&a), "to": s(&b)})).await else { panic!() };
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "a");
        assert!(matches!(call(&e.broker, "files.copy", json!({"from": s(&a), "to": s(&b)})).await, ToolOutcome::Failed { .. }));
        assert!(matches!(call(&e.broker, "files.copy", json!({"from": s(&a), "to": s(&b), "overwrite": true})).await, ToolOutcome::NeedsConfirmation(_)));
        assert!(matches!(call(&e.broker, "files.copy", json!({"from": r"\\127.0.0.1\x\a", "to": s(&b)})).await, ToolOutcome::Failed { .. }));
        e.broker.undo(action_id).await.unwrap();
        assert!(!b.exists() && a.exists());
    }

    #[test]
    fn lexical_refuse_caracteres_et_peripheriques() {
        for ko in [r"C:\t\a*b", r"C:\t\a?b", r"C:\t\a<b", r"C:\t\a>b", r"C:\t\a|b", r#"C:\t\a"b"#, r"C:\t\NUL", r"C:\t\nul.txt", r"C:\t\Con", r"C:\t\COM1.log", r"C:\t\lpt9", r"C:\t\CONIN$", r"C:\t\conout$.x", r"C:\AUX\x", r"C:\t\prn .txt"] {
            assert!(lexical(Path::new(ko)).is_err(), "{ko}");
        }
        for ok in [r"C:\t\console.txt", r"C:\t\com10", r"C:\t\nullable", r"C:\t\a.con"] {
            assert!(lexical(Path::new(ok)).is_ok(), "{ok}");
        }
    }

    #[test]
    fn un_ancetre_de_dossier_protege_est_refuse() {
        let t = tempfile::tempdir().unwrap();
        let prot = [t.path().join("a").join("b")];
        std::fs::create_dir_all(&prot[0]).unwrap();
        assert!(writable(&t.path().join("a"), &prot).is_err());
        assert!(writable(&t.path().join("A"), &prot).is_err(), "casse ignorée");
        assert!(writable(&t.path().join("ab"), &prot).is_ok(), "comparaison par composant");
        assert!(writable(&t.path().join("a").join("c"), &prot).is_ok());
    }

    #[test]
    fn demarrage_et_profils_powershell_proteges() {
        let prot = protected_dirs();
        let (appdata, home) = (std::env::var("APPDATA").unwrap(), std::env::var("USERPROFILE").unwrap());
        refused(&format!(r"{appdata}\Microsoft\Windows\Start Menu\Programs\Startup\x.bat"), &prot);
        refused(&format!(r"{home}\Documents\WindowsPowerShell\Microsoft.PowerShell_profile.ps1"), &prot);
        refused(&format!(r"{home}\Documents\PowerShell\profile.ps1"), &prot);
        refused(&format!(r"{home}\Documents"), &prot);
        refused(&home, &prot);
        assert!(writable(Path::new(&format!(r"{home}\Documents\zz_oniris_x.txt")), &prot).is_ok());
    }

    #[tokio::test]
    async fn appdata_et_son_parent_jamais_touches() {
        let e = setup();
        let appdata = std::env::var("APPDATA").unwrap();
        let parent = Path::new(&appdata).parent().unwrap().to_string_lossy().into_owned();
        let src = e.work.path().join("a.txt");
        std::fs::write(&src, "a").unwrap();
        for target in [&appdata, &parent] {
            assert!(matches!(call(&e.broker, "files.delete", json!({"paths": [target]})).await, ToolOutcome::Failed { .. }), "{target}");
            assert!(matches!(call(&e.broker, "files.rename", json!({"path": target, "new_name": "x"})).await, ToolOutcome::Failed { .. }), "{target}");
            assert!(matches!(call(&e.broker, "files.move", json!({"from": target, "to": s(&e.work.path().join("m"))})).await, ToolOutcome::Failed { .. }), "{target}");
            for tool in ["files.move", "files.copy"] {
                let r = call(&e.broker, tool, json!({"from": s(&src), "to": target, "overwrite": true})).await;
                assert!(matches!(r, ToolOutcome::Failed { .. }), "{tool} {target}");
            }
        }
        assert_eq!(*e.bin.n.lock(), 0);
        assert!(src.exists());
    }

    #[tokio::test]
    async fn la_racine_d_un_dossier_de_travail_demande_une_carte() {
        let e = setup();
        let root = s(e.work.path());
        assert!(matches!(call(&e.broker, "files.delete", json!({"paths": [root]})).await, ToolOutcome::NeedsConfirmation(_)));
        assert!(matches!(call(&e.broker, "files.rename", json!({"path": root, "new_name": "autre"})).await, ToolOutcome::NeedsConfirmation(_)));
        assert!(e.work.path().exists());
    }

    #[tokio::test]
    async fn copie_des_donnees_d_oniris_refusee() {
        let e = setup();
        let db = format!(r"{}\Oniris\oniris.db", std::env::var("APPDATA").unwrap());
        let r = call(&e.broker, "files.copy", json!({"from": db, "to": s(&e.work.path().join("vol.db"))})).await;
        assert!(matches!(r, ToolOutcome::Failed { .. }));
        assert!(!e.work.path().join("vol.db").exists());
    }

    #[tokio::test]
    async fn undo_de_suppression_rejouable_apres_echec_partiel() {
        let e = setup();
        let (a, b) = (e.work.path().join("a.txt"), e.work.path().join("b.txt"));
        std::fs::write(&a, "a").unwrap();
        std::fs::write(&b, "b").unwrap();
        let ToolOutcome::Done { action_id, .. } = call(&e.broker, "files.delete", json!({"paths": [s(&a), s(&b)]})).await else { panic!() };
        std::fs::write(&a, "intrus").unwrap();
        assert!(e.broker.undo(action_id).await.is_err());
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "b");
        std::fs::remove_file(&a).unwrap();
        e.broker.undo(action_id).await.unwrap();
        assert_eq!((std::fs::read_to_string(&a).unwrap(), std::fs::read_to_string(&b).unwrap()), ("a".into(), "b".into()));
    }
}
