//! Outils partagés par les tests.

use std::path::PathBuf;

/// Chemin d'un exemple (`examples/<name>.rs`), compilé par `cargo test` dans `<cible>/<profil>/examples/`.
/// On remonte depuis le binaire de test : selon la version de Cargo, il est dans `<profil>/deps/` ou plus
/// profond (`<profil>/build/<paquet>/<hash>/out/` depuis Rust 1.100).
pub fn example_exe(name: &str) -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let file = format!("{name}.exe");
    exe.ancestors()
        .map(|d| d.join("examples").join(&file))
        .find(|p| p.is_file())
        .unwrap_or_else(|| panic!("{file} introuvable au-dessus de {} : lancer `cargo test` (qui compile les exemples), pas `--lib` seul", exe.display()))
}
