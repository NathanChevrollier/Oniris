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
        "agenda_between",
        "agenda_month_days",
        "agenda_create",
        "agenda_delete",
        "reminders_list",
        "reminders_create",
        "reminders_complete",
        "reminders_delete",
        "notes_list",
        "notes_create",
        "notes_pin",
        "notes_delete",
        "notes_to_reminder",
        "timers_list",
        "timers_start",
        "focus_start",
        "timers_cancel",
        "perf_frame",
        "perf_open_cycles",
    ]);
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(manifest))
        .expect("échec de tauri-build");
}
