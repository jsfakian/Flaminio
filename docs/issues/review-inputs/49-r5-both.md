# Plan review: issue #49 (Chaos test suite), round 5, design-code and design-security

Scope: `git diff 32b9784..a37d46c -- docs/issues/49/ ':!docs/issues/49/report.md'` in
`/home/jsfakian/Documents/src/flaminio-49`. Checked against `origin/main`'s
`scripts/guest/chaos.sh` (:12-16), `scripts/guest/node.sh` (:12, :65: `sudo systemd-run
--unit=flaminio-node "$bin"`, so MainPID is `flaminio-io` itself), `scripts/lib.sh`
(`run_detached`), the spec's S9/S10 and the lab step (spec.md:156-158, 4 rounds).

Counts: High 1, Medium 1, Low 1, Nit 3.

## Earlier findings

- R4-B1: fixed as a rule (in flight now means issued, design.md:62-66, :139-143; decisions say
  so), but the Issued mechanism does not fit the client's I/O primitive (R5-C2), and the windows
  with no write issued now fail sound runs (R5-C1).
- R4-C2: fixed (`kill-at`: clock, `kill(2)`, clock in one process, design.md:238-249; End offsets
  40/80; S10 split at the second sample; sim and `main_test.go` cases).
- R4-C3: fixed (45 s bound after `stop-client`, SIGKILL, named, run fails; script test).
- Nit 1: fixed (inexpressible sim case replaced). Nit 2: fixed (design.md:98-100). Nit 3: fixed
  (reason appended to the report, design.md:277-278).
- No stale text from the round-3 withdrawal: design.md and contracts.md no longer use "kill mark"
  or "invoked in the history"; decisions.md marks round 3's choice, narrowing note and risk as
  withdrawn.

### R5-C1 — High
**Three of each kill-phase cycle's four phases now give `in_flight_at_kill` zero, so a sound build
fails most runs; the design names this as a risk instead of designing it away.** design.md:66-70
and decisions.md (round-4 risk 1). With in flight = an issued, unanswered write, a kill during
the burst's Invoke fdatasync, the checkpoint reads' fdatasync or their I/O, or the tail of a
burst where only reads remain, counts zero and fails the run (S9). The fdatasyncs are on the
VM's local disk (milliseconds each, comparable to a replicated write's latency), so the share of
a cycle with a write outstanding is plausibly well under one half. The lab step (spec.md:156)
runs 4 rounds, each ending in a kill, and every killed segment must count at least one: at a
per-kill chance p the run passes with p^4, e.g. about 6% at p = 0.5. The issue's "Done when" is a
suite that runs on a schedule and passes on a sound build; this one would fail most nights.
Under round 3's meaning those windows counted, so the round-4 fix moved the gap from S9's
meaning into the run's verdict.
**Fix:** gate the kill on an observed outstanding write. Give `kill-at` the segment's history
path (validated as the helper's segment directory is) and have it, before its first sample,
poll the file's tail (whole frames with a good CRC only, a bounded read, e.g. the last 64 KiB) until
some Write has an Issued frame and no Return, bounded at e.g. 10 s (past it, refuse: nothing
sent, the run fails); then sample, `kill(2)`, sample at once. The gap between seeing the write
and the kill is microseconds against a write's milliseconds. (Alternative: in the kill phase,
skip checkpoint reads and overlap the next batch's Invoke fdatasync with this batch's writes,
which still leaves gaps.) Tests: `main_test.go`, `kill-at` with a fixture history whose tail has
no outstanding write waits and refuses at the bound, sending nothing, and with one outstanding
write kills; the script test's helper order includes the gate.

### R5-C2 — Medium
**The Issued frame assumes a submit/reap I/O model, but the client issues blocking O_DIRECT
`Pread`/`Pwrite` (design.md:22), so "sampled after the submission returned: never earlier than
the issue" cannot hold, and the literal reading counts nothing.** contracts.md:49-52,
design.md:49-52 and :62-63, and the workload and recorder tests (design.md:284-285, "Issued after
issue, before any Return of its batch"). A blocking `pwrite(2)` returns only at completion: no
point exists where the batch is "submitted" and its completions not yet "reaped". If Issued is
written after the syscall returns, a write the kill cuts never returns, gets no Issued frame and
is never counted (`in_flight_at_kill` is always zero). If it is written after the goroutines are
started, a goroutine may not yet have entered the syscall at the stamp, so the stamp can be
*earlier* than the issue, contrary to the contract, and a write the target never saw can count.
**Fix:** define Issued for the primitive in use: each operation's goroutine, after its batch's
fdatasync, appends its Issued frame through the recorder and then enters `Pread`/`Pwrite`
directly; the stamp is the last point before the syscall, not after the issue. State in the
contract that an Issued write may not have reached the device for that goroutine-to-syscall gap
(and name it in the risks), and reword the tests as "Issued before its operation's syscall, after
its batch's fdatasync, and before its Return". (Or switch to `io_uring`, which `x/sys/unix` does
not wrap; not the simpler change.)

### R5-C3 — Low
**Two new decoder rules have no test.** contracts.md:53-55 makes an Issued whose kind or block
differs from its Invoke's malformed, and contracts.md:67-68 makes an End with one kill sample 0
without the other, or a second sample earlier than the first, malformed. The `sim/history.rs`
list (design.md:286) names four Issued cases (unknown op, second Issued, Issued after Return,
Return with no Issued) and neither of these; decisions.md calls them "the four Issued
malformations".
**Fix:** add to the S5-S8 row: an Issued whose kind or block differs from its Invoke's, an Issued
with a nonzero field it must leave zero, and an End with one sample 0 or the second earlier than
the first, each undecided.

## Nits

1. design.md:240: `sudo flaminio-history kill-at` will not find the tool: sudo uses its
   `secure_path`, and the copy's path is never named (design.md:25, "`cat > …`"). Name the copy's
   absolute path and invoke it by that path.
2. contracts.md:28, :31: the table still says `value_run_nonce` is "current run for Write" and
   `write_value_id`/`value_block_index` without an Issued exception, while :52 says an Issued's
   other fields are zero. Qualify the rows with "Invoke" (and "zero for Issued").
3. design.md:241-246, `kill-at` as root: say that `--pid` is a strict positive decimal greater
   than 1 refused before any `/proc` read (`kill(2)` with 0 or a negative PID signals groups or
   every process), that the cgroup check matches the unit's whole cgroup path, not a substring,
   and consider `pidfd_open` before the checks with `pidfd_send_signal` for the kill, which closes
   the PID-reuse risk decisions.md names as "not closed". Add the 0/-1 PIDs to `main_test.go`'s
   refusals.

Reviewed commit: 659b0e04810a3073ff981a2ed1c3918bd4a1cd36
