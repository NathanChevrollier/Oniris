# SDD ledger — plan: docs/superpowers/plans/2026-11-05-oniris-04-comptes-en-ligne.md

Spec reachable. Branch plan-04-comptes-en-ligne from main (plan committed at 74ca3d6). Local signed commits authorised, no push.

## Pre-flight scan
| Tasks | Shared file / interface | Finding |
|---|---|---|
| T1 → T2,T3,T6,T8 | SecretStore, TokenCache::access_token, accounts::{add,list,set_status,remove}, Provider configs | consistent |
| T1 ↔ T3,T6 | accounts::remove cascades mails/events, whose tables only exist from T3/T6 | ruling P4-R1 |
| T3 → T4 | gmail::Client {get_full, modify, trash, untrash, send} | consistent |
| T4 → T5 | mail.read → {html,text,attachments,links}; sanitize → Sanitized{html,links} | consistent (fixed during self-review) |
| T7 → T8 | music.seek falls back to Web API, which only exists in T8 | ruling P4-R2 |
| T7 → T9 | NowPlaying.colors → --ambient-* | consistent |
| T10 → T11 | SitesSection hosts VPS line; sites setting extended by import | consistent |
| T6 | Event gains source/color/readonly — plan-2 agenda tests/UI must keep passing | OK, T6 owns the migration of callers |
| settings | google_client_id+spotify_client_id (T1), mail_poll_minutes (T3), sites (T10), zenytt_path (T11) | consistent, one owner each |
| each task | tests vs code self-consistent | OK |

Ruling P4-R1: T1's accounts::remove deletes the account row + secret only; T3 and T6 each extend remove to delete their own rows (mails/mail_sync, external_events/kv sync tokens) — cost: none.
Ruling P4-R2: T7 implements music.seek via SMTC only (returns a friendly error if unsupported); T8 adds the Web API fallback inside the same tool — cost: none.

## Progress
Task 1: dispatched (base 74ca3d6, opus)
Task 1: Ruling: accepted deviations (a) token_url in ProviderConfig, (b) TokenCache::new(db,secrets,events,http,configs)+insert/forget, (c) wrong state ends the flow (single-shot, safer vs forged callback), (d) AppState wiring + 15 s client in Task 2 — cost if wrong: Task 2 adapts call sites.
Task 1: minor (deferred): forget() drops per-account lock mid-refresh; CredWriteW 2560-byte limit unchecked (handled by error); no SO_EXCLUSIVEADDRUSE on loopback bind
Task 1: fix round 1/5 dispatched (null CredentialBlob UB, accept busy loop, 404 on bad request, bind error split + test, Account Debug redaction)
Task 1: fix round 1/5 (all addressed; 238dc03..645cf69)
Task 1: complete (commits 74ca3d6..645cf69, review clean after 1 round)
Task 2: dispatched (base 645cf69, sonnet)
Task 2: Ruling: accepted listen()/serve() split (port bound before browser), extra commands accounts_intro_take/accounts_open_guide (exact-host https allowlist), refusing token responses without refresh token — cost: none.
Task 2: fix round 1/5 dispatched (in-flight guard+cancel, vault-failure consistency, focus/a11y, Spotify id fallback, google app save order, tray notify, userinfo tests)
Task 2: fix round 1/5 (all addressed; ad4298e..39be7ef)
Task 2: complete (commits 645cf69..39be7ef, review clean after 1 round)
Task 2: minor (deferred): cancel during exchange window frees slot while exchange completes; no Rust test for set_google_app branches; panel hides on blur during OAuth (checklist)
Task 3: dispatched (base 39be7ef, opus)
Task 3: Ruling: accepted trash/untrash taking a token; global fmt subscriber in log test (no interference) — cost: none.
Task 3: minor (deferred): backoff resets on restart; poll-minutes change applies after next poll
Task 3: fix round 1/5 dispatched (profile historyId vs dormant resync loop, stop flag vs error burst, concurrent accounts, + minors)
Task 3: fix round 1/5 (all 3 important + minors addressed, 1 new important: panic loses id → hot loop; 1cca871..af36461); fix round 2/5 dispatched
Task 3: fix round 2/5 (panic → settle()+sched.fail with ids kept outside tasks; af36461..59425d0). Ruling: controller re-reviewed the 80-line single-file diff directly (as P3-R8) — cost: no independent eye on a small diff.
Task 3: complete (commits 39be7ef..59425d0, review clean after 2 rounds)
Task 4: dispatched (base 59425d0, opus)
Task 4: Ruling: accepted Tool::journal_args hook (mail.send subject/body masked in actions.args; real args in memory until resolve; post-restart approve fails safe), mime::reply -> Result, archive/delete rewrite cached labels — cost: none.
Task 4: complete (commits 59425d0..17668bd, review clean, approved)
Task 4: minor (deferred → FINAL WAVE, should fix): masked map not purged on expiry + SEND_MAX not checked at call time; card subject 'Re:' rule differs from mime::reply when original lacks Message-ID, subject trim mismatch; links unescape misses &lt;/&gt;; journal_args denylist → allowlist; Message-ID/References not validated/capped; body LRU not purged on account removal + text untruncated; style filter denylist (CSP backstop)
Task 4: note for Task 5: iframe sandbox w/o allow-scripts + CSP img-src limited until click is MANDATORY (sanitize keeps http(s) imgs); confirmation details must render whitespace-preserved, untruncated; pending send after restart fails "arguments invalides" → show like expired
Task 5: dispatched (base 17668bd, sonnet)
Task 5: fix round 1/5 dispatched (scope expired mapping to mail.send approval, bounded reader height + framed iframe + compact actions, notice auto-clear, mail_status last_ok_ms, srcdoc order test)
Task 5: fix round 1/5 (all addressed; c6b404a..4888b0d)
Task 5: complete (commits 17668bd..4888b0d, review clean after 1 round)
Task 5: minor (deferred): no Rust test for mail_status SQL; notice auto-clear untested; long reauth banner can nudge card height
Task 6: dispatched (base 4888b0d, sonnet)
Task 6: note: a leftover Vite (PID 4588, started 17:22 by Task 5 agent) still listens on 1420; controller kill was denied by auto-mode — surfaced to user; context.md now mandates tree-kill
Task 6: Ruling: no calendar.rs (glue ~10 lines in mail.rs), Event.id=0 + key/source/readonly, between() merges Google, calendar in same per-account task, chrono-tz as dep, calendarList each poll — cost: none.
Task 6: minor (deferred): agenda.delete on a Google event shows a confirm card bound to fail (UX wart)
Task 6: fix round 1/5 dispatched (sliding window via token age 24h, sensitive_output on calendar.list/agenda.list, + minors 3-7)
Task 6: fix round 1/5 (6 addressed, 1 open: window still never slides — token updated_ms refreshed each poll; fbe1854..8de2576); fix round 2/5 dispatched (separate gcal_full key)
Task 6: fix round 2/5 (gcal_full key, 0/9/18/27 h test; 8de2576..ffd5d58). Ruling: controller re-reviewed the 2-file diff directly — cost: no independent eye on a small diff.
Task 6: complete (commits 4888b0d..ffd5d58, review clean after 2 rounds)
Task 7: dispatched (base ffd5d58, opus)
Task 7: complete (commits ffd5d58..17ede2d, review clean, approved)
Task 7: minor (deferred → FINAL WAVE, should fix): emit after close / overlapping listeners on quick reopen (generation counter); WinRT .join() without timeout could hang listener (tokens stay); cover retried on every event when thumbnail undecodable; emit even when NowPlaying unchanged (100-250 KB IPC up to 4/s — perf); NOW not cleared when RequestAsync refused; real WinRT errors in controls not logged; SessionsChanged token leak if CurrentSessionChanged subscribe fails; listener coalescing untested
Task 7: checklist: stale previous cover across rapid Spotify track changes; cover re-read on artist change accepted
Task 8: dispatched (base 17ede2d, sonnet)
Task 8: Ruling: accepted all(media,web) signature, cache-only music_playlists, extra dispatch commands, when::norm pub(crate), fullscreen::active() — cost: none.
Task 8: complete (commits 17ede2d..8c483a7, review clean, approved)
Task 8: minor (deferred → FINAL WAVE): norm misses á í ó ú ñ œ + no trim; exact match not preferred over prefix; Spotify URIs not type-validated; volume delta w/o device no transfer + misleading "pas les deux"; forbidden-routes test weak (allow-list over tool tests); failed playlist refresh not retried for 10 min
Task 9: dispatched (base 8c483a7, sonnet)
Task 9: fix round 1/5 dispatched (progress loop scoped to Progress + stop at end, hex colour validation, devices/playlists reload scoping, static wave under reduced motion, + minors)
Task 9: minor (deferred): ambient layer jump on <1.2 s track changes; hash-gradient playlist tiles (no covers, CSP)
Task 9: fix round 1/5 (all addressed; feef0c5..743bf91)
Task 9: complete (commits 8c483a7..743bf91, review clean after 1 round)
Task 10: dispatched (base 743bf91, sonnet)
Ruling P4-R3: "offline" = all probes network-failed (status None) AND Windows NLM reports no Internet; otherwise an all-fail round is a real outage (sites are subdomains of one VPS, so the plan's all_failed rule would hide the main outage) — cost if wrong: on a captive/odd network NLM may say "connected" and the user gets one outage notice per site.
Task 10: fix round 1/5 dispatched (P4-R3, 1440p layout: 3-row site list sorted by severity + compact dock + Importer folded, apply_round in core with tests, + minors)
Task 10: minor (deferred): 401/403 count as Down; https→http downgrade and private redirects allowed; flaky site Down/Recovered pairs (no hysteresis)
Task 10: fix round 1/5 (all addressed; c31f181..78e1e09)
Task 10: complete (commits 743bf91..78e1e09, review clean after 1 round)
Ruling P4-R4: the residual 1440p scroll fade (row 1 music/mails/calendar ≈ 492 px) is a panel-wide layout issue, moved into Task 12 Step 1 (fit at 2560×1440 with no .more fade) — cost: Task 12 grows by a layout pass.
Task 10: minor (deferred): favourite names tooltip-only in compact dock (no focus tooltip for sighted keyboard users)
Task 11: dispatched (base 78e1e09, opus)
Task 11: BLOCKED on 1Password signing; work staged; review on staged diff
Task 11: committed a4c10fb after user unlocked 1Password (review running on identical staged diff)
Task 11: Ruling: accepted real path zenytt-desktop.exe, rmcp ~3.4 with manual spawn, VpsStatus{zenytt,view,error} + Topic::Sites reuse, fake server as example, out-of-scope one-line T10 test fix, first server only — cost: none.
Task 11: complete (commits 78e1e09..a4c10fb, review clean, approved)
Task 11: minor (deferred → FINAL WAVE, should fix): closed_at reset only in poll() (early stop / immediate stop after fullscreen); 1 s loop in fullscreen with panel open; vps.status sends raw Zenytt error + server name to LLM (sensitive_output); orphan child during handshake / crash → Job Object KILL_ON_JOB_CLOSE; unexplained first failure of cargo test zenytt; 100 KB truncated list_sites; server name not sanitized; rmcp-macros 3.9 vs rmcp 3.4; UNC zenytt_path accepted
Task 11: checklist: real Zenytt (unapproved key, no AI-enabled server, cold SSH near 25 s), RunEvent::Exit on tray Quitter
Task 12: dispatched (base a4c10fb, sonnet)
Task 12: fix round 1/5 dispatched (Minuteur missing in 1440 capture — re-measure at true 1440 viewport via CDP, save measurements; checklist numbering; §6.4 date; artist cover item)
Task 12: fix round 1/5 (Minuteur was a mid-animation capture; true 1440 measured: grid 974/974, no .more, timer opacity 1; docs minors; a4c10fb..ed6d8c1). Controller verified measurement file.
Task 12: complete (commits a4c10fb..ed6d8c1, review clean after 1 round)
Final review (opus): fix wave needed — 3 important (truncation notice + text cap, OAuth sentinel capture test, References fold/cap), 7 medium (subject once, read checks account, sensitive_output vps/sites, masked expiry + SEND_MAX card, playlist exact match + norm accents, zenytt fullscreen 1 s loop + closed_at, smtc emit on change), doc port fix. List in final-fix-list.md.
Final review: deferred → plan 5: journal_args allowlist (incl. send recipient, music queries), agenda.delete readonly guard, Spotify URI validation, UNC zenytt_path. → plan 7: remaining T1/T2/T3/T4/T5/T7/T8/T9/T10/T11 minors listed in the review + Spotify playlist cache after account removal.
Final fix wave: done, BLOCKED on 1Password signing (staged code; doc unstaged for separate commit); re-review on working diff
Final fix wave: re-review all 11 ADDRESSED, no new breakage; waiting on 1Password to commit
Final fix wave: committed a634504 + 0b2aa87
Plan 4: complete — merged to main
