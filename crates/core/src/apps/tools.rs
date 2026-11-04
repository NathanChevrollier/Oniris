//! Outils `files.open` et `apps.launch` (libres) ; l'ouverture réelle est injectée via `Opener`.
use super::Inventory;
use crate::launch::{is_executable, safe_path, Opener};
use crate::tools::helpers::{bad, low, parse, spec};
use crate::tools::{ConfirmationText, Permission, Risk, Tool, ToolError, ToolResult, ToolSpec};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

/// Racines autorisées, relues à chaque appel (les réglages peuvent changer).
pub type Roots = Arc<dyn Fn() -> Vec<PathBuf> + Send + Sync>;

pub struct FilesOpen {
    pub opener: Arc<dyn Opener>,
    pub roots: Roots,
}
pub struct FilesRun {
    pub opener: Arc<dyn Opener>,
    pub roots: Roots,
}
pub struct AppsLaunch {
    pub opener: Arc<dyn Opener>,
    pub inventory: Arc<Inventory>,
}

#[derive(Deserialize)]
struct PathArgs {
    path: String,
}
#[derive(Deserialize)]
struct KeyArgs {
    key: String,
}

#[async_trait]
impl Tool for FilesOpen {
    fn spec(&self) -> ToolSpec {
        spec("files.open", "Ouvre un fichier ou un dossier des dossiers de recherche.", json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}), Permission::Free)
    }
    fn describe(&self, _: &Value) -> ConfirmationText {
        low("Ouvrir un fichier")
    }
    async fn run(&self, a: &Value) -> Result<ToolResult, ToolError> {
        let canon = checked(a, &self.roots)?;
        if is_executable(&canon) {
            return Err(bad("exécutable : utilise files.run"));
        }
        self.opener.open_path(&canon);
        Ok(ToolResult { output: json!({"opened": true}), undo: None })
    }
}

/// Chemin canonique (le seul à ouvrir) si l'argument est dans les racines.
fn checked(a: &Value, roots: &Roots) -> Result<PathBuf, ToolError> {
    let PathArgs { path } = parse(a)?;
    safe_path(&PathBuf::from(path), &roots()).ok_or_else(|| bad("chemin hors des dossiers autorisés"))
}

#[async_trait]
impl Tool for FilesRun {
    fn spec(&self) -> ToolSpec {
        spec("files.run", "Exécute un programme ou un script des dossiers de recherche (confirmation requise).", json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}), Permission::Confirm)
    }
    fn describe(&self, a: &Value) -> ConfirmationText {
        let name = a["path"].as_str().and_then(|p| std::path::Path::new(p).file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        ConfirmationText { summary: format!("Exécuter {name}"), details: name, risk: Risk::High }
    }
    async fn run(&self, a: &Value) -> Result<ToolResult, ToolError> {
        let canon = checked(a, &self.roots)?;
        self.opener.open_path(&canon);
        Ok(ToolResult { output: json!({"opened": true}), undo: None })
    }
}

#[async_trait]
impl Tool for AppsLaunch {
    fn spec(&self) -> ToolSpec {
        spec("apps.launch", "Lance une application de l'inventaire du menu Démarrer.", json!({"type":"object","properties":{"key":{"type":"string"}},"required":["key"]}), Permission::Free)
    }
    fn describe(&self, _: &Value) -> ConfirmationText {
        low("Lancer une application")
    }
    async fn run(&self, a: &Value) -> Result<ToolResult, ToolError> {
        let KeyArgs { key } = parse(a)?;
        if !self.inventory.contains_key(&key) {
            return Err(bad("application inconnue"));
        }
        self.opener.open_path(std::path::Path::new(&key)); // le .lnk lance sa cible
        Ok(ToolResult { output: json!({"launched": true}), undo: None })
    }
}

pub fn all(opener: &Arc<dyn Opener>, roots: Roots, inventory: &Arc<Inventory>) -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(FilesOpen { opener: opener.clone(), roots: roots.clone() }),
        Arc::new(FilesRun { opener: opener.clone(), roots }),
        Arc::new(AppsLaunch { opener: opener.clone(), inventory: inventory.clone() }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::{Item, ItemKind};
    use crate::test_util::db;
    use crate::tools::{Origin, Provenance, ToolBroker, ToolCall, ToolOutcome};
    use parking_lot::Mutex;
    use std::path::Path;

    #[derive(Default)]
    struct Fake(Mutex<Vec<String>>);
    impl Opener for Fake {
        fn open_path(&self, p: &Path) {
            self.0.lock().push(p.to_string_lossy().into_owned());
        }
        fn open_url(&self, u: &str) {
            self.0.lock().push(u.into());
        }
        fn spawn_detached(&self, _: &str, _: &[String], _: Option<&Path>) -> std::io::Result<()> {
            Ok(())
        }
    }

    async fn call(b: &ToolBroker, tool: &str, args: Value) -> ToolOutcome {
        b.call(ToolCall { tool: tool.into(), args, provenance: Provenance::User, origin: Origin::User }).await
    }

    #[tokio::test]
    async fn ouvre_seulement_ce_qui_est_autorise() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("r");
        std::fs::create_dir_all(&root).unwrap();
        let (inside, outside) = (root.join("a.txt"), t.path().join("b.txt"));
        std::fs::write(&inside, "").unwrap();
        std::fs::write(&outside, "").unwrap();
        let fake = Arc::new(Fake::default());
        let opener: Arc<dyn Opener> = fake.clone();
        let inv = Arc::new(Inventory::default());
        inv.set(vec![Item { kind: ItemKind::App, key: "C:\\x\\App.lnk".into(), title: "App".into(), subtitle: String::new() }], 1);
        let r = root.clone();
        let broker = ToolBroker::new(Arc::new(Mutex::new(db())), Arc::new(|| 0), all(&opener, Arc::new(move || vec![r.clone()]), &inv)).unwrap();

        assert!(matches!(call(&broker, "files.open", json!({"path": inside})).await, ToolOutcome::Done { .. }));
        assert!(!matches!(call(&broker, "files.open", json!({"path": outside})).await, ToolOutcome::Done { .. }));
        assert!(matches!(call(&broker, "apps.launch", json!({"key": "C:\\x\\App.lnk"})).await, ToolOutcome::Done { .. }));
        assert!(!matches!(call(&broker, "apps.launch", json!({"key": "C:\\Windows\\notepad.exe"})).await, ToolOutcome::Done { .. }));
        assert_eq!(fake.0.lock().len(), 2, "seuls les deux appels valides ouvrent");
    }

    #[tokio::test]
    async fn executables_via_files_run_avec_confirmation() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("r");
        std::fs::create_dir_all(root.join("dossier")).unwrap();
        let fake = Arc::new(Fake::default());
        let opener: Arc<dyn Opener> = fake.clone();
        let r = root.clone();
        let broker = ToolBroker::new(Arc::new(Mutex::new(db())), Arc::new(|| 0), all(&opener, Arc::new(move || vec![r.clone()]), &Arc::new(Inventory::default()))).unwrap();
        for ext in crate::launch::EXECUTABLE {
            let f = root.join(format!("x.{ext}"));
            std::fs::write(&f, "").unwrap();
            let out = call(&broker, "files.open", json!({"path": f})).await;
            assert!(matches!(&out, ToolOutcome::Failed { error, .. } if error.contains("files.run")), "{ext} : {out:?}");
        }
        assert!(fake.0.lock().is_empty());
        let exe = root.join("x.exe");
        let ToolOutcome::NeedsConfirmation(p) = call(&broker, "files.run", json!({"path": exe})).await else { panic!("Confirm attendu") };
        assert!(p.text.summary.contains("x.exe") && p.text.risk == Risk::High);
        assert!(fake.0.lock().is_empty(), "rien n'est ouvert avant confirmation");
        assert!(matches!(call(&broker, "files.open", json!({"path": root.join("dossier")})).await, ToolOutcome::Done { .. }));
        assert_eq!(fake.0.lock().len(), 1);
    }
}
