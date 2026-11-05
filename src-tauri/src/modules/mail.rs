//! Relève des mails Gmail en tâche de fond : une par compte `ok` toutes les `mail_poll_minutes`, délai croissant
//! après un échec, rien du tout sans compte (la tâche dort jusqu'à `wake`). Journal : ids et nombres seulement.
use crate::state::AppState;
use oniris_core::accounts::{self, Status};
use oniris_core::events::Topic;
use oniris_core::mail::gmail;
use oniris_core::mail::sync::{self, SyncError};
use oniris_core::oauth::Provider;
use oniris_core::time_util::now_ms;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::LazyLock;
use tauri::{AppHandle, Manager};
use tokio::sync::Notify;

/// Réveille la boucle (compte ajouté, retiré, reconnecté ; ouverture du panneau).
static WAKE: LazyLock<Notify> = LazyLock::new(Notify::new);
/// Relève demandée par l'ouverture du panneau.
static FORCE: AtomicBool = AtomicBool::new(false);
/// Au moins un compte Gmail `ok` : sinon l'ouverture du panneau ne réveille rien.
static HAS_ACCOUNTS: AtomicBool = AtomicBool::new(false);
/// Dernière relève lancée (ms), en mémoire : l'ouverture du panneau ne lit pas la base.
static LAST_RUN_MS: AtomicI64 = AtomicI64::new(0);
/// Ouverture du panneau : relève immédiate si la dernière date de plus de 60 s.
const STALE_MS: i64 = 60_000;

/// À appeler dans `setup`, état géré.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move { run(app).await });
}

pub fn wake() { WAKE.notify_one(); }

/// Appelé par `windows::open_panel` (fil principal) : ne verrouille pas la base.
pub fn on_panel_open(_app: &AppHandle) {
    if HAS_ACCOUNTS.load(Ordering::SeqCst) && now_ms() - LAST_RUN_MS.load(Ordering::SeqCst) > STALE_MS {
        FORCE.store(true, Ordering::SeqCst);
        WAKE.notify_one();
    }
}

/// Échéance et échecs consécutifs de chaque compte.
#[derive(Default)]
struct Schedule(HashMap<i64, (i64, u32)>);

impl Schedule {
    /// Garde les comptes relevables ; un nouveau est dû tout de suite.
    fn retain(&mut self, ids: &[i64], now: i64) {
        self.0.retain(|id, _| ids.contains(id));
        for id in ids {
            self.0.entry(*id).or_insert((now, 0));
        }
    }

    /// Comptes à relever. `force` avance les échéances normales, jamais un délai d'échec.
    fn due(&self, now: i64, force: bool) -> Vec<i64> {
        let mut ids: Vec<i64> = self.0.iter().filter(|(_, &(at, fails))| at <= now || (force && fails == 0)).map(|(id, _)| *id).collect();
        ids.sort_unstable();
        ids
    }

    fn ok(&mut self, id: i64, now: i64, interval_ms: i64) { self.0.insert(id, (now + interval_ms, 0)); }

    /// Renvoie la fin du délai d'attente.
    fn fail(&mut self, id: i64, now: i64) -> i64 {
        let fails = self.0.get(&id).map_or(0, |e| e.1) + 1;
        let until = now + sync::backoff_ms(fails);
        self.0.insert(id, (until, fails));
        until
    }

    fn next(&self) -> Option<i64> { self.0.values().map(|e| e.0).min() }
}

fn gmail_ok(app: &AppHandle) -> Vec<i64> {
    let st = app.state::<AppState>();
    let list = accounts::list(&st.db.lock());
    match list {
        Ok(l) => l.into_iter().filter(|a| a.provider == Provider::Google && a.status == Status::Ok).map(|a| a.id).collect(),
        Err(e) => {
            tracing::warn!("mails : comptes illisibles : {e}");
            Vec::new()
        }
    }
}

async fn run(app: AppHandle) {
    let st = app.state::<AppState>();
    let client = gmail::Client::new(st.account_http.clone());
    let mut sched = Schedule::default();
    loop {
        let ids = gmail_ok(&app);
        HAS_ACCOUNTS.store(!ids.is_empty(), Ordering::SeqCst);
        sched.retain(&ids, now_ms());
        if ids.is_empty() {
            FORCE.store(false, Ordering::SeqCst);
            WAKE.notified().await;
            continue;
        }
        let force = FORCE.swap(false, Ordering::SeqCst);
        for id in sched.due(now_ms(), force) {
            let now = now_ms();
            LAST_RUN_MS.store(now, Ordering::SeqCst);
            match sync::run(&st.db, &client, &st.accounts, id, now).await {
                Ok(report) => {
                    let minutes = st.settings.read().mail_poll_minutes as i64;
                    sched.ok(id, now, minutes * 60_000);
                    if report.any() {
                        st.events.changed(Topic::Mail);
                    }
                }
                // Le compte est passé à `reauth` : il sort de la liste au tour suivant.
                Err(SyncError::Revoked) => {}
                Err(_) => {
                    let until = sched.fail(id, now);
                    let r = sync::set_backoff(&st.db.lock(), id, Some(until));
                    if let Err(e) = r {
                        tracing::warn!("mails : compte {id} : délai non noté : {e}");
                    }
                }
            }
        }
        let wait = sched.next().map_or(0, |at| (at - now_ms()).max(0)) as u64;
        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_millis(wait)) => {}
            _ = WAKE.notified() => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: i64 = 60_000;

    #[test]
    fn echeances_et_delai_croissant() {
        let mut s = Schedule::default();
        s.retain(&[1, 2], 0);
        assert_eq!(s.due(0, false), [1, 2], "nouveaux comptes : tout de suite");
        s.ok(1, 0, 3 * MIN);
        s.fail(2, 0);
        assert_eq!(s.fail(2, MIN), 3 * MIN, "deux erreurs réseau : 2 minutes");
        assert_eq!(s.next(), Some(3 * MIN));
        assert!(s.due(2 * MIN, false).is_empty());
        assert_eq!(s.due(2 * MIN, true), [1], "le panneau avance la relève, pas le délai d'échec");
        assert_eq!(s.due(3 * MIN, false), [1, 2]);
        for _ in 0..5 { s.fail(2, 0); }
        assert_eq!(s.0[&2].0, 15 * MIN, "15 minutes au plus");
        s.ok(2, 0, 3 * MIN);
        assert_eq!(s.0[&2], (3 * MIN, 0), "un succès remet à zéro");
        s.retain(&[1], 0);
        assert_eq!(s.due(10 * MIN, true), [1], "compte retiré ou à reconnecter : plus de relève");
        s.retain(&[], 0);
        assert_eq!(s.next(), None);
    }
}
