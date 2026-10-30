# Issue #11: Reconcile first the chunks that clients wait on — intent

[Issue #11](https://github.com/jsfakian/Flaminio/issues/11), follow-ups §2, the
[protocol](../../replication-protocol.md) §6.

## Why

A new target serves a chunk only once it is fenced and reconciled (§6). Until then, every client
operation on it waits: reads and writes alike, on any block of the chunk. After a replica
restart, every block of its chunks is uncertain, so every chunk is reconciled before it serves.

**The order is last to first.** `Target::advance_chunk` runs over the chunks from 0 up
(`Target::progress`). Each chunk that becomes fenced pushes its `Digest` work to the front of
the queue (`push_front`). When one `Fenced` reply completes the fence of many chunks, as it does
after a restart, the last chunk ends up first in the queue, and chunk 0 last.

**A waiting client operation changes nothing.** `Target::submit` queues it at the back. The
queue starts work in its order (`Target::pump`):
- the operation waits in the queue while its chunk is not `Ready` (`Target::try_start`);
- the reconciliation of its chunk stays where it was queued. Nothing moves it forward;
- at most 4 bulk items are in flight at once (`DEFAULT_MAX_BULK`), for all chunks together:
  digests and repairs, and a resync's copies, discards and digests. Each holds its slot until it
  is done: a digest until every ISR member has answered, a repair until its write quorum (the
  ISR plus any joiner) has acknowledged its write;
- a resync's copies go to the front of the queue, ahead of reconciliation.

The data plane sends one `Digest` per chunk (65 536 blocks, a default chunk of 256 MiB), and
repairs in pieces of 1 MiB (`max_io` 256 blocks). A fast read is refused while its chunk is not
`Ready` (`Target::try_fast_read`), and goes through the queue.

**What the lab shows.** `vol1` is 16 GiB in 64 chunks, with replicas on fl-a and fl-c
([issue #6](../6/design.md), 2026-10-28).
- **After both replicas restarted** (follow-ups §2, without zero copy), all 64 chunks were
  fenced on the same reply. The first read, of block 0 (chunk 0: last of the 64 digests), came
  0.21–0.25 s after the attach request. So the whole volume reconciled within that: at most
  4 ms a chunk, four at a time.
- **Without a restart,** with nothing to reconcile, the same read came at 0.10–0.12 s (issue #6,
  with zero copy). Different runs and modes, so only an indication: reconciliation took about
  0.1 s of the 0.21–0.25.
- **"One block of every chunk within 0.9–1.0 s"** is the 63 reads after the first, one `dd`
  after another, and a Python probe of a misaligned read (`scripts/checks/reconcile.sh`). It
  bounds those, not reconciliation.
- **The `reconcile` lab check** prints when the device was ready and when block 0 was read. It
  fails on a first read past 5 s, or on a failed read logged: it cannot see this. It always
  reads block 0 first, which the kernel's partition scan and udev's probes read too, with the
  device's end.

So today the first read waits for the whole volume, and that grows with the chunk count. At
the lab's rate, a 1 TiB volume (4 096 chunks) would hold it for about 6–16 s (arithmetic, from
1.5–4 ms a chunk, not measured). Chunks whose digests differ add their repairs, which copy
data, and they all go before chunk 0's.

**The simulator.** A run has 1 or 2 chunks (`Config::random`).
- **A run with 1 chunk keeps its schedule exactly** (spec S2). The fencing check's hint,
  3 626 767 (`catches_fast_reads_across_a_fence`, tried first), is such a run, and so is 18 962,
  the first catch noted for `catches_fast_reads_from_a_replaced_replica` (which searches from
  0). Both should keep catching; the issue asks for the search all the same (step 2).
- **A run with 2 chunks** changes when both chunks have bulk work queued at once, and such runs
  do exercise the new order: the review counted, in 200 seeds, 67 with two chunks waited at
  once, 22 with a waited chunk while other bulk work held every slot, and 24 with a resync
  queued while a chunk was waited. The first seeds §9 lists may move.
- **A catch is any failure today** (`find_failure`, and the fencing check's hint): no check of
  the error, and no control run without the bug. Today every check catches with `not
  linearizable` or `replicas diverge`.
- **The fencing check's search** goes on to 4 000 000 seeds if its hint stops catching: hours,
  past CI's 40 minutes.

## What we want

1. **A client operation waits for its own chunk,** not for the chunks queued before it: its
   chunk's fence and reconciliation, the reconciliation of chunks waited on before it, and, each
   time its chunk has work that can start, the items in flight then, plus at most one resync
   item started after them. Not a bound in time: work in flight can last, as today. This is
   weaker than the issue's "waits only for its own chunk", which no order can give while other
   work holds the slots; approved at the spec gate (2026-10-30).
2. **The other chunks fenced together, in order** from chunk 0 up, so a volume read from its
   start serves early. A chunk fenced later still goes ahead of them, as today.
3. **Resyncs always progress:** while a waited chunk has work that can start or is in flight,
   one resync item at a time, alternating with waited work; otherwise as today.
4. **The rest unchanged:** what is reconciled, how, when a chunk serves, and the order of client
   operations. With no chunk waited, the target tries its queue as today; the queue differs only
   in the digests of chunks fenced together, from chunk 0 up.
5. **Checked:** unit tests of the target (they carry "only for its own chunk"), with a property
   test that client operations start as today; a campaign with more chunks in `make test`, and
   four reconciliation checks run on it; every mutation catch checked for its error and against
   a clean run.
6. **The checks searched again** from seed 0 (issue step 2), their hints and §9 updated, and
   `make sim` failing fast, not searching for hours, if the fencing check's hint stops catching.
7. **A lab run** that compares the first read before and after, on chunk 0 and mid-volume, once
   the lab is free.

## Out of scope

- **Persisting the uncertain set at checkpoints** (follow-ups §2 step 2, the issue's optional
  step 3). It is a change to the store and its on-disk format (a `features_compat` bit,
  [format](../../format/on-disk-format-v1.md)), with its own crash and upgrade tests. It saves
  digest requests after a restart, not waits: with this issue, a read waits for its own chunk,
  whose digests took a few milliseconds in the lab. A separate issue, if the lab ever shows the
  need.
- **Serving the blocks of a chunk that are already reconciled,** or reconciling the waiting
  operation's blocks before the rest of its chunk: a chunk serves as a whole, as today.
- **Stopping bulk work in flight** to make room, or bounding how long an item holds its slot.
- **The bulk limit, and the digest and repair sizes.**
- **More simulator machinery:** an order invariant, mutations of the new order, catch rates, a
  scripted scenario for the fencing check, and sweeping the campaign with more chunks over
  20 000 seeds. The unit tests pin the order down.
- **Zero-copy reads after an attach** (follow-ups §12, issue #5).
