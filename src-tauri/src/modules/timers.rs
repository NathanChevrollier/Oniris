//! Commandes Tauri des minuteurs et branchement sur l'ordonnanceur.
//!
//! Reprogrammation : les outils du cœur signalent `Topic::Timers`, `TauriEvents::changed` appelle alors
//! `reschedule_timers` (création, annulation, undo ; l'UI se rafraîchit par le même événement).
//! Aucun libellé n'est journalisé : ids seulement.
use crate::commands::dispatch_as_user;
use crate::state::AppState;
use crate::tray;
use chrono::Local;
use oniris_core::events::Topic;
use oniris_core::timers::{self, Timer};
use oniris_core::tools::ToolOutcome;
use serde_json::json;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

const KEY: &str = "timers";

#[tauri::command(async)]
pub fn timers_list(state: State<AppState>) -> Result<Vec<Timer>, String> {
    timers::running(&state.db.lock()).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn timers_start(app: AppHandle, text: String) -> ToolOutcome {
    dispatch_as_user(&app, "timer.start", json!({ "text": text })).await
}

#[tauri::command]
pub async fn focus_start(app: AppHandle) -> ToolOutcome {
    dispatch_as_user(&app, "focus.start", json!({})).await
}

#[tauri::command]
pub async fn timers_cancel(app: AppHandle, id: i64) -> ToolOutcome {
    dispatch_as_user(&app, "timer.cancel", json!({ "id": id })).await
}

/// Recalcule la prochaine fin en base et la donne à l'ordonnanceur.
pub fn reschedule_timers(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else { return };
    let Some(scheduler) = state.scheduler.get() else { return };
    let next = timers::next_end(&state.db.lock());
    match next {
        Ok(next) => scheduler.set(KEY, next.and_then(|ms| u64::try_from(ms).ok())),
        Err(e) => tracing::error!("minuteurs : échéance illisible : {e}"),
    }
}

/// Termine les minuteurs échus : notification, et pause automatique après un focus.
fn fire_due(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else { return };
    let now = Local::now().timestamp_millis();
    let done = match timers::take_finished(&state.db.lock(), now) {
        Ok(d) => d,
        Err(e) => { tracing::error!("minuteurs : lecture des échus impossible : {e}"); return; }
    };
    let break_min = state.settings.read().break_minutes;
    for t in &done {
        tracing::info!("minuteur {} terminé ({})", t.id, t.kind);
        let (title, body) = match t.kind.as_str() {
            "focus" => {
                if let Err(e) = timers::after_finish(&state.db.lock(), t, i64::from(break_min) * 60_000, now) {
                    tracing::error!("minuteurs : pause impossible : {e}");
                }
                ("Focus", format!("Focus terminé — pause de {break_min} min"))
            }
            "break" => ("Focus", "Pause terminée".to_string()),
            _ => ("Minuteur", format!("{} terminé", t.label)),
        };
        tray::notify(app, title, &body);
    }
    state.events.changed(Topic::Timers); // reprogramme et rafraîchit l'UI
}

/// À appeler dans `setup`, ordonnanceur en place : gestionnaire d'abord, puis rattrapage des minuteurs
/// échus pendant l'absence (une notification chacun, jamais de pause automatique), puis première échéance.
pub fn start(app: &AppHandle) {
    let Some(scheduler) = app.state::<AppState>().scheduler.get().cloned() else { return };
    let h = app.clone();
    scheduler.on_due(KEY, Arc::new(move |_| fire_due(&h)));
    let state = app.state::<AppState>();
    match timers::take_finished(&state.db.lock(), Local::now().timestamp_millis()) {
        Ok(missed) => for t in &missed {
            tracing::info!("minuteur {} terminé pendant l'absence", t.id);
            tray::notify(app, "Minuteur", &format!("Terminé pendant ton absence : {}", t.label));
        },
        Err(e) => tracing::error!("minuteurs : rattrapage impossible : {e}"),
    }
    state.events.changed(Topic::Timers);
}
