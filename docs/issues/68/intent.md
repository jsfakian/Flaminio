# Issue #68: Instant restore — intent

[Issue #68](https://github.com/jsfakian/Flaminio/issues/68), README roadmap phase 6,
ARCHITECTURE §14.1, §13 and §1.1. This change is the ground under the issue's three steps: the
decision (an ADR) and the rules, in the replication protocol and its simulator. It restores
nothing yet. The store, the data plane, the fetch path, the command and the lab check build on
it: issue #68 stays open after this change. It assumes the first steps of
[#54](https://github.com/jsfakian/Flaminio/issues/54) (restore),
[#52](https://github.com/jsfakian/Flaminio/issues/52) (backups) and
[#51](https://github.com/jsfakian/Flaminio/issues/51) (snapshots) as their intents plan them.
Which step comes first is the planner's recommendation, for the reviewer to confirm.

## Why

1. **The need is not measured.** #54 restores through the new volume's device, so its floor is
   the write path: 1M sequential at 935 MiB/s with RF 1
   ([results](../../benchmarks/2026-10-29-writes-1m-floor.md)) and 106 MiB/s with RF 2, the
   lab's 1 GbE link ([results](../../benchmarks/2026-10-17-writes-rf2-w41.md)). At 106 MiB/s,
   1 TiB takes 2 h 45 min; the lab's 16 GiB volume takes 2.6 min. Reading the bucket,
   decrypting and decompressing come on top; #54's lab check records that rate (its outcome 9).
2. **#54 promises the opposite, for reasons that stay true.** Its volume is "not usable until
   it is whole": a mark in the volume's header, beside `Creating` and `Deleting`
   (`meta/domain/domain.go:50`, `:53`), refuses every attach but the restore's. Without it,
   `NodeStageVolume` formats a device on which it finds no signature
   (`control/csi/node.go:108`–`:114`): a pod's mount runs `mkfs` under the restore. Instant
   restore removes that guard. What must replace it: no read of a block not here yet answers
   zeros.
3. **Nothing below a read can say "not here yet".** A map entry is unmapped, zero or a cluster
   (`Phys`, `flaminio-format/src/meta.rs:450`; format §5.5), and the bottom of the layers reads
   as zeros (`Chunk::resolve`, `flaminio-store/src/state.rs:131`). Everything above takes that:
   - a replica's `Read` (`flaminio-node/src/replica/mod.rs:391`), and the local read the data
     plane does itself (`plan_local_read`, `flaminio-node/src/engine.rs:2253`);
   - the fast path, which goes round the target's queue and zero-fills in the frontend
     (`plan_fast_read`, `engine.rs:2205`; `plan_segments`, `flaminio-store/src/store.rs:1865`;
     `fast_read_zc` and `fast_read_copy`, `flaminio-io/src/ublk.rs:1590`, `:1629`);
   - `digests`, which gives an unwritten block the zero block's CRC (`store.rs:1317`), and
     `mapped`, which omits it (`store.rs:1360`).
4. **The replicas must know, even if only the target fetches.** A block fetched once by the
   target and sent as an ordinary write leaves replicas alike only while nothing fails.
   - **A target crash with that write in flight.** One member holds the block, the other does
     not, and their digests differ. The repair reads its source, the local member first
     (`source`, `flaminio-repl/src/target.rs:1644`, `:1801`), and writes what it read to every
     member (`:1582`). If the local member lacks the block, zeros become data on both, and
     nothing fetches it again.
   - **A resync** discards on the joiner what the source does not hold (`on_mapped`,
     `target.rs:1464`): right only if the joiner's block then also counts as not here yet.
   - **Each replica fetching for itself** is worse. An ISR member holds every acknowledged
     write (`flaminio-repl/src/meta.rs:22`) and any member serves reads (ARCHITECTURE §7.2):
     a member that has not fetched a block would be in sync and unable to serve it.
5. **A small write to a block not here loses the rest of it.**
   - The store completes a write smaller than a cluster with the cluster's visible content,
     zeros here, and maps the result (`store.rs:1790`–`:1818`): 4 KiB of the client's and
     60 KiB of zeros where the backup had data, now indistinguishable from fetched data.
   - Cluster size is per disk (16–256 KiB), so replicas map different ranges after the same
     writes (`mapped`, `store.rs:1333`). The target knows 4 KiB blocks; a backup's unit is
     4 MiB (#52).
   - Client discards are answered without acting (`ublk.rs:1279`). Once they act (§7.4), a
     discarded cluster of a single layer is unmapped again (format §6.3, type 7).
6. **Hydration is a copier without a copier's guard.** A rebuild never overwrites what a live
   write set (ARCHITECTURE §11.4), and a resync copy skips blocks written since `BeginResync`
   (`flaminio-repl/src/replica.rs:262`). A fetched block races client writes the same way, and
   across a failover too: the target holds nothing durable (protocol §1), so a new one does
   not know what the old one fetched or was fetching.
7. **Where "not here yet" is recorded decides whether it is safe.**
   - **With the data, in each replica's map:** changed by the commit that writes the block.
     Format v1 has no place for it (§3 defines no feature for it; §5.5 has the three values),
     and `Digest`, `Mapped`, `Read` and `Copy` would carry it. Format §6.3 expects linked
     clones to add "a cross-slot reference type": the same shape, a read that falls through
     to somewhere else.
   - **Derived by the target:** in the backup's manifest and unmapped on the replicas. No
     format change, but Why 4 and 5 show "unmapped" changing meaning under it.
   - **In the metadata service:** not atomic with the data. Recorded first, a read finds
     zeros; recorded after, a new target fetches again over acknowledged writes. And it is a
     round trip per block.
8. **Nobody on the I/O path can reach a bucket.**
   - The workspaces have no object-store client, BLAKE3, zstd or AEAD (#52, Why 1). #52's ADR
     decides the worker's home (its plan recommends a process of its own, with its own
     identity); the data plane holds no repository key.
   - The engine runs on the attached device's queue thread, with the node's replica reads
     (follow-ups §5; #52, Why 6). A fetch that blocks it stalls the volume and its peers.
   - #54's restore is one command: the key reaches one node while it runs (its outcome 7).
     Fetching on demand needs the bucket and the key on every node that may become the
     target, until hydration ends. Hydration needs a target, as a move does (protocol §7): a
     volume detached early depends on the bucket until its next attach.
9. **It bends two stated rules.** The steady-state I/O path makes no metadata-service round
   trips (AGENTS.md; ARCHITECTURE §2, principle 3): it depends on nothing but the nodes that
   hold the data. Every external dependency is sometimes unreachable (§1.1; ADR 0004,
   consequences), and #52 promises that "no application I/O waits for the object store". Here
   application I/O does. What bounds it: only blocks not yet here, only until hydration ends,
   only volumes whose restore asked for it. Edge sites, with "no or intermittent WAN" (§1.1),
   pay most.
10. **An unreachable store and a damaged block are different failures.**
    - Today I/O that cannot be served safely waits: a read with no usable member waits for one
      (protocol §5), and no delay is bounded (§8). It fails only once deposed or unreadable
      (`OpResult`, `target.rs:41`). Waiting here means a pod that started and then hangs;
      failing means filesystem errors for an outage that would have passed.
    - #54 fails a restore by name on a missing or altered block, before anyone uses the volume
      (its outcome 5). Here the pod finds it at its first read of that block, maybe days later.
11. **A miss is expensive.** A 4 KiB read of a block not here costs a 4 MiB fetch: at least
    37 ms on the lab's link, before decrypting, decompressing and replicating. The fast path,
    which most reads take (protocol §5), cannot serve such a volume as it is (Why 3).
12. **The simulator cannot see any of it.** Its replica is a map of blocks, an unwritten one
    reading 0 (`replica.rs:324`); its check is a register per block (`sim/check.rs`). A volume
    that starts with a backup's values, and a store that goes away, cannot be expressed.
13. **Nothing underneath is built.** #51's first step has no clone, no hydration and no
    snapshot read as a volume, so "hydration, as for linked clones" has nothing to reuse. #52
    and #54 are plans; the CSI controller refuses a content source
    (`control/csi/controller.go:213`).

## What we want

1. **An ADR, accepted before the design,** for what the later steps cannot undo:
   - where "not here yet" is recorded, and what that changes on disk and on the wire (Why 7);
   - who fetches, and where the bucket's credentials and the repository key then live (Why 8);
   - what I/O to a block not here does while the store is unreachable (Why 10);
   - the exception to the rules of Why 9, and its bounds;
   - what linked clones (#51, step 4) share with it.
   It states the need with #54's measured restore rate.
2. **No read returns zeros for a block not here yet.** It returns the backup's content, or
   what the ADR decides for an unreachable store. That holds for client reads on every path,
   and for whatever compares or copies replicas: digests, reconciliation, resync, moves.
3. **In-sync replicas agree on which blocks are here,** after a failover, a replica restart, a
   resync and a move, as they agree on content. A replica joins the ISR only then.
4. **A write never loses the backup's content around it.** A write that covers part of a block
   not here leaves, on every in-sync replica, the backup's content merged with the write.
5. **Hydration never overwrites a newer write.** A fetched block lands only where no client
   write has landed since the restore, whatever the order, and through a failover.
6. **Hydration ends once,** when every block of the backup is durable on every in-sync replica.
   Only the volume's current target says so, under its epoch. From then on no read, write,
   attach or resync reaches the store, and the volume is one like any other.
7. **The store is needed for blocks not here, and for nothing else.** While it is unreachable:
   I/O to blocks that are here, attach, failover, detach, deletion, resync and moves go on.
   A block missing or altered fails its reads by name; it never returns other data.
8. **#54's mark gains a second form.** A volume being filled is either #54's, written by its
   restore and attachable by nothing else, or hydrating: attachable, with its reads allowed
   to reach the store. It is chosen when the restore creates the volume and never changes
   into the other. Both name the backup, and both end in the same whole volume. #54's rule
   stays the default: instant restore is asked for.
9. **The simulator has a repository,** which holds the volume's first content and can become
   unreachable, and restores volumes from it during chaos. It checks rules 2 to 7. Each rule
   has an injected bug that it catches, among them: zeros served for a block not here; a
   repair that copies them; hydration over a newer write; a small write that drops the rest.
10. **`docs/replication-protocol.md` describes the rules,** and ARCHITECTURE §14.1 what is
    decided and what is not built yet.

## Out of scope

- **Step 2: the store and the format.** "Not here yet" on disk as the ADR decides, with the
  format document, the crash-consistency tests and the fuzz targets.
- **Step 3: the data plane and the fetch path.** The engine's part, the fetch from the
  repository, verified before it is written, and the fast local read path during hydration.
- **Step 4: the metadata service, the command and CSI.** #54's mark in its second form, the
  restore command's option, and what a hydrating volume reports.
- **Step 5: the lab check,** the issue's "done when": a pod starts before the data is local,
  the time to its first read is measured, the store stops during hydration and after it, a
  failover happens during it, and the volume equals the backup.
- **Encrypted volumes.** Their ciphertext must land below the target: #54's step 2, #50.
- **Snapshots, backups and shipping of a volume still hydrating:** refused until it is whole.
- **Hydrating a volume that is not attached:** it waits for its next attach (follow-ups §11).
- **Linked clones and volumes from a snapshot** (#51, step 4), **a PVC from a backup** (#52,
  step 5). The ADR says what they share; they are not built here.
- **Deleting a backup a volume still hydrates from.** #52's repository only grows; whoever
  adds deletion holds such backups.
- **The cost of a miss:** read-ahead, the order and rate of hydration, a unit smaller than
  4 MiB, fair queuing with client I/O (§16.1). Measured in step 5, not tuned.
- **Rolling upgrades.** Nodes are upgraded together.
