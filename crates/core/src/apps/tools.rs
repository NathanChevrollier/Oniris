//! Outils `files.open` et `apps.launch` (libres) ; l'ouverture réelle est injectée via `Opener`.
use super::Inventory;
use crate::general::windows::{blocking, placement, placement_schema, screen_label, WindowControl};
use crate::launch::{is_clsid_dir, is_executable, safe_path, Opener};
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
    pub windows: Arc<dyn WindowControl>,
}

#[derive(Deserialize)]
struct PathArgs {
    path: String,
}
#[derive(Deserialize)]
struct LaunchArgs {
    key: String,
    #[serde(default)]
    keep_focus: bool,
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
    let canon = safe_path(&PathBuf::from(path), &roots()).ok_or_else(|| bad("chemin hors des dossiers autorisés"))?;
    if is_clsid_dir(&canon) {
        return Err(bad("dossier spécial refusé"));
    }
    Ok(canon)
}

#[async_trait]
impl Tool for FilesRun {
    fn spec(&self) -> ToolSpec {
        spec("files.run", "Exécute un programme ou un script des dossiers de recherche (confirmation requise).", json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}), Permission::Confirm)
    }
    fn describe(&self, a: &Value) -> ConfirmationText {
        let p = std::path::Path::new(a["path"].as_str().unwrap_or_default());
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        // Le dossier parent aide à repérer un homonyme ; ce texte va à l'écran de confirmation, jamais à tracing.
        let dir = p.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
        ConfirmationText { summary: format!("Exécuter {name} (dans {dir})"), details: name, risk: Risk::High }
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
        let (screen, layout) = placement_schema();
        let schema = json!({"type":"object","properties":{
            "key":{"type":"string","description":"Nom de l'application"},
            "screen":screen,
            "layout":layout,
            "keep_focus":{"type":"boolean","description":"Vrai pour rendre le premier plan à la fenêtre où l'utilisateur travaillait : « sans me prendre le focus », « sans me déranger », « en arrière-plan »."}
        },"required":["key"]});
        spec("apps.launch", "Lance une application ou un jeu installé, désigné par son nom (ex. « Rocket League », « Discord »). Avec plusieurs écrans, peut placer sa fenêtre : « ouvre VS Code sur mon écran de gauche sans me prendre le focus » → {key: VS Code, screen: gauche, keep_focus: true} ; « lance Spotify sur la moitié droite de mon deuxième écran » → {key: Spotify, screen: 2, layout: droite}. Sans screen ni keep_focus : simple lancement.", schema, Permission::Free)
    }
    fn check(&self, a: &Value) -> Result<(), ToolError> {
        parse::<LaunchArgs>(a).and(placement(a)).map(|_| ())
    }
    fn describe(&self, _: &Value) -> ConfirmationText {
        low("Lancer une application")
    }
    async fn run(&self, a: &Value) -> Result<ToolResult, ToolError> {
        let LaunchArgs { key, keep_focus } = parse(a)?;
        let place = placement(a)?;
        let Some((key, title)) = self.inventory.resolve(&key) else { return Err(bad("application introuvable sur ce PC")) };
        let wants = place.is_some() || keep_focus;
        let before = if wants {
            let (k, t) = (key.clone(), title.clone());
            Some(blocking(&self.windows, move |w| w.before(&k, &t)).await?)
        } else {
            None
        };
        if key.starts_with(super::APPS_FOLDER) {
            // Application du Store : l'AppID vient de l'inventaire (vérifié ci-dessus), passé en argument séparé.
            self.opener.spawn_detached("explorer.exe", std::slice::from_ref(&key), None).map_err(|e| ToolError::Failed(format!("lancement impossible : {e}")))?;
        } else {
            self.opener.open_path(std::path::Path::new(&key)); // le .lnk lance sa cible
        }
        let Some(before) = before else { return Ok(ToolResult { output: json!({"launched": title}), undo: None }) };
        let t = title.clone();
        let placed = blocking(&self.windows, move |w| w.place(&key, &t, place, Some(&before), keep_focus)).await?;
        let where_ = place.map(|p| format!(" sur {}", screen_label(p.screen))).unwrap_or_default();
        // Déjà lancée : un échec de placement n'est pas un échec du lancement (sinon IRIS relancerait l'appli).
        let message = match placed {
            Ok(true) => format!("{title} ouvert{where_}"),
            Ok(false) => format!("{title} ouvert{where_}, mais Windows a refusé de rendre le focus"),
            Err(e) => format!("{title} lancé, mais pas placé : {e}"),
        };
        Ok(ToolResult { output: json!({"launched": title, "message": message}), undo: None })
    }
}

pub fn all(opener: &Arc<dyn Opener>, roots: Roots, inventory: &Arc<Inventory>, windows: &Arc<dyn WindowControl>) -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(FilesOpen { opener: opener.clone(), roots: roots.clone() }),
        Arc::new(FilesRun { opener: opener.clone(), roots }),
        Arc::new(AppsLaunch { opener: opener.clone(), inventory: inventory.clone(), windows: windows.clone() }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::general::windows::tests::Fake as WinFake;
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

    fn no_windows() -> Arc<dyn WindowControl> {
        Arc::new(WinFake::default())
    }

    /// Ouvreur qui écrit dans le journal du faux contrôle des fenêtres (pour vérifier l'ordre).
    struct LogOpener(Arc<Mutex<Vec<String>>>);
    impl Opener for LogOpener {
        fn open_path(&self, p: &Path) {
            self.0.lock().push(format!("open {}", p.to_string_lossy()));
        }
        fn open_url(&self, _: &str) {}
        fn spawn_detached(&self, _: &str, _: &[String], _: Option<&Path>) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn lance_puis_place_sur_l_ecran_demande() {
        let inv = Arc::new(Inventory::default());
        inv.set(vec![Item { kind: ItemKind::App, key: r"C:\m\Code.lnk".into(), title: "VS Code".into(), subtitle: String::new() }], 1);
        let run = |fake: WinFake, args: Value| {
            let inv = inv.clone();
            async move {
                let log = fake.log.clone();
                let opener: Arc<dyn Opener> = Arc::new(LogOpener(log.clone()));
                let w: Arc<dyn WindowControl> = Arc::new(fake);
                let b = ToolBroker::new(Arc::new(Mutex::new(db())), Arc::new(|| 0), all(&opener, Arc::new(Vec::new), &inv, &w)).unwrap();
                let out = call(&b, "apps.launch", args).await;
                let log = log.lock().clone();
                (out, log)
            }
        };
        let (out, log) = run(WinFake::default(), json!({"key": "VS Code", "screen": "gauche", "keep_focus": true})).await;
        let ToolOutcome::Done { output, .. } = out else { panic!("{out:?}") };
        assert_eq!(output["message"], "VS Code ouvert sur l'écran de gauche");
        assert_eq!(log, [r"before C:\m\Code.lnk", r"open C:\m\Code.lnk", r"place C:\m\Code.lnk Some(Placement { screen: Left, layout: Full }) attente=true focus=true"]);

        let (out, _) = run(WinFake { focus_refused: true, ..WinFake::default() }, json!({"key": "VS Code", "screen": 2, "layout": "droite", "keep_focus": true})).await;
        let ToolOutcome::Done { output, .. } = out else { panic!("{out:?}") };
        assert_eq!(output["message"], "VS Code ouvert sur l'écran 2, mais Windows a refusé de rendre le focus");

        let (out, _) = run(WinFake { place_err: Some("Je ne vois que 2 écrans".into()), ..WinFake::default() }, json!({"key": "VS Code", "screen": 3})).await;
        let ToolOutcome::Done { output, .. } = out else { panic!("{out:?}") };
        assert_eq!(output["message"], "VS Code lancé, mais pas placé : Je ne vois que 2 écrans");

        // Sans placement : comportement d'avant, aucune fenêtre touchée.
        let (_, log) = run(WinFake::default(), json!({"key": "VS Code"})).await;
        assert_eq!(log, [r"open C:\m\Code.lnk"]);

        // Arguments invalides : refusés avant tout lancement.
        for args in [json!({"key": "VS Code", "screen": "haut"}), json!({"key": "VS Code", "layout": "gauche"}), json!({"key": "VS Code", "screen": "gauche", "layout": "bas"})] {
            let (out, log) = run(WinFake::default(), args.clone()).await;
            assert!(matches!(out, ToolOutcome::Failed { .. }), "{args}");
            assert!(log.is_empty(), "{args}");
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
        let broker = ToolBroker::new(Arc::new(Mutex::new(db())), Arc::new(|| 0), all(&opener, Arc::new(move || vec![r.clone()]), &inv, &no_windows())).unwrap();

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
        let broker = ToolBroker::new(Arc::new(Mutex::new(db())), Arc::new(|| 0), all(&opener, Arc::new(move || vec![r.clone()]), &Arc::new(Inventory::default()), &no_windows())).unwrap();
        for ext in crate::launch::EXECUTABLE {
            let f = root.join(format!("x.{ext}"));
            std::fs::write(&f, "").unwrap();
            let out = call(&broker, "files.open", json!({"path": f})).await;
            assert!(matches!(&out, ToolOutcome::Failed { error, .. } if error.contains("files.run")), "{ext} : {out:?}");
        }
        assert!(fake.0.lock().is_empty());
        let exe = root.join("x.exe");
        let ToolOutcome::NeedsConfirmation(p) = call(&broker, "files.run", json!({"path": exe})).await else { panic!("Confirm attendu") };
        assert!(p.text.summary.contains("x.exe") && p.text.summary.contains(root.to_string_lossy().as_ref()) && p.text.risk == Risk::High);
        assert!(fake.0.lock().is_empty(), "rien n'est ouvert avant confirmation");
        assert!(matches!(call(&broker, "files.open", json!({"path": root.join("dossier")})).await, ToolOutcome::Done { .. }));
        assert_eq!(fake.0.lock().len(), 1);
        let clsid = root.join("Corbeille.{645FF040-5081-101B-9F08-00AA002F954E}");
        std::fs::create_dir_all(&clsid).unwrap();
        assert!(!matches!(call(&broker, "files.open", json!({"path": clsid})).await, ToolOutcome::Done { .. }));
        assert_eq!(fake.0.lock().len(), 1, "jonction CLSID jamais ouverte");
    }
}
