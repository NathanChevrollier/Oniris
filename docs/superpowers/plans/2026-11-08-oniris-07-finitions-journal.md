# SDD ledger — plan: docs/superpowers/plans/2026-11-08-oniris-07-finitions.md

Spec reachable. Branch plan-07-finitions from main 4a5bdd6. Local signed commits authorised, no push, NO release build (needs user approval).

## Pre-flight scan
| Tasks | Shared file / interface | Finding |
|---|---|---|
| T1 | lib.rs setup/Exit, settings load, db | independent |
| T2 → T3 | useSave moved to src/lib/useSave.ts; Header data-sheet buttons | consistent (T2 first) |
| T2 ↔ T3 | tray.rs: T2 resync menu checks, T3 Autostart trait | same file, sequential tasks — consistent |
| T2 ↔ plan-3 projects | projects_hide(…, false) exists? | T2 verifies; adds unhide if missing |
| T4 | Panel/Pill/palette CSS + eslint | after T2/T3 so lint covers new code — consistent |
| T5 | many modules (search, zenytt, music, iris, voice, Panel focus) | disjoint from T4 except Panel.tsx (focusHeaderButton) — sequential, OK |
| T6 | tools/perf only | independent |
| T7 | tauri.conf.json, versions | independent |
| T8 | docs | last |
| each task | tests vs code self-consistent; no Run-key writes, no real app | OK |

## Progress
Task 1: dispatched (base c82f500, sonnet)
Task 1: controller review (92-line diff): logic OK; fix round 1 dispatched — blocking_show on main thread may deadlock → MessageBoxW
Task 1: fix round 1/5 (addressed; f7ccabd..ec7165c). Ruling: controller reviewed the whole Task 1 diff directly (≈100 lines, startup path) — cost: no independent eye.
Task 1: complete (commits c82f500..ec7165c)
Task 2: dispatched (base ec7165c, sonnet)
Task 2: ⚠️ resolved: apply_settings validates (settings.validate) before saving — atomic; sheet is an overlay, card fit re-measured in Task 4.
Task 2: complete (commits ec7165c..4e4a2ce, review clean, approved)
Task 2: minor (deferred → FINAL WAVE): tray test near-tautology; number inputs need noValidate so Rust is the single message source; « Enregistré. » stays after edits; HiddenProjects loads on mount only; « Sites » link missing / links not separated.
Task 3: dispatched (base 4e4a2ce, sonnet)
Task 3: complete (commits 4e4a2ce..31711f1, review clean, approved). Existing install sees the welcome once; its Run entry untouched until Terminer.
Task 3: minor (deferred → FINAL WAVE): « Rien n'est inscrit » wording vs existing Run entry → « Rien de nouveau… »; onboarding_set trusts the front; setting vs Run differ until Terminer; no Panel+Welcome Escape test, no error-path test.
Ruling P7-R1: no zoom to force a 1080p fit — the user's screen is 2560×1440 (spec target) and zoom .76 drops text to ~10 px; at ≤1080p the grid may scroll with the .more fade, nothing hidden — cost if wrong: on a smaller screen the user scrolls the panel.
Ruling P7-R2: lint via @babel/eslint-parser (typescript-eslint refuses TypeScript 7.0), only the two react-hooks rules — cost: no TS-aware lint rules (tsc --noEmit still type-checks in pnpm build).
Task 4: fix round 1/5 dispatched (remove zoom per P7-R1)
Task 4: fix round 1/5 (zoom removed per P7-R1; 5f07821..378b02b)
Task 4: complete (commits 31711f1..378b02b, review clean, approved)
Task 4: minor (deferred → FINAL WAVE): settings sheet state 974/984 overflow at 1440 (layout.mjs ok uses .more only); 1080p marked required → make informational; Pill render test indirect.
Task 5: fix round 1/5 dispatched (notes full-body matching, clean ends_with credits, Topic::Music after cache clear, SlowSpeaker runtime)
Task 5: minor (deferred): installed() exe names by dest_dir string; in-flight fetch_playlists may repopulate cache (generation counter); focus pulled to Refuser on each new pending.
Task 5: fix round 1/5 (4 addressed; 5e586f6..e142cd3). Ruling: controller reviewed the 38-line fix diff directly — cost: no independent eye on a small diff.
Task 5: complete (commits 378b02b..e142cd3)
Task 5: checklist: search latency with many long notes (fuzzy over full bodies) — measure with budgets.mjs.
Task 6: fix round 1/5 dispatched (VRAM path escaping, VRAM scope to AI procs, counter vs no-use, soak Oniris-gone guard, ≥12 samples, CSV naming, main-module guard)
Task 6: fix round 1/5 (6 addressed, 0 open; 656e2ce..5334410)
Task 6: complete (commits e142cd3..5334410, review clean after 1 round)
Task 6: checklist: live Get-Counter VRAM path, ollama runner / claude detection on the real machine.
Task 7: complete (commits 5334410..ff0f4ae + cleanup commit removing a force-added report). Ruling: controller reviewed the config diff directly (73 lines: versions 1.0.0, nsis currentUser/French/lzma, hooks.nsh DeleteRegValue Run "Oniris", data folders kept; config test) — cost: no independent eye.
Task 7: checklist: uninstall removes Run value, keeps data folders (real installer).
Task 8: complete (commits 0f5402c..a4b8608). Controller verified: default hotkey Alt+Space; Tauri 2.12.4 NSIS currentUser installs to %LOCALAPPDATA%\Oniris (exe path in checklist correct); its uninstaller only deletes installed files + non-recursive RMDir $INSTDIR (voice dir and data survive), already removes Run value ${PRODUCTNAME} (our hook redundant, harmless); its "delete app data" checkbox targets %APPDATA%\dev.oniris.app (unused), not Oniris data. Ruling: docs task reviewed by controller (facts checked against code/template) — cost: no independent eye on prose.
Final review: fix wave needed — I1 « Non » disables autostart forever, I2 checklist §3 needs perf build, I3 note search over-matches long notes; include M1–M8 (projects refresh on roots change, notes cache race, MessageBox foreground + exit, no raw error in log, privacy WebView2 cache folder, noValidate, layout 1080p informational, « Rien de nouveau »). List: final-fix-list.md. Deferred → later: tray test tautology, « Enregistré. » persistence, Sites link, onboarding_set trusts front, Panel+Welcome Escape test, settings 974/984, pill test indirect, installed() keyed on dest_dir, fetch_playlists race, focus to Refuser per pending, checkpoint worst case 4 s, Welcome→sheet focus.
Note: P7-R1 overrides Review Focus 4 / Global Constraint (1080p scrolls) — to be confirmed by the user explicitly.
Final fix wave: done (a4b8608..fc744cb); re-review: all 11 ADDRESSED, no new breakage.
Plan 7: complete — merging to main. Release build NOT run (awaits user approval).
