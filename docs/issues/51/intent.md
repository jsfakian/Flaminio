# Issue #51: Snapshots and clones — intent

[Issue #51](https://github.com/jsfakian/Flaminio/issues/51), README roadmap phase 3,
ARCHITECTURE §13 and §17.1. This change is the issue's steps 1, 2 and 7: a volume's snapshot
generation, the cut that makes it crash-consistent, and the simulator's check, with what the
lab needs to verify a snapshot. Steps 3 to 6 build on it: issue #51 stays open after this
change.

## Why

A snapshot bumps the volume's generation; writes carry it; a replica that sees a newer one
pushes a layer on the chunk first (ARCHITECTURE §13). §13 says what is wanted. The code has
the bottom of it only.

1. **The store has layers, and only its tests use them.** `Store::snapshot(key, gen, epoch)`
   (`flaminio-store/src/store.rs:1417`) commits a `LayerCreate`: a new empty top layer, whose
   generation must exceed the top's. Reads look through the layers from the top
   (`Chunk::resolve`, `state.rs:131`). A chunk starts with one layer of generation 0
   (`ChunkCreate`), so the layer of generation *g* holds the writes made at *g*, and the
   snapshot cut by the bump to *g* is the layers below *g*. Callers: `store_behavior.rs:102`
   and the `Snapshot` op of `crash_consistency.rs`.
2. **Nothing reads a snapshot.** `read`, `plan_read`, `digests` (`store.rs:1288`) and `mapped`
   (`store.rs:1334`) all resolve through every layer: the current content. No call returns a
   chunk as of a generation, or what one layer holds. `ChunkInfo` gives the layer count and
   the top generation only.
3. **What a layer costs.** A 16-byte entry of the chunk header at once; a page table and the
   map pages of the clusters written, from the page pool, at the first write
   (`State::pool_needed`, `state.rs:226`); a whole-cluster copy (64 KiB) for the first small
   write to each cluster (`store.rs:1790`; format §8). At most 64 layers per chunk replica
   (`MAX_LAYERS`); a full pool refuses new layers. `Store::snapshot` is a commit of its own,
   and first lands the commits in flight and staged (`guard` → `settle`, `store.rs:715`).
4. **No generation above the store.** `Request::Write` carries an extent, a version and
   `prune` (`flaminio-repl/src/msg.rs:37`; `Frame::Write`, `flaminio-net/src/frame.rs:42`).
   The target, the replica rules and the node's replica service know no generation
   (`flaminio-node/src/replica/mod.rs:360`). `VolumeMeta` (`flaminio-repl/src/meta.rs:32`),
   the Go `Header` (`meta/domain/domain.go`) and `meta.proto` have none, and no snapshot
   record or RPC. The CSI controller refuses a content source (`control/csi/controller.go:213`)
   and advertises no snapshot capability. `flaminio-meta` has no snapshot command.
5. **Resync, reconciliation and moves see the current content only,** so they would break a
   snapshot on a replica without anyone noticing:
   - **A replica out of the ISR at the bump.** It misses writes of generation *g − 1*. Its
     resync copies the dirty blocks' current values (`Copy`), which the replica service writes
     to the top layer like live writes (`replica/mod.rs:373`). The first live write of *g*
     pushes the layer: the copies land above the cut, the layers below still lack the missed
     writes, and the replica joins the ISR. Its current content is right; its snapshot is not.
   - **A full copy** (a move's destination, or a replica a new target did not remove itself:
     `target.rs:2351`) probes `Mapped`, compares `Digest` and copies into one layer of an empty
     chunk. The destination holds no snapshot at all; the drop then leaves it as the only copy.
   - **Reconciliation** compares `Digest` of the current content and repairs with ordinary
     writes of the new epoch (protocol §6). A write in flight when the generation moved can be
     under the cut on one replica and absent on another; the repair lands above the cut on
     both, and the snapshots keep differing. The same holds for a chunk not yet reconciled.
   - **A write sent again** after a session reopens (§4 step 4) would land above a cut it was
     sent below.
6. **The target holds nothing durable** (protocol §1): a new target takes its state from
   `Attach`. The generation must come with it, and a deposed target must not move it.
   `ChangeIsr` shows the pattern: a transaction conditional on the header's revision and the
   target's epoch (`meta/service/service.go:755`). Moves show the other half: an administrator
   only requests, the target carries out, and a volume without a target waits for its next
   attach (protocol §7; follow-ups §11).
7. **The simulator cannot see any of this.** Its replica is one map of blocks per chunk
   (`flaminio-repl/src/replica.rs:292`). It checks linearizability per block and that replicas
   hold identical blocks (`sim/mod.rs:1345`): the current content.
8. **No format change is needed for this step, with one constraint.** `LayerCreate` exists.
   `MapUpdate`, `DeferredWrite` and `Discard` act on the top layer only (format §6.3): a
   replica cannot be given data under a layer it already has. The wire has no version
   negotiation (`frame.rs`, tags): nodes are upgraded together, as the lab does.
9. **Later issues read layers.** #52 backs up what changed between two snapshots, #58 ships
   it, #68 restores. They need generations that mean the same on every replica.

## What we want

1. **A volume has a snapshot generation and snapshots,** in the metadata service: each snapshot
   names the generation it was cut at. An administrator creates and lists them, with
   `flaminio-meta` commands. Generations only grow and are never reused.
2. **A snapshot is a crash-consistent cut.** It holds every write acknowledged before it was
   requested and none submitted after it was reported cut. No write is in flight across the
   cut, and every chunk is reconciled at it. Client I/O waits briefly; it does not fail.
3. **Only the volume's current target moves the generation,** under its epoch, and no write
   of a generation is sent before the metadata service holds it. A deposed target cannot cut.
   A new target starts at the metadata service's generation.
4. **Writes carry the generation,** and a replica pushes the chunk's layer before the first
   write of a newer one. A chunk not written since a snapshot gets no layer. A write below
   the chunk's top generation is refused.
5. **Every in-sync replica holds every snapshot,** with the same content and the same changes
   from one snapshot to the next. That holds after a failover, a replica restart, a resync of
   a replica that was out at one or more cuts, and a move. A replica joins the ISR only then.
6. **A snapshot's content can be read back** from each replica that holds it, without changing
   it: enough for the checks below and for resync. Not yet as a volume.
7. **The simulator takes snapshots during chaos** and checks rules 2 and 5: each snapshot is a
   cut the history allows, identical on every replica at the end. Each new rule has an
   injected bug that it catches (protocol §9).
8. **A lab check:** a snapshot of a volume under write load holds exactly the content at the
   cut on every replica, also after a failover and after resyncing a replica that was stopped
   across the cut.
9. **What later steps find in place:** the snapshot records; per chunk, layers keyed by
   generation, so that the layers from generation *a* up to *b* are what changed between two
   snapshots, on any in-sync replica. `docs/replication-protocol.md` and ARCHITECTURE §13
   describe the rules as built.

## Out of scope

- **Step 3: deleting a snapshot** (`LayerMerge`, a new journal record) and returning its
  space. Until then a volume keeps its snapshots until it is deleted, and their number is
  capped below the 64 layers.
- **Step 4: CSI `VolumeSnapshot`, and volumes from a snapshot or a PVC.** Linked clones need a
  cross-slot reference in the format (format §6.3), hydration, and full clones a copy to new
  placement. Reading a snapshot as a volume comes with them.
- **Step 5: `VolumeGroupSnapshot`:** one barrier across several targets.
- **Step 6: application consistency:** `fsfreeze`, hooks by annotation.
- **Snapshots that outlive their volume.** Kubernetes allows deleting a PVC that has
  snapshots. Here a snapshot goes with its volume; step 4 decides.
- **A snapshot of a volume that is not attached** is cut at its next attach, as a requested
  move is carried out. A target without a device (follow-ups §11) would cut it at once.
- **Backups, shipping and restore:** issues #52, #58 and #68.
- **Scrubbing snapshots.** Nothing scrubs today (ARCHITECTURE §12). Layers under a cut that
  differ through media errors are found only when read.
- **The page pool's size** and the copy-on-write cost of the first small write after a
  snapshot (ARCHITECTURE §20, question 1): measured, not changed.
- **Audit events for snapshot operations** (#73's S7 leaves them to this issue): a follow-up in
  `docs/followups.md`, issue to be filed (plan review round 1).
- **Client discards.** The data plane answers them without acting (`flaminio-io/src/ublk.rs`).
- **Rolling upgrades.** The write frame changes; old and new nodes do not mix.
