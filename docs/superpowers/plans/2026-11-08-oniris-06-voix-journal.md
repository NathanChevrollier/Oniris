# SDD ledger — plan: docs/superpowers/plans/2026-11-08-oniris-06-voix.md

Spec reachable. Branch plan-06-voix from main 353c6a1 (plan committed at be300fd). Local signed commits authorised, no push. Shared context: context.md here.

## Pre-flight scan
| Tasks | Shared file / interface | Finding |
|---|---|---|
| T1 | iris.rs run_turn / agent tests (plan 5 code) | independent |
| T2 → T4,T5,T6,T7 | CATALOG, needed(), status(), voice dir %LOCALAPPDATA%\Oniris\voice, settings voice_* | consistent; T2 owns all five settings |
| T3 → T6 | Mic/Recording traits, resample/wav/speech_ms | consistent |
| T3 → T7 | voice_devices() command added in capture.rs by T7 per plan | ruling P6-R1 |
| T4 → T6 | transcribe/clean/WhisperProcess::{ensure,touch,stop} | consistent |
| T5 → T6 | speakable/sentences/Speaker/SpeakHandle | consistent |
| T6 → T7 | oniris://voice {phase, level}, toasts | toast event name not fixed | ruling P6-R2 |
| T6 ↔ plan 5 | iris_ask path, turn-end notification | consistent (T6 adds the hook) |
| T8 | docs only | consistent |
| each task | tests vs code self-consistent; no real mic/speaker/binaries in tests | OK |

Ruling P6-R1: voice_devices() is created by Task 3 (it owns cpal input) and consumed by Task 7 — cost: none.
Ruling P6-R2: pill toast event is `oniris://toast {text}` emitted to the pill window by Task 6; Task 7 renders it — cost: none.
Ruling P6-R3: wake word deferred to an optional plan 8 (spec §4.3 places it last and optional; needs Python training + network) — cost if wrong: no hands-free "IRIS" until plan 8.
Ruling P6-R4: Whisper on CPU with the official prebuilt binary (no Windows Vulkan build published; no CMake/Vulkan SDK on the PC); GPU build revisited in plan 7 only if the measured latency exceeds 1 s — cost if wrong: transcription slower than with GPU.

## Progress
Task 1: dispatched (base be300fd, sonnet, agent a84518dcb0c893937)
Task 1: complete (commits be300fd..3202ea7). Ruling: controller reviewed the 46-line diff directly (claude_for selects NoClaude when local_only; test proves NotInstalled with a real configured exe) — cost: no independent eye on a small diff.
Task 2: dispatched (base 3202ea7, sonnet, agent a6f1de6011536e229)
Task 2: ⚠️ resolved: controller listed the real whisper-bin-x64.zip (b5454) during planning — Release/whisper-server.exe, whisper.dll, ggml*.dll present; https_only on redirects harmless given SHA-256.
Task 2: complete (commits 3202ea7..917269a, review clean, approved)
Task 2: minor (deferred → plan 7): replace-install removes old target before rename; std::fs in async fetch; no https-only redirect policy; bytes_missing undercounts whisper-bin; no final progress when total unknown
Task 3: dispatched (base 917269a, sonnet, agent adf9d5c6093b5d61b)
Task 3: complete (commits 917269a..be49295, review clean, approved)
Task 3: minor (deferred → FINAL WAVE): level throttle 33 ms (~30.3/s → 34 ms); Instant::now()-1s underflow; linear resample without low-pass; dead_code allow to remove in Task 6.
Task 3: note for Task 6: Mic::start blocks while the device opens — call it via spawn_blocking, never on an async executor thread; remove capture.rs #![allow(dead_code)].
Task 3: checklist: Denied (Windows privacy) and unknown-device fallback on real hardware.
Task 4: dispatched (base be49295, sonnet, agent af2afdf51e13b63c5)
Task 4: BLOCKED on 1Password signing; work staged; review on staged diff
Task 4: review on staged diff: approved, no Critical/Important. Waiting on 1Password to commit.
Task 4: minor (deferred → FINAL WAVE): clean() substring match drops real commands containing "sous-titrage"/"abonnez-vous"; unbalanced ( or [ swallows rest; ensure overwrites idle before install check; client built per start.
Ruling P6-R5: 1Password signing still failing after user 'continue'; Task 5 proceeds on top of the STAGED Task 4 work — Task 5 implementer must commit Task 4 first (staged index as-is) then its own work, or leave its changes unstaged if signing still fails — cost if wrong: one more manual commit step for the user.
Task 5: dispatched (base be49295 + staged T4, sonnet, agent a4245e080ebf982dc)
Task 5: BLOCKED on signing; code in working tree (unstaged); review on working diff
Task 5: review on working diff: approved, no Critical/Important. Waiting on 1Password to commit (T4 then T5).
Task 5: minor (deferred → FINAL WAVE): stop() doesn't wait old sink close (few-ms overlap); speak stop/check/set not atomic; tail may clip last syllable; stdout read error / sample_rate fallback silent; speak needs runtime; volume & odd-byte carry untested; strip_markup mangles snake_case/C#/![img]/unclosed ```.
Task 5: note for Task 6: keep ONE shared PiperSpeaker in AppState; wire Esc + PttStart barge-in.
Task 4: complete (commits be49295..fc83dff, review clean, approved) — committed after user unlocked 1Password
Task 5: complete (commits fc83dff..91c5919, review clean, approved)
Task 6: dispatched (base 91c5919, opus, agent af4623ffb81729c9e)
Ruling P6-R6: barge-in (speaker.stop) on every press even when voice is paused/not installed is accepted (a press always silences IRIS, as the user expects) — cost: none.
Task 6: note for Task 7: voice turns carry a turn id the panel did not request — the front must adopt voice turns (e.g. accept a new turn from `oniris://iris` when idle, or a `voice` flag) so a long/pending voice reply opening the panel is actually displayed.
Task 6: fix round 1/5 dispatched (stuck Speaking after paused barge-in, whisper not finished on early exits in fullscreen, 30 s mic watchdog, phase lock race, skip superseded turn, failed-turn toast / empty reply silent, Escape + SPEAKING tests)
Task 6: minor (deferred): status_label runs full conf() each second; voice_turn aborts a running typed turn (consistent with iris_ask)
Task 6: fix round 1/5 (7 addressed, 0 open; 7ff31d0..3536421)
Task 6: complete (commits 91c5919..3536421, review clean after 1 round)
Task 7: dispatched (base 3536421, sonnet, agent see transcript) — includes voice-turn adoption in the panel store (user text event)
Task 7: complete (commits 3536421..19743fa, review clean, approved)
Task 7: minor (deferred → FINAL WAVE): useSave circular import; Pill re-renders whole pill per level event; empty unload field saves 0; pill toast/mic indicator untested.
Task 7: checklist: pill window resizes with the 320 px toast (real app).
Task 8: complete (commits 19743fa..942da35, review clean, approved)
Task 8: minor (deferred): privacy text omits whisper prewarm on key press; checklist sizes rounding.
Final review: fix wave needed — I1 barge-in race in speak, I2 panel Escape doesn't cancel adopted voice turn (+ abort must be silent); include M1 no_proxy, M2 whisper stop on fullscreen entry, M3 model change restarts whisper, M4 speaking deadline, M5 34 ms throttle, M6 no panel over fullscreen. List: final-fix-list.md. Deferred → plan 7: assets installed() checks folder only; speak blocks a tokio worker; CpalMic::start could hang; remaining ledger minors.
Ruling P6-R7: in fullscreen, a long/pending voice reply shows the toast « Réponse prête dans le panneau » instead of opening the panel over the game (spec §4.3 silent on fullscreen; anti-goal "app qui pèse"/intrusive) — cost if wrong: the user must open the panel themselves after the game.
Final fix wave: done (942da35..67f4ee9); re-review: all 8 ADDRESSED, no new breakage. Accepted side effects: fullscreen entry kills an in-flight transcription (toast « Je n'ai pas pu t'entendre »); a panicking turn is silent (Aborted).
Deferred → plan 7: assets installed() checks folder only; speak blocks a tokio worker; CpalMic::start could hang; remaining ledger minors.
Plan 6: complete — merging to main
