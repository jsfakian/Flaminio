# Issue #13: Replicas left behind by moves — intent

[Issue #13](https://github.com/jsfakian/Flaminio/issues/13), follow-ups §13. It takes in issue
#14's step 1: the tombstone sweep moves into the node agent, which ADR 0001 puts in Rust: the
node daemon, `flaminio-io node` (user decision, 2026-11-05). #14's step 2, confirming by hand
for fl-c, goes. Reopened after the spec's approval for one protocol change, the `NotHeld`
reply (user decision, 2026-11-05). It builds on
[issue #30](https://github.com/jsfakian/Flaminio/issues/30) (done): a volume created again starts
above every earlier epoch of its name, and its first session on a node drops what an earlier
volume of the name left there.

## Why

A move ends when the volume's target drops the source from the chunk's replicas: a `ChangeIsr`
that `validMove` accepts, and that `ApplyIsrChange` applies, clearing `moving`
(`meta/domain/domain.go`). From then on no record names the source for that chunk: no chunk
record, and no tombstone if the volume is deleted later (`replicaNodes`,
`meta/service/service.go`; docs/issues/8, out of scope). Nothing deletes its copy.

1. **The lab.** The item 3 check (follow-ups §3, 2026-10-28) moved all 64 chunks of a 16 GiB
   RF 1 volume from fl-a to fl-b. fl-a still held them (4 GiB) until `node.sh delete` deleted
   them by hand. Phase 1's drain check found the same: "a drained node keeps its old chunks on
   disk" (docs/phase1-plan.md, step 6). On 2026-11-02 fl-c held about 136 copies that no
   record names (follow-ups §14).
2. **Nothing can delete one chunk.** The only deletion is a whole volume's:
   - the control socket's `delete VOLUME` (`flaminio-io/src/control.rs`) sends
     `Input::DeleteReplicas` (`flaminio-node/src/engine.rs`), refused while the volume is attached
     on the node. A delete that times out on the socket (50 s) stays queued, and runs later;
   - `ReplicaService::delete_volume` (`flaminio-node/src/replica/mod.rs`) deletes every chunk of
     the volume in the store, whatever its fence (`Store::delete_volume(uuid, None)`), and
     forgets the volume's fence (`ReplicaCore::remove_volume`).

   So it cannot serve a node that still holds other chunks of the volume, or is its target. The
   store has `Store::delete_chunk(key, epoch)`, which refuses an epoch below the chunk's fence.
   `ReplicaCore::remove_chunk` forgets one chunk and keeps the fence; only the simulator and
   the target's tests use it.
3. **Nothing would call it.** The only sweep, of tombstones, runs in the CSI node plugin
   (`control/csi/tombstones.go`), as the node's agent identity (`control/cmd/flaminio-csi/main.go`),
   so not on fl-c: an operator confirms fl-c's by hand (`lab.sh vol-confirm`, issue #14). The Go
   `flaminio-agent` only lists and claims disks. The node daemon has what a sweep needs: it owns
   the store, and runs a metadata client as the node's `io` identity (`flaminio-io/src/node.rs`),
   on every data node; the CSI node pod mounts only the `agent` identity
   (`deploy/csi/driver.yaml`). A node cannot list its copies either: the control socket has
   `attach`, `detach`, `delete` and `status`.
4. **A copy is not free to go at the drop.** The target that proposed the drop may not know yet
   that it committed (a lost answer; `meta_timeout` and `refresh` are 2 s each, `Timing`,
   `flaminio-repl/src/types.rs`).
   - Its requests for a deleted chunk are dropped (`ReplicaCore::handle`), time out after
     `request_timeout` (3 s), and fail the whole session (`Target::tick`). The node then leaves
     the ISR of every chunk it holds of the volume, and each is resynced.
   - A `Hello` at or above the node's fence creates, empty, every chunk it names that the node
     lacks (`ReplicaService::hello`); the target names those its view places there
     (`engine.rs`, `Request::Hello`). A target that reconnects, at its epoch, with a stale view
     after the deletion makes an empty chunk that it takes for an ISR member.
   - A target cut off from the metadata service never learns of the drop. It keeps the source
     as an ISR member and writes to it (docs/replication-protocol.md §7, moves). Were the copy
     deleted, those writes would be dropped (`flaminio-repl/src/replica.rs`, `_ => Action::Drop`),
     the session would fail, and its writes would wait for an ISR change it cannot make (§4)
     until the partition heals. No wait on the source closes this: a chunk can stay idle for
     any time, and the source can restart. The target must learn of the drop another way.
   - Only the volume's target, at its epoch, bumps a chunk's ISR version: `ApplyIsrChange` is
     the only bump (`meta/domain/domain.go`), and `ChangeIsr` accepts only the current target's
     node, as `io`, at the header's epoch, with authentication on (`service.go`, `auth.go`).
     `MoveReplica` and the drain bump nothing; an attach changes the epoch. But a `GetVolume`
     reads the header, then the chunks, apart (`readVolume`), so a record it returns may pair an
     epoch with a later target's chunk; a target's own refresh reads the same way and relies on
     fencing and leases (the comment in `Attach`, `service.go`). And a target's drop can carry
     a pending removal too (`send_meta`, `target.rs`), so its proposal, not "its view without
     the source", is what it can check a relayed record against.
   - A request that a replica fails without closing the session must be put back: only
     `fail_peer` requeues reads and digests and aborts resync rounds (`target.rs`); adopting a
     view (`apply_view`, `adopt_replicas`) only forgets writes' acknowledgements.
   - No acknowledged write needs the copy: once the target applies the drop, its writes in
     flight forget the source's acknowledgements (issue #11, S11).
   - The simulator deletes the source at the drop's commit (docs/replication-protocol.md §7,
     moves), but its `Hello` creates nothing (`ReplicaCore::handle`; chunks are made at setup,
     `sim/mod.rs`). It is no evidence for the real data plane here.
5. **A copy left behind is a hazard, not only space** (issue #14's review; #30 closed part).
   - A node's store names a volume by a hash of its name (`volume_uuid`: SHA-256 over
     `flaminio-volume:` and the name, `flaminio-node/src/lib.rs`).
   - **Adoption** by a later volume of the name is closed by #30 on every node that volume
     opens a session with: its first epoch is the claim's revision (`Header.Start`), and its
     first `Hello` there deletes the chunks of the name fenced below it (`ReplicaService::hello`).
     On a node it never reaches, the copy holds space for good, and an old capability token
     (they never expire, `pkg/captoken`) still opens a session on it (docs/issues/30, S8).
   - **A whole-volume deletion passes any fence.** One that lands after a new volume of the
     name was placed on the node deletes the new volume's chunk. Only the tombstone, which keeps
     the id taken until each node it names confirms, orders the two.
6. **A fence goes with a node's last chunk of a volume.** The store keeps fences per chunk
   (`Store::fence`); after a restart a node holding no chunk of a volume has no fence for it
   (`ReplicaCore::fenced` answers 0). Deleting chunks one by one makes this happen to volumes
   that still exist. #30 does not cover it: a deposed target of the same volume has an epoch
   above its first epoch.

## What we want

1. A node deletes its copy of a chunk that a move dropped, once no target on it can still use
   it. A target that still sends requests for the chunk, cut off from the metadata service or
   slow to learn, learns of the drop from the node, after the node restarts too, and goes on.
2. Only the node itself deletes it, and only what the metadata service released to it. A
   deletion never touches a copy that the volume, or a later volume of the same name, uses.
   A volume's deletion deletes the copies it released, and the fences left without a chunk.
3. A `Hello` at the epoch a node is already fenced at creates no chunk.
4. A node keeps its fence for a volume after deleting its last chunk of it.
5. Moving a chunk back to a node, deleting the volume, and creating it again all still work.
   At most, they wait for that node's deletion.
6. One sweeper per node, its data plane, for releases and tombstones, on every data node.
7. Draining a node leaves no replicas of the drained volumes on its disk.
8. A node gone for good can be forgotten by an administrator: its releases, fence holds and
   tombstone entries end, so the chunks it left move again and the deletions waiting on it end
   (added after approval, plan review round 2).

## Out of scope

- **Copies no record names:** left before this change, or made by a target or a token of a
  deleted volume (docs/issues/30, S8). This change makes new ones only in one window of its
  rollout (spec, S14), and when a node missed a whole epoch, deleted its copy, and that epoch's
  target, its view stale, then opens a session raising its fence, or resyncs a source it no
  longer knows was dropped (a `BeginResync` creates the chunk; decisions, accepted risks).
  They are listed (spec, S8) and deleted by hand once in the lab. A later volume of the name
  does not adopt them: #30 drops them at its first session on that node. Their automatic
  deletion is a follow-up (follow-ups §13).
- **A volume's incarnation** in the store and the tombstones, beyond #30's first epoch: what
  #30 leaves open stays open here (follow-ups §14).
- **`delete VOLUME` without a token.** Any root process with the control socket can delete a
  volume's copies today. A tombstone token for it is a follow-up.
- **An index of releases by node.** The listing reads every chunk record (spec, S4); an index
  is a follow-up, for clusters larger than the lab.
- **A drain that waits for the deletions.** A drain still ends when the metadata service places
  no replica on the node. It shows the releases still held.
- **Draining volumes that are not attached** (issue #17).
- **Unchanged:** moves and ISR changes, but for the release, its refusals and a forgotten node;
  the two-step deletion (#8); the drain's end. The replication protocol gains one reply,
  `NotHeld` (spec, S17 and S18), and the target and simulator handle it; nothing else in them
  changes.
