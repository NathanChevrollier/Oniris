//! Routines : une phrase lance une suite d'outils sûrs, chacun passé au courtier comme une demande de l'utilisateur.
//! Stockage : clé `routines` de la table `kv` (JSON validé à l'écriture).
use crate::tools::helpers::{bad, failed, low, parse, spec};
use crate::tools::{ConfirmationText, Origin, PendingAction, Permission, Provenance, Tool, ToolBroker, ToolCall, ToolError, ToolOutcome, ToolResult, ToolSpec};
use crate::Db;
use async_trait::async_trait;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::{Arc, OnceLock, Weak};

pub const MAX_ROUTINES: usize = 30;
pub const MAX_STEPS: usize = 20;
pub const MAX_NAME: usize = 60;
pub const MAX_PHRASES: usize = 5;
pub const MAX_PHRASE: usize = 100;
/// Octets du JSON des arguments d'une étape, au plus.
pub const MAX_ARGS: usize = 1000;
const KEY: &str = "routines";

/// Seuls outils permis dans une étape, avec leur libellé (miroir de `src/modules/routines/tools.ts`).
/// Jamais `files.run`, le shell, les mails ni l'alimentation.
pub const ALLOWED: [(&str, &str); 18] = [
    ("apps.launch", "Lancer une application"),
    ("apps.focus", "Mettre une application au premier plan"),
    ("apps.close", "Fermer une application"),
    ("apps.move", "Déplacer une application sur un écran"),
    ("pc.volume", "Régler le son du PC"),
    ("pc.bluetooth", "Activer ou couper le Bluetooth"),
    ("music.play_playlist", "Lancer une playlist Spotify"),
    ("music.play_search", "Lancer une musique Spotify"),
    ("music.toggle", "Lecture ou pause de la musique"),
    ("music.next", "Titre suivant"),
    ("music.volume", "Régler le volume de Spotify"),
    ("timer.start", "Lancer un minuteur"),
    ("focus.start", "Lancer une session focus"),
    ("favorites.open", "Ouvrir un favori"),
    ("projects.resume", "Reprendre un projet"),
    ("files.open", "Ouvrir un fichier ou un dossier"),
    ("notes.create", "Créer une note"),
    ("reminders.create", "Créer un rappel"),
];

pub fn label(tool: &str) -> &str {
    ALLOWED.iter().find(|(t, _)| *t == tool).map_or(tool, |(_, l)| l)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Step {
    pub tool: String,
    #[serde(default = "empty")]
    pub args: Value,
    /// Un échec (ou une attente de confirmation) arrête les étapes suivantes.
    #[serde(default)]
    pub blocking: bool,
}

fn empty() -> Value { json!({}) }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Routine {
    pub name: String,
    #[serde(default)]
    pub phrases: Vec<String>,
    pub steps: Vec<Step>,
}

/// Minuscules sans accents ni ponctuation finale, espaces réduits.
fn norm(s: &str) -> String {
    crate::when::norm(s).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Nettoie et vérifie la liste entière (bornes, liste blanche, noms uniques).
pub fn validate(list: &mut [Routine]) -> Result<(), String> {
    if list.len() > MAX_ROUTINES {
        return Err(format!("{MAX_ROUTINES} routines au plus"));
    }
    let mut names = Vec::new();
    for r in list.iter_mut() {
        r.name = r.name.trim().to_string();
        let n = r.name.chars().count();
        if n == 0 || n > MAX_NAME {
            return Err(format!("nom de routine : 1 à {MAX_NAME} caractères"));
        }
        let key = norm(&r.name);
        if names.contains(&key) {
            return Err(format!("deux routines s'appellent « {} »", r.name));
        }
        names.push(key);
        r.phrases = r.phrases.iter().map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect();
        if r.phrases.len() > MAX_PHRASES || r.phrases.iter().any(|p| p.chars().count() > MAX_PHRASE) {
            return Err(format!("« {} » : {MAX_PHRASES} phrases au plus, {MAX_PHRASE} caractères chacune", r.name));
        }
        if r.steps.is_empty() || r.steps.len() > MAX_STEPS {
            return Err(format!("« {} » : 1 à {MAX_STEPS} étapes", r.name));
        }
        for s in &mut r.steps {
            if !ALLOWED.iter().any(|(t, _)| *t == s.tool) {
                return Err(format!("« {} » : outil non permis dans une routine : {}", r.name, s.tool));
            }
            if s.args.is_null() {
                s.args = empty();
            }
            if !s.args.is_object() || s.args.to_string().len() > MAX_ARGS {
                return Err(format!("« {} » : arguments invalides pour {}", r.name, s.tool));
            }
        }
    }
    Ok(())
}

/// Liste enregistrée ; illisible ou invalide : vide (journalisé), jamais d'étape non vérifiée.
pub fn load(c: &Connection) -> Vec<Routine> {
    let raw = match crate::kv::get(c, KEY) {
        Ok(Some((v, _))) => v,
        Ok(None) => return vec![],
        Err(e) => { tracing::warn!("routines : lecture impossible : {e}"); return vec![]; }
    };
    let mut list: Vec<Routine> = serde_json::from_str(&raw).unwrap_or_default();
    match validate(&mut list) {
        Ok(()) => list,
        Err(_) => { tracing::warn!("routines : liste enregistrée invalide, ignorée"); vec![] }
    }
}

pub fn save(c: &Connection, mut list: Vec<Routine>, now: i64) -> Result<Vec<Routine>, String> {
    validate(&mut list)?;
    let raw = serde_json::to_string(&list).map_err(|e| e.to_string())?;
    crate::kv::set(c, KEY, &raw, now).map_err(|e| e.to_string())?;
    Ok(list)
}

/// Routine désignée par son nom ou une de ses phrases : égalité d'abord, sinon le plus long nom ou la plus longue
/// phrase contenus dans la demande (« lance le mode jeu » → « mode jeu »).
pub fn find<'a>(list: &'a [Routine], query: &str) -> Option<&'a Routine> {
    let q = norm(query);
    if q.is_empty() {
        return None;
    }
    let keys = |r: &'a Routine| std::iter::once(&r.name).chain(&r.phrases).map(move |k| (norm(k), r));
    let all: Vec<(String, &Routine)> = list.iter().flat_map(keys).filter(|(k, _)| !k.is_empty()).collect();
    if let Some((_, r)) = all.iter().find(|(k, _)| *k == q) {
        return Some(*r);
    }
    let padded = format!(" {q} ");
    all.into_iter().filter(|(k, _)| padded.contains(&format!(" {k} "))).max_by_key(|(k, _)| k.len()).map(|(_, r)| r)
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus { Done, Failed, Pending, Rejected, Skipped }

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StepReport {
    pub label: String,
    pub status: StepStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Report {
    pub routine: String,
    pub steps: Vec<StepReport>,
    /// Étapes en attente d'une carte de confirmation.
    pub pending: usize,
    pub summary: String,
}

pub type OnPending = Arc<dyn Fn(&PendingAction) + Send + Sync>;

/// Joue les étapes dans l'ordre, chacune comme une demande de l'utilisateur. Une étape à confirmer montre sa carte
/// et la routine continue ; une étape en échec n'arrête la suite que si elle est bloquante.
pub async fn run(broker: &ToolBroker, r: &Routine, on_pending: &(dyn Fn(&PendingAction) + Send + Sync)) -> Report {
    let mut steps = Vec::with_capacity(r.steps.len());
    let mut stop = false;
    for s in &r.steps {
        let label = label(&s.tool).to_string();
        if stop {
            steps.push(StepReport { label, status: StepStatus::Skipped, error: None });
            continue;
        }
        // Revérifié ici : une liste écrite en base par un autre chemin ne passe jamais un outil hors liste.
        let (status, error) = if !ALLOWED.iter().any(|(t, _)| *t == s.tool) {
            (StepStatus::Failed, Some("outil non permis".to_string()))
        } else {
            let call = ToolCall { tool: s.tool.clone(), args: s.args.clone(), provenance: Provenance::User, origin: Origin::User };
            match broker.call(call).await {
                ToolOutcome::Done { .. } => (StepStatus::Done, None),
                ToolOutcome::NeedsConfirmation(p) => { on_pending(&p); (StepStatus::Pending, None) }
                ToolOutcome::Rejected { .. } => (StepStatus::Rejected, None),
                ToolOutcome::Failed { error, .. } => (StepStatus::Failed, Some(error)),
            }
        };
        stop = s.blocking && status != StepStatus::Done;
        steps.push(StepReport { label, status, error });
    }
    let count = |st: StepStatus| steps.iter().filter(|s| s.status == st).count();
    let (done, pending, skipped) = (count(StepStatus::Done), count(StepStatus::Pending), count(StepStatus::Skipped));
    let mut parts = vec![format!("{done} étape{} sur {} faite{}", if done > 1 { "s" } else { "" }, steps.len(), if done > 1 { "s" } else { "" })];
    for s in steps.iter().filter(|s| s.status == StepStatus::Failed) {
        parts.push(format!("échec : {} ({})", s.label, s.error.as_deref().unwrap_or("erreur")));
    }
    if pending > 0 {
        parts.push(format!("{pending} à confirmer dans le panneau"));
    }
    if skipped > 0 {
        parts.push(format!("{skipped} non lancée{} après une étape bloquante", if skipped > 1 { "s" } else { "" }));
    }
    Report { routine: r.name.clone(), summary: format!("Routine « {} » : {}.", r.name, parts.join(", ")), steps, pending }
}

/// Le courtier n'existe qu'après la liste de ses outils : `routines.run` le reçoit ensuite (référence faible).
pub type BrokerCell = Arc<OnceLock<Weak<ToolBroker>>>;

pub struct RunTool { pub db: Db, pub broker: BrokerCell, pub on_pending: OnPending }
pub struct ListTool { pub db: Db }

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RunArgs { name: String }

#[async_trait]
impl Tool for RunTool {
    fn spec(&self) -> ToolSpec {
        spec("routines.run", "Lance une routine de l'utilisateur (« lance le mode jeu ») : { name } = son nom ou la phrase dite. Rend un résumé à redire.",
            json!({"type":"object","properties":{"name":{"type":"string","maxLength":200}},"required":["name"]}), Permission::Free)
    }
    fn check(&self, a: &Value) -> Result<(), ToolError> {
        let RunArgs { name } = parse(a)?;
        find(&load(&self.db.lock()), &name).map(|_| ()).ok_or_else(|| bad("routine inconnue (routines.list pour les voir)"))
    }
    fn describe(&self, a: &Value) -> ConfirmationText {
        let name = a["name"].as_str().unwrap_or_default();
        ConfirmationText { summary: format!("Lancer la routine « {name} »"), ..low("") }
    }
    async fn run(&self, a: &Value) -> Result<ToolResult, ToolError> {
        let RunArgs { name } = parse(a)?;
        let r = find(&load(&self.db.lock()), &name).cloned().ok_or_else(|| bad("routine inconnue"))?;
        let broker = self.broker.get().and_then(Weak::upgrade).ok_or_else(|| failed("courtier indisponible"))?;
        let report = run(&broker, &r, self.on_pending.as_ref()).await;
        Ok(ToolResult { output: json!(report), undo: None })
    }
}

#[async_trait]
impl Tool for ListTool {
    fn spec(&self) -> ToolSpec {
        spec("routines.list", "Liste les routines de l'utilisateur : nom, phrases de déclenchement et étapes.", json!({"type":"object","properties":{}}), Permission::Free)
    }
    fn describe(&self, _: &Value) -> ConfirmationText { low("Lister les routines") }
    async fn run(&self, _: &Value) -> Result<ToolResult, ToolError> {
        let list = load(&self.db.lock());
        let out: Vec<Value> = list.iter().map(|r| json!({
            "name": r.name, "phrases": r.phrases, "steps": r.steps.iter().map(|s| label(&s.tool)).collect::<Vec<_>>(),
        })).collect();
        Ok(ToolResult { output: json!(out), undo: None })
    }
}

pub fn all(db: &Db, broker: &BrokerCell, on_pending: OnPending) -> Vec<Arc<dyn Tool>> {
    vec![Arc::new(RunTool { db: db.clone(), broker: broker.clone(), on_pending }), Arc::new(ListTool { db: db.clone() })]
}

#[cfg(test)]
mod tests {
    use super::*;
    use parking_lot::Mutex;

    /// Faux outil qui note ses appels ; `fail` : échoue, `confirm` : demande une carte.
    struct Fake { name: &'static str, fail: bool, confirm: bool, log: Arc<Mutex<Vec<String>>> }

    #[async_trait]
    impl Tool for Fake {
        fn spec(&self) -> ToolSpec { spec(self.name, "fake", json!({}), if self.confirm { Permission::Confirm } else { Permission::Free }) }
        fn describe(&self, _: &Value) -> ConfirmationText { low("fake") }
        async fn run(&self, _: &Value) -> Result<ToolResult, ToolError> {
            self.log.lock().push(self.name.into());
            if self.fail { Err(failed("Steam introuvable")) } else { Ok(ToolResult { output: json!({}), undo: None }) }
        }
    }

    fn step(tool: &str, blocking: bool) -> Step { Step { tool: tool.into(), args: json!({}), blocking } }
    fn routine(name: &str, phrases: &[&str], steps: Vec<Step>) -> Routine {
        Routine { name: name.into(), phrases: phrases.iter().map(|p| p.to_string()).collect(), steps }
    }

    struct Fx { db: Db, broker: Arc<ToolBroker>, log: Arc<Mutex<Vec<String>>>, cards: Arc<Mutex<Vec<String>>> }

    fn fx() -> Fx {
        let db: Db = Arc::new(Mutex::new(crate::test_util::db()));
        let log = Arc::new(Mutex::new(vec![]));
        let cards = Arc::new(Mutex::new(vec![]));
        let cell: BrokerCell = Arc::default();
        let c = cards.clone();
        let mut tools = all(&db, &cell, Arc::new(move |p: &PendingAction| c.lock().push(p.tool.clone())));
        for (name, fail, confirm) in [("pc.volume", false, false), ("apps.launch", true, false), ("apps.close", false, true), ("pc.bluetooth", false, false), ("files.run", false, false)] {
            tools.push(Arc::new(Fake { name, fail, confirm, log: log.clone() }));
        }
        let broker = Arc::new(ToolBroker::new(db.clone(), Arc::new(|| 0), tools).unwrap());
        cell.set(Arc::downgrade(&broker)).unwrap();
        Fx { db, broker, log, cards }
    }

    fn call(tool: &str, args: Value) -> ToolCall { ToolCall { tool: tool.into(), args, provenance: Provenance::User, origin: Origin::User } }

    #[test]
    fn validation_liste_blanche_et_bornes() {
        let ok = || routine("Mode jeu", &["lance le mode jeu"], vec![step("pc.volume", false)]);
        assert!(validate(&mut [ok()]).is_ok());
        for bad_tool in ["files.run", "shell.run", "mail.send", "pc.power", "routines.run", "screen.describe"] {
            assert!(validate(&mut [routine("x", &[], vec![step(bad_tool, false)])]).is_err(), "{bad_tool}");
        }
        assert!(validate(&mut [routine(&"a".repeat(61), &[], vec![step("pc.volume", false)])]).is_err());
        assert!(validate(&mut [routine(&"é".repeat(60), &[], vec![step("pc.volume", false)])]).is_ok(), "60 caractères, pas octets");
        assert!(validate(&mut [routine("  ", &[], vec![step("pc.volume", false)])]).is_err());
        assert!(validate(&mut [routine("x", &[], vec![])]).is_err());
        assert!(validate(&mut [routine("x", &[], vec![step("pc.volume", false); 21])]).is_err());
        assert!(validate(&mut [routine("x", &[], vec![step("pc.volume", false); 20])]).is_ok());
        assert!(validate(&mut vec![ok(); 1].into_iter().chain((0..30).map(|i| routine(&format!("r{i}"), &[], vec![step("pc.volume", false)]))).collect::<Vec<_>>()).is_err());
        assert!(validate(&mut [ok(), routine("mode JEU", &[], vec![step("pc.volume", false)])]).is_err(), "noms uniques");
        let mut big = routine("x", &[], vec![Step { tool: "notes.create".into(), args: json!({"text": "a".repeat(1001)}), blocking: false }]);
        assert!(validate(std::slice::from_mut(&mut big)).is_err());
        let mut arr = routine("x", &[], vec![Step { tool: "pc.volume".into(), args: json!([1]), blocking: false }]);
        assert!(validate(std::slice::from_mut(&mut arr)).is_err());
        assert!(validate(&mut [routine("x", &["p"; 6], vec![step("pc.volume", false)])]).is_err());
    }

    #[test]
    fn enregistrement_et_relecture() {
        let f = fx();
        let c = f.db.lock();
        assert!(load(&c).is_empty());
        assert!(save(&c, vec![routine("x", &[], vec![step("files.run", false)])], 1).is_err());
        let saved = save(&c, vec![routine(" Mode jeu ", &[" ", "jeu"], vec![step("pc.volume", false)])], 1).unwrap();
        assert_eq!((saved[0].name.as_str(), saved[0].phrases.clone()), ("Mode jeu", vec!["jeu".to_string()]));
        assert_eq!(load(&c), saved);
        // Écrite en base par un autre chemin : jamais chargée.
        crate::kv::set(&c, KEY, r#"[{"name":"x","steps":[{"tool":"files.run"}]}]"#, 2).unwrap();
        assert!(load(&c).is_empty());
    }

    #[test]
    fn recherche_par_nom_ou_phrase() {
        let list = [
            routine("Mode jeu", &["on joue"], vec![step("pc.volume", false)]),
            routine("Mode", &[], vec![step("pc.volume", false)]),
            routine("Mode boulot", &["au travail"], vec![step("pc.volume", false)]),
        ];
        assert_eq!(find(&list, "mode jeu").unwrap().name, "Mode jeu");
        assert_eq!(find(&list, "Lance le MODE JEU !").unwrap().name, "Mode jeu", "le plus long contenu gagne");
        assert_eq!(find(&list, "on joue").unwrap().name, "Mode jeu");
        assert_eq!(find(&list, "allez, au travail").unwrap().name, "Mode boulot");
        assert_eq!(find(&list, "mode").unwrap().name, "Mode");
        assert!(find(&list, "modem").is_none(), "mot entier seulement");
        assert!(find(&list, "").is_none());
        assert!(find(&list, "cuisine").is_none());
    }

    #[tokio::test]
    async fn execution_ordonnee_echec_non_bloquant_et_carte() {
        let f = fx();
        save(&f.db.lock(), vec![routine("Mode jeu", &[], vec![step("pc.volume", false), step("apps.launch", false), step("apps.close", false), step("pc.bluetooth", false)])], 1).unwrap();
        let ToolOutcome::Done { output, .. } = f.broker.call(call("routines.run", json!({"name": "lance le mode jeu"}))).await else { panic!() };
        assert_eq!(*f.log.lock(), ["pc.volume", "apps.launch", "pc.bluetooth"], "dans l'ordre, la carte ne s'exécute pas seule");
        assert_eq!(*f.cards.lock(), ["apps.close"], "carte de confirmation montrée");
        let st: Vec<&str> = output["steps"].as_array().unwrap().iter().map(|s| s["status"].as_str().unwrap()).collect();
        assert_eq!(st, ["done", "failed", "pending", "done"]);
        assert_eq!(output["pending"], 1);
        let summary = output["summary"].as_str().unwrap();
        assert!(summary.contains("2 étapes sur 4 faites") && summary.contains("Steam introuvable") && summary.contains("1 à confirmer"), "{summary}");
        // Les étapes sont journalisées comme des demandes de l'utilisateur.
        let j = f.broker.journal(10);
        assert!(j.iter().filter(|e| e.tool != "routines.run").all(|e| e.origin == Origin::User && e.provenance == Provenance::User));
    }

    #[tokio::test]
    async fn etape_bloquante_en_echec_arrete_la_suite() {
        let f = fx();
        let r = routine("r", &[], vec![step("apps.launch", true), step("pc.volume", false)]);
        let rep = run(&f.broker, &r, &|_: &PendingAction| {}).await;
        assert_eq!(rep.steps.iter().map(|s| s.status.clone()).collect::<Vec<_>>(), [StepStatus::Failed, StepStatus::Skipped]);
        assert!(f.log.lock().iter().all(|n| n != "pc.volume"));
        assert!(rep.summary.contains("1 non lancée"), "{}", rep.summary);
        // Une liste non vérifiée (hors base) ne lance jamais un outil hors liste blanche.
        let rep = run(&f.broker, &routine("r", &[], vec![step("files.run", false)]), &|_: &PendingAction| {}).await;
        assert_eq!(rep.steps[0].status, StepStatus::Failed);
        assert!(!f.log.lock().iter().any(|n| n == "files.run"));
    }

    #[tokio::test]
    async fn routine_inconnue_et_liste() {
        let f = fx();
        assert!(matches!(f.broker.call(call("routines.run", json!({"name": "nope"}))).await, ToolOutcome::Failed { .. }));
        save(&f.db.lock(), vec![routine("Mode jeu", &["on joue"], vec![step("pc.volume", false)])], 1).unwrap();
        let ToolOutcome::Done { output, .. } = f.broker.call(call("routines.list", json!({}))).await else { panic!() };
        assert_eq!(output, json!([{"name": "Mode jeu", "phrases": ["on joue"], "steps": ["Régler le son du PC"]}]));
    }
}
