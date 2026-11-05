//! Relève des mails Gmail en tâche de fond : une par compte `ok` toutes les `mail_poll_minutes`, délai croissant
//! après un échec, rien du tout sans compte (la tâche dort jusqu'à `wake`). Journal : ids et nombres seulement.
use crate::state::AppState;
use oniris_core::accounts::{self, Status};
use oniris_core::events::Topic;
use crate::commands::dispatch_as_user;
use oniris_core::mail::gmail;
use oniris_core::mail::tools::{self as mail_tools, MailRow};
use oniris_core::tools::ToolOutcome;
use serde_json::json;
use oniris_core::mail::sync::{self, SyncError, SyncReport};
use oniris_core::oauth::Provider;
use oniris_core::time_util::now_ms;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::LazyLock;
use tauri::{AppHandle, Manager};
use tokio::sync::Notify;

/// Réveille la boucle (compte ajouté, retiré, reconnecté ; ouverture du panneau).
static WAKE: LazyLock<Notify> = LazyLock::new(Notify::new);
/// Relève demandée par l'ouverture du panneau.
static FORCE: AtomicBool = AtomicBool::new(false);
/// Dernière relève lancée de chaque compte relevable (ms), recopiée par la boucle : l'ouverture du panneau
/// la lit sans toucher la base. Vide : aucun compte, l'ouverture ne réveille rien.
static LAST_RUNS: Mutex<Vec<i64>> = Mutex::new(Vec::new());
/// Ouverture du panneau : relève immédiate des comptes relevés il y a plus de 60 s.
const STALE_MS: i64 = 60_000;
/// Base illisible : nouvel essai dans une minute.
const DB_RETRY: std::time::Duration = std::time::Duration::from_secs(60);

/// À appeler dans `setup`, état géré.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move { run(app).await });
}

pub fn wake() { WAKE.notify_one(); }

/// Appelé par `windows::open_panel` (fil principal) : ne verrouille pas la base.
pub fn on_panel_open(_app: &AppHandle) {
    let now = now_ms();
    if LAST_RUNS.lock().iter().any(|&t| now - t > STALE_MS) {
        FORCE.store(true, Ordering::SeqCst);
        WAKE.notify_one();
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Entry { due: i64, fails: u32, last_run: i64 }

/// Échéance, échecs consécutifs et dernière relève de chaque compte.
#[derive(Default)]
struct Schedule(HashMap<i64, Entry>);

impl Schedule {
    /// Garde les comptes relevables ; un nouveau est dû tout de suite.
    fn retain(&mut self, ids: &[i64], now: i64) {
        self.0.retain(|id, _| ids.contains(id));
        for id in ids {
            self.0.entry(*id).or_insert(Entry { due: now, fails: 0, last_run: 0 });
        }
    }

    /// Comptes à relever. `force` avance l'échéance des comptes relevés il y a plus de 60 s, jamais un délai d'échec.
    fn due(&self, now: i64, force: bool) -> Vec<i64> {
        let mut ids: Vec<i64> = self.0.iter()
            .filter(|(_, e)| e.due <= now || (force && e.fails == 0 && now - e.last_run > STALE_MS))
            .map(|(id, _)| *id)
            .collect();
        ids.sort_unstable();
        ids
    }

    fn started(&mut self, id: i64, now: i64) {
        if let Some(e) = self.0.get_mut(&id) {
            e.last_run = now;
        }
    }

    fn ok(&mut self, id: i64, now: i64, interval_ms: i64) {
        if let Some(e) = self.0.get_mut(&id) {
            (e.due, e.fails) = (now + interval_ms, 0);
        }
    }

    /// Renvoie la fin du délai d'attente.
    fn fail(&mut self, id: i64, now: i64) -> i64 {
        let e = self.0.entry(id).or_insert(Entry { due: now, fails: 0, last_run: now });
        e.fails += 1;
        e.due = now + sync::backoff_ms(e.fails);
        e.due
    }

    fn next(&self) -> Option<i64> { self.0.values().map(|e| e.due).min() }

    fn last_runs(&self) -> Vec<i64> { self.0.values().map(|e| e.last_run).collect() }
}

/// Applique le résultat d'une relève (`None` : la tâche a planté) ; renvoie (mails changés, délai à noter en base).
fn settle(sched: &mut Schedule, id: i64, now: i64, result: Option<Result<SyncReport, SyncError>>, interval_ms: i64) -> (bool, Option<i64>) {
    match result {
        Some(Ok(report)) => {
            sched.ok(id, now, interval_ms);
            (report.any(), None)
        }
        // Le compte est passé à `reauth` et sort de la liste au tour suivant. Si le statut n'a pas pu être
        // noté, le délai d'échec évite de boucler.
        Some(Err(SyncError::Revoked)) => {
            sched.fail(id, now);
            (false, None)
        }
        // Erreur ou plantage : délai croissant, sinon le compte resterait dû et la boucle le relancerait aussitôt.
        Some(Err(_)) | None => (false, Some(sched.fail(id, now))),
    }
}

fn gmail_ok(app: &AppHandle) -> Result<Vec<i64>, oniris_core::CoreError> {
    let list = accounts::list(&app.state::<AppState>().db.lock())?;
    Ok(list.into_iter().filter(|a| a.provider == Provider::Google && a.status == Status::Ok).map(|a| a.id).collect())
}

async fn run(app: AppHandle) {
    let client = gmail::Client::new(app.state::<AppState>().account_http.clone());
    let mut sched = Schedule::default();
    loop {
        let ids = match gmail_ok(&app) {
            Ok(ids) => ids,
            Err(e) => {
                tracing::warn!("mails : comptes illisibles, nouvel essai dans une minute : {e}");
                tokio::time::sleep(DB_RETRY).await;
                continue;
            }
        };
        sched.retain(&ids, now_ms());
        if ids.is_empty() {
            LAST_RUNS.lock().clear();
            FORCE.store(false, Ordering::SeqCst);
            WAKE.notified().await;
            continue;
        }
        let force = FORCE.swap(false, Ordering::SeqCst);
        // Comptes dus en même temps relevés ensemble : un compte lent ne retarde pas les autres.
        let mut set = tokio::task::JoinSet::new();
        // Compte de chaque tâche, gardé hors de la tâche : un plantage ne le perd pas.
        let mut tasks = HashMap::new();
        for id in sched.due(now_ms(), force) {
            let now = now_ms();
            sched.started(id, now);
            let (app, client) = (app.clone(), client.clone());
            let handle = set.spawn(async move {
                let st = app.state::<AppState>();
                sync::run(&st.db, &client, &st.accounts, id, now).await
            });
            tasks.insert(handle.id(), (id, now));
        }
        *LAST_RUNS.lock() = sched.last_runs();
        let st = app.state::<AppState>();
        let mut changed = false;
        while let Some(done) = set.join_next_with_id().await {
            let (task, result) = match done {
                Ok((task, r)) => (task, Some(r)),
                Err(e) => (e.id(), None),
            };
            let Some((id, now)) = tasks.remove(&task) else { continue };
            if result.is_none() {
                tracing::error!("mails : compte {id} : la relève a planté");
            }
            let interval = st.settings.read().mail_poll_minutes as i64 * 60_000;
            let (any, backoff) = settle(&mut sched, id, now, result, interval);
            changed |= any;
            if let Some(until) = backoff {
                let r = sync::set_backoff(&st.db.lock(), id, Some(until));
                if let Err(e) = r {
                    tracing::warn!("mails : compte {id} : délai non noté : {e}");
                }
            }
        }
        if changed {
            st.events.changed(Topic::Mail);
        }
        let wait = sched.next().map_or(0, |at| (at - now_ms()).max(0)) as u64;
        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_millis(wait)) => {}
            _ = WAKE.notified() => {}
        }
    }
}

// Commandes de la carte Mails. La liste lit le cache (base seule, sans journal) ; tout le reste passe par le courtier.

#[tauri::command]
pub async fn mail_list(state: tauri::State<'_, AppState>, account: Option<i64>, bucket: Option<String>, limit: Option<u32>) -> Result<Vec<MailRow>, String> {
    mail_tools::list(&state.db.lock(), account, bucket.as_deref(), limit.unwrap_or(mail_tools::LIST_MAX)).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn mail_read(app: AppHandle, account: i64, id: String) -> ToolOutcome {
    dispatch_as_user(&app, "mail.read", json!({ "account": account, "id": id })).await
}

#[tauri::command]
pub async fn mail_set_read(app: AppHandle, account: i64, id: String, read: bool) -> ToolOutcome {
    dispatch_as_user(&app, "mail.mark_read", json!({ "account": account, "id": id, "read": read })).await
}

#[tauri::command]
pub async fn mail_archive(app: AppHandle, account: i64, id: String) -> ToolOutcome {
    dispatch_as_user(&app, "mail.archive", json!({ "account": account, "id": id })).await
}

#[tauri::command]
pub async fn mail_delete(app: AppHandle, account: i64, id: String) -> ToolOutcome {
    dispatch_as_user(&app, "mail.delete", json!({ "account": account, "id": id })).await
}

#[tauri::command]
pub async fn mail_send(app: AppHandle, account: i64, to: String, subject: String, body: String, reply_to_id: Option<String>) -> ToolOutcome {
    dispatch_as_user(&app, "mail.send", json!({ "account": account, "to": to, "subject": subject, "body": body, "reply_to_id": reply_to_id })).await
}

#[tauri::command]
pub async fn mail_open_web(app: AppHandle, account: i64, id: String) -> ToolOutcome {
    dispatch_as_user(&app, "mail.open_web", json!({ "account": account, "id": id })).await
}

/// Lien cliqué par l'utilisateur dans un mail affiché.
#[tauri::command]
pub async fn mail_open_link(app: AppHandle, url: String) -> ToolOutcome {
    dispatch_as_user(&app, "mail.open_link", json!({ "url": url })).await
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
        s.started(1, 0);
        s.ok(1, 0, 3 * MIN);
        s.started(2, 0);
        s.fail(2, 0);
        assert_eq!(s.fail(2, MIN), 3 * MIN, "deux erreurs réseau : 2 minutes");
        assert_eq!(s.next(), Some(3 * MIN));
        assert!(s.due(2 * MIN, false).is_empty());
        assert_eq!(s.due(2 * MIN, true), [1], "le panneau avance la relève, pas le délai d'échec");
        assert!(s.due(MIN / 2, true).is_empty(), "relevé il y a moins de 60 s : rien à forcer");
        assert_eq!(s.due(3 * MIN, false), [1, 2]);
        for _ in 0..5 {
            s.fail(2, 0);
        }
        assert_eq!(s.0[&2].due, 15 * MIN, "15 minutes au plus");
        s.ok(2, 0, 3 * MIN);
        assert_eq!((s.0[&2].due, s.0[&2].fails), (3 * MIN, 0), "un succès remet à zéro");
        s.retain(&[1], 0);
        assert_eq!(s.due(10 * MIN, true), [1], "compte retiré ou à reconnecter : plus de relève");
        assert_eq!(s.last_runs(), [0]);
        s.retain(&[], 0);
        assert_eq!(s.next(), None);
    }

    #[test]
    fn fraicheur_par_compte() {
        let mut s = Schedule::default();
        s.retain(&[1, 2], 0);
        s.started(1, 10 * MIN);
        s.ok(1, 10 * MIN, 3 * MIN);
        s.started(2, 0);
        s.ok(2, 0, 30 * MIN);
        assert_eq!(s.due(10 * MIN + 1, true), [2], "seul le compte relevé il y a plus de 60 s est forcé");
    }

    #[test]
    fn revoque_sans_statut_ne_boucle_pas() {
        let mut s = Schedule::default();
        s.retain(&[1], 0);
        s.started(1, 0);
        s.fail(1, 0);
        assert!(s.due(1, true).is_empty(), "repoussé, même à l'ouverture du panneau");
    }

    #[test]
    fn un_plantage_applique_le_delai_croissant() {
        let mut s = Schedule::default();
        s.retain(&[1], 0);
        s.started(1, 0);
        assert_eq!(settle(&mut s, 1, 0, None, 3 * MIN), (false, Some(MIN)), "plantage : délai d'une minute noté");
        assert!(s.due(1, false).is_empty(), "plus dû tout de suite : pas de boucle");
        assert_eq!(s.next(), Some(MIN));
        assert_eq!(settle(&mut s, 1, MIN, None, 3 * MIN), (false, Some(3 * MIN)), "second plantage : 2 minutes");
        let ok = SyncReport { added: 1, removed: 0, changed: 0 };
        assert_eq!(settle(&mut s, 1, 0, Some(Ok(ok)), 3 * MIN), (true, None));
        assert_eq!(s.0[&1].fails, 0);
        assert_eq!(settle(&mut s, 1, 0, Some(Ok(SyncReport::default())), 3 * MIN), (false, None), "rien de changé : pas d'événement");
        assert_eq!(settle(&mut s, 1, 0, Some(Err(SyncError::Network)), 3 * MIN), (false, Some(MIN)));
        assert_eq!(settle(&mut s, 1, 0, Some(Err(SyncError::Revoked)), 3 * MIN), (false, None));
        assert!(s.due(1, true).is_empty());
    }
}
