# SDD ledger — plan: docs/superpowers/plans/2026-11-05-oniris-03-recherche-projets.md

Spec reachable. Branch plan-03-recherche-projets from main (plan committed at 7c1337b). Local signed commits authorised, no push.

## Pre-flight scan
| Tasks | Shared file / interface | Finding |
|---|---|---|
| T1 → T2,T4 | FileIndex, query, UsageMap | consistent |
| T3 → T4,T5,T6,T8 | launch::{safe_url,safe_path}, open_path/open_url/spawn_hidden/spawn_detached | consistent; T8 extends safe_url for the exact vscode:// prefix — ruling P3-R1 |
| T5 ↔ T6 | favorites core + import | sequential OK |
| T7 → T4,T8,T9 | Project, projects_list, Topic::Projects (new) | T4 consumes projects before T7 exists → ruling P3-R2 |
| T2 ↔ T7 | effective roots include projects_roots (setting added in T7) | ruling P3-R3 |
| T5 | FavoritesCard takes slot `vps` | OK (plan 4 adds VPS section) |
| T6 | adds tauri-plugin-dialog; T9 reuses it for folder picker | OK |
| each task | tests vs code self-consistent | OK |

Ruling P3-R1: safe_url stays http/https/mailto; a separate `safe_vscode_claude_uri` accepts only the exact `vscode://anthropic.claude-code/open?` prefix — cost: none.
Ruling P3-R2: T4 wires the projects/favorites sources behind an `extra items` provider list; sources added later (T5, T7) register into it — T4 ships with files, apps, notes, commands — cost: none.
Ruling P3-R3: T2 reads `projects_roots` defensively (field added by T2 itself with default empty if T7 has not landed: T2 adds both `search_roots` and `projects_roots` settings) — cost: none.

## Progress
Ruling P3-R4: migration 10 edited in place (usage.score REAL) — unreleased, no user DB exists — cost: none.
Task 1: minor (deferred): per-keystroke scoped threads (caller must spawn_blocking + drop stale); memory test ignores allocator overhead (~140 B/entry)
Task 1: fix round 1/5 (6 addressed, 1 regression introduced (! negation); b5d0d24)
Task 1: minor (deferred): FRECENCY_WEIGHT=40 plausible but not pinned by a test; remove_prefix ASCII-only fold; file_bonus keyed by path only
Task 1: fix round 2/5 (2 addressed; c2a68ef)
Task 1: complete (commits 7c1337b..c2a68ef, review clean after 2 rounds)
Task 1: parked — "! is fuzzy not substring" — Ruling: nucleo follows fzf syntax where !term is inverse-exact (substring) match; reviewer concern does not apply — cost if wrong: over-exclusion on long paths.
Task 1: note for T4: usage::record opens its own transaction — never call it inside another transaction; perf with ! atoms unmeasured
Ruling P3-R5: skip only junction/symlink reparse tags; keep OneDrive cloud-tagged entries unless RECALL_*/OFFLINE — otherwise all of OneDrive would vanish — cost: one extra syscall on reparse entries only.
Ruling P3-R6: keep git_ignore(true) (tiny files, prunes build output) despite "never open file contents" — cost: .gitignore reads.
Task 2: minor (deferred → checklist): verify OneDrive Known-Folder-Move behaviour live; walker partial errors counted as access errors
Task 2: fix round 1/5 (10 addressed; 2041868)
Task 2: complete (commits c2a68ef..2041868, review clean after 1 round)
Task 2: minor (deferred → final review): remove_prefixes norm_key allocates per entry under the write lock (may hold ~100s ms on 1M entries); reparse_tag on >260-char path fails conservative (skipped); gitignored files inserted by watcher until next reconcile
Ruling P3-R7: files.open refuses executables/scripts; new tool files.run (Confirm, Risk High) launches them — a search click on an .exe shows one confirmation card — cost: one extra click for exe launches from search (apps from Start menu stay one click via apps.launch).
Task 3: fix round 1/5 (7 addressed; 3fba9d5), fix round 2/5 (ADS + blocklist; 8cd5ca7)
Ruling P3-R8: round-2 re-review done by controller directly (15-line single-file diff: contains(":") check + list extension + tests verified) instead of a subagent — cost: no independent eye on a trivial diff.
Task 3: complete (commits 2041868..8cd5ca7, review clean after 2 rounds)
Task 3: minor (deferred): apps.launch trusts Start-menu dirs are not attacker-writable; blocklist approach (not allowlist)
Task 4: fix round 1/5 (6 addressed; d82b072)
Task 4: Ruling: note/command selections record usage (their "open" succeeds in UI) — only Pending is excluded — cost: none.
Task 4: complete (commits 8cd5ca7..d82b072, review clean after 1 round)
Task 4: minor (deferred → plan 7 perf pass): .pal-pop always mounted keeps backdrop-filter at opacity 0 (paint cost); usage table never pruned on a schedule; notes::search locks DB per keystroke (5 results)
Task 5: fix round 1/5 (6 addressed, 1 open contrast + 3 new; 351b166). Ruling: reorder strict set = whole flat live list (what the UI sends) — cost: none.
Task 5: fix round 2/5 (b286df8), fix round 3/5 (db90ca3); controller verified Rust error text + useTopic reload + strict-message test exists
Task 5: complete (commits d82b072..db90ca3, review clean after 3 rounds)
Task 5: minor (deferred → plan 7): reorder error branch matches message substring (brittle; prefer typed error code); no eslint config in project (add react-hooks lint in plan 7)
Task 6: minor (deferred): long import holds DB mutex on async worker (spawn_blocking + position index in plan 7)
Task 6: fix round 1/5 (11 addressed; 60ddfc9)
Task 6: complete (commits db90ca3..60ddfc9, review clean after 1 round)
Task 6: minor (deferred): Teredo/NAT64-local/documentation ranges not decoded (connection pinned, low risk); app killed mid-fetch → 7-day wait for that favicon
Task 7: complete (commits 60ddfc9..79b0a53, review clean)
Ruling P3-R9: empty projects_roots → no projects + "choose roots" state (spec); the home-folder fallback is removed in Task 9 together with the chooser — cost: none.
Task 7: minor (deferred): add -c log.showSignature=false; git helper may keep pipe after kill (bounded); inspect() sequential before git phase
Ruling P3-R10: wt fallback passes NO prompt (constant args, cmd /k claude --continue) — claude is an npm .cmd shim so any prompt text would be parsed by cmd — cost: fallback loses the resume prompt (VS Code route keeps it).
Task 8: fix round 1/5 (6 addressed; fadfd9c) — fix report missing (session interrupted), re-review from diff, controller ran full suite
Task 8: complete (commits 79b0a53..fadfd9c, review clean after 1 round)
Task 8: minor (deferred → checklist): double-encoding verified by simulation only — manual check with real VS Code; settings error message "~600 caractères" optimistic for accented text
Task 9: fix round 1/5 (6 addressed: empty state keyed on projects_roots + no flash, check_roots rejects drive roots/missing dirs, raw paths, sr-only global, menu roles dropped, 3 tests) — controller verified check_roots diff; folded into single commit b95412d (signing was blocked during review)
Task 9: complete (commits fadfd9c..b95412d, review clean after 1 round)
Task 9: minor (deferred → plan 7): hidden projects have no unhide UI; files.open description still says "dossiers de recherche"; .proj-more 18px target
Task 10: complete (2bb00fd); perf median ~10 ms on 300k entries; 1440p fits (empty band under Bientôt cards until plan 4)
Final review (opus): fix wave needed — F1 privacy doc (index.bin + OneDrive skip), F2 graphify, F3 UNC plain(), F4 root guard in validate, F5 vscode URI alphabet; D1 blocklist + D2 run summary folded into fix wave
Final review: deferred → plan 5/7: D3 git filter drivers from untrusted repos; D4 favorite title/category length cap; D5 indexer 100 ms wakeups; D6 reminders missing from search (spec §6.6)
Final fix wave: 3c6d481 + 2992799 (F1,F3,F4,F5,D1,D2); controller re-reviewed diff; residual: symlink to drive root only caught in projects_set_roots (canonical), not in validate
Plan 3: complete — merged to main
