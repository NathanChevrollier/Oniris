//! Le courtier : seul point de passage des actions. Règle de confirmation portée ici, jamais par un modèle.
use super::journal::{self, NewRow};
use super::*;
use serde_json::json;
use parking_lot::Mutex;
use rusqlite::Connection;
use std::collections::HashMap;
use std::sync::Arc;

pub const PENDING_TTL_MS: u64 = 600_000;
pub type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;

pub struct ToolBroker {
    db: Arc<Mutex<Connection>>,
    clock: Clock,
    tools: HashMap<String, Arc<dyn Tool>>,
}

impl ToolBroker {
    pub fn new(db: Arc<Mutex<Connection>>, clock: Clock) -> Self {
        Self { db, clock, tools: HashMap::new() }
    }

    pub fn register(&mut self, tool: Arc<dyn Tool>) {
        self.tools.insert(tool.spec().name, tool);
    }

    pub fn specs(&self) -> Vec<ToolSpec> {
        self.tools.values().map(|t| t.spec()).collect()
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
        let confirm = tool.spec().permission == Permission::Confirm
            || matches!(call.provenance, Provenance::Untrusted { .. });
        let now = (self.clock)();
        if confirm {
            let text = tool.describe(&call.args);
            let expires = now + PENDING_TTL_MS;
            let ins = journal::insert(&self.db.lock(), NewRow {
                ts: now, tool: &call.tool, args: &call.args, origin: &call.origin, prov: &call.provenance,
                status: "pending", result: None, undo: None, expires: Some(expires),
            });
            return match ins {
                Ok(id) => ToolOutcome::NeedsConfirmation(PendingAction {
                    action_id: id, tool: call.tool, text, provenance: call.provenance, expires_at_ms: expires,
                }),
                Err(e) => ToolOutcome::Failed { action_id: -1, error: e.to_string() },
            };
        }
        // Verrou relâché pendant `run`, puis repris pour journaliser.
        let res = tool.run(&call.args).await;
        match res {
            Ok(r) => {
                let ins = journal::insert(&self.db.lock(), NewRow {
                    ts: now, tool: &call.tool, args: &call.args, origin: &call.origin, prov: &call.provenance,
                    status: "done", result: Some(&r.output), undo: r.undo.as_ref(), expires: None,
                });
                match ins {
                    Ok(id) => ToolOutcome::Done { action_id: id, output: r.output },
                    Err(e) => ToolOutcome::Failed { action_id: -1, error: e.to_string() },
                }
            }
            Err(e) => self.failed(&call.tool, &call, e),
        }
    }

    pub async fn resolve(&self, action_id: i64, approved: bool) -> ToolOutcome {
        self.expire_stale();
        let fail = |e: ToolError| ToolOutcome::Failed { action_id, error: e.to_string() };
        let row = {
            let c = self.db.lock();
            let claimed = if approved {
                journal::claim(&c, action_id)
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
        // Action réclamée : elle ne repassera jamais par `pending`, donc `run` part au plus une fois.
        let Some(row) = row else { return fail(ToolError::NotPending) };
        let outcome = match self.tools.get(&row.tool) {
            None => Err(ToolError::UnknownTool(row.tool.clone())),
            Some(t) => t.run(&row.args).await,
        };
        let c = self.db.lock();
        match outcome {
            Ok(r) => {
                let _ = journal::finish(&c, action_id, "done", &r.output, r.undo.as_ref());
                ToolOutcome::Done { action_id, output: r.output }
            }
            Err(e) => {
                let _ = journal::finish(&c, action_id, "failed", &json!({ "error": e.to_string() }), None);
                fail(e)
            }
        }
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
        journal::expire(&self.db.lock(), (self.clock)());
    }
}

#[cfg(test)]
mod tests {
    use crate::tools::*;
    use crate::{migrate, open_db, CORE_MIGRATIONS};
    use async_trait::async_trait;
    use parking_lot::Mutex;
    use rusqlite::Connection;
    use serde_json::{json, Value};
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    use std::sync::Arc;

    struct Echo { perm: Permission, undoable: bool, runs: Arc<AtomicUsize> }

    #[async_trait]
    impl Tool for Echo {
        fn spec(&self) -> ToolSpec {
            ToolSpec { name: "echo".into(), description: "echo".into(), args_schema: json!({}), permission: self.perm }
        }
        fn describe(&self, a: &Value) -> ConfirmationText {
            ConfirmationText { summary: "Echo".into(), details: a.to_string(), risk: Risk::Low }
        }
        async fn run(&self, a: &Value) -> Result<ToolResult, ToolError> {
            self.runs.fetch_add(1, Ordering::SeqCst);
            Ok(ToolResult { output: a.clone(), undo: self.undoable.then(|| json!({"u": 1})) })
        }
        async fn undo(&self, _: &Value) -> Result<(), ToolError> {
            if self.undoable { Ok(()) } else { Err(ToolError::NotUndoable) }
        }
    }

    struct Fx { broker: ToolBroker, runs: Arc<AtomicUsize>, now: Arc<AtomicU64>, db: Arc<Mutex<Connection>>, _dir: tempfile::TempDir }

    fn fx(perm: Permission, undoable: bool) -> Fx {
        let dir = tempfile::tempdir().unwrap();
        let mut c = open_db(&dir.path().join("t.db")).unwrap();
        migrate(&mut c, CORE_MIGRATIONS).unwrap();
        let db = Arc::new(Mutex::new(c));
        let now = Arc::new(AtomicU64::new(0));
        let n = now.clone();
        let mut broker = ToolBroker::new(db.clone(), Arc::new(move || n.load(Ordering::SeqCst)));
        let runs = Arc::new(AtomicUsize::new(0));
        broker.register(Arc::new(Echo { perm, undoable, runs: runs.clone() }));
        Fx { broker, runs, now, db, _dir: dir }
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
    async fn expiration_au_redemarrage() {
        let f = fx(Permission::Confirm, false);
        let id = pending(f.broker.call(call(Provenance::User)).await).action_id;
        let n = Arc::new(AtomicU64::new(PENDING_TTL_MS + 1));
        let b2 = ToolBroker::new(f.db.clone(), Arc::new(move || n.load(Ordering::SeqCst)));
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

    #[tokio::test]
    async fn outil_inconnu() {
        let f = fx(Permission::Free, false);
        let mut c = call(Provenance::User);
        c.tool = "nope".into();
        let ToolOutcome::Failed { action_id, error } = f.broker.call(c).await else { panic!() };
        assert_eq!(error, ToolError::UnknownTool("nope".into()).to_string());
        assert_eq!(status(&f, action_id), "failed");
    }
}
