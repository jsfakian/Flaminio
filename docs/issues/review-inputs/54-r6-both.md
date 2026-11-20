# #54 plan review, round 6 (incremental), design-code and design-security

Scope: `git diff fcce15b..4328ca1 -- docs/issues/54/ ':!docs/issues/54/report.md'` in /home/jsfakian/Documents/src/flaminio-54 (HEAD a01b80d differs from 4328ca1 only in report.md, so the line numbers below hold). I checked the changed lines against several sources:
- #15's design (S1, S2, S8, S9) and its `device-lifetime-contract.md` at revision 5;
- #48's design (the probe, the records, the device states, SIGTERM, attach of a kept volume) and its report;
- #12's S4 and `begin_attach`;
- #74's helper `add`;
- #79's `initialization-contract.md`;
- `flaminio-io/src/ublk.rs:907`.

I ran nothing.

## Round-5 findings
- **R5-C1:** fixed. `device-info` answers `ok none` and `detach-device` answers `ok absent`. Contract revision 5's grammar is unchanged (contract:19-25). `is restoring here` maps to `FailedPrecondition`, alongside #15's stopping (flaminio-15 design.md:401). #79's form is handed off; #79 returns only the matching op/purpose (79 initialization-contract.md:114).
- **R5-C2:** fixed by refusal rather than by the add form. `--privsep` is refused before `reserve`. The kernel-side argument holds: no recovery flag is set today (`ublk.rs:907`), and START_USER_RECOVERY needs a recoverable, quiesced device. The ADR line overstates the cause (Nit 1).
- **R5-C3:** fixed. The slot is freed when the entry ends, there is a 30 s retry after the helper's own detach only, and the test is extended. All four round-5 Nits are fixed; 2,221 bytes is correct (11 + 16 × 138 + 1, plus 1).

## New findings

### R6-C1 (Medium): the round's new cases have no tests, and the late start's resume can meet a starting entry nobody answers
- **Claim:** three of the cases added this round have no named test:
  - the late start (contracts.md:286-287);
  - the restart scan and `flaminio-io cleanup`, `--expired` included (contracts.md:288-291). `restore_device_not_kept_across_restart` covers only a start.
  - a deposed entry keeping its count and the restore slot until restore-detach, the sweep's `delete` or a restart ends it (contracts.md:282-285). `restore_entry_states` checks only `list` and `status VOLUME` after a deposition (design.md:318).

  The late start also has a gap:
  - After `no answer in time`, the entry is still starting for up to METADATA_LIMIT, plus `add_device`'s 10 s, plus START_LIMIT (flaminio-15 design.md:81-87, 124-126, 165).
  - A prompt `resume` reads `restore-list` and sends restore-detach. Neither says what it does for a starting restore entry. #15's own `detach` of a starting entry answers `error: VOLUME is attaching` (flaminio-15 design.md:368).
  - Neither the helper's "counts as ended" list nor its retry list (contracts.md:385-393) names such an answer, so the resume exits 1 for an unstated reason. If it got past that, restore-start would meet `is restoring here`, which is not retried (contracts.md:278-279).
- **Fix:**
  - State that `restore-list` shows a starting restore entry.
  - State that restore-detach of a starting entry answers `error: VOLUME is attaching`, and that the helper retries it on the same 500 ms / 30 s schedule.
  - Add tests:
    - `late_restore_start_ended_by_resume`: a socket timeout, then a resume while the entry is starting and once it is running;
    - `deposed_restore_entry_holds_slot` (`limit:` and `restore slot taken` until restore-detach, after which they are free);
    - `cleanup_leaves_restore_record` (with `--expired` too).

### R6-C2 (Low): SIGTERM. restore-start is not refused after the signal, and #12's preStop check cannot see a restore entry
- **Claim:** contracts.md:292-294 says #12's S4 "holds whole on SIGTERM (the signal waits until the entry ends)". Two parts of S4 are not carried over.
  - **No refusal after the signal.**
    - #12's `begin_attach` refuses every attach once SIGTERM is requested (`error: the node is shutting down`, flaminio-12 design.md:235-237, 464-465).
    - restore-start is not built by `begin_attach`, and the plan gives it no such refusal.
    - So a helper's resume, or its own restore-start after a return to Incomplete, can add a new restore entry during the drain. That keeps deferring the exit until the 3600 s grace kills it.
  - **The idle check sees `none`.**
    - #12's preStop hook and `pod_nodes_idle` decide "idle" by `flaminio-agent status --none`, which exits 0 on plain `status`'s `ok none` (flaminio-12 design.md:468-485).
    - Plain `status` never names a restore entry (contracts.md:254-255), so a node that is mid-restore reports idle.
    - The process then does not exit: `should_exit` counts the restore entry (flaminio-48 design.md:326, 335-336), so it waits out the grace period.
  - This is not a safety issue: the device is unrecoverable, and the resume replays.
- **Fix:**
  - restore-start refuses after SIGTERM with #12's `error: VOLUME: the node is shutting down`, before `reserve`; the helper exits 1.
  - Either make `status --none` read `list` (a hand-off to #12/#15), or name the preStop result as a risk.
  - Extend `sigterm_waits_for_restore_entry` with a restore-start after the signal, refused. Run it without #48 too: #15's `should_exit` over the registry.

### R6-C3 (Low): "#48 is not review-clear" is stale
- **Claim:** this is one of the two stated grounds for deferring `--privsep` (decisions.md:347; design.md:484) and the #48 simulator scenario (decisions.md:294; design.md:481). #48's report records "Status: clear" for round 5, 2026-11-17 (flaminio-48 report.md:173; commit 6886634). The deferral is the user's call and still stands on its other ground: the intent does not need `--privsep`. The stated reason is wrong, though.
- **Fix:** say "#48 owns the helper's keep path and `RestartKept` and is not implemented", or "approved and review-clear, but not landed". Correct all four places.

## Nits
1. **adr-proposal.md:40, "the helper's `add` cannot leave the recovery flags out":** this is true only with #48. #74 alone adds with `ctrl_flags` 0 (74 design.md:58). Say "under `--privsep` with #48", as contracts.md:315-318 does; the refusal can stay unconditional.
2. **contracts.md:279-281:** restore-start answers `is resuming here`, but #48's answer for the same state is `error: VOLUME is resuming` (flaminio-48 design.md:710). Use #48's wording, or say that restore-start has its own.
3. **contracts.md:261, "whatever the tuple":** read it as "whatever well-formed tuple". A malformed tuple still fails before any effect (contract:31-32).
4. **The helper reads `restore-list` twice:** contracts.md:320-321 says "before its first metadata call", and contracts.md:382 says "immediately before restore-start". Say both. Also give the exit code for restore-start's `restore needs direct mode` arriving after AcquireRestoreWriter, which happens if the node restarts under `--privsep` between the two reads (exit 1, restore left Incomplete).
