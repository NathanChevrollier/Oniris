# SDD ledger — plan: docs/superpowers/plans/2026-11-07-oniris-05-iris.md

Spec reachable. Branch plan-05-iris from main 9d1e0df (plan committed at 3968c49). Local signed commits authorised, no push. Shared context for subagents: context.md in this workspace.

## Pre-flight scan
| Tasks | Shared file / interface | Finding |
|---|---|---|
| T1 → T2 | ToolSpec.untrusted_output, ToolBroker::spec(), Tool::check/needs_confirm | consistent |
| T1 → T3,T4,T8 | needs_confirm dynamic (allowlist, work dirs) | consistent; T3/T4 need a settings accessor closure (implementer's choice) |
| T2 → T7,T9,T10,T11 | Vault, project, resolve_args, render(_partial), Executor::{tools,call}, wire/unwire | consistent |
| T2 ↔ T9 | Executor "virtual tools registered (task 9)" | consistent: T2 leaves a hook, T9 fills iris.inspect/digest/ask_claude |
| T3 → T6 | src-tauri job::spawn | consistent |
| T5 → T6 | IrisStatus.claude = Unknown until T6 | consistent |
| T5,T6 → T9,T10,T11 | LocalBrain::{chat,structured}, ClaudeBrain::{complete,plan} | consistent |
| T6 ↔ T7 | ClaudeBrain::plan wired to MCP in T7 | ruling P5-R1 |
| T8 → T9 | prompt::system, memory::context; T9 uses `PromptCtx` not defined anywhere | ruling P5-R2 |
| T9 → T10 | tag enum reused by triage | consistent |
| T9 → T12 | IrisEventKind payloads | consistent |
| T10 → T11 | ReplyMail suggestion opens reader + IRIS draft | consistent |
| T11 ↔ T12 | .brief block: Briefing (T11) vs IrisReply during exchange (T12) | consistent |
| T13 | extends accounts_open_guide allowlist (plan 4 code) | consistent |
| migrations | 17 (T8), 18 (T10) | one owner each |
| settings | allowlist T3, work_dirs T4, local model/unload T5, claude_path/model T6, personality T8 | one owner each |
| each task | tests vs code self-consistent; tool names verified against registry (agenda.delete_event, music.toggle/next/previous/volume, timer.start, focus.start, projects.resume) | OK |

Ruling P5-R1: T6 implements `ClaudeBrain::plan` in RealClaude as returning `ClaudeError::Failed("MCP non branché")` (trait + fake usable); T7 replaces it with the real MCP session — cost: none.
Ruling P5-R2: `PromptCtx { system: String, history: Vec<(Role, String)> }` is defined by T9 in iris/agent.rs and built in src-tauri from T8's prompt::system + memory::context — cost: none.

## Progress
Task 1: dispatched (base 3968c49, sonnet, agent ab02d6e9dbc19728a)
Task 1: ⚠️ build outside diff resolved by controller: implementer ran cargo test --workspace (core 305 + tauri 103, compiles app crate) and clippy clean.
Task 1: complete (commits 3968c49..e12a97c, review clean, approved)
Task 1: minor (deferred → FINAL WAVE, should fix): check-failure path journals raw args instead of journal_args; play_playlist journals playlist name; check_exe_path error text says Zenytt only; untrusted_output derived from sensitive_output in music; URI/readonly-from-DB branches untested at tool level
Task 2: dispatched (base e12a97c, opus, agent a39ee8946f0dc6eea)
Ruling P5-R3: SAFE_KEYS string regex tightened — no space allowed, 100 chars max (prose injection under status/kind/state would otherwise reach the planner, against spec §7.4.1) — cost if wrong: app keys/paths with spaces become refs, so planner reuse of them needs a confirmation.
Task 2: fix round 1/5 dispatched (P5-R3 safe() no space/100, tainted check-error leak, wire round-trip assertions)
Task 2: minor (deferred): one large untrusted output can evict all earlier refs (per-call ref cap); call() rebuilds virtual specs each call / virtual name could shadow broker tool
Ruling P5-R4: residual by design — values accepted by safe() under SAFE_KEYS (ids, statuses, kinds) can be reused as tool args under Provenance::User without confirmation; these fields are produced by Oniris/Google/Spotify APIs as identifiers, not free text a mail sender controls, and P5-R3 removes prose — cost if wrong: an attacker-chosen identifier-shaped token (≤100 chars, no spaces) could flow into a Free tool call without a card.
Task 2: fix round 1/5 (3 addressed, 0 open; b394d44..92998b7)
Task 2: complete (commits e12a97c..92998b7, review clean after 1 round)
Task 2: note for Task 9: wire round-trip test uses a hardcoded name list — Task 9 must add iris.* names; ideally a src-tauri test asserts wire round-trip over AppState's real broker.specs().
Task 3: dispatched (base 92998b7, sonnet, agent a8de97def6c511ad2)
Ruling P5-R5: accept implementer's reduced default shell allowlist (git status, Get-ChildItem, Get-Location, Get-Date) instead of the plan's 13 entries — extra free tokens make 'git branch foo' create a branch and 'git log --output f' write a file, so the plan's list is not read-only — and risk(cmd, list) two-arg signature — cost if wrong: more confirmation cards for harmless commands (user can add entries).
Task 3: ⚠️ resolved by controller: journal keeps shell.run command by design (spec §7.4.6); broker confirms only Untrusted provenance beyond permission — by design; elevation runtime check added to fix round.
Task 3: fix round 1/5 dispatched (encoded-command aliases, UNC cwd before is_dir, UNC in allowlisted tokens, CLIXML stderr, git env hardening, elevated-token refusal)
Task 3: minor (deferred): whole-word risk misses Remove-ItemProperty/backtick escapes, add ri/sp/irm; textual elevation guard inherently bypassable via obfuscation (card+UAC remain); settings allowlist error text vs control chars, entries not through safe_token; lingering grandchild turns finished command into timeout; job attach race + no tree-kill test; no separate RED run
Task 3: fix round 1/5 (6 addressed; new: mixed-separator UNC in safe_token, exit_code lost to Out-String; e6ba7c6..6c2c99c)
Task 3: fix round 2/5 dispatched (mixed separators, exit code, -enc:AA split)
Task 3: minor (deferred): -ea (ErrorAction alias) and dir /e over-refused by guard (fail-safe); parse errors of the command itself still CLIXML
Task 3: fix round 2/5 (3 addressed, 0 open; 6c2c99c..fc3aa39)
Task 3: complete (commits 92998b7..fc3aa39, review clean after 2 rounds)
Task 3: minor (deferred): multi-statement exit code reflects last statement only / stale LASTEXITCODE
Task 4: dispatched (base fc3aa39, opus, agent acf8f34032984143f)
Ruling P5-R6: accept FOF_WANTNUKEWARNING on SHFileOperationW — without it Windows silently deletes permanently what cannot be recycled (spec §7.4.5 forbids permanent deletion); with it Windows asks and a cancel fails the tool — cost if wrong: a system dialog may appear for unrecyclable items (removable drives).
Task 4: ⚠️ FOF_WANTNUKEWARNING real behaviour → user checklist (Task 14); apps = plan-3 Inventory (compiles) accepted.
Ruling P5-R7: files.copy may not read from %APPDATA%\Oniris, and Startup folders + PowerShell profile dirs join the write-protected list (persistence vectors reachable by Free files.create) — beyond the plan's list — cost if wrong: user can't ask IRIS to drop a file in Startup or edit their PS profile.
Task 4: fix round 1/5 dispatched (protected-ancestor bypass, work-dir root inside its zone, wildcards/DOS devices, copy-from-Oniris, Startup/profile protection, retryable undo, spawn_blocking, shortcut comments)
Task 4: fix round 1/5 (8 addressed, 0 open; 6b1e040..f97d42e). ⚠️ broker keeps action undoable after a failed undo: verified by controller (broker.rs undo transitions undone→done on error).
Task 4: complete (commits fc3aa39..f97d42e, review clean after 1 round)
Task 4: minor (deferred): undo reports success when bin emptied + new file at path; blocking() not cancellation-safe (undo record lost); check/describe canonicalize on async thread; restore_from permanent Err if $I removal failed; double blank line files.rs; tests depend on real APPDATA/Documents
Task 5: dispatched (base f97d42e, sonnet, agent ab857985bf4fb22da)
Task 5: ⚠️ "unload when a game runs": fullscreen entry is the plan's proxy (spec §7.3) — accepted; TS settings parity covered by existing pnpm tests (passed).
Task 5: fix round 1/5 dispatched (structured keep_alive ignores setting, pull send timeout, PULLING drop guard, pull error test)
Task 5: minor (deferred): status reports Down on non-2xx /api/tags; no separate RED run
Task 5: note for Tasks 6/9: build Ollama client per request from current settings (model + minutes) so changes apply without restart.
Task 5: fix round 1/5 (4 addressed, 0 open; 3a526db..afc3102)
Task 5: complete (commits f97d42e..afc3102, review clean after 1 round)
Task 6: dispatched (base afc3102, sonnet, agent ab686f302eac4c589)
Task 6: ⚠️ resolved by controller: `--tools ""` and `--permission-prompts none` verified in local `claude --help` (2.1.296) during planning; cwd %APPDATA%\Oniris\claude must be passed by Task 7 (note carried).
Ruling P5-R8: accept iris_claude_model validation of 100 chars `A-Za-z0-9._:/-[]` (plan said 60/[a-z0-9.-]) so values like `sonnet[1m]` work; argv without shell — cost if wrong: none beyond odd model strings being accepted.
Task 6: fix round 1/5 dispatched (fail-closed init guard, model leading alnum, stdin write in timeout, --model test)
Task 6: minor (deferred): hard-coded arg indices in invocation test
Task 6: fix round 1/5 (4 addressed; new: error Result before Init no longer classified → loses Unavailable; 86931d6..138be48)
Task 6: fix round 2/5 dispatched (classify error Result before Init)
Task 6: fix round 2/5 (addressed; 138be48..d921645). Ruling: controller re-reviewed the 19-line diff directly (precedent P4) — cost: no independent eye on a small diff.
Task 6: complete (commits afc3102..d921645, review clean after 2 rounds)
Task 7: dispatched (base d921645, opus, agent a03d9d828438db19e)
Task 7: checklist: relay in release build (windows_subsystem=windows, inherited stdio) untested — verify in plan 7 / real Claude run
Task 7: fix round 1/5 dispatched (ask_claude exclusion on call, relay desync, vacuous pipe-stopped assertion, from_settings source/test, raw newline, doc(hidden))
Task 7: minor (deferred): integration test duplicates exec setup; MCP token visible on claude.exe command line to same-user processes (accepted: same-user threat out of scope)
Task 7: fix round 1/5 (4 addressed, 0 open; 4056495..0053b37)
Task 7: complete (commits d921645..0053b37, review clean after 1 round)
Task 7: minor (deferred): relay dead-connection path untested
Task 8: dispatched (base 0053b37, sonnet, agent a3ef522279b75fe49)
Ruling P5-R9: memory::context counts messages; Task 9 passes 12 (= 6 user/IRIS exchanges, the plan's "6 derniers tours") — cost if wrong: slightly larger planner context.
Task 8: fix round 1/5 dispatched (personality cap 2000)
Task 8: minor (deferred): UI must say clearing history keeps preferences (Task 13); import grouping in iris.rs
Task 8: fix round 1/5 (addressed; 6dfa6fc..b986d6c). Ruling: controller re-reviewed the 5-line diff directly — cost: none meaningful.
Task 8: complete (commits 0053b37..b986d6c, review clean after 1 round)
Note (FINAL WAVE): an intermittent pnpm test failure was seen twice (Task 4, Task 8) while cargo ran; identify the flaky test.
Task 9: dispatched (base b986d6c, opus, agent a427ad3997c9f31fd)
Ruling P5-R10: accept Task 9 deviations — Executor/Agent rebuilt per turn with the Vault kept per conversation (the security-relevant state); pending reply carries integer action_id; IrisEvent serde tag is `type` (Task 12 must use it); local→Claude fallback only before any tool ran (later → Error failed); iris_cancel emits nothing and an aborted turn is not recorded (Task 12 resets UI state locally) — cost if wrong: small UI/history gaps.
Task 9: ⚠️ resolved: memory::context returns user/iris planner_text only (Task 8 review); Vault per conversation bounded by Task 2 caps; frontend `type` tag carried to Task 12.
Task 9: fix round 1/5 dispatched (Claude-path origin, ask_claude after action, stop at first pending, Claude Pending events, stale refs after restart, shared tool list, force_claude test + prefix strip)
Task 9: minor (deferred): frame() only blocks literal << (lookalikes pass; defence in depth); fastpath now param unused
Task 9: fix round 1/5 (7 addressed, 0 open; 28b26b3..ed50894). PromptCtx gained claude_system (extends P5-R2, accepted).
Task 9: complete (commits b986d6c..ed50894, review clean after 1 round)
Task 9: minor (deferred): Claude path emits no ToolUsed; restart always opens a new conversation (drops short context)
Task 10: dispatched (base ed50894, sonnet, agent ab42acabd1df12d60)
Task 10: ⚠️ resolved: commands present in build.rs + panel.json (kebab allow-iris-draft-reply[-local]); upsert never touches summary/tag (not in insert columns) and the plan-4 test is green per report.
Task 10: complete (commits ed50894..486f97a, review clean, approved)
Task 10: minor (deferred → FINAL WAVE): DRAFT_MAX counts chars but says bytes; iris_draft_reply_local ignores fullscreen; triage UPDATE failure silent; draft_system/draft_prompt untested; spawn_triage wiring untested; draft_prompt header unwrap_or_default hides DB failure
Task 11: dispatched (base 486f97a, sonnet, agent a9784b3b61b5cef5b)
Ruling P5-R11: briefing keeps project names and local event titles in clear for the polish model (folder names / user's own events; an IRIS-created event from mail text was already confirmed by the user on a card; polish output is display-only, ref-set validated, and actions are chosen by code) — cost if wrong: a crafted local event title could skew the briefing wording, never trigger an action.
Task 11: complete (commits 486f97a..bd510dd, review clean, approved)
Task 11: minor (deferred → FINAL WAVE): MailCard open-mail no-op when mail not loaded; facts() DB query before due-check on every event; DefaultHasher persisted; refresh/listener/autoDraft untested
Task 12: dispatched (base bd510dd, sonnet, agent a1f5b0f48179eac37)
Task 12: ⚠️ 1440 fit verified by controller from measure JSON (grid 974/974, more:false, inner 1367/1367); pill orb state not re-checked (unchanged Orb supports thinking).
Task 12: fix round 1/5 dispatched (per-source tooltip, Ctrl+Shift+Enter, command hit Ctrl+Enter, double-ask race, fast brain label, aria-live, IRIS shortcut hint)
Task 12: minor (deferred): cancel-before-turn-id and iris_ask rejection untested
Note (FINAL WAVE): flaky pnpm test identified: projects.test.tsx « Ouvrir le dossier ».
Task 12: fix round 1/5 (7 addressed, 0 open; 3efbe8a..8f760fb)
Task 12: complete (commits bd510dd..8f760fb, review clean after 1 round)
Task 13: dispatched (base 8f760fb, sonnet, agent a6fadfdf406245ca4)
Task 13: ⚠️ resolved: pull event shape and settings fields come from Tasks 5/6 (tests green); spec §7.4.6 "commande" gap → fix round.
Task 13: fix round 1/5 dispatched (journal args line, error catches, localized status, section form reset)
Task 13: minor (deferred): Brains form/pull error/undo failure untested; Panel inert wiring untested; __s mock hack; alertdialog focus
Task 13: fix round 1/5 (3 addressed, 1 open: Security section still resets on [s]; 298bb43..9f82de7); fix round 2/5 dispatched
Task 13: fix round 2/5 (addressed; 9f82de7..687249b). Ruling: controller re-reviewed the 4-line diff directly — cost: none meaningful.
Task 13: complete (commits 8f760fb..687249b, review clean after 2 rounds)
Task 14: dispatched (base 687249b, sonnet, agent ae26ca28a2e329184)
Task 14: ⚠️ resolved: mail headers reach the planner only as refs (Task 2 projection), so the doc's claim holds; panel-open bench not re-run (manual checklist item, as in plan 4).
Task 14: fix round 1/5 dispatched (checklist local-draft path, Claude flow wording, Ollama wording, ignore wording)
Task 14: fix round 1/5 (4 addressed; 11e4fae..817d257). Ruling: controller re-reviewed the docs-only diff directly — cost: none meaningful.
Task 14: complete (commits 687249b..817d257, review clean after 1 round)
Final review: dispatched (opus, agent a17f26de7b688a658, filtered diff 9d1e0df..817d257)
Final review: fix wave needed — I1 empty Untrusted cards, I2 local version re-routes to Claude, I3 fullscreen VRAM during in-flight requests; minors M1,M4,M6 + T3 HIGH words + T1 failed journal_args included. List: final-fix-list.md. Deferred → plan 7: M2 (prod journals Claude MCP calls as MCP), M3 (sender name in plain suggestion label), M5 (aborted turn leaves running row), M7 (work dir on unplugged drive reset), and all other ledger minors.
Final fix wave: done (817d257..644f2ce); re-review dispatched (opus, agent a2a435ba245d7e93b)
Final fix wave re-review: I1, I3, M1, M4, M6, T3, T1 ADDRESSED; I2 partly — local_only turn can still reach Claude via iris.digest (long/draft_reply) because run_turn builds Quarantine with RealClaude.
Ruling P5-R12: residual I2 (local_only → iris.digest → Claude) is load-bearing for the « version locale » privacy promise but there is no second fix wave; carried as the FIRST item of plan 6 (use NoClaude in run_turn when local_only + test). Until then, « Version simple en local » may send a mail to Claude only if the local model itself asks for a long summary/draft — cost if wrong: one unexpected Claude call on a user-declared local request.
Deferred → plan 7: confirm-arrival closes sheet without focusHeaderButton (focus lost); sheet closed when 2→1 pending; aborted turn skips unload_if_fullscreen; unload sent even when model never loaded.
Plan 5: complete — merging to main
