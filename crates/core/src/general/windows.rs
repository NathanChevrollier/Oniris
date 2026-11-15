//! Outils `apps.focus`, `apps.move` (libres) et `apps.close` (confirmation, `WM_CLOSE`) ; le vrai contrôle vit
//! dans src-tauri. Le choix de l'écran et le calcul du rectangle cible sont purs (ici, testés).
use crate::apps::Inventory;
use crate::tools::helpers::{bad, failed, low, parse, spec};
use crate::tools::{ConfirmationText, Permission, Risk, Tool, ToolError, ToolResult, ToolSpec};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

/// Attente maximale de la fenêtre d'une application qu'on vient de lancer, et pas du sondage.
pub const WAIT_MS: u64 = 15_000;
pub const POLL_MS: u64 = 150;
/// Appli déjà ouverte sans nouvelle fenêtre après ce délai : instance unique qui a réactivé la sienne.
pub const REUSE_AFTER_MS: u64 = 2_000;

/// Rectangle en pixels écran (bords droit et bas exclus).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// Un écran : sa surface entière et sa zone de travail (hors barre des tâches).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Monitor {
    pub area: Rect,
    pub work: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Left,
    Right,
    Primary,
    Secondary,
    /// Rang 1..n de gauche à droite.
    Number(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    Full,
    LeftHalf,
    RightHalf,
    Center,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    pub screen: Screen,
    pub layout: Layout,
}

/// Relevé pris avant un lancement : fenêtres déjà ouvertes de l'application et fenêtre au premier plan (si ce
/// n'est pas Oniris). Identifiants de fenêtre opaques.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Before {
    pub windows: Vec<isize>,
    pub foreground: Option<isize>,
}

/// `app_key` : clé de l'inventaire (raccourci `.lnk`/`.url` ou `shell:AppsFolder\…`) ; `title` : son nom, pour
/// retrouver les fenêtres quand le raccourci ne désigne pas d'exécutable.
pub trait WindowControl: Send + Sync {
    fn focus(&self, app_key: &str, title: &str) -> Result<(), String>;
    /// Demande polie (`WM_CLOSE`) : l'application peut proposer d'enregistrer. Jamais `TerminateProcess`.
    fn close(&self, app_key: &str, title: &str) -> Result<(), String>;
    fn before(&self, app_key: &str, title: &str) -> Before;
    /// Avec `before` (juste après un lancement) : attend la fenêtre (`choose_window`, `WAIT_MS` au plus) ; sinon
    /// prend la fenêtre ouverte. La place (si `placement`), puis rend le premier plan à la fenêtre qui l'avait si
    /// `keep_focus`. Bloquant. Rend `false` si Windows a refusé de rendre le premier plan.
    fn place(&self, app_key: &str, title: &str, placement: Option<Placement>, before: Option<&Before>, keep_focus: bool) -> Result<bool, String>;
}

/// Fenêtre à placer : une nouvelle, sinon (après `REUSE_AFTER_MS`) une déjà ouverte avant le lancement.
pub fn choose_window(before: &[isize], now: &[isize], waited_ms: u64) -> Option<isize> {
    let fresh = now.iter().find(|h| !before.contains(h));
    fresh.or_else(|| now.iter().find(|h| waited_ms >= REUSE_AFTER_MS && before.contains(h))).copied()
}

fn too_few(n: usize) -> String {
    if n == 1 { "Je ne vois qu'un écran".into() } else { format!("Je ne vois que {n} écrans") }
}

/// Écran visé, parmi ceux détectés (principal : celui qui commence en (0,0)).
pub fn pick_monitor(monitors: &[Monitor], screen: Screen) -> Result<Monitor, String> {
    let mut sorted = monitors.to_vec();
    sorted.sort_by_key(|m| (m.area.left, m.area.top));
    let n = sorted.len();
    if n == 0 {
        return Err("aucun écran détecté".into());
    }
    let primary = sorted.iter().position(|m| m.area.left == 0 && m.area.top == 0).unwrap_or(0);
    let i = match screen {
        Screen::Left => 0,
        Screen::Right => n - 1,
        Screen::Primary => primary,
        Screen::Secondary => (0..n).find(|&i| i != primary).ok_or_else(|| too_few(n))?,
        Screen::Number(k) if (1..=n).contains(&k) => k - 1,
        Screen::Number(_) => return Err(too_few(n)),
    };
    Ok(sorted[i])
}

/// Rectangle cible dans la zone de travail ; `size` : taille actuelle de la fenêtre (pour le centrage).
pub fn target_rect(work: Rect, layout: Layout, size: (i32, i32)) -> Rect {
    let (w, h) = (work.right - work.left, work.bottom - work.top);
    let mid = work.left + w / 2;
    match layout {
        Layout::Full => work,
        Layout::LeftHalf => Rect { right: mid, ..work },
        Layout::RightHalf => Rect { left: mid, ..work },
        Layout::Center => {
            let (cw, ch) = (size.0.clamp(1, w), size.1.clamp(1, h));
            let (left, top) = (work.left + (w - cw) / 2, work.top + (h - ch) / 2);
            Rect { left, top, right: left + cw, bottom: top + ch }
        }
    }
}

pub fn screen_label(s: Screen) -> String {
    match s {
        Screen::Left => "l'écran de gauche".into(),
        Screen::Right => "l'écran de droite".into(),
        Screen::Primary => "l'écran principal".into(),
        Screen::Secondary => "l'écran secondaire".into(),
        Screen::Number(n) => format!("l'écran {n}"),
    }
}

fn parse_screen(v: &Value) -> Result<Screen, ToolError> {
    let s = match v {
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.trim().to_lowercase(),
        _ => String::new(),
    };
    Ok(match s.as_str() {
        "gauche" | "left" => Screen::Left,
        "droite" | "right" => Screen::Right,
        "principal" | "primary" => Screen::Primary,
        "secondaire" | "secondary" => Screen::Secondary,
        n => match n.parse::<usize>() {
            Ok(k) if k >= 1 => Screen::Number(k),
            _ => return Err(bad(format!("écran inconnu « {n} » : gauche, droite, principal, secondaire ou un numéro (1 = le plus à gauche)"))),
        },
    })
}

fn parse_layout(s: &str) -> Result<Layout, ToolError> {
    Ok(match s.trim().to_lowercase().as_str() {
        "plein" | "plein écran" | "maximisé" | "full" => Layout::Full,
        "gauche" | "left" => Layout::LeftHalf,
        "droite" | "right" => Layout::RightHalf,
        "centre" | "center" => Layout::Center,
        o => return Err(bad(format!("disposition inconnue « {o} » : plein, gauche, droite ou centre"))),
    })
}

/// Arguments `screen` / `layout` facultatifs ; `None` sans écran. Une disposition sans écran est refusée.
pub fn placement(a: &Value) -> Result<Option<Placement>, ToolError> {
    let layout = match &a["layout"] {
        Value::Null => None,
        Value::String(s) => Some(parse_layout(s)?),
        _ => return Err(bad("disposition : texte attendu")),
    };
    match &a["screen"] {
        Value::Null if layout.is_some() => Err(bad("disposition sans écran : précise aussi « screen »")),
        Value::Null => Ok(None),
        v => Ok(Some(Placement { screen: parse_screen(v)?, layout: layout.unwrap_or(Layout::Full) })),
    }
}

/// Description commune des arguments `screen` et `layout` pour le modèle.
pub fn placement_schema() -> (Value, Value) {
    (
        json!({"type":"string","description":"Écran visé : « gauche », « droite », « principal », « secondaire », ou un numéro « 1 », « 2 »… compté de gauche à droite. Ex. « sur mon écran de gauche » → gauche ; « sur mon deuxième écran » → 2 ; « sur l'autre écran » → secondaire."}),
        json!({"type":"string","enum":["plein","gauche","droite","centre"],"description":"Disposition sur cet écran (défaut : plein = maximisée). « gauche » / « droite » : moitié de l'écran ; « centre » : taille actuelle, centrée. Ex. « sur la moitié droite de l'écran principal » → screen principal, layout droite."}),
    )
}

/// Les appels Win32 (énumération, attente de fenêtre) bloquent : hors du worker async.
pub async fn blocking<T: Send + 'static>(w: &Arc<dyn WindowControl>, f: impl FnOnce(&dyn WindowControl) -> T + Send + 'static) -> Result<T, ToolError> {
    let w = w.clone();
    tokio::task::spawn_blocking(move || f(w.as_ref())).await.map_err(failed)
}

#[derive(Deserialize)]
struct KeyArgs {
    key: String,
}

pub struct AppsFocus {
    pub windows: Arc<dyn WindowControl>,
    pub inventory: Arc<Inventory>,
}
pub struct AppsClose {
    pub windows: Arc<dyn WindowControl>,
    pub inventory: Arc<Inventory>,
}
pub struct AppsMove {
    pub windows: Arc<dyn WindowControl>,
    pub inventory: Arc<Inventory>,
}

fn known(inv: &Inventory, a: &Value) -> Result<(String, String), ToolError> {
    let KeyArgs { key } = parse(a)?;
    inv.resolve(&key).ok_or_else(|| bad("application introuvable sur ce PC"))
}

fn key_schema() -> Value {
    json!({"type":"object","properties":{"key":{"type":"string","description":"Nom de l'application"}},"required":["key"]})
}

#[async_trait]
impl Tool for AppsFocus {
    fn spec(&self) -> ToolSpec {
        spec("apps.focus", "Met au premier plan une application ouverte de l'inventaire.", key_schema(), Permission::Free)
    }
    fn check(&self, a: &Value) -> Result<(), ToolError> {
        known(&self.inventory, a).map(|_| ())
    }
    fn describe(&self, _: &Value) -> ConfirmationText {
        low("Mettre une application au premier plan")
    }
    async fn run(&self, a: &Value) -> Result<ToolResult, ToolError> {
        let (key, title) = known(&self.inventory, a)?;
        self.windows.focus(&key, &title).map_err(failed)?;
        Ok(ToolResult { output: json!({"focused": true}), undo: None })
    }
}

#[async_trait]
impl Tool for AppsClose {
    fn spec(&self) -> ToolSpec {
        spec("apps.close", "Ferme les fenêtres d'une application de l'inventaire (confirmation requise).", key_schema(), Permission::Confirm)
    }
    fn check(&self, a: &Value) -> Result<(), ToolError> {
        known(&self.inventory, a).map(|_| ())
    }
    fn describe(&self, a: &Value) -> ConfirmationText {
        let key = a["key"].as_str().unwrap_or_default();
        let title = self.inventory.items.read().iter().find(|i| i.key == key).map(|i| i.title.clone()).unwrap_or_default();
        ConfirmationText { summary: format!("Fermer {title}"), details: "Ses fenêtres reçoivent une demande de fermeture : rien n'est tué de force.".into(), risk: Risk::Medium }
    }
    async fn run(&self, a: &Value) -> Result<ToolResult, ToolError> {
        let (key, title) = known(&self.inventory, a)?;
        self.windows.close(&key, &title).map_err(failed)?;
        Ok(ToolResult { output: json!({"closed": true}), undo: None })
    }
}

impl AppsMove {
    fn args(&self, a: &Value) -> Result<(String, String, Placement), ToolError> {
        let (key, title) = known(&self.inventory, a)?;
        let p = placement(a)?.ok_or_else(|| bad("écran manquant : précise « screen »"))?;
        Ok((key, title, p))
    }
}

#[async_trait]
impl Tool for AppsMove {
    fn spec(&self) -> ToolSpec {
        let (screen, layout) = placement_schema();
        let schema = json!({"type":"object","properties":{"key":{"type":"string","description":"Nom de l'application déjà ouverte"},"screen":screen,"layout":layout},"required":["key","screen"]});
        spec("apps.move", "Déplace une application déjà ouverte sur un autre écran, sans lui donner le focus. Ex. « mets Discord sur mon écran de droite » → {key: Discord, screen: droite} ; « VS Code sur la moitié gauche de l'écran 2 » → {key: VS Code, screen: 2, layout: gauche}. Pour une application pas encore ouverte : apps.launch avec screen.", schema, Permission::Free)
    }
    fn check(&self, a: &Value) -> Result<(), ToolError> {
        self.args(a).map(|_| ())
    }
    fn describe(&self, _: &Value) -> ConfirmationText {
        low("Déplacer une application sur un écran")
    }
    async fn run(&self, a: &Value) -> Result<ToolResult, ToolError> {
        let (key, title, p) = self.args(a)?;
        let t = title.clone();
        blocking(&self.windows, move |w| w.place(&key, &t, Some(p), None, true)).await?.map_err(failed)?;
        Ok(ToolResult { output: json!({"moved": true, "message": format!("{title} déplacé sur {}", screen_label(p.screen))}), undo: None })
    }
}

pub fn all(windows: &Arc<dyn WindowControl>, inventory: &Arc<Inventory>) -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(AppsFocus { windows: windows.clone(), inventory: inventory.clone() }),
        Arc::new(AppsClose { windows: windows.clone(), inventory: inventory.clone() }),
        Arc::new(AppsMove { windows: windows.clone(), inventory: inventory.clone() }),
    ]
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::search::{Item, ItemKind};
    use crate::test_util::db;
    use crate::tools::{Origin, Provenance, ToolBroker, ToolCall, ToolOutcome};
    use parking_lot::Mutex;

    /// Faux contrôle des fenêtres : journal partagé (aussi avec un faux `Opener`), réponse de `place` réglable.
    #[derive(Default)]
    pub(crate) struct Fake {
        pub log: Arc<Mutex<Vec<String>>>,
        pub place_err: Option<String>,
        pub focus_refused: bool,
    }
    impl WindowControl for Fake {
        fn focus(&self, k: &str, _: &str) -> Result<(), String> {
            self.log.lock().push(format!("focus {k}"));
            Ok(())
        }
        fn close(&self, k: &str, _: &str) -> Result<(), String> {
            self.log.lock().push(format!("close {k}"));
            Ok(())
        }
        fn before(&self, k: &str, _: &str) -> Before {
            self.log.lock().push(format!("before {k}"));
            Before { windows: vec![1], foreground: Some(9) }
        }
        fn place(&self, k: &str, _: &str, p: Option<Placement>, before: Option<&Before>, keep_focus: bool) -> Result<bool, String> {
            self.log.lock().push(format!("place {k} {p:?} attente={} focus={keep_focus}", before.is_some()));
            match &self.place_err {
                Some(e) => Err(e.clone()),
                None => Ok(!self.focus_refused),
            }
        }
    }

    fn inv() -> Arc<Inventory> {
        let inv = Arc::new(Inventory::default());
        inv.set(vec![Item { kind: ItemKind::App, key: r"C:\m\App.lnk".into(), title: "App".into(), subtitle: String::new() }], 1);
        inv
    }

    #[tokio::test]
    async fn focus_libre_close_sur_carte() {
        let inv = inv();
        let fake = Arc::new(Fake::default());
        let w: Arc<dyn WindowControl> = fake.clone();
        let b = ToolBroker::new(Arc::new(Mutex::new(db())), Arc::new(|| 1_000), all(&w, &inv)).unwrap();
        let call = |tool: &str, key: &str| ToolCall { tool: tool.into(), args: json!({"key": key}), provenance: Provenance::User, origin: Origin::User };
        assert!(matches!(b.call(call("apps.focus", r"C:\m\App.lnk")).await, ToolOutcome::Done { .. }));
        let ToolOutcome::NeedsConfirmation(p) = b.call(call("apps.close", r"C:\m\App.lnk")).await else { panic!() };
        assert_eq!(p.text.summary, "Fermer App");
        assert_eq!(*fake.log.lock(), [r"focus C:\m\App.lnk"]);
        assert!(matches!(b.resolve(p.action_id, true).await, ToolOutcome::Done { .. }));
        assert_eq!(fake.log.lock().len(), 2);
        assert!(matches!(b.call(call("apps.close", r"C:\Windows\notepad.exe")).await, ToolOutcome::Failed { .. }));
        assert_eq!(fake.log.lock().len(), 2);
    }

    #[tokio::test]
    async fn move_place_sans_attente_et_refuse_les_arguments_invalides() {
        let inv = inv();
        let fake = Arc::new(Fake::default());
        let w: Arc<dyn WindowControl> = fake.clone();
        let b = ToolBroker::new(Arc::new(Mutex::new(db())), Arc::new(|| 1_000), all(&w, &inv)).unwrap();
        let call = |args: Value| ToolCall { tool: "apps.move".into(), args, provenance: Provenance::User, origin: Origin::User };
        let out = b.call(call(json!({"key": r"C:\m\App.lnk", "screen": 2, "layout": "droite"}))).await;
        let ToolOutcome::Done { output, .. } = out else { panic!("{out:?}") };
        assert_eq!(output["message"], "App déplacé sur l'écran 2");
        assert_eq!(*fake.log.lock(), [r"place C:\m\App.lnk Some(Placement { screen: Number(2), layout: RightHalf }) attente=false focus=true"]);
        for bad_args in [json!({"key": r"C:\m\App.lnk"}), json!({"key": r"C:\m\App.lnk", "screen": "haut"}), json!({"key": r"C:\m\App.lnk", "screen": 0}), json!({"key": r"C:\m\App.lnk", "screen": "gauche", "layout": "bas"})] {
            assert!(matches!(b.call(call(bad_args.clone())).await, ToolOutcome::Failed { .. }), "{bad_args}");
        }
        assert_eq!(fake.log.lock().len(), 1, "rien n'est déplacé sur des arguments invalides");
        let w: Arc<dyn WindowControl> = Arc::new(Fake { place_err: Some("aucune fenêtre ouverte pour cette application".into()), ..Fake::default() });
        let b = ToolBroker::new(Arc::new(Mutex::new(db())), Arc::new(|| 1_000), all(&w, &inv)).unwrap();
        let out = b.call(call(json!({"key": r"C:\m\App.lnk", "screen": "gauche"}))).await;
        assert!(matches!(&out, ToolOutcome::Failed { error, .. } if error.contains("aucune fenêtre")), "{out:?}");
    }

    fn r(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
        Rect { left, top, right, bottom }
    }
    /// Écran de 1920×1080 à `x`, barre des tâches de 40 px en bas.
    fn mon(x: i32, y: i32) -> Monitor {
        Monitor { area: r(x, y, x + 1920, y + 1080), work: r(x, y, x + 1920, y + 1040) }
    }

    #[test]
    fn choix_de_l_ecran() {
        let one = [mon(0, 0)];
        for s in [Screen::Left, Screen::Right, Screen::Primary, Screen::Number(1)] {
            assert_eq!(pick_monitor(&one, s), Ok(one[0]));
        }
        assert_eq!(pick_monitor(&one, Screen::Secondary), Err("Je ne vois qu'un écran".into()));
        // Deux écrans, le secondaire à gauche en x négatif (ordre d'énumération quelconque).
        let two = [mon(0, 0), mon(-1920, 0)];
        assert_eq!(pick_monitor(&two, Screen::Left), Ok(two[1]));
        assert_eq!(pick_monitor(&two, Screen::Right), Ok(two[0]));
        assert_eq!(pick_monitor(&two, Screen::Primary), Ok(two[0]));
        assert_eq!(pick_monitor(&two, Screen::Secondary), Ok(two[1]));
        assert_eq!(pick_monitor(&two, Screen::Number(1)), Ok(two[1]));
        assert_eq!(pick_monitor(&two, Screen::Number(2)), Ok(two[0]));
        assert_eq!(pick_monitor(&two, Screen::Number(3)), Err("Je ne vois que 2 écrans".into()));
        assert_eq!(pick_monitor(&two, Screen::Number(0)), Err("Je ne vois que 2 écrans".into()));
        // Trois écrans, principal au milieu.
        let three = [mon(1920, 0), mon(0, 0), mon(-1920, 0)];
        assert_eq!(pick_monitor(&three, Screen::Left), Ok(three[2]));
        assert_eq!(pick_monitor(&three, Screen::Right), Ok(three[0]));
        assert_eq!(pick_monitor(&three, Screen::Secondary), Ok(three[2]), "premier non principal de gauche à droite");
        assert_eq!(pick_monitor(&three, Screen::Number(2)), Ok(three[1]));
        assert!(pick_monitor(&[], Screen::Left).is_err());
    }

    #[test]
    fn rectangles_cibles() {
        let w = mon(-1920, 0).work;
        assert_eq!(target_rect(w, Layout::Full, (800, 600)), w);
        assert_eq!(target_rect(w, Layout::LeftHalf, (800, 600)), r(-1920, 0, -960, 1040));
        assert_eq!(target_rect(w, Layout::RightHalf, (800, 600)), r(-960, 0, 0, 1040));
        assert_eq!(target_rect(w, Layout::Center, (800, 600)), r(-1920 + 560, 220, -1920 + 1360, 820));
        assert_eq!(target_rect(w, Layout::Center, (5000, 5000)), w, "plus grande que l'écran : ramenée à la zone de travail");
    }

    #[test]
    fn fenetre_nouvelle_ou_reprise() {
        assert_eq!(choose_window(&[1], &[1, 2], 0), Some(2));
        assert_eq!(choose_window(&[1], &[1], 500), None, "on attend encore une nouvelle fenêtre");
        assert_eq!(choose_window(&[1], &[1], REUSE_AFTER_MS), Some(1), "instance unique : sa fenêtre existante");
        assert_eq!(choose_window(&[], &[], 10_000), None);
    }

    #[test]
    fn arguments_de_placement() {
        assert_eq!(placement(&json!({})), Ok(None));
        assert_eq!(placement(&json!({"screen": "Gauche"})), Ok(Some(Placement { screen: Screen::Left, layout: Layout::Full })));
        assert_eq!(placement(&json!({"screen": "2", "layout": "centre"})), Ok(Some(Placement { screen: Screen::Number(2), layout: Layout::Center })));
        assert_eq!(placement(&json!({"screen": "secondaire", "layout": "gauche"})), Ok(Some(Placement { screen: Screen::Secondary, layout: Layout::LeftHalf })));
        assert!(placement(&json!({"layout": "gauche"})).is_err(), "disposition sans écran");
        assert!(placement(&json!({"screen": "haut"})).is_err());
        assert!(placement(&json!({"screen": true})).is_err());
    }
}
