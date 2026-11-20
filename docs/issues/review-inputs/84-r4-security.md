# Issue #84 — plan review round 4 (incremental), focus: design-security

Diff reviewed: `git diff d94baaa..317ef09 -- docs/issues/84/ ':!docs/issues/84/report.md'`
(checkout `/tmp/flaminio-plans-2026-11-12/84`; HEAD adds only `report.md`). The code was read
only; no command was run against the lab or the worktree.

## Earlier findings

- R3-S1: fixed. The `cleanup` verb, the recorded process group and step 5's background jobs are
  cut; the check signals nothing.
- R3-S2: fixed in part. `-k 10` is there and the `cleanup` wait is gone, but the 370 s bound
  that replaces them does not follow: R4-S1.
- R3-S3: fixed. The check calls no `delete_volume`; the Lab section says what the operator's
  `vol-delete` starts, and with which settings (`scripts/lab.sh:1733-1751`, confirmed).
- R3-S4: fixed. Only `none` is `unsupported`; every other answer is `fail`/`guard`, with rows.
- R3-S5: fixed. Every writing `dd` has `conv=nocreat`; `blkdiscard` never creates; row added.
- R3 Nit: fixed. No record; only an id matching the check's pattern is printed after `VOL=`.

Checked and found sound in the new text: the run id and the token
(`^[A-Za-z0-9+/=]+$`, the alphabet of `node.rs:446-456` and of the service's JSON `bytes`) are
validated before `ENV_VARS` (`lib.sh:157`); `VOL=<run> make vol-delete` works as printed
(`lab.conf:108`, `lab.sh:728`), also for an id that was never created or is only a tombstone;
a tombstone goes once confirmed (`service.go:655`), so a finished `vol-delete` does not make
the next run `leftover`; at most one `bc-` volume exists at a time.

## Findings

### R4-S1 — Low

**The guest's 370 s bound does not follow, and the operator is told to rely on it.** The
deadline is checked "before each of steps 2–7"; each command that opens the device is bounded
by 70 s. That gives 300 + 70 only if a step runs one such command. Step 5 runs two writes, two
zero-guard reads and `blockdev`; step 7 reads four ranges; the identity guard's `status` goes
through `control`, whose socket waits 180 s and is no "command that opens the device". A slow
node, short of the timeout, gives well over 500 s. Second, the order of `timeout` and `sudo` is
not stated, and the stubs list both: `timeout -k 10 60 sudo dd …`, run as the lab user
(`lab.conf:19`), cannot signal its child, because `sudo` becomes full root so that its caller
cannot kill it; `timeout` then waits for ever, and the fixture, with a stub `timeout`, passes.
After `interrupted` the operator waits "370 s" and runs `vol-delete` under a guest that still
writes: the case R3-S5 was about.

- Evidence: `kernel-check.md:55-59,97-98,165,167`; `decisions.md:64-67`;
  `scripts/guest/common.sh:22-30`.
- Fix: check the deadline before every command that opens the device or the socket; put the
  `status` call under the same `timeout`; write the form once, `sudo timeout -k 10 60 <command>`.
  Then 370 s holds. Or drop the number: the operator waits until `/run/flaminio-bc` is gone.
  Fixture: the deadline passes between step 5's two writes, and the second does not start; the
  recorded calls show `timeout` after `sudo`.

## Nits

- `kernel-check.md:98-99`: "once no `block-contracts.sh` runs there" names a process that never
  exists under that name: `run_on_vm` runs a guest script as `bash /tmp/tmp.<x>`
  (`lib.sh:126,157`). An operator who looks for it finds nothing and removes the directory under
  a live run. Name a test that works, e.g. `pgrep -f /dev/ublkb` finds nothing on NODE (every
  bounded child has the device path in its arguments).
- `kernel-check.md:60-61` against `:160`: the trap "removes the trace instance and the
  directory", and the row for a `run` that finds the directory present asserts only "the
  directory is left". Say that the trap is set after the `mkdir` succeeded, and add "and the
  trace instance" to that row, so a second `run` cannot remove a live run's instance.

Reviewed commit: 87a60b3e84e95a7170708ed27b11fa24872ef11f
