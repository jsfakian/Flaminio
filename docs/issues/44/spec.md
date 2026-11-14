# Issue #44: Dirty-region resync — specification

The issue's "Done when" as a lab check, and a resync that says what it did ([intent](intent.md)).
It builds none of the issue's four steps. Every rule follows the recommendation of open
question 1 (measure first); S7's sizes follow that of question 3. [Design](design.md);
[decisions](decisions.md).

## Terms

- **Member:** the replica node that goes away and comes back. **Away:** its data plane stopped.
- **Round:** one resync of one chunk onto one member, from its `Mapped` probes to its join, or to
  its abort.
- **Plan:** the blocks a round starts from. Its **source** is *dirty* (this target's dirty set
  for the member) or *whole* (the whole chunk).
- **Counts** of a round, in blocks:
  - *planned:* the plan;
  - *compared:* blocks whose digests the target got from both the source and the member;
  - *copied:* blocks of the copies the member acknowledged;
  - *discarded:* blocks of the discards the member acknowledged.
- **Summary:** a member's totals for one volume (S3).
- **W:** what the check writes while the member is away, in blocks.
- **Replaced target:** a new attach of the volume, so a new epoch; the node may be the same.

## The invariant

Counting observes. With the counts and the summary, a target sends the same messages and
metadata requests, in the same order, and holds the same protocol state, as without them.

## Behaviour

### What a resync reports

**S1. Every round is counted,** in `flaminio-repl`, by the terms above. A copy or a discard counts
once, when the member acknowledges it, whatever part of it the member's resync filter skipped.
One that was sent and never acknowledged is not counted.

**S2. A member's totals add up its rounds,** aborted ones included, over every chunk of the
volume, since its last summary (or since they were dropped, S3), or else since the target
started. They also hold: the chunks that joined, how many of those had a *dirty* plan in their
last round, and the aborted rounds.

**S3. The summary.** After a view in which a member with counted rounds is in the ISR of every
chunk it holds, the target reports its totals once, with the time from its first counted round
to that view, by the target's clock. Its totals then start again. The view may commit its last
join, or remove its last replica outside an ISR (a move's source). A member that holds no
chunk after a view gets no summary: its totals are dropped. A move's destination is a member
like any other.

**S4. The node logs each summary** on one line: the volume, the member's name, the target's
epoch, and every figure of S2 and S3. Nothing else carries it: no message, no metric, no
control command.

**S5. Totals go with their target.** A target that is deposed or detached reports nothing for
the rounds it had counted. The next target counts from its own start.

**S6. Nothing else changes** (the invariant): the protocol's messages and rules, the on-disk
format, the metadata service, and `scripts/checks/resync.sh` and `reconcile.sh`.

### The lab check

**S7. The scenario.** The check creates its own volume: 16 GiB in 64 chunks, RF 2,
`minInSyncReplicas` 1, attached on node T, every chunk with a replica on T and on the member.
It then:
1. writes data the member will hold, and waits for full ISRs;
2. stops the member;
3. writes W, random data, in half of the chunks only: part over data the member holds, part in
   ranges never written. W is at most a sixteenth of the data written in step 1, so that what
   a *whole* plan adds (the member's digests of all the source maps, and the discards) is not lost
   beside what W itself moves;
4. starts the member, and waits until it is in every ISR;
5. verifies (S10).

**S8. Two runs.**
- **Same target:** T's target serves throughout.
- **Replaced target:** the target is replaced while the member is away. Half of W is written
  before, by the old target, and half after, by the new one.

**S9. What each run reports:**
- the bytes the member received over the overlay, and the bytes it sent, from its start until
  it is in every ISR. The copies are received. The member's digests are sent: the target is on
  T, beside the source's replica, whose digests cross no network;
- the summary (S4) from T's log, and from it the time the resync took;
- the wall time of that interval, on one VM's clock, with its resolution: it includes the
  member's start and the polling.

**S10. Verification.**
- **Through the target:** every range written in steps 1 and 3 reads back, and ranges never
  written read zeros.
- **The member's own copy:** its chunks hold what T's hold, checked without a read that T's
  replica could serve.

**S11. A run fails** when:
- S10 fails;
- no summary for the member is logged, or more than one, or it counts an aborted round;
- *copied* exceeds W;
- **same target:** a chunk's plan is *whole*, or *planned* exceeds W;
- **replaced target:** a chunk's plan is *dirty*: the run did not test what it meant to;
- the member received more than W plus an allowance. The design sets the allowance, from the
  frame encoding and the transport's overhead on W. The bytes it sent have no limit;
- the member is not in every ISR within a limit the design sets (the allowance is sized for
  it), or the check loses its watch on the ISRs before it sees them full: an invalid run.

**S12. It leaves the lab as it found it.**
- It refuses to start, before it creates or stops anything, unless:
  - the sweep runs on T and on the member: the check never confirms a tombstone
    (docs/issues/14). T is fl-a and the member fl-b, the nodes with a sweep: not parameters;
  - each runs its data plane, with no volume attached: stopping a node removes its device;
  - they are the only alive nodes that are neither draining nor witnesses: placement would put
    chunks on any other, whose tombstone the check could not clear;
  - no volume attached on another node (one that is draining, or that runs but is no longer
    alive) has a replica on T or the member: its resync would be in the figures. An answer
    that cannot be read, or a volume that cannot be looked up, refuses;
  - its volume's id does not exist.
- On any exit after its create succeeded, it detaches and deletes its volume, and its replicas
  on both nodes. It deletes no volume it did not create.

**S13. Repetitions.** Each run goes three times, the two interleaved, the same day, on the same
binaries, whose commit is recorded. A figure whose spread exceeds 10% is reported as unstable.

### Results and docs

**S14. The results** go to `docs/benchmarks/`, with the logs under `data/`:
- per run, the figures of S9, as median and spread;
- the difference between the two runs: in bytes, each way, and in time, taken from the
  summaries; in total and per GiB the member held: the most that a record outliving the target
  could save at this size. The wall times are listed beside it, not subtracted: their
  resolution is coarser than the digest work;
- what was measured (16 GiB, the data written in step 1, W), and any figure for another size
  marked as extrapolated.

**S15. Docs.**
- `docs/lab.md`: the check, what it needs, and what it reports.
- Follow-ups §3: the result. Follow-ups §2 step 2: that the record of #44 needs it (intent,
  Why 9).
- Issue #44 stays open, its steps as they are. ARCHITECTURE §8.3 and §11.4 stay.

## Tests

- **S1–S3, S5** (unit, the target against in-memory replicas):
  - a member removed by this target and resynced: a *dirty* plan, *planned* equal to the blocks
    written since, *copied* equal to the blocks that differed;
  - a new target: a *whole* plan, *compared* equal to the blocks the source maps;
  - a round aborted and started again: both in the totals, each acknowledged copy once;
  - a copy over live writes, partly skipped by the filter: counted whole;
  - one summary when the last chunk joins, none before, totals at zero after; a move's
    destination; a deposed target reports nothing;
  - a view that removes the member's last replica outside an ISR: one summary; a member that
    holds no chunk after a view: none, and its totals gone.
- **S1, S6** (simulator): every run checks that in each round *copied* is at most *compared*,
  and that a joined chunk's last round has nothing left to copy or discard. The campaigns and
  every mutation check pass, each first catch on the seed it had before.
- **S4** (`flaminio-node`): a unit test fixes the line's exact text for given figures. Engine
  test, over in-memory stores as its other engine tests: a member stopped, written around and
  started again gives the engine's log sink one line, with the figures the unit test expects.
- **S7–S13** (script test, `make lint`, the VMs faked): each failure of S11 from a prepared log
  and byte counts, received and sent; each refusal of S12, before any change; a create that
  fails deletes nothing; the cleanup on a failed run; the repetitions.
- **S7–S14** (lab, a later step): the check on fl-a and fl-b, both runs, three times each.

## Open questions

1. **Measure first, or build the durable record now?** The record is a format change (a compat
   feature) and a protocol change, with three rules of its own (intent, Why 9), for a gain no
   run has shown. *Recommended: measure first;* every rule above assumes it.
   Settled at approval, 2026-11-13: measure first (see decisions.md, Settled at approval).
2. **When is the record (issue step 1) worth building?**
   - (a) By a budget set now: only if the replaced-target run's extra cost, scaled per GiB held
     to one 4 TiB disk (ARCHITECTURE §16.2), passes 10 minutes or 1% of the data held.
     Otherwise step 1 is dropped, and ARCHITECTURE §8.3 and §11.4 describe digests.
   - (b) By judgement, once the results are in.
   - *Recommended: (a).* Extrapolated (intent, Why 3): about 3.5 minutes, and 0.12% per party
     that is not on the target's node (5 bytes per 4 KiB block). Scaling from 16 GiB is a
     guess until a larger volume can be measured, which needs question 3.
   - Settled at approval, 2026-11-13: (a), by the budget set now (see decisions.md, Settled at approval).
3. **`ChangeIsr` with more than 64 changes** (intent, Why 7): a volume of more than 64 chunks
   may be unable to remove or re-add a member. A static trace only.
   - (a) Its own issue, with a failing test first; this check stays at 64 chunks (S7).
   - (b) Fixed in this change, so that the check can run larger.
   - *Recommended: (a).* It blocks writes, which is worth more than this measurement.
   - Settled at approval, 2026-11-13: (a), its own issue; this check stays at 64 chunks (see decisions.md, Settled at approval).
4. **The order of #45 and this issue.** #45 depends on #44, but maintenance mode (step 3) needs
   #45's rebuild to hold back. *Recommended:* #45 depends on this step only; step 3 follows #45.
   Settled at approval, 2026-11-13: #45 depends on this step only; step 3 follows #45 (see decisions.md, Settled at approval).
5. **Blocks that differ under equal CRC-32C values are not copied** (about one in 2^32 per
   differing block), in reconciliation and resync alike, as today.
   - (a) Accepted for now, and noted on #46 (scrubbing), which compares replicas the same way.
   - (b) Its own issue, towards authenticated checksums (the format's reserved incompat bit 1).
   - *Recommended: (a).* No rule here depends on it.
   - Settled at approval, 2026-11-13: (a), accepted for now, and noted on #46 (see decisions.md, Settled at approval).
