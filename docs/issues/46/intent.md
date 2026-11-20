# Issue #46: Scrubbing and repair — intent

[Issue #46](https://github.com/jsfakian/Flaminio/issues/46), from the README roadmap (phase 2)
and ARCHITECTURE §11.1 (silent corruption) and §12. This change is the issue's step 1, the pass
that finds, with the node's own count of step 5. It repairs nothing. Steps 2–4 build on what it
finds: issue #46 stays open after this change.

## Why

ARCHITECTURE §12 describes a scrub and a repair. Neither exists: no code in the tree scrubs,
and no path rewrites a block whose stored data went bad.

1. **A block is checked only when a client reads it.** The store verifies each 4 KiB block
   against the CRC-32C in its map entry on every read (`ReadPlan::verify`,
   `flaminio-store/src/store.rs:366`; `read_range`, `store.rs:1987`). A block nobody reads is
   never read back. With RF 2 it can go bad unnoticed until the other copy is lost: then the
   volume has no good copy.
2. **`Digest` cannot find it.** It answers from the map's checksums, "no data is read from the
   device" (`Store::digests`, `store.rs:1283`). It says what the metadata records, not what the
   medium holds. Reconciliation and resync compare those digests, so a bad block with a right
   checksum matches its peers and is skipped (`on_digest`, `flaminio-repl/src/target.rs:1487`).
   Only reading the data on the replica's own disk finds it.
3. **The one full check is offline.** `Store::fsck(true)` reads every mapped cluster and checks
   it (`store.rs:2176`). Only the store's tests call it. It runs to the end in one call on the
   thread that owns the store, has no rate limit, and reports the first 20 errors as one string.
   `flaminio-io digest` and `scripts/guest/digest.sh` need the services stopped.
4. **A client read that finds a bad local block is served, and the block stays bad.** The
   frontend plans and reads it once more, then reports the failure (`local_read`,
   `flaminio-io/src/ublk.rs:1293`; fast reads, `ublk.rs:1682`). The target sends the read to
   another ISR member; the local replica stays a member (`on_read_failed`, `target.rs:1019`,
   docs/issues/5). "No data or membership changes" (replication-protocol.md §5). With no other
   member the client gets an I/O error. The only record is a line on stderr.
5. **Read through the replica service, a bad block costs a membership change and is still
   bad.** A read that fails its checksum closes the session (`Reply::Close`,
   `flaminio-node/src/replica/mod.rs:402`). The target learns nothing but the loss: after the
   request timeout (3 s, `flaminio-repl/src/types.rs:60`) it fails the peer and proposes its
   removal when the minimum allows (`fail_peer`, `target.rs:1214`). The resync that follows
   copies only the writes in flight at the removal and those since (`apply_view`,
   `target.rs:2526`), minus blocks whose digests match (item 2). The replica rejoins with the
   bad block, and the next read of it repeats all of this. The same path serves the local
   replica's reads for repairs and resync copies (`deliver_local`,
   `flaminio-node/src/engine.rs:1366`: "the target's timeout handles it"), so a bad block on
   the only source of a resync fails every copy of it. Traced in the code, not reproduced: no
   test or lab check corrupts stored data.
6. **Nothing counts.** A mismatch is not an I/O error to the device, and no component keeps a
   count per disk. The metadata service has no health record for a disk: `control/agent` knows
   devices only for the claim policy (`disks.go`). ARCHITECTURE §5.2's health reporting is not
   built.
7. **What exists and works.**
   - Every mapped block has its checksum in the map, in memory, and reads verify it.
   - A read can be planned by the store and carried out elsewhere while the engine keeps
     going, with the clusters it reads kept from reuse (`plan_read`, `finish_read`). "A
     concurrent write to the same range can make `verify` fail; plan and read again"
     (`store.rs:1900`): a mismatch seen once is not yet a bad block.
   - Every node runs the engine that owns its store, with or without an attached volume
     (`flaminio-io/src/node.rs`), and a root-only control socket (`control.rs`).
   - The target bounds its bulk work (4 in flight) for the reason a scrub needs a limit: work
     queued ahead of heartbeats times them out (protocol §7, §10).
8. **No measurement exists.** The lab's disks read sequentially at 885 MiB/s (fl-a) and
   3 324 MiB/s (fl-b) raw, 1M QD8 ([raw baseline](../../benchmarks/2026-10-08-raw-baseline.md)).
   vol1 is 16 GiB at RF 2 on 100 GiB disks: an unthrottled pass would take seconds there, so
   the limit, not the disk, sets a pass's length. To measure in the lab: a pass's rate and
   length, and the foreground's throughput and p99 with and without a pass
   ([performance plan](../../performance-plan.md)'s rules).

## What we want

1. **A scrub on every node that holds replicas:** a background pass over its own disk that
   reads every mapped block of every chunk replica and checks it against its checksum, whether
   or not the volume is attached anywhere. Each replica reads its own disk: only it can.
2. **It never calls a good block bad.** A block written while the pass reads it is read again;
   only a mismatch that persists is a finding.
3. **It changes nothing:** no write, no membership change, no metadata call. Client I/O,
   replication, reconciliation and resync behave as today, beside a pass and after a finding.
4. **Throttled.** A pass keeps to a rate limit, and client and replication I/O, heartbeats and
   leases keep their timing while it runs. The default comes from a lab measurement.
5. **It runs by itself and on request:** every 14 days by default (§12), and when an operator
   asks through the node's control socket, which is how the lab checks it.
6. **Findings are kept and shown:** each bad block (volume, chunk, block), the pass's progress
   and result, and a count of bad blocks per disk, on the node. This is what the repair (step
   3) and disk health (step 5) will read.
7. **The lab check, which is the issue's "done when" without the repair:** a block of a volume
   the check creates (not vol1: #84's rule for a check that writes), overwritten on one
   replica's disk, under fio, is named by the next pass on that node, and by no pass on the
   other; the pass ends, and fio's verification of the volume still passes.

## Out of scope

- **Step 2: comparing digests across replicas.** Finds copies that differ although each
  matches its own checksum (a lost or misdirected write). It needs a target: only a target
  knows the ISR and holds the epoch. A comparison of digests cannot tell apart blocks that
  differ under equal CRC-32C values (#44, question 5, which notes it here): step 2's plan
  starts from it.
- **Step 3: repair.** A repair is a write under an epoch, by the volume's target, from an ISR
  member whose data verifies, to the member that reported the block. It must never put an older
  value over a newer one: a new safety rule, with its simulator check and an injected bug the
  simulator catches. It also needs a way for a replica to tell the target which block is bad;
  today it only closes the session (Why 5).
- **Repair of a volume with no target.** Only a target can write. The one planned way to have
  one without a client is #17's maintenance target (not built, depends on #15). Until then a
  finding on an unattached volume waits for its next attach.
- **Step 4: repair when a client read finds a mismatch,** and with it the two faults of Why 5:
  a bad block read through the replica service drops the member, and the resync leaves it bad.
  They are recorded here so that step 3's plan starts from them.
- **Step 5 beyond the node:** bad-block counts in a disk's health record, a `Degraded` disk,
  auto-drain. No health record exists (Why 6); it comes with the node agent's health reporting
  and gray-failure detection (roadmap phase 2).
- **Metadata pages and the journal.** They are checksummed and read at recovery only. A pass
  over them is not in the issue; the spec records it.
- **Scheduling across the cluster** (ARCHITECTURE §5.5's scrub scheduler: not two replicas of
  one chunk at a time, not during a rebuild). Each node keeps its own period here.
- **Weighted fair queuing between I/O classes** (ARCHITECTURE §16.1). The throttle is a rate
  limit.
- **Sequence numbers and the authenticated mode's replay check** (§12): not built.
