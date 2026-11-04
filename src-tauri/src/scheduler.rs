use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::AppHandle;
use tokio::sync::Notify;

type Handler = Arc<dyn Fn(&AppHandle) + Send + Sync>;

/// Plus petite échéance : `(clé, at_ms)`. Logique pure.
pub fn next_due(entries: &HashMap<&'static str, u64>) -> Option<(&'static str, u64)> {
    entries.iter().map(|(k, v)| (*k, *v)).min_by_key(|&(_, v)| v)
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

#[derive(Default)]
struct Shared {
    entries: Mutex<HashMap<&'static str, u64>>,
    notify: Notify,
}

/// Boucle unique. Aucun réveil périodique : sans échéance, elle n'attend que `notify`.
/// Les échéances sont en heure murale (ms) et le délai de sommeil est recalculé à chaque réveil :
/// après une mise en veille du PC, tout ce qui est échu part au réveil suivant, sans dérive cumulée.
/// Chaque clé échue est signalée une fois, par ordre d'échéance, hors de tout verrou ;
/// le rappel doit appeler `set` pour sa clé s'il a une prochaine échéance.
async fn run_loop(shared: Arc<Shared>, fire: impl Fn(&'static str)) {
    loop {
        let notified = shared.notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable(); // un `set` entre le calcul et l'attente ne sera pas perdu
        let now = now_ms();
        let mut due: Vec<(&'static str, u64)> = {
            let mut e = shared.entries.lock();
            let due: Vec<_> = e.iter().filter(|(_, &at)| at <= now).map(|(k, v)| (*k, *v)).collect();
            for (k, _) in &due { e.remove(k); }
            due
        };
        if !due.is_empty() {
            due.sort_by_key(|&(_, at)| at);
            for (k, _) in due { fire(k); }
            continue;
        }
        let next = next_due(&shared.entries.lock());
        match next {
            None => notified.await,
            Some((_, at)) => {
                tokio::select! {
                    () = tokio::time::sleep(Duration::from_millis(at.saturating_sub(now))) => {}
                    () = notified => {}
                }
            }
        }
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
            if let Some(h) = h { h(&app); }
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

    #[allow(dead_code)] // appelé par les modules à partir de la tâche 3
    pub fn on_due(&self, key: &'static str, f: Handler) {
        self.handlers.lock().insert(key, f);
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
        tokio::spawn(run_loop(shared.clone(), move |k| f.lock().push(k)));
        let now = now_ms();
        {
            let mut e = shared.entries.lock();
            e.insert("late", now + 80);
            e.insert("x", now + 30);
        }
        shared.notify.notify_one();
        tokio::time::sleep(Duration::from_millis(400)).await;
        assert_eq!(*fired.lock(), vec!["x", "late"]);
    }
}
