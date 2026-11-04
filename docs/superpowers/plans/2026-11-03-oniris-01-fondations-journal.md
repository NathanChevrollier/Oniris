# SDD ledger — plan: docs/superpowers/plans/2026-11-03-oniris-01-fondations.md

Spec: docs/superpowers/specs/2026-11-03-oniris-design.md (reachable).
Branch: plan-01-fondations (from main @ 14d482b). Commits authorised by user (local only, no push). Signing via 1Password works.

## Pre-flight scan

| Tasks | Shared file / interface | Finding |
|---|---|---|
| T1–T10 | src-tauri/src/lib.rs (modified by 3,4,5,6,9,10) | sequential edits, no conflict |
| T3 ↔ T6/T7/T10 | commands.rs created T3, extended later | OK |
| T3 ↔ T5 | `Rect` (PartialEq, Debug, Clone, Copy needed by tests) | OK, implied by tests |
| T3 ↔ T10 | `oniris://panel-state {open, at_ms}` | consistent |
| T4 ↔ T6 | HotkeyConfig{vk:u16} vs HotkeySetting{key:String} | needs a mapping — ruling R1 |
| T5 ↔ T6 | hide_pill_in_fullscreen consumed before it exists | ruling R2 |
| T6 ↔ T7 | CORE_MIGRATIONS v1 "creates settings and actions (task 7)" but v1 lands in T6 | ruling R3 |
| T7 ↔ T8 (TS) | ConfirmCard test uses `{Untrusted:{source}}`, risk "High" | requires serde default enum repr — ruling R4 |
| T2 ↔ T8 | Orb/Card props | consistent |
| T7 ↔ T8 | PendingAction TS mirror | T7 must export TS type in src/lib/ipc.ts — ruling R4 |
| T1 | step 5 mentions "app.windows[].backgroundColor" | odd key — ruling R5 |
| T2 | tests vs code: Card must render a heading (h3) | self-consistent |
| T3 | tests vs code | self-consistent (150% case: x=-1260, w=600, h=72) |
| T4 | tests vs code | self-consistent |
| T5 | tests vs code | self-consistent |
| T6 | 4th test is prose → implementer writes it | OK |
| T7 | tests are prose comments → implementer writes them faithfully | OK |
| T8 | formatLongDate(3 nov 2026) = "Mardi 3 novembre" (Tuesday ✓) | OK |
| T9 | manual only | OK |
| T10 | release build needs user approval | Ruling R6 |

Ruling R1: T6 adds `HotkeyConfig::from_setting(&HotkeySetting, threshold_ms)` (key "Space"→VK_SPACE, "A"-"Z", "F1"-"F12") in src-tauri/src/hotkey/mod.rs — the plan says T6 rewires the hook from settings but names no mapping fn — cost if wrong: trivial rename.
Ruling R2: T5 reads a `hide_pill_in_fullscreen` flag defaulting to true (AtomicBool in AppState); T6 wires it to Settings — cost if wrong: none.
Ruling R3: T6 ships CORE_MIGRATIONS v1 = settings only; T7 appends v2 = actions — spec requires versioned migrations; editing a shipped migration is bad practice — cost if wrong: none (no real DB yet).
Ruling R4: all Rust enums crossing IPC use serde's default (externally tagged, PascalCase variant names); T7 adds matching TS types (PendingAction, Provenance, Risk, ConfirmationText) to src/lib/ipc.ts — matches the plan's TS test — cost if wrong: rename.
Ruling R5: transparency via `transparent: true` + transparent html/body CSS; no backgroundColor key — cost: none.
Ruling R6: T10 step 6 (release build) is NOT run during autonomous execution; dev measurements only, release measurement deferred to the user (memory: no long release builds without approval) — cost: framework decision based on dev numbers, re-checked in plan 7.

## Progress
Task 1: complete (commits 14d482b..42566f9, review clean; ⚠️ .gitignore verified in base 14d482b)
Task 1: minor (deferred): gen/schemas committed; TS ^7 / vitest ^5.1 newer than MacroWorld; vite watch ignores **/target/**
Task 2: complete (commits 42566f9..1513304, review clean)
Task 2: minor (deferred): Orb lacks aria-hidden; Card `aside &&` renders 0; no test for delay/span class
Ruling R7: the pill orb in state "idle" must be static (no spin/breathe) since the pill is always visible — idle CPU/GPU budget ≈0 %; animations only for listening/thinking/speaking and inside the open panel — carried into T8 — cost if wrong: less lively pill.
Task 3: complete (commits 1513304..2271909, review clean; ⚠️ manual check done by implementer per report, capabilities core:default verified)
Task 3: minor (deferred): set_focus error before flag update leaves stale state; toggle trusts flag only (later tasks must hide via close_panel)
Task 4: STOPPED by controller — implementer simulated keyboard input and opened Explorer windows in a loop on the user's live desktop (user complaint). Partial uncommitted work left in tree (hotkey/, lib.rs, windows/mod.rs, Cargo).
Ruling R8: no implementer may inject input (SendInput) or open/focus windows on the live desktop; all manual checks needing real interaction (T4 step 6, T5 step 5, T8 step 5 visuals ok via CDP screenshot of our own window only, T9 step 2) are collected into a user checklist at the end of plan 1 — cost: those behaviours verified later by the user.
Task 4: complete (commits 2271909..373f7ac, review clean; live checks → user checklist in task-4-report.md)
Task 4: minor (deferred, MUST handle in plan 6 before PTT drives audio): lost Space-up (Win+L/UAC/hook removal) leaves chord held → PTT stuck + 40ms timer running; fix = reset on session lock (WTS) + hard PTT cap / repeat-timeout
Task 4: minor (deferred): stale mask_pending after lost Alt-up; fallback = inject mask on ChordDown if user checklist item 3 fails
Task 4: minor (deferred): T6 must hold the HotkeyConfig write lock briefly (no I/O under lock); 5 GetAsyncKeyState per keystroke
Ruling R9 (T5 plan-mandated finding): drop QUNS_BUSY from quns_busy (it is reported for non-foreground fullscreen apps → pill hidden while user works elsewhere); keep QUNS_RUNNING_D3D_FULL_SCREEN and QUNS_PRESENTATION_MODE — spec says hide when *an app is in fullscreen* meaning what the user sees — cost if wrong: pill stays visible during some background-fullscreen edge cases.
Ruling R10 (T5): exclude maximized windows that have a caption (IsZoomed && WS_CAPTION) from fullscreen detection via a new pure helper, so auto-hide-taskbar users do not lose the pill on every maximized window — cost if wrong: a captioned app in true fullscreen (rare) keeps the pill.
Task 5: deferred to T6: re-apply pill visibility when hide_pill_in_fullscreen setting changes while fullscreen is active.
Task 5: fix round 1/5 (3 addressed, 0 open; commits d6a23b3..dc6926d)
Task 5: complete (commits 373f7ac..dc6926d, review clean after 1 round)
Task 5: minor (deferred): eprintln invisible in release — need a file logger (plan 7 or earlier); param name is_shell_window now means excluded
Task 6: minor (deferred): startup failure silent (no log/dialog) — add file logger + error dialog (plan 7); settings_save not atomic DB/memory; emit failure returns Err after success; loaded settings not re-validated (hover_delay_ms never validated); no migration rollback/downgrade test; rusqlite pinned twice
Task 6: fix round 1/5 (1 addressed, 0 open; commits c9c03c1..af1f118)
Task 6: complete (commits dc6926d..af1f118, review clean after 1 round)
Ruling R11 (T7 finding vs plan text): extend actions.status CHECK (plan listed pending/done/failed/rejected/expired/undone) with running and interrupted, in the not-yet-shipped v2 — spec §7.4.6 requires every action journaled truthfully; Free path journals running before run; leftover running rows → interrupted at startup — cost if wrong: none (schema unshipped).
Task 7: minor (deferred): expiry not folded into claim UPDATE; startup leaves non-expired pendings approvable but invisible; wall-clock TTL; journal() shows stale pending; no partial index on pending; UI ignores resolve errors, card lacks tool name/expiry; unparseable origin read as User; future MCP relay must never expose tool_resolve/tool_undo (plan 5 constraint)
Task 7: fix round 1/5 (2 addressed, 0 open; commits 5f51c68..f7e6ab4)
Task 7: complete (commits af1f118..f7e6ab4, review clean after 1 round)
Task 7: minor (deferred → carried into T8): ConfirmCard armed reset in useEffect leaves ~1 frame enabled after id change — derive per action_id; ConfirmQueue keeps expired entries & hides card even if resolve errored; mark_interrupted ignores SQL errors
Task 8: minor (deferred): ConfirmCard steals focus to Refuser while typing in CommandBar (Space/Enter refuses) — focus only if not in an input; timeline endHour<=startHour unguarded in TS (Rust validates); scrim clickable when closed (harmless); closed class set in effect; orb idle override untested by computed style; hover-open then click: reviewer says click lands on panel not scrim (verify in user checklist); CommandBar hint hardcodes Alt Espace; TS default settings duplicated
Task 8: fix round 1/5 (1 addressed, 0 open; commits f287141..7f80f4a)
Task 8: complete (commits f7e6ab4..7f80f4a, review clean after 1 round)
Task 8: minor (deferred): armedFor not reset on id change while open (A→B→A within 500 ms re-arms instantly; ids monotonic so unlikely)
Task 9: minor (deferred): no WAL checkpoint on quit; single-instance callback before app.manage could panic (narrow race); tray click vs blur-hide to verify manually; pause is_checked error swallowed; autostart default true writes Run key on first real launch without asking (consider first-launch consent in plan 7)
Task 9: fix round 1/5 (1 addressed, 0 open; commits 2ed95ae..8d14eac)
Task 9: complete (commits 7f80f4a..8d14eac, review clean after 1 round; Run key verified free of oniris entries)
Ruling R12: T10 builds the perf harness and its unit test, but does NOT execute the measurement run (perf_open_cycles opens the full-screen panel 30× over the user's live desktop and needs tauri dev — conflicts with R8); the run and the framework decision (step 7) are handed to the user as a one-command procedure — cost if wrong: framework decision delayed until the user runs it.
Task 10: fix round 1/5 (3 addressed, 0 open; commits 8afa7f8..16851f5)
Task 10: complete (commits 8d14eac..16851f5, review clean after 1 round)
Task 10: minor (deferred): perf_frame has no cycle id (late frame consumes next waiter → counted as timeout)
Final review (opus, 14d482b..16851f5): With fixes — 9 Important, 10 Minor; fix wave spec in final-fixes.md
Ruling R13: final Important #9 (confirmed-action outcome back to caller + pending list) deferred to plan 5, where the first real caller (IRIS) exists — cost if wrong: plan 2 Confirm tools (if any) give no result feedback to their caller.
Ruling R14: file logger deferred to the first task of plan 2 — cost: eprintln-only diagnostics until then.
Ruling R15: autostart default stays true; perf builds skip autostart; consent screen in plan 7 — cost: first real release launch registers autostart.
Final fix wave: commits 16851f5..cc09a86, re-review: all findings addressed
Final: minor (deferred plan 5): blur-close hides a confirmation-opened panel while action stays pending — re-show pending on reopen
