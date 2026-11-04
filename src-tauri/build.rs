fn main() {
    // Déclarer les commandes soumet chacune à l'ACL : une fenêtre n'appelle que ce que sa capacité autorise.
    // Les commandes perf_* ne sont enregistrées qu'en debug ou avec la feature `perf`.
    let manifest = tauri_build::AppManifest::new().commands(&[
        "panel_open",
        "panel_close",
        "panel_toggle",
        "pill_resize",
        "settings_get",
        "settings_save",
        "tool_resolve",
        "tool_undo",
        "journal_list",
        "perf_frame",
        "perf_open_cycles",
    ]);
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(manifest))
        .expect("échec de tauri-build");
}
