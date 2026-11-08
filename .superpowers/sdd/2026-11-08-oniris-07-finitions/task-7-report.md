# Task 7 - NSIS 1.0.0 (configuration seulement, aucun build)
- tauri.conf.json : version 1.0.0, bundle.publisher "Oniris", copyright, short/longDescription FR, windows.nsis {installMode currentUser, languages [French], displayLanguageSelector false, compression lzma, installerHooks nsis/hooks.nsh}. Cles verifiees dans config.schema.json.
- src-tauri/nsis/hooks.nsh : NSIS_HOOK_POSTUNINSTALL -> DeleteRegValue HKCU ...\Run "Oniris" (tauri-plugin-autostart utilise package_info().name = productName). Aucune suppression des dossiers de donnees (laisses, pas de question).
- Versions 1.0.0 : tauri.conf.json, package.json, src-tauri/Cargo.toml, crates/core/Cargo.toml, Cargo.lock.
- CSP inchangee ; iframes mail en sandbox/srcdoc (MailReader.tsx) non touches.
- Test : src-tauri/tests/nsis_config.rs (3 tests).
- RED : 3 failed (version 0.1.0, pas de bloc nsis). GREEN : cargo test --workspace tout ok (3 passed), clippy -D warnings propre.
- pnpm test/build non relances (JS inchange hors version).
