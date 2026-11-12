// Configuration de l'installeur NSIS : lue sans rien construire.
use serde_json::Value;

const CONF: &str = include_str!("../tauri.conf.json");

fn conf() -> Value {
    serde_json::from_str(CONF).unwrap()
}

#[test]
fn installeur_par_utilisateur_en_francais() {
    let c = conf();
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
    // Source de vérité : tauri.conf.json ; les trois autres fichiers doivent porter la même version.
    let c = conf();
    let v = c["version"].as_str().unwrap();
    assert_eq!(env!("CARGO_PKG_VERSION"), v, "src-tauri/Cargo.toml");
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let pkg: Value = serde_json::from_str(&std::fs::read_to_string(root.join("package.json")).unwrap()).unwrap();
    assert_eq!(pkg["version"], v, "package.json");
    let core = std::fs::read_to_string(root.join("crates/core/Cargo.toml")).unwrap();
    assert!(core.lines().any(|l| l.trim() == format!("version = \"{v}\"")), "crates/core/Cargo.toml");
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
