# Plan review: issue #49 (Chaos test suite), round 12, design-code and design-security

Scope: `git diff e208909..4c40c0a -- docs/issues/49/ ':!docs/issues/49/report.md'` in
`/home/jsfakian/Documents/src/flaminio-49`. The diff changes the kill phase (design.md:93-96), the
segment end step (design.md:121-125), the kill helper (design.md:264-270), the S3/S4 and S9/S10
test rows (design.md:349, 351) and decisions.md's round-11 entry. I checked them against the
recorder rules (design.md:52-68), the SIGTERM drain (design.md:115-120), `kill-at`'s gate
(design.md:293-306), `in_flight_at_kill` (design.md:164-168), `stop-client` and `chaos-heal`
(design.md:312-330) and S13 (spec.md:103-107).

Counts: Medium 1, Low 1.

## Earlier findings

- R11-B1: fixed in the rule. After `kill-ready` the client never exits on its own, and at a kill
  any `.rc` fails the run (design.md:93-96, 121-123). The SIGTERM path still drains for 30 s within
  the harness's 45 s, and `chaos-heal` still stops a client left waiting (design.md:325), so S13
  holds. The new test does not pin the rule, though (R12-B2), and the change silences a recorder
  failure (R12-B1).
- R11-B2: fixed. `test -e || test -L` catches a dangling symlink. `stat -c '%F:%u'` without `-L`
  reports a symlink as `symbolic link`, and GNU `stat`'s `%F` strings for a regular file are
  `regular empty file` and `regular file`. Both cases are tested (design.md:351).
- Nit (wait's failure conditions; "sends nothing"): fixed (design.md:264-269).

### R12-B1 — Medium
**After `kill-ready`, a recorder failure no longer reaches the run's verdict, and it can fake S9's
evidence.** design.md:64 says "Failed recorder stops issuance and fails the run". The only channel
for that is the exit code: "non-zero when its recorder failed" (design.md:118). The new rule says
that once the client has created `kill-ready` it "never exits on its own" (design.md:93-96). So
when a `write(2)` of a Return fails in the kill phase, the client stops issuing and waits. The
unit is then SIGKILLed at the kill, and no `.rc` is written. Nothing fails the run. Worse, a write
whose answer could not be recorded looks outstanding in the tail. `kill-at` can fire on it
(design.md:299-301), and `in_flight_at_kill` counts it as issued with no successful Return
(design.md:164-166). The S9 check, "at least one" (design.md:314), can then pass on a write that
was answered before the kill. Before round 11, a client that exited non-zero on a recorder failure
left a `.rc` that failed the run (round 10's rule).
**Fix:** design.md:93: make a recorder failure the one exception. The client exits non-zero at
once, so the new "a `.rc` present at a kill fails the run" rule (design.md:121-123) catches it.
Tests: in `recorder_test.go`, a recorder failure after `kill-ready` exits non-zero. In the S9/S10
row, a fake client that hits a recorder failure after `kill-at` fails the run.

### R12-B2 — Low
**The new S9/S10 case does not pin the "any value" rule, nor the window it guards.** The rule
changed from "present and non-zero" to "present (any value)" (design.md:121-123). The test is "a
fake client that exits non-zero after SIGUSR1 and before its unit's SIGKILL fails the run"
(design.md:351). It would pass under round 10's rule, because it uses a non-zero exit. "After
SIGUSR1" also covers a client that exits before creating `kill-ready`. The helper's 10 s wait
already fails that case (design.md:266-269), as R11-B1 noted for the earlier version. decisions.md
says "a fake client exiting after SIGUSR1", with no code, so it drifts from design.md.
**Fix:** design.md:351 and decisions.md: a fake client that creates `kill-ready` and then exits
**0** after `kill-at`'s SIGKILL and before its unit's SIGKILL fails the run.

Reviewed commit: 33d943371265aa1f39b0ae6d82270cc0fe35179b
