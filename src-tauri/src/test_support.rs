//! Outils partagés par les tests.

use std::path::PathBuf;

/// Chemin d'un exemple (`examples/<name>.rs`) compilé par `cargo test` ou `cargo build --examples`.
/// `cargo build --examples` produit `examples/<name>.exe` ; `cargo test` seul ne garde parfois que
/// `examples/<name>-<hash>.exe` : on prend le nom simple, sinon le plus récent des noms hachés.
pub fn example_exe(name: &str) -> PathBuf {
    let dir = std::env::current_exe().unwrap().parent().unwrap().parent().unwrap().join("examples");
    let plain = dir.join(format!("{name}.exe"));
    if plain.is_file() {
        return plain;
    }
    let prefix = format!("{name}-");
    let hashed = std::fs::read_dir(&dir).into_iter().flatten().flatten()
        .filter(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            n.starts_with(&prefix) && n.ends_with(".exe")
        })
        .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok());
    match hashed {
        Some(e) => e.path(),
        None => {
            let seen: Vec<String> = std::fs::read_dir(&dir).into_iter().flatten().flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with(".exe")).collect();
            panic!("{name} absent de {} : lancer `cargo test` (qui compile les exemples), pas `--lib` seul. Trouvés : {seen:?}", dir.display());
        }
    }
}
