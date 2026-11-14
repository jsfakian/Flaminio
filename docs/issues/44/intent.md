# Issue #44: Dirty-region resync — intent

[Issue #44](https://github.com/jsfakian/Flaminio/issues/44), from the README roadmap (phase 2)
and ARCHITECTURE §8.3, §10.4 and §11.4. This change is the issue's "Done when", taken first: its
scenario as a lab check, run against what exists, and a resync that says what it did. It builds
none of the issue's four steps. They are decided by its result: issue #44 stays open.

## Why

The issue was written from ARCHITECTURE, which describes dirty bitmaps kept by ISR peers. The code
has none, and since the issue's premise was set, digests changed what a resync costs.

1. **The dirty set is the target's, in memory** (`Chunk::dirty`, `flaminio-repl/src/target.rs`).
   When this target removes a member during I/O, it records the writes in flight that the member
   had not acknowledged (`apply_view`), then every later write of the chunk (`start_write`). A
   target of a later epoch has no entry, and plans the whole chunk (`drive_resync`). Every target
   start is a new epoch (replication-protocol.md §3), so a target restart loses it too.
2. **A resync already copies only what differs, whoever plans it** (follow-ups §3). The plan is
   cut to what the source maps, the rest is discarded on the joiner, and the source's and the
   joiner's per-block digests are compared before any copy (`on_mapped`, the `BeginResync` reply,
   `on_digest`). Only blocks whose digests differ are copied. The digests come from the store's
   maps, which are in memory (`Store::digests`): no data and no metadata is read from the disk.
3. **So a record that outlives the target would save the digests and the discards, not the
   copies.** With one, a new target compares the recorded blocks; without, every mapped block
   of every chunk, and it discards on the member every range the source does not map.
   - *Bytes, by the frame encoding, not measured:* a digest reply is 5 bytes per block the
     source maps (`Frame::Digests`, `flaminio-net`): 1.25 MiB per GiB, sent to the target by each
     party that is not on the target's node. A discard is one small request per 256 blocks of
     hole (the node's `max_io`). The 46 MiB of follow-ups §3 (2026-10-28) is what fl-a
     received in a run whose target's node was not recorded: it is not a figure for digests.
   - *Time:* not recorded for that run. The reconcile check does the same digest work for two
     members: one block of every chunk of `vol1` read 0.8–1.0 s after the attach
     ([2026-11-04](../../benchmarks/2026-11-04-reconcile-order.md)): about 0.05 s per GiB.
   - *Extrapolated, not measured:* 1 TiB mapped would cost about 1.25 GiB of digests per such
     party, and a minute. The lab's disks are 100 GiB.
4. **The issue's scenario has never been run.** A member out while part of a volume is written,
   a failover in between, the member back: no check does it. `scripts/checks/resync.sh` resyncs
   into an empty replica; the 46 MiB run wrote nothing while the member was away. By the trace
   above, the issue's "Done when" (copying only what was written) may hold today.
5. **A resync does not say what it did.** The target's only event is a failed session
   (`Event`), and the node logs the ISR it proposes. The lab can read the bytes a node received
   and sent over the overlay, and nothing else: not what was planned, compared, copied or discarded.
6. **Every chunk is resynced after every outage, written or not.** A member whose session fails
   is removed from the ISR of every chunk of the volume (`fail_peer`, `advance_chunk`), and a
   new target removes its silent members likewise. Reads need it: the lease set is the ISR
   (ADR 0005). So ARCHITECTURE §11.4's "only for chunks whose ISR it was removed from" holds
   trivially, and each outage costs every chunk two ISR changes, a `Mapped` probe of the whole
   chunk (even with a dirty set) and a `BeginResync`.
7. **A volume of more than 64 chunks cannot do that** (static trace; no test, and no lab volume
   is larger than 16 GiB). The target proposes every pending change in one `ChangeIsr`
   (`send_meta`, no bound); the metadata service refuses more than 64 changes
   (`MaxChangesPerRequest`, `meta/service/service.go`); the engine drops the refusal
   (`MetaWait::Isr`), and the target proposes the same request again after its timeout. Writes
   wait for the removal. Measuring above 16 GiB needs this fixed first.
8. **Maintenance mode has nothing to hold back.** Nothing re-places a failed node's replicas:
   `control/` has the drain and the failover of targets only. "No rebuild starts in the window"
   needs the rebuild of #45, which depends on this issue.
9. **A record on the replicas is more than a bitmap.** Tracing what a new target would need for
   the record to be safe (each a rule with a simulator check, none built here):
   - it must hold every write in flight on the chunk when the member was removed, also those the
     member acknowledged: the old target may die before any ISR member applies them;
   - a returning member may hold writes no ISR member has (live writes of an aborted resync).
     Only its own uncertain set knows them, and after a restart that is the whole chunk
     (`ReplicaCore::restart`): maintenance restarts the member, so the record needs follow-ups
     §2 step 2 (the uncertain set persisted) as well;
   - an ISR member that was itself resynced meanwhile has a record with a gap.

## What we want

1. **A resync says what it did.** Per chunk and member, counted in `flaminio-repl`: where the plan
   came from (this target's dirty set, or the whole chunk), and the blocks planned, compared by
   digest, copied and discarded. The node logs the totals when a member is back in every ISR of
   a volume, with the time it took. No message and no rule of the protocol changes.
2. **The issue's scenario as a lab check.** A volume with data, a member stopped, a known amount
   written, the member back, the data verified. Twice: with the same target throughout (the
   in-memory dirty set), and with the target replaced while the member is away (digests). It
   reports the bytes the member received and sent, the time until every ISR is full, and the
   counts of 1. It fails if more was copied than was written.
3. **The numbers, recorded** in `docs/benchmarks/` and follow-ups: what a resync after a failover
   costs today, and the difference between the two runs, which is the most a record that
   outlives the target could save at that size. The decision on the issue's step 1 is taken
   from them.
4. **`docs/lab.md`** describes the check.

## Out of scope

- **The record that outlives the target** (the issue's steps 1 and 4): durable on the in-sync
  replicas, with its simulator check and injected bug. An on-disk format change (a compat
  feature) and a protocol change, with the three rules of Why 9. Decided after this change's
  numbers.
- **Resyncing only the chunks a member was removed from** (step 2): today every chunk is (Why
  6). Leaving a silent member in the ISR of idle chunks touches the lease rules (ADR 0005).
- **Maintenance mode** (step 3): after #45, which gives it a rebuild to hold back (Why 8).
- **`ChangeIsr` for more than 64 chunks** (Why 7): a bug of its own, to be confirmed by a test
  and fixed in its own issue. Until then the check runs at 16 GiB.
- **Persisting the uncertain set** (follow-ups §2 step 2): needed by the record (Why 9), not by
  this change.
- **What a 32-bit digest misses.** Blocks compare equal when their CRC-32C values do
  (`digest_mismatch`): a block that differs with an equal CRC is not copied, about one in 2^32
  per differing block. As today; a record would not change it, since recorded blocks are
  compared too.
- **Volumes without a target** are not resynced until attached (#17).
- **Replicas left by moves** (#13) and **the rebuild** (#45).
- **ARCHITECTURE §8.3 and §11.4** keep describing what is wanted.
