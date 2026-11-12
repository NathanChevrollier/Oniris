// Configuration de l'installeur NSIS 1.0.0 : lue sans rien construire.
use serde_json::Value;

const CONF: &str = include_str!("../tauri.conf.json");

fn conf() -> Value {
    serde_json::from_str(CONF).unwrap()
}

#[test]
fn installeur_par_utilisateur_en_francais() {
    let c = conf();
    assert_eq!(c["version"], "1.0.0");
    let b = &c["bundle"];
    assert_eq!(b["targets"], serde_json::json!(["nsis"]));
    let n = &b["windows"]["nsis"];
    assert_eq!(n["installMode"], "currentUser");
    assert_eq!(n["languages"], serde_json::json!(["French"]));
    assert_eq!(n["displayLanguageSelector"], false);
    assert_eq!(n["compression"], "lzma");
    assert_eq!(b["copyright"], "© 2026 Oniris");
    assert!(b["shortDescription"].is_string() && b["longDescription"].is_string());
}

#[test]
fn les_versions_concordent() {
    let v = env!("CARGO_PKG_VERSION");
    assert_eq!(v, "1.0.0");
    assert_eq!(conf()["version"], v);
}

#[test]
fn la_desinstallation_retire_run_et_garde_les_donnees() {
    let c = conf();
    let rel = c["bundle"]["windows"]["nsis"]["installerHooks"].as_str().unwrap();
    let hooks = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)).unwrap();
    assert!(hooks.contains("NSIS_HOOK_POSTUNINSTALL"));
    assert!(hooks.contains(r#"DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "Oniris""#));
    let lower = hooks.to_lowercase();
    assert!(!lower.contains("rmdir") && !lower.contains("appdata"));
}

#[test]
fn wake_models_are_bundled() {
    assert!(conf()["bundle"]["resources"].as_array().unwrap().iter().any(|r| r == "resources/wake/*"));
}
