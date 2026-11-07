//! Le courtier : seul point de passage des actions. Règle de confirmation portée ici, jamais par un modèle.
use super::journal::{self, NewRow};
use super::*;
use crate::CoreError;
use serde_json::json;
use crate::Db;
use std::collections::HashMap;
use std::sync::Arc;

pub const PENDING_TTL_MS: u64 = 600_000;
pub type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;

pub struct ToolBroker {
    db: Db,
    clock: Clock,
    tools: HashMap<String, Arc<dyn Tool>>,
    /// Vrais arguments des actions en attente dont le journal n'a qu'une forme masquée (mémoire seulement :
    /// après un redémarrage, l'action approuvée échoue au lieu de s'exécuter avec le masque). `(expiration, arguments)` :
    /// purgé avec les actions expirées.
    masked: parking_lot::Mutex<HashMap<i64, (u64, Value)>>,
}

impl ToolBroker {
    /// Outils fixés à la construction. Deux outils du même nom : `CoreError::Invalid`, jamais de remplacement.
    pub fn new(db: Db, clock: Clock, tools: Vec<Arc<dyn Tool>>) -> Result<Self, CoreError> {
        let mut map = HashMap::new();
        for t in tools {
            let name = t.spec().name;
            if map.contains_key(&name) {
                return Err(CoreError::Invalid(format!("outil enregistré deux fois : {name}")));
            }
            map.insert(name, t);
        }
        Ok(Self { db, clock, tools: map, masked: Default::default() })
    }

    pub fn specs(&self) -> Vec<ToolSpec> {
        self.tools.values().map(|t| t.spec()).collect()
    }

    pub fn spec(&self, name: &str) -> Option<ToolSpec> {
        self.tools.get(name).map(|t| t.spec())
    }

    fn failed(&self, tool: &str, c: &ToolCall, e: ToolError) -> ToolOutcome {
        let err = e.to_string();
        let id = journal::insert(&self.db.lock(), NewRow {
            ts: (self.clock)(), tool, args: &c.args, origin: &c.origin, prov: &c.provenance,
            status: "failed", result: Some(&json!({ "error": err })), undo: None, expires: None,
        })
        .unwrap_or(-1);
        ToolOutcome::Failed { action_id: id, error: err }
    }

    pub async fn call(&self, call: ToolCall) -> ToolOutcome {
        let Some(tool) = self.tools.get(&call.tool) else {
            return self.failed(&call.tool, &call, ToolError::UnknownTool(call.tool.clone()));
        };
        if let Err(e) = tool.check(&call.args) {
            return self.failed(&call.tool, &call, e);
        }
        let confirm = tool.needs_confirm(&call.args)
            || matches!(call.provenance, Provenance::Untrusted { .. });
        let now = (self.clock)();
        let logged = tool.journal_args(&call.args);
        if confirm {
            let text = tool.describe(&call.args);
            let expires = now + PENDING_TTL_MS;
            let ins = journal::insert(&self.db.lock(), NewRow {
                ts: now, tool: &call.tool, args: &logged, origin: &call.origin, prov: &call.provenance,
                status: "pending", result: None, undo: None, expires: Some(expires),
            });
            return match ins {
                Ok(id) => {
                    if logged != call.args {
                        self.masked.lock().insert(id, (expires, call.args.clone()));
                    }
                    ToolOutcome::NeedsConfirmation(PendingAction {
                    action_id: id, tool: call.tool, text, provenance: call.provenance, expires_at_ms: expires,
                })
                }
                Err(e) => ToolOutcome::Failed { action_id: -1, error: e.to_string() },
            };
        }
        // Ligne `running` écrite AVANT `run` (journal fidèle même si l'app meurt), verrou relâché pendant `run`.
        let id = match journal::insert(&self.db.lock(), NewRow {
            ts: now, tool: &call.tool, args: &logged, origin: &call.origin, prov: &call.provenance,
            status: "running", result: None, undo: None, expires: None,
        }) {
            Ok(id) => id,
            Err(e) => return ToolOutcome::Failed { action_id: -1, error: e.to_string() },
        };
        self.finish_run(id, tool.spec().sensitive_output, tool.run(&call.args).await)
    }

    /// Journalise le résultat d'une action `running`. Une erreur de journal n'efface jamais un résultat obtenu.
    /// `sensitive` : le journal ne garde que le nombre d'éléments, l'appelant reçoit tout.
    fn finish_run(&self, id: i64, sensitive: bool, res: Result<ToolResult, ToolError>) -> ToolOutcome {
        let c = self.db.lock();
        match res {
            Ok(r) => {
                let logged = if sensitive {
                    json!({ "redacted": true, "count": r.output.as_array().map_or(1, Vec::len) })
                } else {
                    r.output.clone()
                };
                if let Err(e) = journal::finish(&c, id, "done", &logged, r.undo.as_ref()) {
                    tracing::error!("oniris: journal de l'action {id} non mis à jour : {e}");
                }
                ToolOutcome::Done { action_id: id, output: r.output }
            }
            Err(e) => {
                let err = e.to_string();
                if let Err(e) = journal::finish(&c, id, "failed", &json!({ "error": err }), None) {
                    tracing::error!("oniris: journal de l'action {id} non mis à jour : {e}");
                }
                ToolOutcome::Failed { action_id: id, error: err }
            }
        }
    }

    pub async fn resolve(&self, action_id: i64, approved: bool) -> ToolOutcome {
        self.expire_stale();
        let fail = |e: ToolError| ToolOutcome::Failed { action_id, error: e.to_string() };
        // Retirés quelle que soit l'issue : une action ne se résout qu'une fois.
        let masked = self.masked.lock().remove(&action_id).map(|(_, a)| a);
        let row = {
            let c = self.db.lock();
            let claimed = if approved {
                journal::claim(&c, action_id, (self.clock)())
            } else {
                journal::transition(&c, action_id, "pending", "rejected")
            };
            if !claimed.unwrap_or(false) {
                let e = if journal::status(&c, action_id).as_deref() == Some("expired") {
                    ToolError::Expired
                } else {
                    ToolError::NotPending
                };
                return fail(e);
            }
            journal::load(&c, action_id)
        };
        if !approved {
            return ToolOutcome::Rejected { action_id };
        }
        // Action `running` : elle ne repassera jamais par `pending`, donc `run` part au plus une fois.
        let Some(row) = row else { return fail(ToolError::NotPending) };
        let (sensitive, outcome) = match self.tools.get(&row.tool) {
            None => (false, Err(ToolError::UnknownTool(row.tool.clone()))),
            Some(t) => {
                let args = masked.as_ref().unwrap_or(&row.args);
                // L'état a pu changer depuis la carte : on refait la prévérification.
                (t.spec().sensitive_output, match t.check(args) { Ok(()) => t.run(args).await, Err(e) => Err(e) })
            }
        };
        self.finish_run(action_id, sensitive, outcome)
    }

    pub async fn undo(&self, action_id: i64) -> Result<(), ToolError> {
        let (name, payload) = {
            let c = self.db.lock();
            // Réclamation atomique : un second undo échoue ici.
            let (name, payload) = journal::load_undo(&c, action_id).ok_or(ToolError::NotUndoable)?;
            if !journal::transition(&c, action_id, "done", "undone").unwrap_or(false) {
                return Err(ToolError::NotUndoable);
            }
            (name, payload)
        };
        let res = match self.tools.get(&name) {
            Some(t) => t.undo(&payload).await,
            None => Err(ToolError::UnknownTool(name)),
        };
        if res.is_err() {
            let _ = journal::transition(&self.db.lock(), action_id, "undone", "done");
        }
        res
    }

    pub fn journal(&self, limit: u32) -> Vec<JournalEntry> {
        journal::list(&self.db.lock(), limit)
    }

    pub fn expire_stale(&self) {
        let now = (self.clock)();
        journal::expire(&self.db.lock(), now);
        self.masked.lock().retain(|_, (expires, _)| *expires >= now);
    }

    /// À appeler une fois au démarrage, avant tout appel d'outil.
    pub fn mark_interrupted(&self) {
        journal::mark_interrupted(&self.db.lock());
    }
}

#[cfg(test)]
mod tests {
    use crate::tools::*;
    use async_trait::async_trait;
    use parking_lot::Mutex;
    use serde_json::{json, Value};
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use std::sync::Arc;

    struct Echo { perm: Permission, undoable: bool, sensitive: bool, runs: Arc<AtomicUsize>, db: crate::Db, during: Arc<Mutex<Vec<String>>> }

    #[async_trait]
    impl Tool for Echo {
        fn spec(&self) -> ToolSpec {
            ToolSpec { name: "echo".into(), description: "echo".into(), args_schema: json!({}), permission: self.perm, sensitive_output: self.sensitive, untrusted_output: false }
        }
        fn describe(&self, a: &Value) -> ConfirmationText {
            ConfirmationText { summary: "Echo".into(), details: a.to_string(), risk: Risk::Low }
        }
        async fn run(&self, a: &Value) -> Result<ToolResult, ToolError> {
            self.runs.fetch_add(1, Ordering::SeqCst);
            let st: String = self.db.lock().query_row("SELECT status FROM actions ORDER BY id DESC LIMIT 1", [], |r| r.get(0)).unwrap();
            self.during.lock().push(st);
            Ok(ToolResult { output: a.clone(), undo: self.undoable.then(|| json!({"u": 1})) })
        }
        fn check(&self, a: &Value) -> Result<(), ToolError> {
            if a.get("bad").is_some() { Err(ToolError::Failed("refusé".into())) } else { Ok(()) }
        }
        fn needs_confirm(&self, a: &Value) -> bool {
            if a.get("safe").is_some() { a["safe"] != json!(true) } else { self.perm == Permission::Confirm }
        }
        fn journal_args(&self, a: &Value) -> Value {
            if a.get("secret").is_some() { json!({"redacted": true}) } else { a.clone() }
        }
        async fn undo(&self, _: &Value) -> Result<(), ToolError> {
            if self.undoable { Ok(()) } else { Err(ToolError::NotUndoable) }
        }
    }

    struct Fx { during: Arc<Mutex<Vec<String>>>, broker: ToolBroker, runs: Arc<AtomicUsize>, now: Arc<AtomicU64>, db: crate::Db }

    fn fx(perm: Permission, undoable: bool) -> Fx { fx_with(perm, undoable, false) }

    fn fx_with(perm: Permission, undoable: bool, sensitive: bool) -> Fx {
        let db = Arc::new(Mutex::new(crate::test_util::db()));
        let now = Arc::new(AtomicU64::new(0));
        let n = now.clone();
        let runs = Arc::new(AtomicUsize::new(0));
        let during = Arc::new(Mutex::new(vec![]));
        let echo = Arc::new(Echo { perm, undoable, sensitive, runs: runs.clone(), db: db.clone(), during: during.clone() });
        let broker = ToolBroker::new(db.clone(), Arc::new(move || n.load(Ordering::SeqCst)), vec![echo]).unwrap();
        Fx { during, broker, runs, now, db }
    }

    fn call(prov: Provenance) -> ToolCall {
        ToolCall { tool: "echo".into(), args: json!({"x": 1}), provenance: prov, origin: Origin::User }
    }
    fn status(f: &Fx, id: i64) -> String {
        f.db.lock().query_row("SELECT status FROM actions WHERE id=?", [id], |r| r.get(0)).unwrap()
    }
    fn pending(o: ToolOutcome) -> PendingAction {
        match o { ToolOutcome::NeedsConfirmation(p) => p, other => panic!("attendu NeedsConfirmation, reçu {other:?}") }
    }

    #[tokio::test]
    async fn outil_libre_demande_par_l_utilisateur_s_execute() {
        let f = fx(Permission::Free, false);
        let ToolOutcome::Done { action_id, output } = f.broker.call(call(Provenance::User)).await else { panic!() };
        assert_eq!(output, json!({"x": 1}));
        assert_eq!(status(&f, action_id), "done");
        assert_eq!(f.runs.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn sortie_sensible_masquee_dans_le_journal_mais_rendue_a_l_appelant() {
        for perm in [Permission::Free, Permission::Confirm] {
            let f = fx_with(perm, false, true);
            let o = f.broker.call(call(Provenance::User)).await;
            let o = if perm == Permission::Confirm { f.broker.resolve(pending(o).action_id, true).await } else { o };
            let ToolOutcome::Done { action_id, output } = o else { panic!() };
            assert_eq!(output, json!({"x": 1}), "l'appelant reçoit tout");
            let result: String = f.db.lock().query_row("SELECT result FROM actions WHERE id=?", [action_id], |r| r.get(0)).unwrap();
            assert_eq!(serde_json::from_str::<Value>(&result).unwrap(), json!({"redacted": true, "count": 1}));
        }
        let f = fx(Permission::Free, false);
        let ToolOutcome::Done { action_id, .. } = f.broker.call(call(Provenance::User)).await else { panic!() };
        let result: String = f.db.lock().query_row("SELECT result FROM actions WHERE id=?", [action_id], |r| r.get(0)).unwrap();
        assert_eq!(result, r#"{"x":1}"#, "outil ordinaire : journal complet");
    }

    #[tokio::test]
    async fn outil_a_confirmer_reste_en_attente() {
        let f = fx(Permission::Confirm, false);
        let p = pending(f.broker.call(call(Provenance::User)).await);
        assert_eq!(p.expires_at_ms, PENDING_TTL_MS);
        assert_eq!(f.runs.load(Ordering::SeqCst), 0);
        assert_eq!(status(&f, p.action_id), "pending");
    }

    #[tokio::test]
    async fn provenance_non_fiable_force_la_confirmation() {
        let f = fx(Permission::Free, false);
        pending(f.broker.call(call(Provenance::Untrusted { source: "mail:abc".into() })).await);
        assert_eq!(f.runs.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn refus_journalise_et_jamais_execute() {
        let f = fx(Permission::Confirm, false);
        let id = pending(f.broker.call(call(Provenance::User)).await).action_id;
        assert_eq!(f.broker.resolve(id, false).await, ToolOutcome::Rejected { action_id: id });
        assert_eq!(f.runs.load(Ordering::SeqCst), 0);
        assert_eq!(status(&f, id), "rejected");
    }

    #[tokio::test]
    async fn acceptation_execute_une_seule_fois() {
        let f = fx(Permission::Confirm, false);
        let id = pending(f.broker.call(call(Provenance::User)).await).action_id;
        assert!(matches!(f.broker.resolve(id, true).await, ToolOutcome::Done { .. }));
        assert_eq!(
            f.broker.resolve(id, true).await,
            ToolOutcome::Failed { action_id: id, error: ToolError::NotPending.to_string() }
        );
        assert_eq!(f.runs.load(Ordering::SeqCst), 1);
        assert_eq!(status(&f, id), "done");
    }

    #[tokio::test]
    async fn attente_expiree_ne_s_execute_jamais() {
        let f = fx(Permission::Confirm, false);
        let id = pending(f.broker.call(call(Provenance::User)).await).action_id;
        f.now.store(PENDING_TTL_MS + 1, Ordering::SeqCst);
        assert_eq!(
            f.broker.resolve(id, true).await,
            ToolOutcome::Failed { action_id: id, error: ToolError::Expired.to_string() }
        );
        assert_eq!(f.runs.load(Ordering::SeqCst), 0);
        assert_eq!(status(&f, id), "expired");
    }

    #[tokio::test]
    async fn arguments_masques_purges_a_l_expiration() {
        let f = fx(Permission::Confirm, false);
        let mut c = call(Provenance::User);
        c.args = json!({"secret": "s"});
        let id = pending(f.broker.call(c).await).action_id;
        assert_eq!(f.broker.masked.lock().len(), 1);
        f.broker.expire_stale();
        assert_eq!(f.broker.masked.lock().len(), 1, "encore valide");
        f.now.store(PENDING_TTL_MS + 1, Ordering::SeqCst);
        f.broker.expire_stale();
        assert!(f.broker.masked.lock().is_empty(), "expiré : purgé");
        assert_eq!(status(&f, id), "expired");
    }

    #[tokio::test]
    async fn expiration_au_redemarrage() {
        let f = fx(Permission::Confirm, false);
        let id = pending(f.broker.call(call(Provenance::User)).await).action_id;
        let n = Arc::new(AtomicU64::new(PENDING_TTL_MS + 1));
        let b2 = ToolBroker::new(f.db.clone(), Arc::new(move || n.load(Ordering::SeqCst)), vec![]).unwrap();
        b2.expire_stale();
        assert_eq!(status(&f, id), "expired");
    }

    #[tokio::test]
    async fn annulation() {
        let f = fx(Permission::Free, true);
        let ToolOutcome::Done { action_id, .. } = f.broker.call(call(Provenance::User)).await else { panic!() };
        assert!(f.broker.journal(10)[0].undoable);
        assert_eq!(f.broker.undo(action_id).await, Ok(()));
        assert_eq!(status(&f, action_id), "undone");
        assert_eq!(f.broker.undo(action_id).await, Err(ToolError::NotUndoable));
    }

    #[test]
    fn nom_d_outil_en_double_refuse() {
        let f = fx(Permission::Free, false);
        let echo = || -> Arc<dyn Tool> {
            Arc::new(Echo { perm: Permission::Free, undoable: false, sensitive: false, runs: f.runs.clone(), db: f.db.clone(), during: f.during.clone() })
        };
        let res = ToolBroker::new(f.db.clone(), Arc::new(|| 0), vec![echo(), echo()]);
        assert!(matches!(res, Err(crate::CoreError::Invalid(m)) if m.contains("echo")));
    }

    #[tokio::test]
    async fn outil_inconnu() {
        let f = fx(Permission::Free, false);
        let mut c = call(Provenance::User);
        c.tool = "nope".into();
        let ToolOutcome::Failed { action_id, error } = f.broker.call(c).await else { panic!() };
        assert_eq!(error, ToolError::UnknownTool("nope".into()).to_string());
        assert_eq!(status(&f, action_id), "failed");
    }

    #[tokio::test]
    async fn pendant_l_execution_le_journal_dit_running() {
        let f = fx(Permission::Free, false);
        let ToolOutcome::Done { action_id, .. } = f.broker.call(call(Provenance::User)).await else { panic!() };
        assert_eq!(*f.during.lock(), vec!["running".to_string()]);
        assert_eq!(status(&f, action_id), "done");
    }

    #[tokio::test]
    async fn action_approuvee_finit_done_jamais_failed() {
        let f = fx(Permission::Confirm, true);
        let id = pending(f.broker.call(call(Provenance::User)).await).action_id;
        assert!(matches!(f.broker.resolve(id, true).await, ToolOutcome::Done { .. }));
        assert_eq!(*f.during.lock(), vec!["running".to_string()]);
        assert_eq!(status(&f, id), "done");
        assert!(f.broker.journal(1)[0].undoable);
    }

    #[tokio::test]
    async fn ligne_running_devient_interrupted_au_redemarrage() {
        let f = fx(Permission::Free, false);
        f.db.lock().execute(
            "INSERT INTO actions(ts_ms, tool, args, origin, provenance, status) VALUES (0,'echo','{}','\"User\"','\"User\"','running')",
            [],
        ).unwrap();
        f.broker.mark_interrupted();
        assert_eq!(status(&f, 1), "interrupted");
    }

    #[tokio::test]
    async fn check_en_echec_journalise_failed_sans_carte_ni_execution() {
        let f = fx(Permission::Confirm, false);
        let mut c = call(Provenance::User);
        c.args = json!({"bad": 1});
        let ToolOutcome::Failed { action_id, error } = f.broker.call(c).await else { panic!() };
        assert!(error.contains("refusé"));
        assert_eq!(status(&f, action_id), "failed");
        assert_eq!(f.runs.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn confirmation_dynamique_et_provenance_non_fiable() {
        let f = fx(Permission::Confirm, false);
        let mut c = call(Provenance::User);
        c.args = json!({"safe": true});
        assert!(matches!(f.broker.call(c.clone()).await, ToolOutcome::Done { .. }));
        c.args = json!({"safe": false});
        pending(f.broker.call(c.clone()).await);
        c.args = json!({"safe": true});
        c.provenance = Provenance::Untrusted { source: "mail:1".into() };
        pending(f.broker.call(c).await);
        assert_eq!(f.runs.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn resolve_refait_check() {
        let f = fx(Permission::Confirm, false);
        let id = pending(f.broker.call(call(Provenance::User)).await).action_id;
        // L'état change entre la carte et la décision : on réécrit les arguments journalisés.
        f.db.lock().execute("UPDATE actions SET args='{\"bad\":1}' WHERE id=?", [id]).unwrap();
        assert!(matches!(f.broker.resolve(id, true).await, ToolOutcome::Failed { .. }));
        assert_eq!(f.runs.load(Ordering::SeqCst), 0);
        assert_eq!(status(&f, id), "failed");
    }

    #[test]
    fn spec_par_nom() {
        let f = fx(Permission::Free, false);
        assert!(f.broker.spec("echo").is_some() && f.broker.spec("nope").is_none());
    }
}
