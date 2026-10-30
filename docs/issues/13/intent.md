# Issue #13: Replicas left behind by moves — intent

[Issue #13](https://github.com/jsfakian/Flaminio/issues/13), follow-ups §13. It takes in issue
#14's step 1: the tombstone sweep moves into the node agent. It depends on
[issue #30](https://github.com/jsfakian/Flaminio/issues/30): a volume created again starts above
every earlier epoch of its name.

## Why

A move ends when the volume's target drops the source from the chunk's replicas: a `ChangeIsr`
that `validMove` accepts, and that `ApplyIsrChange` applies, clearing `moving`
(`meta/domain/domain.go`). From then on no record names the source for that chunk: no chunk
record, and no tombstone if the volume is deleted later (`replicaNodes`,
`meta/service/service.go`; docs/issues/8, out of scope). Nothing deletes its copy.

1. **The lab.** The item 3 check (follow-ups §3, 2026-10-28) moved all 64 chunks of a 16 GiB
   RF 1 volume from fl-a to fl-b. fl-a still held them (4 GiB) until `node.sh delete` deleted
   them by hand. Phase 1's drain check found the same: "a drained node keeps its old chunks on
   disk" (docs/phase1-plan.md, step 6).
2. **Nothing can delete one chunk.** The only deletion is a whole volume's:
   - the control socket's `delete VOLUME` (`flaminio-io/src/control.rs`) sends
     `Input::DeleteReplicas` (`flaminio-node/src/engine.rs`), refused while the volume is attached
     on the node. A delete that times out on the socket (50 s) stays queued, and runs later;
   - `ReplicaService::delete_volume` (`flaminio-node/src/replica/mod.rs`) deletes every chunk of
     the volume in the store at epoch `u64::MAX`, and forgets the volume's fence
     (`ReplicaCore::remove_volume`).

   So it cannot serve a node that still holds other chunks of the volume, or is its target. The
   store has `Store::delete_chunk(key, epoch)`, which refuses an epoch below the chunk's fence.
   `ReplicaCore::remove_chunk` forgets one chunk and keeps the fence; only the simulator uses it.
3. **Nothing would call it.** The node agent (`control/cmd/flaminio-agent`) is a command that
   lists and claims disks. It has no loop and no metadata client. The only sweep, of
   tombstones, runs in the CSI node plugin (`control/csi/tombstones.go`), as the node's agent
   identity (`control/cmd/flaminio-csi/main.go`), so not on fl-c (issue #14). A node cannot list
   its copies either: the control socket has `attach`, `detach`, `delete` and `status`.
4. **A copy is not free to go at the drop.** The target that proposed the drop may not know yet
   that it committed (a lost answer; `meta_timeout` and `refresh` are 2 s each, `Timing`,
   `flaminio-repl/src/types.rs`).
   - Its requests for a deleted chunk are dropped (`ReplicaCore::handle`), time out after
     `request_timeout` (3 s), and fail the whole session (`Target::tick`). The node then leaves
     the ISR of every chunk it holds of the volume, and each is resynced.
   - A `Hello` creates, empty, every chunk it names that the node lacks, whatever its epoch
     against the fence (`ReplicaService::hello`); the target names those its view places there
     (`engine.rs`, `Request::Hello`). A target that reconnects, at its epoch, with a stale view
     after the deletion makes an empty chunk that it takes for an ISR member.
   - The simulator deletes the source at the drop's commit (docs/replication-protocol.md §7,
     moves), but its `Hello` creates nothing (`ReplicaCore::handle`; chunks are made at setup,
     `sim/mod.rs`). It is no evidence for the real data plane here.
5. **A copy or a fence left behind is a hazard, not only space** (issue #14's review).
   - A node's store names a volume by a hash of its name (`volume_uuid`: SHA-256 over
     `flaminio-volume:` and the name, `flaminio-node/src/lib.rs`), and a `Hello` keeps the chunks
     a node has.
   - So a copy left behind of a volume that is deleted, and created again with that chunk placed
     on that node, becomes the new volume's chunk there: it serves the old data. No tombstone
     names that node, so the id is free while the copy is there.
   - A new volume of the name starts again at epoch 1 (`domain.Header.Attach` from 0). A fence
     left by the old one deposes it; an old capability token (they never expire,
     `pkg/captoken`) is valid against it.
   - A deletion that lands after a new volume of the same name was placed on the node deletes the
     new volume's chunk: `u64::MAX` passes any fence.
6. **A fence goes with a node's last chunk of a volume.** The store keeps fences per chunk
   (`Store::fence`); after a restart a node holding no chunk of a volume has no fence for it
   (`ReplicaCore::fenced` answers 0). Deleting chunks one by one makes this happen to volumes
   that still exist.

## What we want

1. A node deletes its copy of a chunk that a move dropped, once no target can still use it.
2. Only the node's own agent deletes it, and only what the metadata service released to it. A
   deletion never touches a copy that the volume, or a later volume of the same name, uses.
   No copy a record named is left for a later volume of the same name to adopt.
3. A `Hello` at the epoch a node is already fenced at creates no chunk.
4. A node keeps its fence for a volume after deleting its last chunk of it.
5. Moving a chunk back to a node, deleting the volume, and creating it again all still work.
   At most, they wait for that node's deletion.
6. One sweeper per node, the agent, for releases and tombstones, on every data node.
7. Draining a node leaves no replicas of the drained volumes on its disk.

The new volume's epochs (item 5's fences and tokens) are issue #30's.

## Out of scope

- **Copies no record names:** left before this change, or by a node that came back after an
  administrator confirmed a tombstone for it (issue #14). This change makes no new ones, but in
  one window of its rollout (spec, S14). They are listed (spec, S8) and deleted by hand once in
  the lab. A later volume of the name does not adopt them: #30 drops them at its first session.
  Their automatic deletion is a follow-up (follow-ups §13).
- **A volume's incarnation** in the store and the tombstones, beyond #30's first epoch: what
  #30 leaves open stays open here (follow-ups §14).
- **`delete VOLUME` without a token.** Any root process with the control socket can delete a
  volume's copies today. A tombstone token for it is a follow-up.
- **Forgetting a node** that is gone for good: its releases stay, and a deletion of their
  volume names it on the tombstone, which nothing can then confirm (spec, S11). A follow-up.
- **An index of releases by node.** The listing reads every chunk record (spec, S4); an index
  is a follow-up, for clusters larger than the lab.
- **A drain that waits for the deletions.** A drain still ends when the metadata service places
  no replica on the node. It shows the releases still held.
- **Draining volumes that are not attached** (issue #17).
- **Unchanged:** the replication protocol and the target; moves and ISR changes, but for the
  release and its refusal; the two-step deletion (#8); the drain's end; the simulator, but that
  it confirms the releases it makes.
