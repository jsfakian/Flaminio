# Plan review: issue #49 (Chaos test suite), round 4, design-code and design-security

Scope: `git diff ec75178..cb0acb2 -- docs/issues/49/ ':!docs/issues/49/report.md'` in
`/home/jsfakian/Documents/src/flaminio-49`. Checked against `origin/main`'s `scripts/lib.sh`
(`run_detached`, :161-195), `scripts/guest/chaos.sh` (:12-16), spec S9/S13/S14 and intent gap 4.

Counts: Medium 2, Low 1, Nit 3.

## Earlier findings

- R3-C1: fixed (design.md:87-94, :232-234; `stop-client` signals the PID after the kill helper's
  checks, `.rc` must be 0, script test). The SIGTERM path still has no harness-side bound, R4-C3.
- R3-C2: fixed (design.md:84-87, :247; contracts.md:97; `reset-failed`; the fake reports `failed`).
- R3-C3: fixed as wording (stamp time stated, window sentence corrected, sim case). But the
  meaning it settles on narrows S9 (R4-B1), and the new risk claim about when zero occurs is
  false (R4-C2).
- R3-C4: fixed (design.md:73-74, :258-259; contracts.md:73; 8 MiB bound for the other kept files,
  checked after the last write, with a test).

### R4-B1 — Medium
**"In flight means invoked in the history" narrows S9 below what the spec and intent promise,
and decisions.md says no rule was narrowed.** The intent's gap 4 (intent.md:46-49) is that "the
kill hits an idle volume": reconciliation of in-flight *writes* after a target crash never runs.
Spec S9 (spec.md:73-74) answers it: at the kill "the workload's operations are in flight", in the
ordinary sense of issued and unanswered. The design now counts as in flight an operation whose
Invoke batch's fdatasync had not returned at the mark (design.md:59-63, :132-135). Each kill-phase
cycle has four phases: the burst's Invoke fdatasync (nothing issued), the burst's I/O (writes
issued), the checkpoint read's Invoke fdatasync (nothing issued), the checkpoint read's I/O (only
reads). Only the second cuts an issued write. A kill in any of the other three passes S9's check
while the target crashed with no write at the device, which is gap 4 again for that kill; the
fdatasyncs are on the VM's local disk and are a realistic share of each cycle. decisions.md
("Rules narrowed: none in the spec's text") and the risk ("not that the SIGKILL cut one issued at
the device") name the gap but present it as no narrowing; an approved spec's rule cannot be
weakened by a design definition.
**Fix:** either record issue and require it: after submitting a batch, the client appends an
Issued frame (issue ns; a client SIGKILL does not lose completed `write(2)`s, so no fsync before
the next batch is needed), and `in_flight_at_kill` counts *writes* issued before the mark with no
successful Return; or take the narrowing to the human as a spec change (S9 states "invoked", the
intent's gap 4 is named as only partly closed). Test: a `sim/history.rs` case where the only
unanswered operations at the mark are unissued, or are reads, gives zero.

### R4-C2 — Medium
**The new claim "zero comes only from a mark between a drain's last Return and the next Invoke
stamps" is false, and the real zero case is likely: the mark-to-SIGKILL gap is longer than a
burst.** decisions.md (round-3 risk) and design.md:63-65. An operation answered with success after
the mark is not counted (design.md:134-135), and one stamped after the mark is not counted
either. The helper takes the mark with a `flaminio-history clock` process, writes `kill-mark`, then
kills through `sudo systemctl kill --signal=KILL flaminio-node` (`chaos.sh`:13 on origin/main):
an exec, sudo and a D-Bus call, tens of milliseconds. Back-to-back bursts on a healthy target
complete in a few milliseconds each. So every operation stamped before the mark is usually
answered with success before the SIGKILL lands, the operations the kill does cut were stamped
after the mark, and `in_flight_at_kill` is zero: the run fails on a sound build, and the script
test ("`in_flight_at_kill` zero fails") passes with fakes that hide the timing.
**Fix:** make the mark and the kill one step: a `flaminio-history kill-at --pid <flaminio-node
MainPID>` that reads the clock, sends `kill(2)` SIGKILL directly and reads the clock again; count
operations stamped before the first sample with no Return (or an error Return) after the second.
Correct the risk text. Test: a history where the mark-to-kill interval exceeds every operation's
latency, and a `workload_test.go` or script-test case checking the helper's one-process order.

### R4-C3 — Low
**After `stop-client`'s SIGTERM nothing bounds the harness's wait, so a client that does not exit
hangs the run, and S13's "named and fails the run" has no trigger.** design.md:87-94: the 30 s
drain is the client's own; "A client still running after that is named" now follows only the
SIGKILL's 30 s wait. `run_detached` polls with no deadline (`lib.sh`:177-193), and with the
wrapper alive and no `.rc` the unit stays active. A client stuck exiting with an uncancellable
io_uring request on a stalled ublk device, or a client bug, keeps the harness waiting forever.
**Fix:** after `stop-client`, wait a stated bound (e.g. 45 s) for `.rc`; past it SIGKILL the unit,
name the client and fail the run. Script test: a fake client that ignores SIGTERM is killed after
the bound and fails the run.

## Nits

1. design.md:267: "an unanswered operation whose Invoke was stamped before the mark and issued
   after it gives one" cannot be expressed in a history (it records no issue time), so it is the
   same fixture as the existing unanswered case. Drop it or replace it with R4-B1's test.
2. design.md:92-93: "Only after that SIGKILL does the harness's wait end on a unit that wrote no
   `.rc`" — the poll ends that way in every case (design.md:247-249); only after a SIGKILL is the
   return of 1 not a failure. Say that.
3. design.md:258-259: the 8 MiB check runs after the verdict and report are written, so a run it
   fails keeps a report that does not say why. Check before writing the report, or append the
   reason to it.

Reviewed commit: 32b9784e46e44d1a6724c68bed4c82de670e74cd
