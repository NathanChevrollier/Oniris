//! Commandes Tauri des rappels et branchement sur l'ordonnanceur.
//!
//! Reprogrammation : les outils du cœur ne voient pas l'ordonnanceur, ils signalent `Topic::Reminders`.
//! `TauriEvents::changed` appelle alors `reschedule_reminders`, ce qui couvre création, cochage et
//! suppression (outil, UI ou undo) en un seul endroit.
//!
//! Veille du PC : aucune gestion de `WM_POWERBROADCAST`. L'ordonnanceur plafonne son sommeil à 60 s et
//! recalcule contre l'heure murale, donc un rappel échu pendant la veille part au plus ~60 s après la reprise.
//! Il est alors signalé « Rappel en retard » s'il a plus de 60 s de retard (`is_late`).
use crate::commands::dispatch_as_user;
use crate::state::AppState;
use crate::tray;
use chrono::Local;
use oniris_core::reminders::{self, is_late, Reminder};
use oniris_core::tools::ToolOutcome;
use serde_json::json;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

const KEY: &str = "reminders";

#[tauri::command(async)]
pub fn reminders_list(state: State<AppState>) -> Result<Vec<Reminder>, String> {
    reminders::list_today(&state.db.lock(), Local::now().timestamp_millis()).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn reminders_create(app: AppHandle, text: String) -> ToolOutcome {
    dispatch_as_user(&app, "reminders.create", json!({ "text": text })).await
}

#[tauri::command]
pub async fn reminders_complete(app: AppHandle, id: i64, done: bool) -> ToolOutcome {
    dispatch_as_user(&app, "reminders.complete", json!({ "id": id, "done": done })).await
}

#[tauri::command]
pub async fn reminders_delete(app: AppHandle, id: i64) -> ToolOutcome {
    dispatch_as_user(&app, "reminders.delete", json!({ "id": id })).await
}

/// Recalcule la prochaine échéance en base et la donne à l'ordonnanceur.
pub fn reschedule_reminders(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else { return };
    let Some(scheduler) = state.scheduler.get() else { return };
    let next = reminders::next_due(&state.db.lock());
    match next {
        Ok(next) => scheduler.set(KEY, next.and_then(|ms| u64::try_from(ms).ok())),
        Err(e) => tracing::error!("rappels : échéance illisible : {e}"),
    }
}

/// Marque et notifie les rappels échus. Aucun texte n'est journalisé : ids seulement.
fn fire_due(app: &AppHandle) {
    let now = Local::now().timestamp_millis();
    let due = match app.try_state::<AppState>().map(|s| reminders::take_due(&s.db.lock(), now)) {
        Some(Ok(d)) => d,
        Some(Err(e)) => { tracing::error!("rappels : lecture des échus impossible : {e}"); return; }
        None => return,
    };
    for r in &due {
        let late = r.due_ms.is_some_and(|d| is_late(d, now));
        tracing::info!("rappel {} notifié (en retard : {late})", r.id);
        tray::notify(app, if late { "Rappel en retard" } else { "Rappel" }, &r.text);
    }
    reschedule_reminders(app);
}

/// À appeler dans `setup`, ordonnanceur en place : gestionnaire d'abord, puis, sur un autre fil pour ne pas
/// retarder `setup`, rattrapage des rappels échus pendant l'absence (notifiés « en retard ») et première échéance.
pub fn start(app: &AppHandle) {
    let Some(scheduler) = app.state::<AppState>().scheduler.get().cloned() else { return };
    let h = app.clone();
    scheduler.on_due(KEY, Arc::new(move |_| fire_due(&h)));
    let h = app.clone();
    tauri::async_runtime::spawn_blocking(move || fire_due(&h));
}
