//! Routines : commandes des Réglages, entrées de la recherche (« Routine : mode jeu ») et liaison de
//! `routines.run` au courtier (ses étapes y repassent comme des demandes de l'utilisateur).
use crate::commands::{dispatch_as_user, show_confirm};
use crate::state::AppState;
use oniris_core::routines::{self, BrokerCell, OnPending, Routine};
use oniris_core::search::{Item, ItemKind};
use oniris_core::time_util::now_ms;
use oniris_core::tools::{PendingAction, Tool, ToolBroker, ToolOutcome};
use oniris_core::Db;
use parking_lot::RwLock;
use serde_json::{json, Value};
use std::sync::{Arc, LazyLock, OnceLock};
use tauri::AppHandle;

/// Carte de confirmation, posée par `bind` (hors des tests : sinon toute la pile de fenêtrage serait liée au binaire de test).
static SHOW: OnceLock<OnPending> = OnceLock::new();
static BROKER: LazyLock<BrokerCell> = LazyLock::new(Default::default);
/// Entrées de recherche, reconstruites à chaque enregistrement (la frappe ne lit jamais la base).
static ITEMS: RwLock<Option<Arc<Vec<Item>>>> = parking_lot::const_rwlock(None);

pub fn tools(db: &Db) -> Vec<Arc<dyn Tool>> {
    let on_pending = Arc::new(|p: &PendingAction| {
        if let Some(show) = SHOW.get() {
            show(p);
        }
    });
    routines::all(db, &BROKER, on_pending)
}

/// Appelé par `AppState::open` une fois le courtier construit.
pub fn bind(app: &AppHandle, broker: &Arc<ToolBroker>, db: &Db) {
    *ITEMS.write() = Some(Arc::new(to_items(&routines::load(&db.lock()))));
    let app = app.clone();
    let _ = SHOW.set(Arc::new(move |p: &PendingAction| show_confirm(&app, p)));
    let _ = BROKER.set(Arc::downgrade(broker));
}

fn to_items(list: &[Routine]) -> Vec<Item> {
    list.iter()
        .map(|r| Item {
            kind: ItemKind::Routine,
            key: r.name.clone(),
            title: format!("Routine : {}", r.name),
            subtitle: r.steps.iter().map(|s| routines::label(&s.tool)).collect::<Vec<_>>().join(" · "),
        })
        .collect()
}

/// Entrées de la recherche (cache rempli par `bind` puis à chaque enregistrement).
pub fn items() -> Arc<Vec<Item>> {
    ITEMS.read().clone().unwrap_or_default()
}

#[tauri::command]
pub async fn routines_list(state: tauri::State<'_, AppState>) -> Result<Vec<Routine>, String> {
    Ok(routines::load(&state.db.lock()))
}

/// Remplace la liste entière (validée : bornes, liste blanche, noms uniques).
#[tauri::command]
pub async fn routines_save(state: tauri::State<'_, AppState>, routines: Vec<Routine>) -> Result<Vec<Routine>, String> {
    let saved = routines::save(&state.db.lock(), routines, now_ms())?;
    *ITEMS.write() = Some(Arc::new(to_items(&saved)));
    Ok(saved)
}

/// Bouton « Lancer » : même chemin qu'IRIS, journalisé comme une demande de l'utilisateur ; rend le rapport.
#[tauri::command]
pub async fn routines_run(app: AppHandle, name: String) -> Result<Value, String> {
    match dispatch_as_user(&app, "routines.run", json!({ "name": name })).await {
        ToolOutcome::Done { output, .. } => Ok(output),
        ToolOutcome::NeedsConfirmation(_) => Ok(json!({ "pending": 1 })),
        ToolOutcome::Rejected { .. } => Err("action refusée".into()),
        ToolOutcome::Failed { error, .. } => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oniris_core::routines::Step;

    #[test]
    fn entrees_de_recherche() {
        let r = Routine { name: "Mode jeu".into(), phrases: vec![], steps: vec![Step { tool: "pc.volume".into(), args: json!({"percent": 40}), blocking: false }, Step { tool: "apps.launch".into(), args: json!({"key": "Steam"}), blocking: false }] };
        let i = &to_items(&[r])[0];
        assert_eq!((i.kind, i.key.as_str(), i.title.as_str()), (ItemKind::Routine, "Mode jeu", "Routine : Mode jeu"));
        assert_eq!(i.subtitle, "Régler le son du PC · Lancer une application");
    }

    /// Miroir de la liste blanche dans l'interface (`src/modules/routines/tools.ts`).
    #[test]
    fn liste_blanche_synchronisee_avec_l_interface() {
        let ts = include_str!("../../../src/modules/routines/tools.ts");
        for (tool, label) in routines::ALLOWED {
            assert!(ts.contains(&format!("\"{tool}\": {{ label: \"{label}\"")), "{tool} absent ou libellé différent dans tools.ts");
        }
        assert_eq!(ts.matches("{ label: \"").count(), routines::ALLOWED.len());
    }
}
