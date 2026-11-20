# Plan review: issue #49 (Chaos test suite), round 3, design-code and design-security

Scope: `git diff b026fe8..df9175f -- docs/issues/49/ ':!docs/issues/49/report.md'` in
`/home/jsfakian/Documents/src/flaminio-49`. Checked against `origin/main`'s `scripts/lib.sh`
(`run_detached`, :161-195), `scripts/guest/chaos.sh`, `scripts/guest/write-load.sh` and
`scripts/lab.sh:1633` (`valid_id`).

Counts: Low 4.

## Earlier findings

- R2-C1: fixed as a mechanism (`client-pid`, PID checks, the helper signals only that PID, the
  poll ends on an inactive unit). Two gaps in the new text: R3-C1 (SIGTERM still goes to the unit)
  and R3-C2 (a SIGKILLed unit is `failed`, not `inactive`).
- R2-C2: fixed (definition at design.md:124-127, the two windows at design.md:59-62, two
  `sim/history.rs` cases). The new "zero in a window" claim is not quite right, R3-C3.
- R2-C3: fixed (design.md:139-141 uses `valid_id`'s exact grammar, as at `lab.sh:1633`; `main_test.go` case).
- R2-C4: fixed for the segment files (`--max-bytes`, reservation per batch, checker refusal,
  three tests). The other kept files' share of the cap is R3-C4.
- R2-C5: fixed (design.md:267-269).
- R2-S1: fixed (design.md:200-204; a `tc qdisc add` over a non-default root fails with its
  exclusivity flag, so the check has no TOCTOU hole either; script-test case at design.md:258).
- Nit 1 (kind bullets overlap): fixed (design.md:154).
- Nit 2 (foreign content beyond S6): fixed (design.md:159-161).
- Nit 3 (ledger matching): fixed (contracts.md:88-89).

### R3-C1 — Low
**The SIGTERM before the move back still goes to the client's unit. That kills the bash wrapper,
so the client's exit status is never recorded and a client failure during the drain cannot fail
the run.** R2-C1 showed that a signal sent to the unit also reaches the `run_detached` wrapper
(`lib.sh:171-173`). Bash's default action for SIGTERM is to exit, so the wrapper dies before its
`echo $? > …rc`. The revision keeps this on purpose (decisions.md, R2-C1: "the SIGTERM before
the move back still go to the unit … the new poll rule covers"). Under the new poll rule
(design.md:236-239), that wait returns 1, and the harness ignores it because it "stopped that
unit". The client's own result is therefore lost in every segment that ends with a move back:
a recorder failure ("Failed recorder stops issuance and fails the run", design.md:51), or
answers still outstanding when its 30 s drain runs out (design.md:82-84). The history usually
stays conservative, but the rule that such a client fails the run has nothing to carry it.
**Fix:** send the SIGTERM the way SIGUSR1 is sent: to the `client-pid` PID, after the same
checks. The wrapper then survives and writes `.rc`, and the harness requires `.rc` = 0. Keep
the unit-level signal (and the "inactive without `.rc`" exit) for the SIGKILL alone. Script
test: a SIGTERMed client that exits non-zero fails the run.

### R3-C3 — Low
**"A kill mark that lands in one [window] gives `in_flight_at_kill` zero" is false for most of
each window.** design.md:59-62 says nothing is outstanding while an Invoke batch's fdatasync
runs. But each Invoke frame carries its `local ns` (design.md:68-69) and is written before that
fdatasync (design.md:49-50). So the checkpoint read's Invoke, or the next burst's, is stamped
before a mark that falls during the fdatasync. That operation is then issued to the killed target
and gets no successful Return, so under the new definition (design.md:124-127) it is counted:
`in_flight_at_kill` is one while nothing was issued at the kill. Only a mark between the last
Return and the next Invoke stamp gives zero. The decisions' named risk repeats the same claim. This
is the over-count R2-C2 raised (`>= 1` not showing that the kill cut an issued operation), moved
to a smaller window.
**Fix:** state when an Invoke is stamped, and either accept "invoked in the history" as S9's
meaning of in flight (say so, and correct the window sentence and the risk), or exclude an
operation whose batch's fdatasync had not returned at the mark (an issue time recorded per
batch). Add a `sim/history.rs` case for a mark between an Invoke's stamp and its issue.

### R3-C2 — Low
**After a SIGKILL the client's unit is `failed`, not `inactive`, so a wait for "inactive" never
ends.** design.md:82 waits up to 30 s "for it to be inactive", and the new poll rule ends "when
the unit is inactive and wrote no `.rc`" (design.md:236-237). When a transient service's main
process is killed by SIGKILL, systemd marks the unit `failed` (only SIGTERM, SIGINT, SIGHUP and
SIGPIPE count as a clean exit). A failed unit stays loaded until `reset-failed`. If the wait
checks for `inactive` literally, the kill path times out after 30 s and the backgrounded
`run_detached` loops forever: the R2-C1 hang again. The script test's fake
(design.md:257, "a SIGKILLed client unit with no `.rc` ends the harness's wait") passes if the
fake simply reports `inactive`.
**Fix:** say "not active (`inactive` or `failed`)" in both places, run `systemctl reset-failed`
on the client's unit after the kill (as `chaos.sh kill-node` does for `flaminio-node`), and have
the fake report `failed` after a SIGKILL.

### R3-C4 — Low
**`--max-bytes` subtracts "the other kept files", whose sizes are not known when a segment
starts, and the checker counts only the segment files. The "total artifacts 128 MiB" cap is
therefore still not enforced.** design.md:70-74: the budget is "128 MiB less the earlier
segments' sealed lengths and the other kept files". But the fault log keeps growing, and the
verdict, the report, `FLRUN001` and fio's summary lines (design.md:247-249) are written after
the last segment. The checker's refusal (contracts.md:72-73) sums only `sealed_length`. So the
segments can use up the budget and the later files push the run past 128 MiB, with nothing
refusing it. The S14 test (design.md:259) checks only the subtraction.
**Fix:** either make 128 MiB the cap on the segment files' total (the contract's rule), with
each other kept artifact given its own stated bound, or subtract a fixed, stated reserve for
those artifacts and have the harness fail a run whose kept directory passes 128 MiB.

Reviewed commit: ec7517829e51a232e44b1a64a0ec18f2f53cf8fb
