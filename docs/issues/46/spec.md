# Issue #46: Scrubbing and repair — specification

Step 1 of the issue: the pass that finds ([intent](intent.md)). Settled choices and accepted
risks: [decisions](decisions.md). How: [design](design.md). Each choice a human made is under
[Open questions](#open-questions), and a rule that follows one names it.
Settled at approval, 2026-11-13: Q1 to Q6, each with its recommendation (see decisions.md,
Settled at approval). Changed after approval by the plan review's round 1: the Finding term,
S1, S3, S4, S7, S9, S11, S12 and the tests; by its round 2: S1 and the tests; by its round 3:
a test's wording; by its round 4: a test (see decisions.md).

## Terms

- **Disk:** the device of a node's replica store. A node has one today.
- **Stored block:** a 4 KiB block of a data cluster that a map entry of a chunk replica
  references, in any layer. The map holds its CRC-32C. Unwritten and zero ranges store nothing.
- **Pass:** one walk over a disk that reads and checks every stored block. A node runs at most
  one at a time.
- **Mismatch:** a stored block that, read from the disk, does not match its checksum.
- **Unreadable block:** a stored block whose device read fails.
- **Finding:** a mismatch that persists (S3), or an unreadable block. It names the volume by
  the store's id for it, the chunk, the block and the kind. A replica keeps no volume name:
  the id follows from the name by a fixed rule (S12). It says the medium does not hold what
  the map records. It does not say the block is current: a stale replica's blocks match their
  checksums.

## The invariant

**A pass only reads, and tells the truth both ways.**
- Nothing a pass does or finds changes what any read returns, any replica's data, fence,
  session, uncertain set or membership, or any metadata record.
- A block whose stored data matches its checksum is never a finding.
- A pass that ends complete has checked every stored block that stayed in place from its start
  to its end.

No replication rule changes and `flaminio-repl` is not touched, so this step adds no simulator
check. The repair (step 3) brings the safety rule, its check and its injected bug.

## Behaviour

### The pass

**S1. Where it runs.** Every `flaminio-io node` runs passes over its own disk: with or without
an attached volume, for every chunk replica on the disk, whether or not its volume has a
target, a session, or a place in any ISR. A pass needs no token and no epoch, and a replica's
fence does not stop it: its data goes to no one. A pass reads the medium, not the page cache:
a node whose store runs buffered (`--buffered`, a development mode) runs no pass, and its
status says so. A node under the privilege-separation helper (#74) runs passes as any other.

**S2. Coverage.** A pass reads each stored block that exists at its start once, and checks it.
- **A block rewritten, discarded or deleted during the pass** may be skipped: what replaced it
  was checked when it was written. A chunk created during the pass may be skipped whole.
- **A chunk deleted during the pass** (a volume deleted, or created again, docs/issues/30)
  yields no finding from then on.
- **Nothing else is read:** not unwritten ranges, free clusters, metadata pages or the journal.

**S3. No false finding.** A mismatch is a finding only when it persists: the block is read a
second time, and both reads fail the checksum the map holds for the same cluster. A block
that a write, a discard or a deletion was applied to during either read is not a finding: it
is skipped (S2). Writes in place and redirected writes both race a pass. A pass reads a block
a bounded number of times and waits for no write, so writes to a chunk never keep a pass from
ending.

**S4. Unreadable blocks.** A device read that fails is read again block by block: a block
whose own read fails is an unreadable block, and the others are checked as usual. The pass
goes on. A read error does not poison the store, as a failed local client read does not.

**S5. How a pass ends.** Complete (S2 held to the end); stopped (an operator asked, or the
node shut down); or failed (the store became unusable), with the reason. A pass ends with its
process and is not resumed: the next one starts from the beginning.

### Beside other work

**S6. It changes nothing.** The invariant's first line. After a finding the replica still
serves that block as today (intent, Why 4 and 5): the target is not told, the replica stays a
member, and no repair is written.

**S7. Throttle.**
- **A rate limit:** a pass reads at most the configured number of bytes from the disk in any
  second, with a small fixed number of reads in flight.
- **Default:** 32 MiB/s (Q4), until the lab measurement settles it.
- **It never holds the engine:** while a pass's read is in flight, however long it takes,
  client I/O, replication requests, heartbeats and metadata replies are handled as without a
  pass.
- **What it adds to the disk** is one read in flight, at the rate. On a device that serves
  that read beside its other I/O, no session fails, no request times out and no lease lapses
  because of a pass. A device that stalls its other I/O on a read (a failing sector) is
  outside this rule: isolating the classes of I/O on one device is #75's.
- **It takes no bulk slot** of any target, and sends no message.

### Running it

**S8. Automatic and on request.**
- **Automatic** (Q2, Q3): a pass starts one period after the node started, and then one
  period after the last pass ended, however it ended. The period is 14 days by default; it can
  be set, or set to off, when the node starts. The clock is not kept across a restart: a node
  restarted more often than its period runs no automatic pass. The node logs when the next
  pass is due.
- **On request,** through the node's control socket (local, root only): start a pass, stop the
  running one, show the status. A start while a pass runs starts none and says so. The
  requests take no volume or other id.
- **The lab** gets the same three through `scripts/lab.sh` and the Makefile, per VM.

**S9. Status and findings.**
- **The status shows,** for the running pass and for the last one that ended: its start and
  end, how it ended, the rate limit, blocks checked, blocks skipped as rewritten during their
  read (S3), an estimate of what is left, and its findings. And for the disk, since the node
  started: passes ended, blocks checked, mismatches, unreadable blocks.
- **No free text:** every string in the status is an id or a fixed word; an error's text goes
  to the node's log only.
- **Each finding is logged once,** when found.
- **Bounded:** a pass keeps and logs at most a fixed number of findings in detail; beyond it,
  it only counts. The counts are exact.
- **A finding is what the pass read.** A later write to the block does not remove it; the next
  pass no longer lists it. Whoever acts on a finding (step 3) checks the block again.
- **Kept in memory only** (Q2): a restart forgets findings, counts and the last pass.
- **No block data** appears in the status or the log.

### The rest

**S10. Unchanged.** The on-disk format; the replication protocol and its frames; the metadata
service; `Digest`, reconciliation, resync and both read paths, with their faults (Q5);
`Store::fsck` and the offline `flaminio-io digest`. Nodes are upgraded in any order.

**S11. The lab check,** a script under `scripts/checks/`, run on request. It works on a volume
it creates, and takes none from its caller (#84's rule for a check that writes). It needs
nodes built with the lab-only fixture, and stops before any write on one without it. With its
volume at RF 2, full ISRs, and fio writing and verifying on the target:
1. it picks a stored block of the volume outside the range fio writes, on a replica of its
   chunk away from the target's node, saves it, overwrites it on the disk under the running
   node, and names it;
2. a pass on that node ends complete with exactly that block as a mismatch;
3. a pass on the node of the chunk's other replica ends complete with no finding;
4. no ISR changed, no session failed, and fio's verification passes;
5. whatever failed before, it puts the saved block back if the running node still holds the
   block as it was when saved, and a pass on the node then finds nothing. Otherwise (the node
   restarted, or the block changed) it writes nothing and fails: the block goes with the
   check's volume when the operator deletes it. The check says that nothing repaired the
   block.

**S12. Docs.** `docs/lab.md` (the commands, the check and its build, the automatic pass off
for a measurement, the rule that gives a volume's id from its name); the Makefile's help;
`dataplane/README.md`; ARCHITECTURE §12 and the README roadmap say what is built (the pass)
and what is not (comparison, repair).

## Tests

- **S2, S3 (store, unit and property):** a pass over a store with layers, discards and zero
  ranges checks each stored block once and nothing else. Random writes (in place and
  redirected), discards and chunk deletions racing a pass, with no damage: no finding, ever.
  With blocks damaged on the device: exactly those, each once, unless rewritten first. A
  damaged block under continuous writes to the rest of its chunk is reported, and the pass
  ends. A block whose entry changed between its two reads is skipped. Two injected bugs must
  each fail a test: a mismatch reported at its first read, and one reported for a block
  written during each of its reads.
- **S4, S5 (store):** a device failing one block of a larger read: that block is recorded,
  the others are checked, the pass completes, the store is not poisoned. A poisoned store ends
  the pass as failed, and a write that fails while its block is being read gives no finding.
  A read that comes back short is a read error, never a mismatch.
- **S1, S6 (engine):** a pass on a node with a target, and on one with replicas only, fenced
  at any epoch. With a finding on an ISR member's block: the same replies, ISR, sessions and
  metadata calls as a run without the pass. A pass with a read in flight across an attach and
  a detach goes on. A buffered node runs no pass and its status says so; a node under #74's
  helper runs one.
- **S7 (unit, engine):** with a clock under test, the bytes read in any second stay within the
  limit, and an idle node of each kind comes within 10% of it. Woken only at the engine's
  next deadline, the pass's deadline is never in the past after a step, and a wake at it
  reads or scans. With a pass's read held and never
  completed, the engine answers writes, reads and heartbeats. A device that stalls its
  other I/O is not tested: it is outside the rule.
- **S8, S9 (unit):** the period, off, and the clock after a pass; rates and periods out of
  range refused; the socket's three requests, a start during a pass, malformed lines refused;
  the bound on findings, with exact counts; an error text with quotes and newlines leaves the
  status one valid line.
- **S8, S11 (script tests, `make lint`):** the lab commands and the check with the VMs faked:
  each step's stop, each guard before a raw write, no run without full ISRs or on a build
  without the fixture; the block put back after a failure; no write once the block changed or
  the node restarted; the restore run again after a killed run. A build without the fixture
  refuses its lines (unit). The lint that keeps the fixture out of other builds fails on each
  doctored manifest and build line.
- **The lab, on request:** S11. S7's measurement, by the performance plan's rules: fio's 4K
  QD32 and 1M reads and writes on vol1, three interleaved runs with and without a pass, on the
  target's node and on the other replica's; and a pass's measured rate and length.

## Open questions

Each is a choice for a human, with a recommendation the rules follow.

- **Q1. Is the pass alone the first step?** Recommended: yes. It is built and checked alone,
  and steps 2–4 read its findings. The issue's "done when" asks for the repair too, so the
  issue stays open. The other choice: ship the pass only with the repair.
  Settled at approval, 2026-11-13: yes, the pass alone is the first step; the issue stays open (see decisions.md, Settled at approval).
- **Q2. Do findings and the last pass's time survive a restart?** Recommended: not yet (S8,
  S9). Keeping them is an on-disk format change (a compat feature of format v1), and the next
  pass finds the blocks again. The cost is S8's: no automatic pass on a node restarted within
  its period. The other choice: the format change now.
  Settled at approval, 2026-11-13: not yet; findings and the last pass are kept in memory only (see decisions.md, Settled at approval).
- **Q3. Is the automatic pass on by default while nothing repairs?** Recommended: on, one
  period after the start (S8): a finding is worth having before the other copy is lost. Other
  choices: on request only until step 3; or a first pass soon after every start, which covers
  a node restarted often but reads beside the resync a restart brings.
  Settled at approval, 2026-11-13: on, one period after the start (see decisions.md, Settled at approval).
- **Q4. The default rate.** Recommended: 32 MiB/s (about 37 TiB in 14 days; a full 100 GiB lab
  disk in 53 minutes), kept if the lab's results with a pass stay within the 10% spread the
  performance plan allows between runs, and lowered otherwise.
  Settled at approval, 2026-11-13: 32 MiB/s, kept within the 10% spread, lowered otherwise (see decisions.md, Settled at approval).
- **Q5. The fault of Why 5, on its own issue?** A bad block read through the replica service
  drops the member, and the resync leaves it bad. It exists today and needs no scrub to fix.
  Recommended: file it now, ahead of step 4.
  Settled at approval, 2026-11-13: filed now on its own issue, ahead of step 4 (see decisions.md, Settled at approval).
- **Q6. Metadata pages and the journal:** checksummed, read at recovery only, not in the
  issue. Recommended: a later step of #46, after the repair.
  Settled at approval, 2026-11-13: a later step of #46, after the repair (see decisions.md, Settled at approval).
