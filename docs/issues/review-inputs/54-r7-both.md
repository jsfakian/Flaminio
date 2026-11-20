# #54 plan review, round 7 (incremental), design-code and design-security

Scope: `git diff 4328ca1..8a485ad -- docs/issues/54/ ':!docs/issues/54/report.md'` in /home/jsfakian/Documents/src/flaminio-54. 8a485ad is HEAD~1, "Address the review of the plan of #54, round 6"; HEAD f4ed629 adds only report.md. Line numbers below are HEAD's.

Checked against #15's design (errors :73-78, attach steps :79-98, detach by state :367-375, S10 :443-475), #12's design (`begin_attach` :230-262, `should_exit` and `status --none` :458-489, upgrade steps :543-550), #48's design (SIGTERM :326-340), `control/csi/tombstones.go:34-46`, `flaminio-io/src/ublk.rs:697-703`, `control.rs:33-35`. Nothing run.

## Round-6 findings
- **R6-C1:** fixed. `restore-list` shows a starting entry and restore-detach answers it `is attaching` (contracts.md:228-232, 245-247, 294-297); the sweep retries (`tombstones.go:40-42`); the helper's 500 ms / 30 s retry covers the ~20 s late-start tail; the three tests are added.
- **R6-C2:** fixed. The refusal sits in `reserve`'s lock acquisition (contracts.md:194-197) with #15's `error: VOLUME:` prefix; exit 1 at once (contracts.md:415-417); the preStop result is a named risk (design.md:451-458); the test runs without #48 but depends on #12 (R7-C1).
- **R6-C3:** fixed in all four places.
- **Round-6 Nits:** all four fixed.

## New findings

### R7-C1 (Low): the SIGTERM refusal and `sigterm_waits_for_restore_entry` rest on #12 but are not gated on it
- main has no SIGTERM handling; #12's S4 adds it (flaminio-12 design.md:235, 458). The refusal (contracts.md:194-197, design.md:155-158) reads a flag that exists only with #12, and the test (design.md:321) needs #12's `should_exit`. Only the `should_exit`-over-registry statement is gated (design.md:488-490), and #12's `begin_attach` list does not name restore-start.
- Fix: mark the refusal and the test "with #12, by whichever of #12 and #54 lands second"; add the cross-plan item that #12's `begin_attach` predicate also guards restore-start's `reserve` and `no_attach_after_sigterm` gains a restore-start; "lands second" among three plans should read "lands last".

### R7-C2 (Low): `cleanup_leaves_restore_record`'s `first_id` assertion fails on correct code in its "deleted" branch
- `first_id` is the highest of the `ublkc*` ids and the recorded ids (flaminio-15 design.md:102-107). After `cleanup` deletes a dead device and its record, neither source holds the id (flaminio-15 design.md:469), so the next start may reuse it. The assertion holds only for a left record and for a start's own scan (contracts.md:302).
- Fix: assert `first_id` above the id only for the record left and for a start after the kill with no prior `cleanup`; for the deleted branch assert only that the record and the device are gone.

## Nits
1. design.md:168-172: the retried answers omit restore-detach's `is attaching` (contracts.md:408-410).
2. contracts.md:409, decisions.md:388: "#15's own detach wait" — #15's detach of a starting entry doesn't wait; say "the same 30 s as #15's detach wait".
3. contracts.md:305-306, 310-311: under #48 only a *running* restore entry holds the process (flaminio-48 design.md:327, 332-334).
4. design.md:321, `restore_entry_states`: the sweep's `delete` answers `is attaching` only once GetVolume shows the volume gone, Deleting or of another operation (contracts.md:271-276).
5. adr-proposal.md:40-42: "so restore-start refuses under `--privsep`, with or without #48" — without #48 there's no cause; say "and, for one rule, also without #48".
