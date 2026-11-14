//! Outils `pc.*` : son, Bluetooth, Gestionnaire des tâches (libres) et alimentation (confirmation).
//! Le vrai contrôle (Core Audio, WinRT, Win32) vit dans src-tauri.
use crate::tools::helpers::{bad, failed, low, parse, spec};
use crate::tools::{ConfirmationText, Permission, Risk, Tool, ToolError, ToolResult, ToolSpec};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Power {
    Lock,
    Sleep,
    Restart,
    Shutdown,
}

/// Volume maître du périphérique de sortie par défaut, en pourcentage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Volume {
    pub percent: u8,
    pub muted: bool,
}

pub trait PcControl: Send + Sync {
    fn volume(&self) -> Result<Volume, String>;
    fn set_volume(&self, percent: u8) -> Result<(), String>;
    fn set_mute(&self, muted: bool) -> Result<(), String>;
    /// `None` : aucun adaptateur Bluetooth.
    fn bluetooth(&self) -> Result<Option<bool>, String>;
    fn set_bluetooth(&self, on: bool) -> Result<(), String>;
    fn task_manager(&self) -> Result<(), String>;
    fn power(&self, action: Power) -> Result<(), String>;
}

type Pc = Arc<dyn PcControl>;

/// Les appels Windows peuvent attendre (WinRT, COM) : hors du worker async.
async fn blocking<T: Send + 'static>(pc: &Pc, f: impl FnOnce(&dyn PcControl) -> Result<T, String> + Send + 'static) -> Result<T, ToolError> {
    let pc = pc.clone();
    tokio::task::spawn_blocking(move || f(pc.as_ref())).await.map_err(failed)?.map_err(failed)
}

pub struct VolumeTool(pub Pc);
pub struct BluetoothTool(pub Pc);
pub struct TaskManagerTool(pub Pc);
pub struct PowerTool(pub Pc);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VolumeArgs {
    percent: Option<i64>,
    delta: Option<i64>,
    muted: Option<bool>,
}

#[async_trait]
impl Tool for VolumeTool {
    fn spec(&self) -> ToolSpec {
        spec("pc.volume", "Son du PC (sortie par défaut) : `percent` (0 à 100) ou `delta` (relatif), et/ou `muted` (couper ou rétablir). Sans argument : lit l'état.",
            json!({"type":"object","properties":{"percent":{"type":"integer","minimum":0,"maximum":100},"delta":{"type":"integer","minimum":-100,"maximum":100},"muted":{"type":"boolean"}}}), Permission::Free)
    }
    fn check(&self, a: &Value) -> Result<(), ToolError> {
        let v: VolumeArgs = parse(a)?;
        match (v.percent, v.delta) {
            (Some(_), Some(_)) => Err(bad("donne `percent` ou `delta`, pas les deux")),
            (Some(p), _) if !(0..=100).contains(&p) => Err(bad("`percent` : 0 à 100")),
            (_, Some(d)) if !(-100..=100).contains(&d) => Err(bad("`delta` : -100 à 100")),
            _ => Ok(()),
        }
    }
    fn describe(&self, _: &Value) -> ConfirmationText { low("Régler le son du PC") }
    async fn run(&self, a: &Value) -> Result<ToolResult, ToolError> {
        let VolumeArgs { percent, delta, muted } = parse(a)?;
        let v = blocking(&self.0, move |pc| {
            let cur = pc.volume()?;
            let target = percent.or(delta.map(|d| cur.percent as i64 + d)).map(|p| p.clamp(0, 100) as u8);
            if let Some(p) = target {
                pc.set_volume(p)?;
            }
            // Régler un niveau rétablit le son, comme le curseur de Windows.
            if let Some(m) = muted.or(target.map(|_| false)) {
                pc.set_mute(m)?;
            }
            pc.volume()
        }).await?;
        Ok(ToolResult { output: json!(v), undo: None })
    }
}

#[derive(Deserialize)]
struct OnArgs {
    on: bool,
}

#[async_trait]
impl Tool for BluetoothTool {
    fn spec(&self) -> ToolSpec {
        spec("pc.bluetooth", "Active (`on`: true) ou désactive (`on`: false) le Bluetooth du PC.",
            json!({"type":"object","properties":{"on":{"type":"boolean"}},"required":["on"]}), Permission::Free)
    }
    fn check(&self, a: &Value) -> Result<(), ToolError> { parse::<OnArgs>(a).map(|_| ()) }
    fn describe(&self, _: &Value) -> ConfirmationText { low("Basculer le Bluetooth") }
    async fn run(&self, a: &Value) -> Result<ToolResult, ToolError> {
        let OnArgs { on } = parse(a)?;
        blocking(&self.0, move |pc| pc.set_bluetooth(on)).await?;
        Ok(ToolResult { output: json!({"on": on}), undo: None })
    }
}

#[async_trait]
impl Tool for TaskManagerTool {
    fn spec(&self) -> ToolSpec {
        spec("pc.task_manager", "Ouvre le Gestionnaire des tâches de Windows.", json!({"type":"object","properties":{}}), Permission::Free)
    }
    fn describe(&self, _: &Value) -> ConfirmationText { low("Ouvrir le Gestionnaire des tâches") }
    async fn run(&self, _: &Value) -> Result<ToolResult, ToolError> {
        blocking(&self.0, |pc| pc.task_manager()).await?;
        Ok(ToolResult { output: json!({"opened": true}), undo: None })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PowerArgs {
    action: Power,
}

fn power_args(a: &Value) -> Result<Power, ToolError> {
    match parse::<PowerArgs>(a)?.action {
        Power::Lock => Err(bad("action : sleep, restart ou shutdown")),
        p => Ok(p),
    }
}

#[async_trait]
impl Tool for PowerTool {
    fn spec(&self) -> ToolSpec {
        spec("pc.power", "Met le PC en veille (`sleep`), le redémarre (`restart`) ou l'éteint (`shutdown`) — confirmation requise.",
            json!({"type":"object","properties":{"action":{"type":"string","enum":["sleep","restart","shutdown"]}},"required":["action"]}), Permission::Confirm)
    }
    fn check(&self, a: &Value) -> Result<(), ToolError> { power_args(a).map(|_| ()) }
    fn describe(&self, a: &Value) -> ConfirmationText {
        let (summary, risk) = match power_args(a) {
            Ok(Power::Sleep) => ("Mettre le PC en veille", Risk::Medium),
            Ok(Power::Restart) => ("Redémarrer le PC", Risk::High),
            _ => ("Éteindre le PC", Risk::High),
        };
        ConfirmationText { summary: summary.into(), details: "Le travail non enregistré dans les applications ouvertes peut être perdu.".into(), risk }
    }
    async fn run(&self, a: &Value) -> Result<ToolResult, ToolError> {
        let p = power_args(a)?;
        blocking(&self.0, move |pc| pc.power(p)).await?;
        Ok(ToolResult { output: json!({"done": true}), undo: None })
    }
}

pub fn all(pc: &Pc) -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(VolumeTool(pc.clone())),
        Arc::new(BluetoothTool(pc.clone())),
        Arc::new(TaskManagerTool(pc.clone())),
        Arc::new(PowerTool(pc.clone())),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::db;
    use crate::tools::{Origin, Provenance, ToolBroker, ToolCall, ToolOutcome};
    use parking_lot::Mutex;

    /// Faux PC : rien ne touche la vraie machine.
    struct Fake {
        vol: Mutex<Volume>,
        bt: Mutex<bool>,
        log: Mutex<Vec<String>>,
    }
    impl PcControl for Fake {
        fn volume(&self) -> Result<Volume, String> { Ok(*self.vol.lock()) }
        fn set_volume(&self, p: u8) -> Result<(), String> { self.vol.lock().percent = p; Ok(()) }
        fn set_mute(&self, m: bool) -> Result<(), String> { self.vol.lock().muted = m; Ok(()) }
        fn bluetooth(&self) -> Result<Option<bool>, String> { Ok(Some(*self.bt.lock())) }
        fn set_bluetooth(&self, on: bool) -> Result<(), String> { *self.bt.lock() = on; Ok(()) }
        fn task_manager(&self) -> Result<(), String> { self.log.lock().push("taskmgr".into()); Ok(()) }
        fn power(&self, a: Power) -> Result<(), String> { self.log.lock().push(format!("{a:?}")); Ok(()) }
    }

    fn setup() -> (Arc<Fake>, ToolBroker) {
        let fake = Arc::new(Fake { vol: Mutex::new(Volume { percent: 50, muted: true }), bt: Mutex::new(false), log: Mutex::default() });
        let pc: Pc = fake.clone();
        (fake, ToolBroker::new(Arc::new(Mutex::new(db())), Arc::new(|| 1_000), all(&pc)).unwrap())
    }

    fn call(tool: &str, args: Value) -> ToolCall {
        ToolCall { tool: tool.into(), args, provenance: Provenance::User, origin: Origin::User }
    }

    #[tokio::test]
    async fn volume_muet_et_bluetooth_sans_carte() {
        let (fake, b) = setup();
        let ToolOutcome::Done { output, .. } = b.call(call("pc.volume", json!({"percent": 30}))).await else { panic!() };
        assert_eq!(output, json!({"percent": 30, "muted": false}));
        b.call(call("pc.volume", json!({"delta": 90}))).await;
        assert_eq!(fake.vol.lock().percent, 100);
        b.call(call("pc.volume", json!({"muted": true}))).await;
        assert_eq!(*fake.vol.lock(), Volume { percent: 100, muted: true });
        for bad in [json!({"percent": 101}), json!({"percent": 5, "delta": 1}), json!({"volume": 3})] {
            assert!(matches!(b.call(call("pc.volume", bad.clone())).await, ToolOutcome::Failed { .. }), "{bad}");
        }
        assert!(matches!(b.call(call("pc.bluetooth", json!({"on": true}))).await, ToolOutcome::Done { .. }));
        assert!(*fake.bt.lock());
        assert!(matches!(b.call(call("pc.task_manager", json!({}))).await, ToolOutcome::Done { .. }));
        assert_eq!(*fake.log.lock(), ["taskmgr"]);
    }

    #[tokio::test]
    async fn alimentation_sur_carte_seulement() {
        let (fake, b) = setup();
        let ToolOutcome::NeedsConfirmation(p) = b.call(call("pc.power", json!({"action": "shutdown"}))).await else { panic!() };
        assert_eq!(p.text.summary, "Éteindre le PC");
        assert!(fake.log.lock().is_empty(), "rien avant l'approbation");
        assert!(matches!(b.resolve(p.action_id, false).await, ToolOutcome::Rejected { .. }));
        assert!(fake.log.lock().is_empty());
        let ToolOutcome::NeedsConfirmation(p) = b.call(call("pc.power", json!({"action": "restart"}))).await else { panic!() };
        assert!(matches!(b.resolve(p.action_id, true).await, ToolOutcome::Done { .. }));
        assert_eq!(*fake.log.lock(), ["Restart"]);
        assert!(matches!(b.call(call("pc.power", json!({"action": "lock"}))).await, ToolOutcome::Failed { .. }));
    }
}
