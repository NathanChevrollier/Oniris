use crate::state::AppState;
use oniris_core::time_util::now_ms;
use oniris_core::CoreError;
use parking_lot::Mutex;
use rusqlite::Connection;
use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tokio::sync::Notify;

type Handler = Arc<dyn Fn(&AppHandle) + Send + Sync>;

/// Plus petite échéance : `(clé, at_ms)`. Logique pure.
pub fn next_due(entries: &HashMap<&'static str, u64>) -> Option<(&'static str, u64)> {
    entries.iter().map(|(k, v)| (*k, *v)).min_by_key(|&(_, v)| v)
}

#[derive(Default)]
struct Shared {
    entries: Mutex<HashMap<&'static str, u64>>,
    notify: Notify,
}

/// Plafond d'un sommeil tant qu'une échéance existe : l'horloge de tokio peut ne pas compter
/// la mise en veille du PC, on recalcule donc contre l'heure murale au plus tard après ce délai.
const MAX_SLEEP_MS: u64 = 60_000;

/// Boucle unique. Sans échéance, elle n'attend que `notify` (aucun réveil). Avec échéance, elle dort
/// `min(échéance - maintenant, MAX_SLEEP_MS)` puis recalcule contre l'heure murale (ms) : après une veille,
/// le retard est borné à `MAX_SLEEP_MS`. Chaque clé échue est signalée une fois, par ordre d'échéance,
/// hors de tout verrou ; le rappel doit appeler `set` pour sa clé s'il a une prochaine échéance.
/// `fire` renvoie `false` s'il n'y a pas de gestionnaire : l'entrée reste en file (« parquée »,
/// sans boucler) jusqu'au prochain réveil. Un rappel qui panique est journalisé
/// (clé seulement) et la boucle continue : un gestionnaire doit être court et lancer tout travail lourd.
async fn run_loop(shared: Arc<Shared>, fire: impl Fn(&'static str) -> bool) {
    let mut parked: Vec<&'static str> = Vec::new();
    loop {
        let notified = shared.notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable(); // un `set` entre le calcul et l'attente ne sera pas perdu
        let now = now_ms() as u64;
        let (mut due, next) = {
            let mut e = shared.entries.lock();
            let live: HashMap<_, _> = e.iter().filter(|(k, _)| !parked.contains(*k)).map(|(k, v)| (*k, *v)).collect();
            let due: Vec<_> = live.iter().filter(|(_, &at)| at <= now).map(|(k, v)| (*k, *v)).collect();
            for (k, _) in &due { e.remove(k); }
            (due, next_due(&live).map(|(_, at)| at))
        };
        if !due.is_empty() {
            due.sort_by_key(|&(_, at)| at);
            for (k, at) in due {
                let handled = std::panic::catch_unwind(AssertUnwindSafe(|| fire(k))).unwrap_or_else(|_| {
                    tracing::error!("ordonnanceur : le rappel de « {k} » a paniqué");
                    true
                });
                if !handled {
                    tracing::warn!("ordonnanceur : aucun gestionnaire pour « {k} », en attente");
                    shared.entries.lock().entry(k).or_insert(at);
                    parked.push(k);
                }
            }
            continue;
        }
        match next {
            None => notified.await,
            Some(at) => {
                let wait = at.saturating_sub(now).min(MAX_SLEEP_MS);
                tokio::select! {
                    () = tokio::time::sleep(Duration::from_millis(wait)) => {}
                    () = notified => {}
                }
            }
        }
        // Réveil par `notify` (set / on_due) ou fin de sommeil : on réessaie les clés parquées.
        parked.clear();
    }
}

pub struct Scheduler {
    shared: Arc<Shared>,
    handlers: Arc<Mutex<HashMap<&'static str, Handler>>>,
}

impl Scheduler {
    pub fn start(app: AppHandle) -> Arc<Scheduler> {
        let s = Arc::new(Scheduler { shared: Arc::default(), handlers: Arc::default() });
        let (shared, handlers) = (s.shared.clone(), s.handlers.clone());
        tauri::async_runtime::spawn(run_loop(shared, move |key| {
            let h = handlers.lock().get(key).cloned();
            h.map(|h| h(&app)).is_some()
        }));
        s
    }

    /// Remplace l'échéance de `key` (`None` la retire) et réveille la tâche.
    #[allow(dead_code)] // appelé par les modules à partir de la tâche 3
    pub fn set(&self, key: &'static str, at_ms: Option<u64>) {
        {
            let mut e = self.shared.entries.lock();
            match at_ms {
                Some(at) => { e.insert(key, at); }
                None => { e.remove(key); }
            }
        }
        self.shared.notify.notify_one();
    }

    /// Le gestionnaire doit être court et lancer tout travail lourd (il bloque la boucle).
    #[allow(dead_code)] // appelé par les modules à partir de la tâche 3
    pub fn on_due(&self, key: &'static str, f: Handler) {
        self.handlers.lock().insert(key, f);
        self.shared.notify.notify_one(); // rejoue une échéance parquée faute de gestionnaire
    }
}

/// Lecture en base de la prochaine échéance d'une clé (ms), `None` s'il n'y en a pas.
pub type NextFn = fn(&Connection) -> Result<Option<i64>, CoreError>;

/// Recalcule en base la prochaine échéance de `key` et la donne à l'ordonnanceur.
pub fn reschedule(app: &AppHandle, key: &'static str, next_fn: NextFn) {
    let Some(state) = app.try_state::<AppState>() else { return };
    let Some(scheduler) = state.scheduler.get() else { return };
    let next = next_fn(&state.db.lock());
    match next {
        Ok(next) => scheduler.set(key, next.and_then(|ms| u64::try_from(ms).ok())),
        Err(e) => tracing::error!("ordonnanceur : échéance de « {key} » illisible : {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_due_empty_and_min() {
        let mut m = HashMap::new();
        assert_eq!(next_due(&m), None);
        m.insert("a", 500);
        m.insert("b", 200);
        assert_eq!(next_due(&m), Some(("b", 200)));
    }

    #[tokio::test]
    async fn fires_once_in_order_when_due() {
        let shared = Arc::new(Shared::default());
        let fired = Arc::new(Mutex::new(Vec::new()));
        let f = fired.clone();
        tokio::spawn(run_loop(shared.clone(), move |k| { f.lock().push(k); true }));
        let now = now_ms() as u64;
        {
            let mut e = shared.entries.lock();
            e.insert("late", now + 80);
            e.insert("x", now + 30);
        }
        shared.notify.notify_one();
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert_eq!(*fired.lock(), vec!["x", "late"]);
    }

    type Handlers = Arc<Mutex<HashMap<&'static str, Box<dyn Fn() + Send + Sync>>>>;
    fn spawn_with(shared: &Arc<Shared>, h: &Handlers) {
        let h = h.clone();
        tokio::spawn(run_loop(shared.clone(), move |k| match h.lock().get(k) {
            Some(f) => { f(); true }
            None => false,
        }));
    }

    #[tokio::test]
    async fn due_without_handler_waits_then_fires_on_registration() {
        let shared = Arc::new(Shared::default());
        let h: Handlers = Arc::default();
        spawn_with(&shared, &h);
        shared.entries.lock().insert("k", now_ms() as u64);
        shared.notify.notify_one();
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(shared.entries.lock().contains_key("k"));
        let hits = Arc::new(Mutex::new(0));
        let c = hits.clone();
        h.lock().insert("k", Box::new(move || *c.lock() += 1));
        shared.notify.notify_one();
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(*hits.lock(), 1);
        assert!(!shared.entries.lock().contains_key("k"));
    }

    #[tokio::test]
    async fn panicking_handler_does_not_stop_later_deadlines() {
        let shared = Arc::new(Shared::default());
        let h: Handlers = Arc::default();
        let hits = Arc::new(Mutex::new(0));
        let c = hits.clone();
        h.lock().insert("boom", Box::new(|| panic!("test")));
        h.lock().insert("ok", Box::new(move || *c.lock() += 1));
        spawn_with(&shared, &h);
        let now = now_ms() as u64;
        shared.entries.lock().insert("boom", now + 20);
        shared.entries.lock().insert("ok", now + 80);
        shared.notify.notify_one();
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert_eq!(*hits.lock(), 1);
    }
}
