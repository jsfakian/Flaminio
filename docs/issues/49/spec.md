# Issue #49: Chaos test suite — specification

Step 3 of the issue, for the faults `make chaos` injects today ([intent](intent.md)). Settled
choices: decisions.md, after the review. How: design.md, after approval.

## Terms

- **Block:** 4 KiB of the volume's device, the unit the workload reads and writes.
- **Value:** what one write puts in a block. It names the run, the block and the write, fills
  the block, and can be told from any other content, torn or mixed content included.
- **Segment:** one attachment of the volume on one node, with the one client that runs there.
- **History:** a run's record of every operation the clients issued, segment by segment.
- **Unanswered:** an operation with no answer in its segment. A write answered with an error
  counts as unanswered: it may have taken effect. A read answered with an error says nothing.
- **Fault:** one of the four `make chaos` injects on the replica's side (intent, Why 1). The
  **kill** is the SIGKILL of the target's node that ends a round.

## The invariant

A run passes only when its history is complete and every block of it is linearizable, every
fault it counts took effect and was healed, and nothing it started is left. The check never
passes a history it could not decide, and no verdict compares two nodes' clocks.

## Behaviour

### The workload

**S1. Values.** Every write of a run carries a value no other write of any run carries. A read
records the value it found, or that the content is not a value.

**S2. Operations.** Reads and writes of single blocks, several in flight at once. On one block,
reads overlap writes and writes overlap writes (the simulator runs one operation per block at a
time). Each block's first operation in the run is a write, acknowledged before anything else on
it. Each later segment's first operation on each block is a read, answered before that segment
writes the block, so a write the failover dropped is seen. The blocks lie in several chunks, at
least one of which the fio load also writes.

**S3. The record.** An operation is recorded before it is issued, and again when it is answered,
with the time on its node's boot-time clock (`CLOCK_BOOTTIME`, monotonic). A client killed or hung
at any moment leaves a record of every operation it issued.

**S4. Segments.** A segment's client starts only after the volume is attached on its node. The
segment ends at a mark on the same clock, made once its client is stopped or its target's node
is confirmed dead, and before the next segment's client starts. An answer after the mark is not
used: its operation is unanswered. So every answer of a segment precedes every operation of the
next, with no clock compared.

### The check

**S5. Linearizable, per block,** by the simulator's rules (protocol §9), the recommendation of
open question 2. Within a segment, operations are ordered by their times; across segments, by
S4. An unanswered write may take effect or not, but not after the first operation of a later
segment on that block was answered.

**S6. Foreign content.** A read that found anything but a value written to that block in this
run is a violation: another block's value, an earlier run's, a torn block.

**S7. Never a pass by default.** The check fails, as undecided, a history that is malformed, that
lacks a segment or its end mark, in which a segment before the last acknowledged no write, in which
a later segment did not first read every block (S2), in which the last segment did not read every
block (S9), or that it cannot decide within its limits. The workload issues no more than the check
can decide.

**S8. The verdict** is pass, violation or undecided. A violation names its kind (a lost
acknowledged write, a stale read, a write that resurfaced, foreign content), the block, the
operations that cannot be ordered, and the faults in effect around them (S11). The same history
gives the same verdict anywhere, with no lab: the check runs again on a kept history (S14).

### `make chaos`

**S9. The run.** Rounds as today, the target alternating. The workload runs from the first
attachment to the end: through every round's faults, through each kill, and on the new target
after each failover.
- **The kill comes under load:** every ISR is full, as today, and the workload's operations are
  in flight. The fio load has ended.
- **The final read:** after the last failover, and the move back when there is one, with every
  ISR full and no write in flight, the last segment reads every block.
- **The fio load stays** (today's `write-load.sh` job; the recommendation of open question 3),
  with its verification after each failover. It and the workload never write the same block.
- **Only its own volume is written.** Before anything else (before it detaches, attaches or
  writes), the run refuses a `VOL` that is not a volume id, or that is named as another user's
  volume: `pvc-…` (the CSI driver's), `bench-…` (issue #9's), `bc-…` (issue #84's). Before each
  segment's client and each fio job starts, the node on that VM must say that its device serves
  `VOL`; otherwise the run stops, writing nothing.
- **The faults reach the checked blocks:** at each round's start, the chunk of every history
  block has its replicas on the round's target and on the node the faults hit, or the run fails.

**S10. What fails a run,** besides what fails it today:
- a verdict other than pass;
- an operation answered with an error while its segment's target was alive (the recommendation
  of open question 4): the faults hit the replica's side, and the target must keep serving;
- no operation answered between the last fault's healing and the kill: a stalled device;
- a fault that S11 cannot confirm.

**S11. Faults that count.** The same four kinds and the kill; none is added. Each fault is
confirmed in effect on its node after it is injected, and confirmed healed after it is healed.
A step that fails, or cannot be confirmed, fails the run: no status is dropped. The run logs
each fault's kind, node and times. Those times inform the report; no verdict depends on them.

**S12. The seed.** The run prints its seed and accepts one. The same seed gives the same
sequence of fault kinds. It does not give the same interleaving with the I/O.

**S13. Nothing left behind.** However a run ends (a failure, an interrupt, a verdict), every
fault of every kind is healed: the partition, the delay, a stopped node, fl-w's etcd member and
metadata service. No client and no fio job keeps running; one that cannot be stopped is named,
and fails the run. The same healing is a command by itself, safe to repeat, for a run whose
workstation lost its connection.

**S14. What is kept,** pass or fail, on the workstation: the run's history, fault log, seed and
verdict. None of what is kept or uploaded holds a capability token, a key or a journal excerpt.

**S15. Remote commands.** Every value the harness puts into a remote command comes from a fixed
set or is validated (a number, an id, a ublk device path) and quoted.

### Proof and seams

**S16. The check is shown to fail,** in CI, with no lab (the recommendation of open question
1). The workload runs against a device that lies in one way, and the check must name that
violation; against the same device without the lie, it passes. One lie per kind:
- **a lost acknowledged write:** a write acknowledged and dropped, within a segment and at the
  change of segment (seen by the next segment's first read);
- **a stale read:** a read served from before a write that was acknowledged;
- **a write that resurfaced:** an unanswered write applied after the next segment's operations;
- **foreign content:** another block's value, and a torn block.

**S17. Usable by another check.** The workload and the check take a device, a segment and a
history, and name neither `make chaos`, nor vol1, nor a lab node. Issue #56's check runs them
as they are. What a lost VM needs of the record is #56's.

**S18. Unchanged.** What the data plane and the control plane do: this step adds tests, tools
and scripts. `make node-fault`, `make drain-test`, `make partition-test` and their checks. The
lab's concurrency: no lock; a run still takes the lab over, by hand.

**S19. Docs.** `docs/lab.md`: what `make chaos` runs and checks, the seed, what is kept, the
healing command. `docs/ci.md`: the Lab workflow's `chaos` row; where S16 runs. The README's
chaos claim, the Makefile's help and `lab.sh`'s usage say what was checked (open question 5).

## Tests

- **S1–S4 (unit):** the workload on a file: unique values, two segments' clients on one file
  included; overlapping operations on one block occur; a later segment reads every block before
  it writes it; killed at random moments, every value on the file has its record; marks and late
  answers.
- **S5–S8 (unit):** the simulator's checker cases; hand-written histories for each kind, within
  and across segments (an acknowledged write in one segment, an older value read first by the
  next: a lost write); every undecided case of S7; histories of a correct register pass, and
  fail once one answer is altered; the same verdict on a second run.
- **S16 (in CI, `make contracts`):** each lie caught and named; no lie, a pass.
- **S9–S15 (script tests, `make lint`, the VMs faked):** the order of a round; a fault that does
  not take effect, or does not heal, fails the run; an interrupt at each step heals all four
  kinds and stops the clients; the healing command twice; one seed, one sequence; a violation
  and an undecided verdict fail the run and keep the files; refused values; a refused `VOL`, a
  device that serves another volume, a history block's chunk elsewhere, and a missing segment
  each stop the run; no kept file holds the fixture's token.
- **S17 (script test):** the workload and the check run with no lab variable set.
- **The lab, a later step, by hand:** `make chaos`, 4 rounds, with a seed, and the Lab
  workflow's `chaos` check: both pass. Recorded: per segment, the operations and the unanswered
  writes at the kill (more than none shows it came under load); the duration against today's
  20–30 min; the kept history checked again on the workstation, with the same verdict; the
  intent's fio measurement (Why 2) repeated with the VMs' fio, on a scratch file.

## Open questions

No accepted ADR is contradicted. Each is a human's choice; the rules follow the recommendation.

1. **What stands for "a build with an injected bug" in a lab check** (the issue's "done when").
   (a) A device that lies under the workload, in CI: it proves the check, not the lab. (b) The
   engine in process with a `flaminio-repl` switch on, in CI: the switches are `#[cfg(test)]`,
   and would need a build feature no release enables. (c) A mutated build deployed to the lab
   for one run: the lab has no lock. **Recommended: (a) now (S16);** (b), not (c), with steps 1
   and 2.
   Settled at approval, 2026-11-13: (a) now; (b), not (c), with steps 1 and 2 (see decisions.md, Settled at approval).
2. **Our checker, or Jepsen's (Knossos, Elle).** Ours has the rule for unanswered writes across
   attachments and adds no dependency; Jepsen's is the known tool. **Recommended: ours (S5).**
   Settled at approval, 2026-11-13: ours (see decisions.md, Settled at approval).
3. **Keep the fio load, or replace it.** It covers 4 GiB for corruption; the history covers few
   blocks, densely. **Recommended: keep both (S9).**
   Settled at approval, 2026-11-13: keep both (see decisions.md, Settled at approval).
4. **An operation that fails while its target is alive: a failed run, or one more unanswered
   operation.** fio's errors fail the run today. **Recommended: a failed run (S10).**
   Settled at approval, 2026-11-13: a failed run (see decisions.md, Settled at approval).
5. **The README's "a chaos run … loses no byte".** The 2026-10-09 run checked corruption and
   I/O errors, not loss (intent, Why 2). Reword it now, or once a run with the history check
   is recorded. **Recommended: now (S19),** and again after that run.
   Settled at approval, 2026-11-13: reword now, and again after that run (see decisions.md, Settled at approval).
