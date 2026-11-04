//! Commandes Tauri de l'agenda. Lecture directe de la base ; création et suppression passent par le courtier.
use crate::commands::dispatch_tool_call;
use crate::state::AppState;
use chrono::{Local, NaiveDate, TimeZone};
use oniris_core::agenda::{self, Event};
use oniris_core::tools::{Origin, Provenance, ToolCall, ToolOutcome};
use serde_json::json;
use tauri::{AppHandle, State};

#[tauri::command]
pub fn agenda_between(state: State<AppState>, from_ms: i64, to_ms: i64) -> Result<Vec<Event>, String> {
    agenda::between(&state.db.lock(), from_ms, to_ms).map_err(|e| e.to_string())
}

/// Début du mois (heure locale) en ms.
fn month_start_ms(year: i32, month: u32) -> Option<i64> {
    let d = NaiveDate::from_ymd_opt(year, month, 1)?.and_hms_opt(0, 0, 0)?;
    Some(Local.from_local_datetime(&d).earliest()?.timestamp_millis())
}

#[tauri::command]
pub fn agenda_month_days(state: State<AppState>, year: i32, month: u32) -> Result<Vec<u32>, String> {
    let (ny, nm) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
    let (Some(from), Some(to)) = (month_start_ms(year, month), month_start_ms(ny, nm)) else {
        return Err("mois invalide".into());
    };
    agenda::days_with_events(&state.db.lock(), from, to).map_err(|e| e.to_string())
}

async fn as_user(app: &AppHandle, tool: &str, args: serde_json::Value) -> ToolOutcome {
    dispatch_tool_call(app, ToolCall { tool: tool.into(), args, provenance: Provenance::User, origin: Origin::User }).await
}

#[tauri::command]
pub async fn agenda_create(app: AppHandle, text: String) -> ToolOutcome {
    as_user(&app, "agenda.create_event", json!({ "text": text })).await
}

/// Passe par la confirmation : la suppression n'a lieu qu'après l'accord dans la carte du panneau.
#[tauri::command]
pub async fn agenda_delete(app: AppHandle, id: i64) -> ToolOutcome {
    as_user(&app, "agenda.delete_event", json!({ "id": id })).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn debut_de_mois() {
        assert!(month_start_ms(2026, 11).unwrap() < month_start_ms(2026, 12).unwrap());
        assert!(month_start_ms(2026, 13).is_none());
    }
}
