# SDD ledger — plan: docs/superpowers/plans/2026-11-04-oniris-02-modules-locaux.md

Spec: docs/superpowers/specs/2026-11-03-oniris-design.md. Branch plan-02-modules-locaux from main @ 4a? (plan committed at bc296ef). Local commits authorised, no push.

## Pre-flight scan
| Tasks | Shared file / interface | Finding |
|---|---|---|
| T2 → T4-T10 | Events/Topic, Scheduler, useTopic, CardGrid slots | consumers match producers |
| T3 → T4,T5,T7 | parse_when / When | T7 uses its own parse_duration — OK |
| T4,T5 ↔ T6 | notes.to_reminder uses reminders core fn | T6 after T5 — OK |
| T4 → T5 | AgendaCard modified by T5 (ReminderList inside) | sequential — OK |
| T7, T8, T10 | Settings fields added + validate/fix + TS mirror | three tasks touch settings.rs — sequential, OK |
| T2 → T9 | system emits oniris://system (not a Topic) | intentional (stream, open-only) |
| all | CORE_MIGRATIONS versions 3..8 in task order | OK |
| all | every new command: lib.rs + build.rs + capability | stated in Global Constraints |
| T1 | tests self-consistent | OK |
| T3 | DST test needs tz-independent approach | plan lets implementer choose (chrono-tz dev-dep) |
| T4 | agenda_delete via broker → Confirm card | matches spec §7.5 |
| T8 | clipboard_paste SendInput product code — must not run in tests | R8 |
| T11 | depends on all cards | OK |

Ruling P2-R1: plan 1 perf measurement + manual checks not yet done by the user; proceeding with React per user "continue" — cost if wrong: cards ported to another framework later.

## Progress
Task 1: complete (commits bc296ef..a2f4398, review clean; + c chore gitattributes for generated files EOL)
Task 1: minor (deferred): logs before setup / panic path lost (init could move to run()); guard may not flush on app.exit; no per-day size cap; redundant "oniris:" prefixes
Ruling P2-R2: binding convention added to context.md — never log clipboard/note/mail content, only ids/lengths/kinds — cost: none.
Ruling P2-R3: scheduler sleep capped at 60 s ONLY while a deadline exists (no deadline → no wake) — robust to PC suspend/clock changes, ≤60 wakes/h only when something is scheduled — bends plan "no periodic wake"; cost if wrong: negligible CPU.
Task 2: minor (deferred): useTopic per-window (pill needs its own open semantics); failed fetch retried only on next event; scheduler test timing-sensitive
Task 2: fix round 1/5 (4 addressed, 0 open; commits 0e70768..838dd70)
Task 2: complete (commits a2f4398..838dd70, review clean after 1 round; no panic=abort in profiles)
Task 2: minor (deferred): parked-key warn repeats each wake; unmount guard untested; timing tests use fixed sleeps
Task 3: fix round 1/5 (4 addressed, 0 open; commits ecf2f76..5c0d805)
Task 3: complete (commits 838dd70..5c0d805, review clean after 1 round)
Task 3: minor (deferred, revisit when IRIS lands in plan 5): bare midi/soir/nuit in titles read as time; "aujourd hui matin" can be past; "8h du soir" not 20h; leading link word at text start stripped; "ce matin"/"cet après-midi" unsupported
Ruling P2-R4: all-day events stored as local midnight → next local midnight (computed with local dates, DST-safe); shared time_util helper in core — cost: none.
Task 4: minor (deferred): error matched by substring; DayList multi-day start time; duplicate today query Panel/AgendaCard; tests depend on machine TZ
Task 4: fix round 1/5 (5 addressed, 0 open; commits bf0a486..754ef46)
Task 4: complete (commits 5c0d805..754ef46, review clean after 1 round)
Task 5: complete (commits 754ef46..506bfe3, review clean)
Task 5: minor (deferred → T11 polish): completed reminders vanish instantly (mockup shows struck-through done until midnight; needs done_ms) and no UI undo/delete; at-most-once delivery (document); dots memo recomputes each minute
Task 6: complete (commits 506bfe3..ad4613a, review clean; extra notes.pin tool accepted)
Task 6: minor (deferred → T11 visual): pin marker left:-8px may clip; textarea JS height (could use field-sizing)
Task 7: complete (commits ad4613a..26b637e, review clean)
Task 7: minor (deferred → T11): restore hidden slot-music span in Pill.tsx alongside PillTimer; focus_cycle always NULL (reserved); after_finish failure → notification still announces a break
Ruling P2-R5: clipboard image cap raised 20→40 MB (keeps 4K screenshots); thumbnail bounded 160×160 — cost: larger DB rows.
Task 8: minor (deferred → T10 confidentialite.md + T12 checklist): browser-extension password copies not caught by exclusion formats; two-session flag writers; Clipboard Viewer Ignore not read; settings change does not purge until next copy; list payload up to 64 KB/entry
Task 8: fix round 1/5 (8 addressed, 0 open; STAGED, commit blocked by 1Password signing — waiting for user to unlock)
Task 8: minor (deferred): SetForegroundWindow may be refused by foreground-lock (safe fallback); message window not destroyed on listener failure; search query args journaled
Task 8: complete (commits 26b637e..204f665, review clean after 1 round)
Task 9: minor (deferred): stale-ppid reuse may count foreign webview2; VRAM used/total may refer to different adapters on iGPU+dGPU; *_mb are MiB
Task 9: fix round 1/5 (6 addressed; 39db88a), fix round 2/5 (2 addressed; 54f1bfb)
Task 9: complete (commits 204f665..54f1bfb, review clean after 2 rounds)
Task 9: minor (deferred): special()/empty-array mapping untested; Idle resets failure counter
Ruling P2-R6: Open-Meteo forecast_days=2 (plan said 1) so the 12 h rain window works in the evening — cost: slightly larger response.
Task 10: minor (deferred): on_panel_open takes DB mutex for one kv read (could be atomic); manual weather_refresh ignores 10 min window (no caller yet); no DST test (12 wall-clock hours)
Task 10: fix round 1/5 (5 addressed; 7ffe382)
Task 10: complete (commits 54f1bfb..7ffe382, review clean after 1 round)
Task 10: minor (deferred): old-city error can briefly show for new city; offline classified by French message prefix; confidentialite omits 10-min retry
Ruling P2-R7: T11 also absorbs carried polish — restore hidden slot-music in pill, completed reminders shown struck-through until local midnight (adds migration 9: reminders.done_ms), notes pin-marker clipping — cost: a 9th migration beyond the plan list.
Ruling P2-R8: "Bientôt" placeholders are compact (≈120 px, start-aligned) so live cards fit at 2560×1440; scroll fade mask at 1080p — cost: layout shifts again when plans 3-4 fill slots.
Task 11: minor (deferred → plan 7 CSS pass): generic class names (.add, .err, .card button…) should be module-prefixed; hover background transition on .dl li
Task 11: fix round 1/5 (5 addressed; 5a32e5f) — Annuler in header & ring text verified on screenshot v2
Task 11: complete (commits 7ffe382..5a32e5f, review clean after 1 round)
Task 11: minor (deferred): .more fade observer only watches initial children; empty grid voids under Bientôt rows until plans 3-4
Ruling P2-R9: Task 12 (docs-only checklist) reviewed as part of the final whole-branch review instead of a separate task review — cost: none, the final reviewer covers it explicitly.
Task 12: complete (commit 360e2ac, 59 items)
Final review (opus, bc296ef..360e2ac): With fixes — 5 Important, 10+ Minor; fix wave spec in final-fixes.md (incl. boilerplate extraction before plan 3)
Ruling P2-R10: clipboard on/off exposed now via a tray toggle (settings screen is plan 7) — the only escape hatch for browser-extension password copies — cost: small tray change.
Final fix wave: commits 360e2ac..632f6ca, re-review: all findings addressed
Final: minor (deferred plan 7): tray toggle read-modify-write of Settings vs concurrent settings_save; tray check not resynced from other save paths; main-thread scan test only matches current attribute style
