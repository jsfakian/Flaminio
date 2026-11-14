# Issue #13: Replicas left behind by moves — design

How the [spec](spec.md)'s rules (S1–S18) are built. Settled choices: [decisions](decisions.md).
It builds on #30 (a volume's first epoch, `Store::delete_volume(uuid, below)`), #10 (held
groups, `settle_local`), #11 (S11, a dropped replica's acknowledgements) and #14 (the harness).

**The facts it starts from:**
- **A drop forgets the source.** `ApplyIsrChange` clears `moving` when the new replicas lack the
  source (`meta/domain/domain.go:272`); `CheckMove` refuses only a non-replica source, a replica
  destination or a pending move (`domain.go:287`). `replicaNodes` names replicas and a move's
  two ends only (`meta/service/service.go:92`). `readVolume` reads the header, then the chunks,
  apart (`service.go:303`). `ListVolumes` omits volumes being created or deleted
  (`service.go:853`).
- **Authorization.** `ListTombstones` and `ConfirmDeletion` allow an administrator or the node's
  `agent` (`meta/service/auth.go:75-78`); `ChangeIsr` any node's `io`, the method checking the
  target (`service.go:801`), with authentication on only.
- **Authentication is always on for a data plane.** `flaminio-io node` refuses an empty
  `--trust-domain` (`flaminio-io/src/node.rs:210-216`); `flaminio-meta serve` requires one
  unless `--insecure` (`meta/cmd/flaminio-meta/main.go:111-113`); the metadata client speaks only
  mutual TLS (`flaminio-metaclient/src/lib.rs:85-95`), so it cannot reach an `--insecure`
  service.
- **The only sweep** is the CSI node plugin's (`control/csi/tombstones.go`, started at
  `control/cmd/flaminio-csi/main.go:96`), as `agent` (`deploy/csi/driver.yaml:114`).
- **The node daemon** has one metadata client, as `io`, 3 s per endpoint (`node.rs:308-316`),
  one thread and a FIFO queue (`flaminio-metaclient/src/lib.rs:110`). Its control socket (0600)
  knows `attach`, `detach`, `delete`, `status` (`flaminio-io/src/control.rs:83-111`). `delete
  VOLUME` sends `Input::DeleteReplicas`, refused while the volume is in `targets` or `attaching`
  (`flaminio-node/src/engine.rs:696`); `ReplicaService::delete_volume` then deletes with
  `Store::delete_volume(uuid, None)` (`flaminio-node/src/replica/mod.rs:813`).
- **The replica drops** a request at its session for a chunk it lacks, and one out of range
  (`flaminio-repl/src/replica.rs:282`); the target's request then times out (3 s) and fails the
  session (`target.rs:807`). A `BeginResync` for a chunk it lacks creates it, whatever the
  record says (`replica.rs:259-261`). Only `fail_peer` requeues reads and digests and aborts
  resync rounds (`target.rs:1214`); `apply_view` and `adopt_replicas` only forget writes' sends
  and acknowledgements (`target.rs:2510`, `2609`).
- **Proposals are rebuilt** from chunk state at each send: the ISR less removals plus a joiner,
  and a move's step (`send_meta`, `target.rs:2389`, `2433-2440`). `meta_inflight` is cleared at
  `meta_timeout` (`target.rs:850-855`); a reply is seen only for the current `meta_inflight`
  (`on_meta_inner`, `target.rs:1087`).
- **A `Hello`** creates every chunk it names that `vols` lacks, then fences
  (`replica/mod.rs:231-258`); `Store::fence` writes nothing for a volume with no chunk
  (`flaminio-store/src/store.rs:1386`). A known volume's shape is always checked
  (`replica/mod.rs:206`). In the core and the simulator a `Hello` carries no chunk list and
  creates nothing (`replica.rs:173-184`), and a restart keeps every fence (`replica.rs:136`).
- **The frame decoder** refuses an unknown type (`flaminio-net/src/frame.rs:537`); the client
  side then ends the session (`flaminio-node/src/net.rs:279-284`).
- **The simulator** deletes a move's source at the drop's commit, in the step that commits it
  (`sim/mod.rs:769-794`), so no release is ever pending in a run, though `request_move` picks
  former sources (`sim/mod.rs:1261-1285`). `end_chaos` clears `down_links`
  (`sim/mod.rs:1332-1341`). Two mutation checks rest on hinted seeds:
  `catches_fast_reads_across_a_fence` (103 223) and `catches_keep_dropped_acks` (1 183 196)
  (`sim/tests.rs:455-501`).
- **The harness** deletes a deleted volume's copies on every data node itself
  (`scripts/lab.sh:1755-1769`), stops when a volume is pending for fl-c (`lab.sh:1777-1786`),
  and confirms by hand through `vol-confirm` (`lab.sh:1799-1847`, `scripts/guest/confirm.sh`,
  `flaminio-meta confirm`, `meta/cmd/flaminio-meta/main.go:219-300, 475-495`).

## Releases in the metadata service (S1–S5, S12)

**The fields** (`meta/domain/domain.go`), both sorted sets, omitted when empty:
- `Chunk.Released []string` (`released`): nodes holding a copy to delete; two at most, one at
  most beside a move (`MaxReleased` = 2, S2's bound).
- `Header.Fenced []string` (`fenced`): fence holders; 16 at most (`MaxFenceHolders`, S3).

**Their bounds, and #80's budget** (`B(N,R) = 2048 + N × (288 + 264 × (R+1))` ≤ 4 MiB, #80's
design; 132 B a name, 128 bytes and its tag and length). #13 changes #80's formula to

```text
B(N,R) = 4160 + N × (288 + 264 × (R+1) + 132)  ≤ 4,194,304
caps N: 4419, 3457, 2838, 2408, 2090 for RF 1 to 5  (#80's today: 5137, 3881, 3119, 2607, 2239)
```

- **`released`: two a chunk, one beside a move.** Encoded, a chunk takes 14 B (its tag and
  length in `Volume.chunks`, 3; `isr_ver`, 11) and 131 B a name (128 bytes, a tag, a 2-byte
  length). In a move, between the join and the drop, it has R + 1 replicas, R + 1 in the ISR and
  two endpoints: `538 + 262R` B. #80's term, `552 + 264R`, leaves 16 B (RF 1) to 24 B (RF 5):
  no release beside a move. The 132 B added to the term (orchestrator's decision, round 2) hold
  one: `669 + 262R` against `684 + 264R`. At rest a chunk has room for five, but every release is
  made by a move's drop, so a chunk that held one beside its move holds two after the drop, and
  a third would need a move beside two. Hence: a move of a chunk holding two is refused (S2's
  bound), and a chunk holds at most two at rest and one during a move.
- **`fenced`** adds 16 × 132 = 2112 B to the constant (round 1): 2048 + 2112 = 4160.
- **#80's worst-view fixtures** (Create, Get, Attach) gain a full `fenced` and every chunk in a
  move beside one release (the worst chunk), and prove `proto.Size` within the new `B`.
  Whichever of #80 and #13 lands second changes the constant, the per-chunk term and the
  fixtures.

**The domain rules:**
- **`ApplyIsrChange`** (S1): where it clears `moving` because the source left the new replicas,
  it adds the source to `Released`. It is the chunk record's existing put: no new operation in
  `ChangeIsr` (64 changes at most, `MaxChangesPerRequest`, under `kv.MaxTxnOps` 120).
- **`ApplyIsrChange`, for a forgotten source** (S19): `ChangeIsr` then takes the source out of
  `Released` again (`domain.Forget`), so a drop releases no forgotten node.
- **`CheckMove(c, from, to, forgotten)`** returns why it refuses, or nothing, checking in order:
  today's causes; `to` in `Released`, `node T releases chunk C` (S2); `Released` holding
  `MaxReleased`, `holding 2 releases of chunk C` (S2's bound: no two stand beside a move);
  `to` forgotten, `node T is forgotten` (S19). `MoveReplica` then answers `Applied: false`, the
  chunk and that `reason` (new, field 3; today's causes worded as the message's comment).
  Release and move write one key, and each compares it. A pending move is refused too, and a
  drop releases and clears `moving` in one put, so with the bound a chunk in a move holds at most
  one release.
- **`ConfirmRelease(ch *Chunk, node string) bool`** (new): removes `node` from `Released`,
  reporting whether it was there.

**`ConfirmRelease(node, volume, chunks[{chunk, revision}])`** (`service.go`, new; S3; with #72's
change to it, #72's S11):
1. Ids valid; 1 to 118 chunks, none twice; else `InvalidArgument`. The node forgotten (the
   mark read, S19): `FailedPrecondition`.
2. `readHeader`: `NotFound` if gone, creating or deleting (the tombstone covers the node). Each
   chunk below the header's chunk count, else `InvalidArgument`, as `ChangeIsr` checks after its
   `readHeader` (`service.go:776-778`).
3. Each chunk record: absent, `chunkMissing`; not at its listed revision, `Aborted` ("chunk N
   changed since it was listed"), nothing written; node not in `Released`, `InvalidArgument`.
   Then the node's ledger `/flaminio/v1/capacity/<node>` (#72's key), its revision as read, 0 if
   absent.
4. One `Txn`: compares the header's revision, each listed revision and the ledger's (120 at
   most). Puts each chunk with the node removed; the header with the node added to `Fenced` only
   if absent and `Fenced` holds fewer than 16; the ledger by #72's release rule
   (`releaseCharge`, #72's design): n chunk sizes and n slots less, one slot fewer when this call
   adds the node to `Fenced`; absent or unreadable, compared and not written (120 at most). No
   ISR version changes. #72 lands with or after #13; until it does no ledger exists, and the
   rule's absent case is the whole of it.
5. A failed compare: read again from step 2. Every chunk still at its listed revision: the
   header or the ledger moved (an attach, another charge or release of the node), retry as
   other methods do (`retries`, `backoff`). Otherwise `Aborted`.
6. Once written, one `slog.Info("release confirmed", ...)` as `logConfirmation` does
   (`service.go:678`): caller, peer, certificate serial, volume, node, chunks, revision.

The ledger read is the service's work within the sweep's one call: the node's own bound on its
metadata calls (S6's one sweep call, S17's one lookup) does not change.

**`ListReleases(node)`** (S4): `ValidID(node)`, else `InvalidArgument`; for the node's own
`io`, a forgotten node: `FailedPrecondition` (S19). One `List` of
`volumes/`, one of `chunks/`. A chunk is listed when its volume's header parses and is neither
`creating` nor `deleting`, and its `Released` has the node: `{chunk, revision}` (the entry's
`Rev`), grouped by volume, in key order, the first 1024 (`maxReleasesListed`). Records without
a header, or that do not parse, are skipped.

**Deletion** (S5): `replicaNodes(fenced []string, chunks)` also adds each chunk's `Released` and
the marked header's `Fenced` (`service.go:566`). The mark changes the header, so `ChangeIsr`
and `ConfirmRelease`, which compare it, change neither once the deletion has begun.

**Records out:** `chunkPB` sets `released`, `volumePB` sets `fenced` (`service.go:329, 337`); its
reader is the administrator (`flaminio-meta get`, Lab check 2). The key comment at the top of
`service.go` names both fields.

**Authorization** (`auth.go:62-83`; S12). With `io(node)` = `id.Component == "io" && id.Node ==
node`:
- `ListReleases`: `admin || io(r.Node)`.
- `ConfirmRelease`: `io(r.Node)` only.
- `ListTombstones`, `ConfirmDeletion`: `admin || io(r.Node)`. `agent` loses both, so an old CSI
  sweep gets `PermissionDenied`.
- `ForgetNode`: joins the administrator-only case (`auth.go:64`); no node identity, the
  forgotten node's own included.

## Forgetting a node (S19)

**The mark,** one key, `/flaminio/v1/forgotten`: `{"nodes":["n1",...]}`, sorted; never written but
by `ForgetNode`, never shrinks. One key, so that a method checks every node it touches with one
compare.

**`ForgetNode(node)`** (`service.go`, new), rerunnable:
1. `ValidID(node)`, else `InvalidArgument`; `nodes/<node>` present, else `NotFound`.
2. **Mark,** unless already marked: it reads the mark and `drain/<node>` (with #78, the gate
   too), then one `Txn` comparing the mark's revision (0 if absent), `drain/<node>`'s (0 if
   absent) and, with #78, the gate's, putting the mark with the node added and, only when
   `drain/<node>` is absent, `drain/<node>` (so placement and the drain's candidates skip it as a
   draining node).
   An existing drain marker is left as it is, its drain generation unchanged (a running drain
   of the node goes on). A failed compare retries (`retries`, `backoff`).
3. **Releases:** one `List` of `volumes/`, then one of `chunks/`, as `ListReleases` reads them.
   Per volume whose chunks name the node in `Released`: `deleting`, skipped, not named (its
   tombstone, written after the mark, leaves the node off, and its records go with it);
   `creating`, named in `pending`; otherwise batches of 118, one `Txn` each comparing the header
   and each chunk (119), putting each chunk without the node (118). A failed compare reads that
   volume again and retries.
4. **Fence holds:** a second `List` of `volumes/`, after every step 3; each header naming the
   node in `Fenced`: `deleting`, skipped as in step 3; otherwise compare it, put it without the
   node.
5. **Tombstones:** each naming the node: compare it, put it without the node in `nodes` and
   `confirmed`, or delete it when no node is left unconfirmed, as `ConfirmDeletion` does
  (and, once #72 lands, its `charges` entry).
6. Answer `{releases, fence_holds, tombstones, pending}` (counts cleared; volumes left), and one
   `slog.Info("node forgotten", ...)`: caller, peer, certificate serial, node, the counts.

No clearing transaction compares the mark: it is final once read. A tombstone written after the
mark never names the node (`DeleteVolume`, below), so a volume being deleted needs no later run.
`pending` names only volumes being created, read again by a later run once created.

**After the mark,** each of these reads the mark:
- **`MoveReplica`:** reads the mark as the first read of each attempt, inside its loop (after
  `nodes/<to>`, read once before it, `service.go:870`), then the header and the chunk;
  `CheckMove` refuses a forgotten `to`; the transaction also compares the mark (3 compares, 4
  with #72's ledger). A move that read the mark before it was written and the chunk after step 3
  cleared the release is the one the compare stops: it fails, retries and is refused. Without
  it, that move back would commit. Once #78 lands, its gate compare does this instead (below).
- **`ChangeIsr`:** a drop of a forgotten source leaves no release (`domain.Forget` after
  `ApplyIsrChange`). A request that drops a source compares the mark too (66 at most).
- **Placement:** `createVolume` gains a retry loop (today it makes one claim, and a failure
  returns, `service.go:372-473`): up to 32 attempts (`retries`), each reading the mark first,
  then the tombstone check, `nodes()`, the candidates, `Place` and the claim; the chunk scan,
  which only orders placement, is read once before the loop, as in #72's. It skips a node the
  mark or `drain/` names (`service.go:399`; the mark's transaction also drained it). Its claim
  compares the mark beside the volume and tombstone keys (`service.go:415`). After a failed
  claim, it reads the volume key and the tombstone key again, a fresh `Get` each. The
  deleting-header branch comes first, as today (`service.go:424`): an interrupted deletion is
  finished, and `Aborted`. Otherwise either key present is `ErrExists` at once, with no retry;
  both absent means the claim placed against a mark that moved (with #72, or a ledger), and the
  next attempt places again; after the last, `Aborted`. This is the loop #72 adds its ledgers
  to.
  `DrainNode(draining: false)` for a forgotten node: `FailedPrecondition`, its transaction
  comparing the mark.
- **With #78** (whichever of #13 and #78 lands second makes it so): `ForgetNode`'s mark ensures
  #78's placement gate. Only when it writes the drain marker (none exists) does it write the
  gate too, comparing the marker's absence, as #78's own mark does for a new marker (3
  compares, 3 writes). An existing marker is kept with its generation and the gate unwritten (2
  compares, 1 write): it already keeps the node out of every claim and move that reads it, and
  the gate compare, bumped when that marker was written, stops those that read before it.
  #78's gate compare in the claim and in `MoveReplica` then covers the forgetting, and #13's
  own mark compare in both goes ("cut before adding"). **The claim's 120 until then:** #13's
  mark compare takes the place #78's gate compare would hold, and the two never stand together.
  So with #72 at `MaxPlacementNodes` 116 the claim carries 2 + 1 + 116 = 119 compares, and #76's
  admission, which omits the gate without #78 (#76's design), 3 + 1 + 116 = 120. Neither a
  landing order nor a smaller `MaxPlacementNodes` is needed.
- **`DeleteVolume`:** reads the mark after its deleting mark and before listing the chunks,
  in each attempt of its loop, and leaves forgotten nodes off the tombstone
  (`replicaNodes`' replicas and move endpoints as well as releases and fence holds,
  `service.go:92, 567`); its tombstone transaction compares the header and the mark (2
  compares), and a failed one goes round its loop again. A tombstone left with no node is not
  written. Safe: no move or placement brings the id back to a forgotten node, so what stays on
  it is of the kind "copies no record names". #72's tombstone `charges` must leave it out too
  (a change for #72).
- **`RegisterNode`:** checks the mark before its no-change return (`service.go:201-203`), the
  usual path of a restarting node: `FailedPrecondition` ("node N is forgotten"); a write also
  compares the mark beside the node record. A machine coming back registers under a new id.
- **`ListReleases`, `ConfirmRelease` by the node's `io`:** `FailedPrecondition`. `ConfirmRelease`
  compares no mark (it would be its 121st compare): a confirmation racing the mark either commits
  before the run rewrites its chunks (step 3), and step 4, whose listing comes after every step
  3, takes out the fence hold it added, or fails its chunk compares: `Aborted`.

Its replicas are not touched: the drain or #45's rebuild moves them off, the drops releasing
nothing. Its copies stay on it.

**The reference model** (`flaminio-repl/src/meta.rs`): `Metadata` gains `released:
BTreeMap<(VolumeId, ChunkIdx), BTreeSet<NodeId>>`, outside `ChunkMeta`, so that targets, views
and the engine are untouched, and `forgotten: BTreeSet<NodeId>`. `ChangeIsr`'s drop adds the
source unless forgotten; `MoveReplica` is `Rejected` for a released `to` (S2), for a chunk at the
bound, and for a forgotten `to`; `pub fn released(&self, vol, chunk) -> BTreeSet<NodeId>`, `pub
fn confirm_release(&mut self, vol, chunk, node) -> bool` (false for a forgotten node), `pub fn
forget(&mut self, node) -> usize` (marks it, then takes it out of every release: S19). Injected
bugs (`#[cfg(test)] meta::Bugs`, `Metadata.bugs`): `move_while_released`, `MoveReplica` ignoring
`released`, S2 and its bound alike; `forget_without_mark`, `forget` clearing the releases without
marking (S19). The simulator catches both.

**The vectors** (`examples/meta_vectors.rs`): each `after` chunk gains `"released":[...]`; a new
op, one draw in ten, `{"op":"confirm_release","chunk":C,"node":"nX","applied":B,"after":{...}}`,
its node a released one 80% of the time. The Go replay (`meta/domain/domain_test.go`) reads
`released` into `vectorChunk`, compares it, and replays `confirm_release` with
`domain.ConfirmRelease`. Another op, one draw in twenty, `{"op":"forget","node":"nX","after":
{...}}`; the replay keeps the forgotten set, applies `domain.Forget` to each chunk, and passes the
set to `CheckMove` and to the drop. `make meta-vectors` regenerates; `make contracts` checks.

**The admin CLI** (`meta/cmd/flaminio-meta/main.go`):
- **`releases <node>`** (new): `ListReleases`, printed as JSON like the others, `{}` for none.
- **`forget <node>`** (new): `ForgetNode`, its answer printed as JSON; usage: "forget a node gone
  for good, for good: no move, placement or registration to it again (docs/issues/13)".
- **`confirm` goes,** with `confirmArgs`, `agentIdentity`, `pendingFor`, `--trust-domain`, and
  their usage lines and doc comment (S13).

## The sweep (S6, S11, S14, S15)

**The option** (`flaminio-io/src/node.rs`): `--delete-released`, no value, off by default;
`NodeConfig.delete_released`. Usage: "delete chunks the metadata service released here
(docs/issues/13): only once every metadata instance and node runs this version, and off before
any downgrade". Changing it is a restart of the node's data plane.

**`EngineConfig`** (`engine.rs:167`) gains `sweep: Option<Duration>` (`Some(SWEEP_EVERY)`, 5 s,
from `node.rs`; `None` in engine tests that do not test it) and `delete_released: bool`.

**`flaminio-node/src/sweep.rs`** (new): the sweep's state, driven by `Engine::turn` and by
`MetaWait::Sweep` answers. At most one metadata call outstanding, none queued behind it.

```text
Idle { next }                    -> ListTombstones(node)
Tombstones { list, at }          -> per tombstone: delete here, then ConfirmDeletion(volume, node)
Releases { tombstoned }          -> ListReleases(node)          (only with delete_released)
Read { vols, tombstoned }        -> GetVolume(vols[0])          (one per volume)
Delete { vols, cur }             -> no call: one chunk per engine turn (S7)
Confirm { vols, cur, batches }   -> ConfirmRelease(node, volume, <= 118 chunks), in turn
```

A volume's chunks to confirm go in batches of `CONFIRM_BATCH` (118), in index order, one call
each (#72's S11).

1. **Tombstones,** one per answer. A volume in `targets` or `attaching` is refused ("attached
   here"), not confirmed. Otherwise `settle_local`, `service.delete_volume(volume)`; `Ok` sends
   `ConfirmDeletion`, `Err` is logged and not confirmed.
2. **Releases,** with `delete_released`: the listed volumes, but those step 1 listed, one at a
   time (S7). Then the deleted and absent chunks are confirmed at their listed revisions.
- **Failures** (S15): a failed list ends the sweep; a failed or `NotFound` `GetVolume`, or a
  failed confirmation, skips that volume or batch. Nothing is retried within a sweep; the next
  lists again. The next sweep starts `sweep` after the last step ends.
- **`Engine::next_wakeup`** includes the sweep's `next`, and now while `Delete` has chunks.
- **Log lines** (stderr): `flaminio-node: sweep: VOLUME: N replicas deleted, confirmed`;
  `...: VOLUME: refused: attached here`; `...: VOLUME: N released chunks deleted, A absent, K
  kept`; `flaminio-node: sweep: CALL failed: CODE: MESSAGE`.

**The metadata client** (`flaminio-metaclient`): `Call::{ListTombstones, ConfirmDeletion,
ListReleases, ConfirmRelease}`, their `Answer`s and `call_one` arms; `pb.rs` gains the messages
of Contracts and `ChunkMeta.released` (6). The data plane does not read `Volume.fenced`.

**One `GetVolume` constructor** (`engine.rs`, `get_volume_call(volume) -> Call`) builds every
`GetVolumeRequest` the data plane sends: the attach (`engine.rs:668`), the refresh (`1782`), the
sweep's read and the lookups' read. #75's `enforces_qos: true` is set there, so it covers all
four, whichever of #75 and #13 lands second.

**The CSI sweep goes:** `control/csi/tombstones.go` and its test, its goroutine and comment
(`control/cmd/flaminio-csi/main.go:88-96`), `Meta.ListTombstones` and `Meta.ConfirmDeletion`
(`control/csi/meta.go:108-121`), `Dataplane.Delete` and `ControlSocket.Delete`
(`control/csi/dataplane.go:23, 125`), and the fakes' `Delete` (`csi_test.go:198`).

**One sweeper per node** (S11): no lock. A block device opens `O_EXCL`
(`flaminio-uring/src/uring.rs:131`); the lab's stores are block devices.

## Deleting released chunks (S7)

**The read:** the volume's `GetVolume` answer. Kept: `E` (its epoch), `read_at` (the answer's
instant), `gen` (`service.deletions()`), and a `ReleaseRead` per listed chunk (below). A chunk
whose record has this node in `released` and does not name it, and that this node does not
hold, is absent: confirmed with no deletion.

**S7's decision is sans I/O, in `flaminio-repl`** (`src/release.rs`, new), so that the sweep and
the simulator run the same rule:

```rust
pub const RELEASE_READ_MAX_AGE: Time = 5 * 1_000_000;                    // 5 s
pub struct ReleaseRead { pub epoch: Epoch, pub at: Time,                 // the volume's read
                         pub released: bool, pub named: bool }           // this node, this chunk
pub struct Here { pub now: Time, pub placed: bool, pub fence: Epoch, pub deleted_since: bool }
pub enum Keep { NotReleased, Named, TooOld, Placed, Fenced, Deleted }
pub fn may_delete(read: &ReleaseRead, here: &Here) -> Result<(), Keep>
```

`named`: the record has this node in `replicas`, `moving_from` or `moving_to`. The checks, in
order: `released`, else `NotReleased`; not `named`, else `Named`; `now - at <=
RELEASE_READ_MAX_AGE`, else `TooOld`; not `placed`, else `Placed`; `fence <= epoch`, else
`Fenced`; not `deleted_since`, else `Deleted`.

**Each chunk, one per engine turn, checked in that turn** by `may_delete`, the sweep giving
`now` and `at` as the engine's microseconds (`Engine::us`), `placed` = the volume is in
`attaching`, or a local target places the chunk here (`rt.view` or `rt.sm.places(chunk,
rt.local)`), `fence` = `service.fenced_of(volume)`, `deleted_since` = `service.deletions() !=
gen`:
- `NotReleased` or `Named`: that chunk is kept, not confirmed; the volume goes on;
- `TooOld`: the volume stops here, the rest waits for the next sweep's read;
- `Placed`, `Fenced`, `Deleted`: that chunk and the rest of the volume are kept, not confirmed.

An attached local target does not refuse a deletion; only its view does.

**The deletion,** in the turn of the check: `settle_local(now)`, then
`service.delete_released(volume, chunk, E) -> Result<Released, String>` (`Deleted | Absent`):
- `Store::delete_chunk_keep_fence(key, E)`: `check_epoch(slot, E)` refuses a fence above `E`;
  one batch of `ChunkDelete`, and `FenceOnly` at the volume's highest fence when it was the
  volume's last chunk (S10);
- `core.remove_chunk(vol, chunk)` (`replica.rs:148`, keeps the fence), and the chunk leaves
  `vols[uuid].chunks`; `vols[uuid]` stays, with its shape.

**`ReplicaService::deletions() -> u64`:** a counter bumped before the store is called by
`delete_volume`, and by `hello`'s #30 drop, whether or not the store call succeeds. A deletion
that outlives its read cannot touch a new volume's chunks or fence.

## Listing copies (S8)

**`Input::ListCopies { volume: Option<String>, reply: mpsc::Sender<Vec<Copy>> }`** answers from
`service.copies()`: the store's `chunks()` and `fences()`, from memory, with no settle.

**The control socket** (`control.rs`), one line each way, answered within `LIST_LIMIT` (10 s,
below the CSI's limit, checked by `the_limits_are_their_steps_and_below_the_csi_limit`):

```text
list [VOLUME]  ->  ok [ENTRY ...]          (or: error ...)
ENTRY          =   HASH:INDEX:FENCE        a chunk
                 | HASH:fence:FENCE        a fence-only record
HASH           =   the volume's store id, as Uuid's Display (36 characters)
```

Sorted by hash then index, the fence entry last. With `VOLUME`, only its entries; an invalid id
(`[a-zA-Z0-9][a-zA-Z0-9._-]{0,127}`) gets `error: 'X' is not a volume id`. The usage error gains
`| list [VOLUME]`, the module comment the line.

**`scripts/guest/node.sh list`** (`VOLUME` optional, checked by `volume_id` only when set:
`volume_id` exits on an unset `VOLUME`, `node.sh:102-105`):
`control "list ${VOLUME:-}"`; stdout what follows `ok `, exit 0; `not running`, exit 3;
anything else, exit 1, as `delete`.

## Opening a session (S9)

**The decision moves into the core** (`flaminio-repl`), so the simulator checks it:
- `Request::Hello { chunks: Vec<ChunkIdx>, shape: (u32, Block) }` (`msg.rs`), the one shape for
  #13 and #86: `chunks` as #86 defines it, the ids, ascending, of the chunks whose `replicas`
  hold the node, computed in `Target::open_session` and kept in `Session.hello_chunks` (#86's
  required set); `shape`, the volume's chunk count and blocks per chunk (#13). The engine takes
  the list for `Frame::Hello` and `Accepted.chunks` instead of `rt.view`'s (`send_from_target`,
  `engine.rs:1915`); `ReplicaService::hello` builds the request from `Accepted`. Whichever of #86
  and #13 lands first adds `chunks`; #13 adds `shape`, and makes the core use `chunks` (#86's
  core ignores it).
- `Action::Fence { epoch, create: Vec<ChunkIdx>, reply }` (`replica.rs`): the core accepts the
  `Hello` (epoch at or above the fence), then sets the volume's shape and, only when the epoch
  is above the fence, lists in `create` the named chunks it lacks, within the shape, and holds
  them (empty, certain). At the fence, `create` is empty. A `Hello` answered `Stale` changes
  nothing, the shape included (S9, review F2).
- The caller makes `create` and the fence durable before the reply: the simulator's `Replica`
  creates the chunks' data (what `create` lists, not every listed chunk it lacks as in #86's
  simulator); the replica service writes them (below).

**The reply to a `Hello` at the fence** (#86's required-row rule, which #86 leaves to #13): the
`Fenced` reply has a row for each chunk the node holds after the `Hello` (#86: an explicit empty
row for each it created), so a named chunk the node lacks (deleted, S7) has none. A named chunk
with no row is not held there:
- **the target** (`on_fenced`, `target.rs:1311`) gives it nothing from that reply: `from` joins
  `c.fenced`, and `first_fence` and the uncertain extents are taken, only for a chunk whose row
  the reply has. The session opens; the chunk's first request there gets `NotHeld` (S17), handled
  by S18. Fewer facts than an empty row: in `Fencing`, a member without one is left to the
  takeover wait, as an unfenced member today;
- **#86's validator** (whichever of #86 and #13 lands second makes it so): its "all required ids
  present" becomes "a required id with no row is not held there, and given nothing"; every
  other check (counts, order, extents, ids within the volume) stays, and a row present is
  checked as #86 states. A reply #86 refuses still fails the session.

`ReplicaService::hello` (`replica/mod.rs:177`):
- **#30's drop** first, as today, committed on its own.
- **The shape check** only when the volume holds a chunk here: a mismatch is refused before
  the core. A `Hello` whose `size / chunk_size` exceeds `u32::MAX` is refused.
- **`saved = (core.save(vol), vols.get(&uuid).cloned())`** after #30's drop: fence, session,
  chunks, filters, shape, and the service's entry.
- **On `Action::Fence`:** `vols[uuid]` takes the `Hello`'s sizes and the token's volume name
  (S17); `Store::open_volume(uuid, chunk_size, size, create, epoch)` writes a `ChunkCreate` for
  each, then `Fence`, in one batch. One over `max_batch_bytes` (4 MiB, about 65 000 creations)
  is refused (`StoreError::Invalid`).
- **On failure** (no slot, I/O, too large): both parts of `saved` are restored (a `VolumeId`
  made for this `Hello` removed); the `Hello` is refused.
- **`Store::fence`** stays for its other callers (store tests).

**Injected bugs** (`replica::Bugs`): `hello_creates_at_fence` (`create` lists missing chunks at
the fence too); `fence_lost_with_last_chunk` (`restart` sets the fence of a volume with no chunk
to 0, a store without the fence-only record).

## The fence-only record (S10)

**Format** (`docs/format/on-disk-format-v1.md`, `flaminio-format`):
- **Chunk header state 4, fence only** (§5.3, `ChunkState::FenceOnly`, `meta.rs:214`):
  `volume_uuid`, `fenced_epoch`; `chunk_index`, sizes, `last_write_seq` and `layer_count` 0.
  `ChunkHeader::validate` (`meta.rs:273`) requires exactly that for state 4.
- **Journal record 9, `FenceOnly`** (§6.3, `record.rs`): `slot u32`, reserved u32 (must be 0),
  `volume_uuid [16]`, `epoch u64` (32 B), not optional. Valid only on a free slot, for a volume
  with no chunk and no other fence-only slot.
- **`Fence` (3)** raises its volume's fence-only slot too. **`ChunkDelete` (2)** on a state-4
  slot frees it. **`ChunkCreate` (1)** may take its own volume's fence-only slot; the chunk's
  fence is the higher of the two.
- **Incompatible feature bit 2 (value 4)**, `FEATURE_INCOMPAT_FENCE_ONLY` (`consts.rs`, in
  `KNOWN_FEATURES_INCOMPAT`): set in both superblock copies, as `set_pipelined_commits` does
  (`store.rs:673`), before the first batch holding a `FenceOnly`.
- **Version rule:** format 1 with a feature bit. A disk without it has no state-4 slot and opens
  as before; an older binary refuses a disk with it (`superblock.rs:124`).
- §3's line "Feature flags in 1.0: none are defined. Reserved for later: incompat bit 0 =
  external journal device" is corrected: bit 0 pipelined commits, bit 1 reserved for MAC
  checksums (the unknown-bit test, `superblock.rs:227`, keeps using it), bit 2 fence-only.

**Refused as corrupt at open** (header load, `store.rs:554`, and replay, `state.rs:253`):
- a state-4 slot, or a record 9, on a disk without bit 2;
- a state-4 slot with a non-zero index, size, `last_write_seq` or `layer_count` (`validate`);
- a record 9 with a non-zero reserved field (`Record::decode`);
- two fence-only slots of one volume, or one beside a chunk of its volume (after loading the
  headers, and after replay).

**The store** (`flaminio-store`): a fence-only slot is an allocated `chunks[s]` with `state:
FenceOnly` and no layers, never in `by_key`.
- **Taken everywhere:** the allocator (`create_chunk`, `store.rs:1189`) skips it, but gives a
  volume its own fence-only slot first, and refuses an epoch below its fence (`StaleEpoch`); the
  checkpoint writes its state-4 header; opening loads it (`store.rs:554`) into `chunks[s]` only.
- **Hidden from chunk lookups:** `chunks()` (`store.rs:2154`) leaves it out; `fences() ->
  Vec<FenceInfo { volume, fenced_epoch }>` lists it; `fence()` counts it in its highest and
  lowest; `fsck` and `stats` count it apart; slot records (layers, maps, writes, discards) on it
  are corrupt at replay (`state.rs:354-460`).
- **`delete_volume(uuid, below)`** (`store.rs:1234`): frees it when `below` is `None` or its
  fence is below `below`, in the last batch.
- **`ReplicaService::new`** (`replica/mod.rs:127`): `core.recover_fence(vol, epoch)` for each,
  with a `vols` entry of unknown shape (sizes 0); `create_moved_chunk` refuses an unknown shape.
- **`flaminio-disk inspect`** (`flaminio-disktool/src/main.rs:352`) prints the fence-only slots
  beside the chunk slots in use.

## `NotHeld`: the replica (S17)

**The core** (`flaminio-repl/src/replica.rs`):
- `CoreVol.shape: Option<(u32, Block)>`, set by an accepted `Hello` (S9); `recover_fence(vol,
  fenced)`; `is_current(vol, epoch, session)`; `holds(vol, chunk)`; `save(vol) ->
  Option<SavedVol>`, `restore(vol, Option<SavedVol>)`.
- **`Action::NotHeld { chunk }`:** a request at the current session for a chunk the volume
  lacks, the shape known, `chunk` below the count, and its extent, if it has one, non-empty and
  within the blocks: `Write`, `Read`, `Digest`, `Mapped`, `Discard`, `Copy`, `Settle`.
  `BeginResync` still answers `CreateChunk` (an accepted risk, decisions); everything else stays
  `Drop`.

**The replica service:** `start` (`replica/mod.rs:316`), `stage` (`490`) and `admit_read`
(`759`) turn `Action::NotHeld` into `Reply::Lookup(Lookup { sref, uuid, req, chunk, kind })`,
`kind` a `LookupKind` tag; no payload. The engine takes it in `deliver_local`
(`engine.rs:1356`) and at `admit_read`'s answer (`engine.rs:2061`), with the reply's route
(`LocalFrom`). `net.rs`'s writer treats it as `Nothing`; it never reaches a connection.

**The engine's lookups** (`flaminio-node/src/lookup.rs`, new, a struct with its own tests):

```rust
pub const LOOKUP_WAITERS_MAX: usize = 4096;                     // node-wide
pub const LOOKUP_WAITERS_PER_VOLUME: usize = 256;
pub const LOOKUP_CACHE_FOR: Duration = Duration::from_secs(3);
struct Waiter { route: LocalFrom, sref: SessionRef, req: u64, chunk: u32, kind: LookupKind }
struct VolLookup { name: String, waiters: Vec<Waiter>, cache: Option<Cache> }
enum Cache { Gone { at: Instant },
             Read { at: Instant, epoch: u64,
                    chunks: BTreeMap<u32, (u64, Vec<String>, Vec<String>)> } }
pub struct Lookups { vols: BTreeMap<Uuid, VolLookup>, turn: VecDeque<Uuid>,
                     in_flight: Option<Uuid>, waiting: usize }
```

- **A new waiter,** while its volume's cache is under 3 s old: `Gone` drops it unread; `Read`
  holding its chunk answers it at once with `NotHeld`. Otherwise it joins its volume's waiters,
  for the read in flight or the one queued; past `LOOKUP_WAITERS_PER_VOLUME` or
  `LOOKUP_WAITERS_MAX`, it is dropped. A volume with waiters and no read goes to the back of
  `turn`.
- **One read in flight** (`MetaWait::Lookup { uuid }`, `Call::GetVolume`), besides the sweep's.
  When it ends, the front of `turn` is read next: volumes in turn.
- **The answer,** for each waiter whose session is still current (`core.is_current`) and whose
  chunk is still not held: the chunk's record does not name `cfg.node` in `replicas` →
  `Frame::NotHeld`; it does → dropped. `NotFound` drops every waiter, and the cache becomes
  `Gone`. A failed read drops every waiter and clears the cache. Otherwise the cache becomes
  `Read`: the answer's instant, epoch, and the parts of the chunks its waiters asked for whose
  record does not name the node (review code 5).
- **The route:** `LocalFrom::Conn` → that connection's channel; `LocalFrom::Target` →
  `reply_to_target`.
- `delete_volume` and #30's drop forget the volume's lookups. A volume created again has the
  same uuid (a hash of the name, `flaminio-node/src/lib.rs`); its first `Hello`'s #30 drop forgets
  the old entry, so it starts with a fresh one.
- **Pruned:** each engine turn, `Lookups::prune(now)` drops a volume's entry with no waiters, no
  read in flight or queued, and a cache older than `LOOKUP_CACHE_FOR` (or none).
- Lookups run whatever `delete_released` says (S14).

## `NotHeld`: the target (S18)

**The message** (`flaminio-repl/src/msg.rs`):

```rust
pub struct ChunkRecord { pub epoch: Epoch, pub isr_ver: u64,
                         pub replicas: Vec<NodeId>, pub isr: BTreeSet<NodeId> }
Response::NotHeld { chunk: ChunkIdx, record: Option<ChunkRecord> }
```

The engine maps the frame's names through `rt.nodes` without adding any (`node_id`,
`engine.rs:386`, adds): an unknown name gives `record: None`. It drops the `rt.reads` entry of a
read answered so.

**Open proposals** (`target.rs`): `Chunk.open: Vec<Proposal>`, `Proposal { record: ChunkRecord,
req: u64 }`, `MAX_OPEN_PROPOSALS = 4`. `#[cfg(test)] Chunk.closed: VecDeque<ChunkRecord>`, the
last 4 closed by a reply or a drop, for the injected bug and the forger only.
- **Opened** in `send_meta`, per change: `ChunkRecord { epoch, isr_ver: expected_ver + 1,
  replicas: new_replicas or the chunk's, isr: new_isr }`. Equal to an open one: that entry's
  `req` becomes this request's, in its place. Otherwise pushed; a fifth evicts the first.
- **Closed:**
  - by a seen reply (`on_meta_inner`, before `apply_view`): every proposal whose `req` it
    answers, `Applied` or `Rejected`;
  - by the view passing the version: `apply_chunk` adopting `isr_ver > c.isr_ver` clears all;
  - by a dropped change: where `send_meta` clears `add_replica` or `drop_replica` with no
    commit, the proposals whose `replicas` differ from the chunk's;
  - a timeout closes none.
- `check_accounting` checks: at most 4, distinct, each at `self.epoch` and `c.isr_ver + 1`.

**On a `NotHeld`** for a request `(kind, sent_at)`, already out of `inflight` (`on_reply_inner`,
`target.rs:889`), for the chunk `kind` names (`ReqKind::Settle` gains `chunk`; for `Hello` and
`Heartbeat`, step 3):
1. **The view no longer names the sender** for that chunk (`!c.replicas.contains(&from)`):
   `fail_request(now, from, kind)`, the session kept, the record ignored.
2. **Otherwise adopted,** when all hold:
   - the frame's chunk is the request's;
   - `record` drops the sender: `!record.replicas.contains(&from)` (every honest `NotHeld` meets
     it, S17);
   - `record` keeps every other ISR member: `c.isr.iter().all(|n| n == &from ||
     record.isr.contains(n))`. A joiner added is fine: the target already counts it for leases
     and writes (`Chunk.joining`), so it asks no fewer acknowledgements;
   - it equals an open proposal's, epoch included.

   Then `apply_chunk(now, i, &ChunkMeta { replicas, isr, isr_ver, moving })`, `moving` cleared if
   its source left `replicas` (as `ApplyIsrChange`); `fail_request(now, from, kind)`;
   `after_view(now)`. Event `Event::Adopted { node: from, chunk, record }`. So a liar can have
   only itself dropped (review F1, N1). **Leases:** after adoption the ISR is the old one less
   the sender, plus at most the joiner, whose confirmations the target already required; the
   sender's are no longer needed, and `may_read_from` stops reading from it. No read or write
   is admitted that the old view would have refused for any other member.
   A drop carrying a pending removal fails this test: `send_meta` (`target.rs:2433-2440`) puts
   the removal in the same `IsrChange` as the drop, so when the answer was lost both did commit,
   and the target just cannot adopt them from a `NotHeld`. It learns them with its next view
   (a refresh or an answer); until then the session to the source fails on each `NotHeld`, as
   today, and reads of that source's chunks wait. Nothing is lost.
3. **Otherwise** `put_back` and `fail_peer`, reason `"a NotHeld that is no proposal of ours"`.

No configuration turns adoption off: authentication is always on (the facts above), so S18's
"off: never" holds by construction.

**The engine on `Event::Adopted`** (`drain_one`, `engine.rs:1770`, whose irrefutable `let
Event::SessionFailed` becomes a `match`): `rt.view.chunks[chunk]` takes the record (names
mapped back, `moving` as the target cleared it), and the log line `flaminio-node: VOLUME: chunk
C: NODE no longer holds it: adopted the committed record at version V`.

**The refactors,** with no change of behaviour:
- `fail_request(now, n, kind)`: `fail_peer`'s body per request (`target.rs:1222-1273`): bulk
  slots released, a `Mapped` probe's round aborted, digests and reads requeued at the front.
  Also, for `Copy`, `Discard` and `BeginResync` to `n`, `n`'s resync round aborted; for a write,
  `check_write`, the view having forgotten `n`'s send (#11 S11). `fail_peer` calls it for each
  request in flight.
- `apply_view` splits into `apply_chunk(now, i, m)`, its loop body, and `after_view(now)`
  (sessions to nodes holding nothing end, `check_write`).

**Accessors:** `places(&self, chunk, node) -> bool` (S7); `#[cfg(test)]` `open_proposals(&self,
chunk)` and `closed_proposals(&self, chunk) -> Vec<ChunkRecord>` (the forger).

**Injected bugs** (`target::Bugs`):
- `not_held_any_epoch`: the match ignores the record's epoch;
- `not_held_any_next_version`: any record at the view's version + 1 and the target's epoch is
  adopted, unmatched;
- `not_held_keep_closed`: proposals closed by a reply or a drop still match;
- `not_held_any_sender`: a record still naming the sender is adopted when it matches;
- `not_held_other_leavers`: the "keeps every other ISR member" test is skipped.

All `Bugs` are `#[cfg(test)]`, as today.

## The simulator (S2, S7, S9, S10, S17, S18, S19)

`flaminio-repl/src/sim/mod.rs`:
- **`Hello`s and restarts** go through the core's rules (S9, above): the target sends its
  chunk list and shape, and the sim's `Replica` creates what `Action::Fence` lists. A restart
  keeps fences, as the store does with the fence-only record.
- **Honest replicas:** `Replica::handle` returns `None` for `Action::NotHeld` and keeps a
  `Lookup` (`take_lookups`). The sim sends `Body::RelayReq` to `META`, lost as other metadata
  traffic is. `META` reads the volume's epoch then and schedules `Ev::RelayChunk` 0–2 ms later
  (one read in ten: 0.5–3 s), which reads the chunk then: a split read. The relay's messages'
  latencies and this gap draw from `relay_rng` (seeded `seed ^ 0x5E1A_7000`), not the run's
  `rng`, so a run with no lookup draws nothing new (review 2, code 2). `Body::RelayRep` reaches
  the node, which answers `NotHeld` on the request's session if it is current, the chunk still
  lacked, and the record does not name the node.
- **A released source keeps its copy until its sweep deletes it** (S2, S7), so a release stays
  pending while moves go on. Where a `ChangeIsr` releases a node (the model's `released` gains it:
  the hook factored out of `deliver`'s `MetaReq` arm, `sim/mod.rs:769-794`, which deleted the
  copy there), the sim schedules `Ev::SweepRead { node, chunk, inc }` after a delay drawn from
  `Config.sweep_read`. There the node, if up and reaching `META`, reads the model into a
  `ReleaseRead` (the volume's epoch, the node's clock, `released`, `named`) and schedules
  `Ev::SweepDelete { node, chunk, inc, read }` after a delay drawn from `Config.sweep_delete`.
  There `release::may_delete` decides (S7, the sweep's own function), with `placed` from the
  node's target (`Target::places`), `fence` from its core and `deleted_since` false (the sim
  deletes no volume). Allowed: the copy goes (`Replica::remove_chunk`, the fence kept), then
  `meta.confirm_release` if the node reaches `META`. `TooOld`, `Placed` or `Fenced`, a node down
  or cut from `META`, a crash since the read (`inc`), or a confirmation not sent: the next
  `SweepRead`, 5 s on, where a released node that lacks the chunk only confirms. `NotReleased` or
  `Named` ends it. `Config::random` sets `sweep_read` to 0–5 s and `sweep_delete` to 0–6 s
  (constants, no draw from the configuration's `Rng`), so a read past 5 s is refused too; both
  delays draw from `relay_rng`, so a run with no lookup and no move draws nothing new.
  `scenario_config` fixes both at 100 ms.
- **Target events:** `after()` drains each target's `take_events()`, so the invariant below runs
  on every `Adopted`.
- **The forging node,** only in a `forging` configuration of the tests (`Config::random` with
  `forger: Some(n)`, `forge_pct: 10`). `Config.forger`, `Config.forge_pct` and the forging code
  are `#[cfg(test)]`; `Config::random` sets them to `None` and 0, so `make sim`'s campaign
  (the `sim` example) and the hinted seeds never forge (review code 2, review 2 code 5). Its
  choices use an `Rng` of its own. During chaos, a request to the forger is answered, at that
  rate, with a forged `NotHeld`, unhandled:
  - (a) the target's epoch, the view's version + 1, the view's replicas less the forger, another
    ISR;
  - (b) an open proposal at another epoch (± 1);
  - (c) a closed proposal (`closed_proposals`): rejected, or a dropped change;
  - (d) an open proposal that still names the forger;
  - (e) an open proposal that drops the forger and also removes another ISR member (forged
    when one is open, else (a)).
  - Never an open proposal at the target's epoch that drops the forger and keeps every other
    ISR member (`open_proposals`).
- **The invariant:** at each `ChangeIsr` `Applied`, the sim records each changed chunk's record
  with the epoch it was committed at: `committed: BTreeSet<(ChunkIdx, u64, Vec<NodeId>,
  BTreeSet<NodeId>, Epoch)>`. Each `Event::Adopted` of a target at epoch `E` must match one
  committed at `E` or later (later: that target is deposed). Else the run fails with `adopted a
  record never committed: chunk C version V at epoch E`.
- **`try_seed`** (`sim/tests.rs:229`) counts that failure as a catch.
- **Scripted steps** (`sim::Action`) gain:
  - `Move(chunk, from, to)`: the whole move at `META` in one step, at the volume's epoch:
    `MoveReplica`, then the add, the join and the drop as `ChangeIsr`s. When the add and join
    commit, the destination gets a copy of the blocks of a member in the ISR then, as a finished
    resync would: the source when it is the only member (both scenarios, RF 1), any other member
    otherwise; never left empty. The drop goes through the hook that starts the source's sweep
    (above), so under `scenario_config` the source's copy goes 200 ms after the drop. A refused
    `MoveReplica` ends the step, nothing changed. No target is involved.
  - `Restart(node)`: a crash, then the restart 100 ms later.
  - `CutOneWay(from, to)`: a set of its own, `one_way: BTreeSet<(NodeId, NodeId)>`, checked in
    `send` and `deliver` beside `link_down`; a message lost to it breaks its session connection
    (`break_conn`) as any lost message. `Heal(a, b)` clears both directions and the two-way cut;
    `end_chaos` clears `one_way` as it clears `down_links` (`sim/mod.rs:1332-1341`), so a script
    that leaves a one-way cut still settles.
  - `AssertStale(node, chunk)`: the run fails with `scenario: the target's view is not stale`
    unless the target on `node` places the chunk on a node (`Target::places(chunk, n)`, the S7
    accessor) that `META`'s record of the chunk no longer names.
  - `AssertDeposed(node)`: the run fails with `scenario: the target on N is not deposed` unless
    the target on `node` is deposed (`Target::is_deposed`).
  - `Forget(node)`: `meta.forget(node)` at `META` (S19); the node runs on, its sweep's reads
    already taken kept.

| Injected bug | What trips it |
|---|---|
| `not_held_any_epoch` | forged (b): an open proposal at another epoch |
| `not_held_any_next_version` | forged (a): version + 1, another ISR |
| `not_held_keep_closed` | forged (c): a rejected or dropped exact proposal |
| `not_held_any_sender` | forged (d): an open proposal still naming the forger |
| `not_held_other_leavers` | forged (e): an open drop of the forger, another member removed |
| `hello_creates_at_fence` | `scenario_reconnect_after_deletion` (below) |
| `fence_lost_with_last_chunk` | `scenario_old_target_after_restart` (below) |
| `move_while_released` (`meta::Bugs`) | `scenario_move_back_before_deletion` (below); `catches_move_while_released` |
| `forget_without_mark` (`meta::Bugs`) | `scenario_move_back_after_forgetting` (below) |

- **`scenario_reconnect_after_deletion`** (`scenario_config(3, 1)`, chunk 0 on `[1]`):
  `0: Attach(0)`; `500 ms: Write(0, 0, 0)`; `1 s: CutOneWay(META, 0)` (the target's answers and
  views lost, its requests still arriving); `1.5 s: Move(0, 1, 2)` (node 1's copy deleted by
  its sweep at 1.7 s);
  `2 s: AssertStale(0, 0)`; `2.5 s: Cut(0, 1)`, `4 s: Heal(0, 1)`: heartbeats fail the session
  with no request for chunk 0, and the reconnecting `Hello` names it; `6 s: Read(0, 0, 0)`, from
  node 1, the view's only member; `10 s: Heal(0, META)`. Without the bug, node 1 creates
  nothing, its `Fenced` reply has no row for chunk 0 and the session opens (S9), and it answers
  the read `NotHeld` (no open proposal: the session fails, the read is requeued).
  The heal at 10 s lets the target's refresh learn the move, and the read completes from node
  2; `settle_limit` (120 s) covers the rest: passes. With
  `hello_creates_at_fence`, node 1 creates the chunk empty and the read returns 0: `chunk 0
  block 0: not linearizable`.
- **`scenario_old_target_after_restart`** (`scenario_config(4, 1)`, `chaos: 20 s`, chunk 0 on
  `[1]`): `0: Attach(0)` (T1, epoch 1); `500 ms: Write(0, 0, 0)`; `1 s: Cut(0, META)`,
  `Cut(0, 1)`; `1.2 s: Attach(2)` (T2, epoch 2, fencing node 1); `7 s: Write(2, 0, 0)`, past
  T2's takeover wait; `8 s: Move(0, 1, 3)` (node 1's last chunk deleted by its sweep at
  8.2 s); `8.5 s: Restart(1)`;
  `9 s: Heal(0, 1)` (T1 reconnects at epoch 1); `14.5 s: Read(0, 0, 0)` (the heal, a 0.5 s
  heartbeat and the 5 s lease later). Without the bug, node 1 keeps fence 2, answers T1's
  `Hello` `Stale`, T1 is deposed and its read step is skipped; the clean run's script ends with
  `15 s: AssertDeposed(0)`, so the pass is not vacuous (the bug run omits it). With
  `fence_lost_with_last_chunk`, node 1 takes the `Hello`, creates the chunk empty, and T1 reads
  0 after T2's write completed: `chunk 0 block 0: not linearizable`.
- **`scenario_move_back_before_deletion`** (S2; `scenario_config(3, 1)` with `sweep_delete`
  fixed at 4 s, chunk 0 on `[1]`): `0: Attach(0)` (T1, epoch 1); `500 ms: Write(0, 0, 0)`;
  `1 s: Move(0, 1, 2)` (node 1 released: its sweep reads at 1.1 s, deletes at 5.1 s);
  `2 s: Move(0, 2, 1)`, a move back inside that window; `6 s: Attach(2)` (T2, epoch 2);
  `12 s: Read(2, 0, 0)`, past T2's takeover wait. Without the bug, the move back is refused
  (S2; one release, under the bound): chunk 0 stays on `[2]`, node 1's copy goes at 5.1 s and
  its release is confirmed, and T2 reads the write from node 2: passes. With
  `move_while_released`, the move back commits (node 1 given node 2's blocks and joined; node 2
  dropped and released); at 5.1 s node 1 deletes on its 4 s old read, which released it and did
  not name it, so the only replica has no copy; T2's `Hello` raises node 1's fence and creates
  the chunk empty (S9), and the read returns 0: `chunk 0 block 0: not linearizable`.
- **`scenario_move_back_after_forgetting`** (S19): the same script with `1.5 s: Forget(1)`,
  between node 1's read and its deletion, as an administrator forgetting a node that is in fact
  still running. Without the bug, `Forget` marks node 1 and clears its release, and the move back
  at 2 s is refused (node 1 forgotten); node 1 deletes at 5.1 s on its read, its confirmation
  refused, and T2 reads the write from node 2: passes. With `forget_without_mark`, the release
  is cleared but node 1 not marked, so the move back commits, and the run fails as above: `chunk
  0 block 0: not linearizable`.
- **`catches_move_while_released`**, `mutation!(…, meta(|b| b.move_while_released = true),
  20_000, Config::releases)`: `Config::releases(seed)` is `Config::random(seed)` with 3 nodes,
  RF 1 or 2 (by the seed's parity), `min_isr` 1, one chunk and `move_pct` 30, set after the
  draws, so that a move back lands inside a sweep's window in random runs. The model's bound is
  the service's: with one chunk on three nodes, a move off it while a release is pending is
  taken, and RF 1 reaches two releases, where moves wait; a move back is refused by S2 alone,
  which is what the bug removes. Right after the
  simulator change, `scripts/dev/mutation-search.sh` searches its first catch; a budget or hint
  it needs is set and reported in the PR. None within 4 000 000 seeds: the test goes,
  `scenario_move_back_before_deletion` stays the catch, and the PR says so.

**The hinted seeds** (review code 2): right after the simulator change, `make sim` runs the
hints of `catches_fast_reads_across_a_fence` (103 223) and `catches_keep_dropped_acks`
(1 183 196). A hint that no longer catches is searched again with `scripts/dev/mutation-search.sh`
over its test's budget (4 000 000 and 20 000 seeds, from the old hint), its new hint and the date
written in its comment. None found within the budget: a scripted scenario of that bug replaces
the hint. Then the whole `make sim` runs, and any `mutation!` test that needs a new budget or
hint is reported in the PR (review 2, code 2). Runs shift anyway: a raising `Hello` now creates
the chunks it names, so a joiner whose target failed over after the add is created by the new
target's `Hello`, not its `BeginResync`; a failure there is a real finding, not noise. Runs
with moves shift too: a source's copy now outlives its drop, until its sweep.

## The drain (S16)

`control/drain/drain.go`:
- **A chunk at S2's bound** (`len(GetReleased())` at `MaxReleased`, 2) waits in `stepVolume`
  (`drain.go:115`), as a pending move does: counted in `Remaining`, no move requested, no error
  (S2's bound would refuse it).
- **`leastLoaded`** (`drain.go:150`) excludes the chunk's `GetReleased()` with its replicas (S2);
  #18's `choose`, which replaces it, must exclude them too (a change for #18, decisions).
  **Only released nodes left:** the chunk waits likewise; "no node to move to" (`drain.go:122`)
  stays for other causes.
- **`Progress.Held []string`:** each waiting chunk's reason, `volume V: holding 2 releases of
  chunk C (nodes N1, N2)` or `volume V: chunk C: only nodes releasing it are left (N)`, and the
  `reason` of a move refused this step; `String()` adds them, so a drain that waits says why,
  whoever's release it waits on.
- **`Progress.Released int`:** chunk records with the drained node in `released`; `String()`
  adds `"; N releases on the node not yet deleted"`.
- A move a late release refused (`Applied: false`: a move and its drop committed between the
  step's read and its request) is left to the next steps, which wait on that release (above),
  then request it again, as today.
- **With release deletion off** where releases are held (the default), a chunk moves twice, then
  waits with its reason until one of those nodes runs with release deletion on
  (`DELETE_RELEASED=1`, open once every metadata instance and node is new, S14) and deletes, or,
  if gone
  for good, is forgotten; and it never moves back to one of them. Forgetting is for a node gone
  for good only: it retires the id for good.

## The lab harness (S13)

- **`delete_volume`** (`lab.sh:1733`): its own deletion of the copies on every data node goes,
  with the re-check before it (`lab.sh:1755-1769`): it could now run after the id is free again,
  and `delete VOLUME` passes any fence (review F6). The sweeps delete and confirm. Starting a
  stopped node stays, so that its sweep runs. The fl-c stop and its procedure go
  (`1777-1786`); its comment names the data planes' sweeps; the time-out names the pending nodes
  and `make deploy`.
- **`vol-confirm` goes:** `cmd_vol_confirm` and its comment (`lab.sh:1799-1847`), its usage and
  dispatch lines (`2231`, `2287`), and `scripts/guest/confirm.sh`.
- **`node_env`** (`lab.sh:677`) passes `DELETE_RELEASED=${DELETE_RELEASED:-0}`; `node.sh up`
  adds `--delete-released` when it is 1.
- **Checks:** `make drain-test` and `scripts/checks/resync.sh` gain the checks of Lab;
  `scripts/checks/releases.sh` is new.
- **`cmd_drain_test`** (`lab.sh:1265`) dies at once unless `DELETE_RELEASED=1`: its move back
  (`drain "$VM_C"`, `lab.sh:1303`) moves chunks off fl-c, and those fl-b released when it was
  drained cannot go back to fl-b while it holds their release (S2); with deletion off on fl-b,
  one whose only free node is fl-b waits for good, and the step never ends. With it on,
  fl-b's sweep (started again by `cmd_node up b`, which passes the variable through `node_env`)
  deletes and confirms within a sweep, and the drain goes on.
- **fl-c's releases** (`ensure_node_c`, `lab.sh:1240-1253`, starts fl-c only when inactive, so a
  running fl-c keeps the setting it was started with): before the first drain,
  `cmd_drain_test` restarts fl-c's node through `node_env` (`node.sh down`, then
  `ensure_node_c`), so that it deletes with this run's `DELETE_RELEASED=1`, and waits, 60 s at
  most, until `metactl releases fl-c` is empty. Otherwise a fl-c left holding a release of
  every chunk of vol1 by an earlier run's move back is the first drain's only candidate, and
  that drain waits for good. fl-a and fl-b are restarted through `node_env` before the move back
  (`lab.sh:1297-1298`).

## Files touched

- `api/proto/flaminio/meta/v1/meta.proto`, `api/gen/` (`make proto`): S2, S3, S4, S12, S19.
- `meta/domain/domain.go`, `domain_test.go`: S1–S3, S19, vectors; with #80, its admission
  constant (the bounds above).
- `meta/service/service.go`, `service_test.go`: S1–S5, S19; with #80, its worst-view fixtures.
- `meta/service/auth.go`, `auth_test.go`: S12, S19.
- `meta/testdata/isr-vectors.jsonl` (`make meta-vectors`): S1–S3, S19.
- `meta/cmd/flaminio-meta/main.go`, `main_test.go`: `releases`, `forget`; `confirm` removed
  (S13).
- `control/drain/drain.go`, `drain_test.go`: S16.
- `control/csi/tombstones.go`, `tombstones_test.go` (deleted), `meta.go`, `dataplane.go`,
  `csi_test.go`, `control/cmd/flaminio-csi/main.go`: S11.
- `dataplane/crates/flaminio-repl/src/meta.rs`, `examples/meta_vectors.rs`: S1–S3, S19.
- `dataplane/crates/flaminio-repl/src/release.rs` (new), `lib.rs`: S7's decision.
- `dataplane/crates/flaminio-repl/src/msg.rs`: S9, S17, S18.
- `dataplane/crates/flaminio-repl/src/replica.rs`: S9, S10, S17.
- `dataplane/crates/flaminio-repl/src/target.rs`: S9 (`Hello`'s list, rows), S18.
- `dataplane/crates/flaminio-repl/src/sim/mod.rs`, `sim/tests.rs`: S2, S7, S9, S10, S17, S18,
  S19, the check.
- `dataplane/crates/flaminio-format/src/{consts,meta,record}.rs`: S10.
- `dataplane/crates/flaminio-store/src/{store,state}.rs`: S7, S9, S10.
- `dataplane/crates/flaminio-store/tests/{store_behavior,crash_consistency,recovery_edge_cases}.rs`:
  S7, S9, S10.
- `dataplane/crates/flaminio-net/src/{frame,session}.rs`: S17.
- `dataplane/crates/flaminio-node/src/replica/mod.rs`, `replica/tests.rs`: S7–S10, S17.
- `dataplane/crates/flaminio-node/src/{engine,sweep,lookup,net}.rs`, `tests/engine.rs`: S6–S9,
  S11, S15, S17, S18.
- `dataplane/crates/flaminio-metaclient/src/{lib,pb}.rs`, `tests/interop.rs`: S6, S12.
- `dataplane/crates/flaminio-io/src/{node,control}.rs`: S8, S14.
- `dataplane/crates/flaminio-disktool/src/main.rs`: S10.
- `dataplane/fuzz/regressions/{repl_frame,journal,metadata_pages}/`: new inputs (Tests).
- `scripts/lab.sh`, `scripts/guest/node.sh`, `scripts/guest/confirm.sh` (deleted),
  `scripts/checks/resync.sh`, `scripts/checks/releases.sh` (new): S13, S14, the lab.
- `scripts/tools/lab-deletion-test.sh`, `node-scripts-test.sh`: S13.
- Unchanged: `scripts/dev/mutation-search.sh` (used to search hints again).
- `docs/replication-protocol.md`, `docs/ARCHITECTURE.md`, `docs/format/on-disk-format-v1.md`,
  `docs/lab.md`, `docs/followups.md`: Docs.

## Contracts

**Proto** (`meta.proto`):

```proto
// The chunks released on this node: copies a move left there, by volume, at most 1024
// (docs/issues/13). Caller: that node's data plane, or an administrator.
rpc ListReleases(ListReleasesRequest) returns (ListReleasesResponse);
// Confirms that this node deleted the listed copies, at the revisions listed: all or none, 1 to
// 118. The node becomes one of the volume's fence holders, 16 at most. Caller: that node's data
// plane, unless the node is forgotten.
rpc ConfirmRelease(ConfirmReleaseRequest) returns (ConfirmReleaseResponse);
// Forgets a node gone for good, for good: marks it (no move, placement or registration to it
// again; no release of it), then takes it out of every release, fence-holder list and
// tombstone. Rerunnable. Caller: an administrator.
rpc ForgetNode(ForgetNodeRequest) returns (ForgetNodeResponse);

message ChunkMeta { ...; repeated string released = 6; }  // nodes holding a copy to delete;
                                                          // two at most, one beside a move
message Volume { ...; repeated string fenced = 15; }      // nodes that confirmed a release, which
                                                          // keep its fence; 16 at most; for
                                                          // administrators
message ChunkRelease { uint32 chunk = 1; int64 revision = 2; }
message VolumeReleases { string volume = 1; repeated ChunkRelease chunks = 2; }
message ListReleasesRequest { string node = 1; }
message ListReleasesResponse { repeated VolumeReleases volumes = 1; }
message ConfirmReleaseRequest {
  string node = 1; string volume = 2; repeated ChunkRelease chunks = 3;
}
message ConfirmReleaseResponse {}
message ForgetNodeRequest { string node = 1; }
message ForgetNodeResponse {
  uint32 releases = 1; uint32 fence_holds = 2; uint32 tombstones = 3;  // cleared by this run
  repeated string pending = 4;  // volumes being created, skipped: run again once created
}
message MoveReplicaResponse { ...; string reason = 3; }  // why it was refused; empty if applied
```

`ListTombstones` and `ConfirmDeletion` comments: "Caller: that node's data plane, or an
administrator". `ConfirmRelease`'s errors: `InvalidArgument` (ids; 0 or over 118 chunks; a
duplicate; out of range; not released), `NotFound` (volume gone, creating or deleting),
`Aborted` (a chunk changed since it was listed), `FailedPrecondition` (the node forgotten);
`PermissionDenied` from `Authorize`. `ListReleases`: `InvalidArgument` for an invalid node id,
`FailedPrecondition` for a forgotten node's own call. `ForgetNode`: `InvalidArgument`,
`NotFound` (not registered), `Aborted` (contention). `RegisterNode`, and `DrainNode` undraining:
`FailedPrecondition` for a forgotten node. `MoveReplica`'s comment names its `reason`s.

**Field numbers:** `ChunkMeta.released = 6`; `Volume.fenced = 15`, the lowest `Volume` number no
plan of the batch names (9 and 10 are #17's, #18's and #79's; 11–14 #70's in #75's design;
20–21 #50's; 30–31 #51's; 40 #75's; 40–41 #54's). `MoveReplicaResponse.reason = 3`, after its
fields 1–2; no plan of the batch adds to that message (#18 keeps it as it is).

**etcd:** one new key, `forgotten` (S19, above); `released` in `chunks/<id>/<index>`, `fenced`
in `volumes/<id>`. The key comment at the top of `service.go` names the new key.
`ConfirmRelease` also compares, and writes by #72's release rule, #72's
`capacity/<node>` (Releases, above).

**Vectors** (`isr-vectors.jsonl`): chunk objects gain `"released":["nX",...]` (sorted; `[]` when
empty); the ops `confirm_release` and `forget` as above.

**Replication frame** (`flaminio-net/src/frame.rs`): tag 71, `NOT_HELD`, a reply:

```text
req u64 · chunk u32 · epoch u64 · isr_ver u64
· n_replicas u8, n × (u16 len, id bytes) · n_isr u8, n × (u16 len, id bytes)
```

`Frame::NotHeld { req, chunk, epoch, isr_ver, replicas: Vec<String>, isr: Vec<String> }`.
`decode` refuses more than `MAX_RECORD_NODES` (16) in either list, an id empty, over 128 bytes or
not UTF-8, and trailing bytes; `encode` asserts the same, and the engine checks before it
builds one (a record past the limits is dropped, logged). `session::check_request` refuses it as
a request; `req_of` (`replica/mod.rs`) knows it. `Frame::Hello` is unchanged.

**On disk:** chunk header state 4; journal record 9, `FenceOnly` (32 B body, reserved 0);
incompat bit 2; the corruption rules above.

**Control socket:** `list [VOLUME]` as above. **CLI:** `flaminio-io node --delete-released`;
`flaminio-meta releases <node>`; `node.sh list` (`VOLUME` optional); `node.sh up` with
`DELETE_RELEASED=1`.

**Rust crate boundaries:**
- `flaminio-repl`: `Request::Hello { chunks, shape }` (one shape with #86's),
  `Action::Fence { epoch, create, reply }`, `ChunkRecord`, `Response::NotHeld`,
  `Action::NotHeld`, the core and target methods above, `Metadata::{released, confirm_release}`,
  `release::{may_delete, ReleaseRead, Here, Keep, RELEASE_READ_MAX_AGE}`.
- `flaminio-store`: `delete_chunk_keep_fence(key, epoch)`, `open_volume(uuid, chunk_size,
  volume_size, chunks: &[u64], epoch)`, `fences()`.
- `flaminio-node::replica`: `Reply::Lookup(Lookup)`, `delete_released`, `deletions()`,
  `copies()`, `fenced_of(volume)`, `volume_name(uuid)`, `is_current(sref)`.

## Tests

- **S1, S2** (`service_test.go`, in memory and on etcd): `TestMoveReleasesTheSource` (a move's
  steps leave the source released; an add, a shrink or a join none);
  `TestNoMoveBackWhileReleased` (`Applied: false`, `reason` `node T releases chunk C`, then
  `true` after the confirmation; another chunk `true`); `TestReleaseBound` (a move elsewhere of a
  chunk holding one release: `true`, the chunk then holding one beside the move and two after its
  drop; a third move: `Applied: false`, `reason` `holding 2 releases of chunk C`; `true` once one
  is confirmed, or its node forgotten; `released` never over two, nor over one beside a move);
  `TestReleaseAndMoveRace` (`interleaveStores`: one of them commits).
- **S3:** `TestConfirmRelease`, in memory and on etcd: after an ISR change, `Aborted` and the
  release stays; the release confirmed, the chunk moved back and dropped there again, then the
  first confirmation replayed at its old revision: `Aborted`, the new release stays; 119
  chunks, none, a
  duplicate: `InvalidArgument`; 118 accepted, on etcd too (`kv.MaxTxnOps` enforced there); a
  chunk released only to another node, a chunk released to nobody, a chunk at or beyond the
  chunk count: `InvalidArgument`; a batch of one released chunk and one not: `InvalidArgument`,
  nothing written (chunks, header, ledger), `fenced` unchanged, no log line; the first adds
  `fenced`, the second writes no header; no `isr_ver` change; after the deletion's mark,
  `NotFound`; an attach, or a write of the node's ledger, between the read and the write:
  retried, applied; the log line's fields, none on a failure. `TestFenceHoldersAtMost16`: the
  17th node's confirmation applies, `fenced` keeps 16, the header is not written. With #72
  (whichever of #13 and #72 lands second adds it): two confirmations by that 17th node, of n
  chunks each, each credit its ledger n chunk sizes and n slots, no slot kept; a rerun of #72's
  `InitCapacity` then leaves that ledger as it is.
- **S4:** `TestListReleases`: by node; 1024 of 1500; creating, deleting and header-less records
  left out; an invalid node id.
- **S5:** `TestTombstoneNamesReleasedAndFenceHolders` (both on it; the id refused until they
  confirm); `TestOldInstanceRewriteDropsReleased` (an old `domain.Chunk`: the move back is
  accepted, the risk S14 states).
- **S12** (`auth_test.go`): `io` for its own node only, on all four; `agent` refused on all four
  (an old CSI sweep); administrators list releases and keep the tombstone methods, never
  `ConfirmRelease`; `ForgetNode` administrators only, every node identity refused.
- **S19** (`service_test.go`, in memory and on etcd):
  - `TestForgetNodeRefusals`: after the mark, a move to the node (`reason` `node T is
    forgotten`), its placement in a new volume, `RegisterNode` (a changed record, and an
    unchanged re-registration, which writes nothing), `DrainNode` undraining it, its `io`'s
    `ListReleases` and `ConfirmRelease` (`FailedPrecondition`), and a drop of a move off it (no
    release) are each refused; a move off it is accepted;
  - `TestForgetKeepsARunningDrain`: forgetting a node already marked draining leaves
    `drain/<node>`'s revision as it was; with #78 (whichever lands second adds it), the gate is
    unwritten too, and the running drain's `CheckDrain` with its generation still answers, not
    `FailedPrecondition`;
  - `TestForgetNodeClears`: releases on 119 chunks of a volume (two batches), on two volumes, a
    fence hold, a tombstone naming it with another node pending (kept, without it) and one naming
    only it (deleted, the id free); a volume being deleted at the run, not named in `pending`,
    whose tombstone, once written, does not name the node; a volume being created, named in
    `pending`; a volume with a replica on the node, deleted after the
    forgetting, writes no tombstone naming it (one with only that node writes none, the id
    free); a second run clears nothing; the log line;
  - `TestForgetRacesAMoveBack` (`interleaveKV`; the node released by the chunk, not a fence
    holder, with no drain marker): `ForgetNode` runs in a hook on the move's header `Get`
    (`readHeader`), the first read after every destination check. Without #78, the move read
    the mark as absent and reads the chunk after step 3 cleared the release, so only the mark
    compare stops it; with #78, it read the gate and `drain/<to>` before the mark, so only the
    gate compare stops it. The move is refused, never committed, and the test fails with that
    compare removed;
  - `TestForgetRacesAConfirmation` (two stacked `interleaveKV` wrappers, one hook each, held on
    channels: the outer on the `Get` of `forgotten`, the inner on the `List` of `volumes/`): the
    node's `ConfirmRelease`, the node not yet a fence holder, reads the mark before
    `ForgetNode`'s step 2 (the outer hook, held), then runs and commits in the inner hook, on
    step 3's `List` of `volumes/`, which holds whatever order step 3 reads in; after both, the
    node is in no `released` or `fenced`. It fails if step 4 works from step 3's listing;
  - **`claimCounter`** (new test helper in `service_test.go`): a `kv.KV` wrapper that counts the
    creation's claims, the `TxnRev` calls whose compares hold the volume key at revision 0 and
    whose ops put it, whatever their result (a deletion's `deleteChunks` compares that key at 0
    but puts nothing of it). `interleaveKV.txns` counts every write of its key, the creation's
    last header write included, so it cannot count claims. The two tests below use it;
  - `TestForgetRacesAPlacement` (`interleaveKV` over `claimCounter`): `ForgetNode` runs in a
    hook on `createVolume`'s `List` of `drain/`, the last read of `nodes()`, so the creation read
    the nodes, `drain/` included, before the mark; the first placement uses the node
    (`PreferNode` the node). The first claim fails; the second attempt reads the mark, skips the
    node and claims: `CreateVolume` succeeds, with exactly two claims (`claimCounter`), and no
    chunk of the volume names the node. It fails with the claim's mark compare removed (with
    #78, its gate compare);
  - `TestCreateVolumeRetries` (the new loop; claims counted by `claimCounter`): a test KV that
    rewrites `forgotten` (a fresh node, not a candidate, forgotten) after each of the creation's
    reads of it, so every claim's mark compare fails: `Aborted` after 32 claims; a taken id:
    `ErrExists` after one claim, no retry; a tombstone written between the attempt's tombstone
    check and its claim (a hook on the check's `Get`): `ErrExists` after one claim; a tombstone
    already there: `ErrExists` before any claim, as today; an interrupted deletion found after a
    failed claim: finished, `Aborted`, as today;
  - `TestForgetRacesADeletion` (`interleaveKV`): the node a replica of the volume only, neither
    released nor a fence holder, so `ForgetNode` writes nothing of the volume; `ForgetNode` runs
    in a hook on `DeleteVolume`'s `Get` of `forgotten`, read after the deleting mark and before
    the chunks' listing: the tombstone, written after the retry, does not name the node. It
    fails with the tombstone's mark compare removed.
- **Vectors** (`domain_test.go`, `make contracts`); `meta.rs` tests: the drop releases, a move
  back is rejected while released and any move at the bound, `confirm_release`; `forget` clears,
  then a move to the node is rejected and a drop of it releases nothing.
- **The bounds** (with #80, `service_test.go`): #80's worst-view fixtures with a full `fenced`
  and every chunk in a move beside one release fit the new `B` at each RF's cap.
- **S6, S11, S14, S15** (`tests/engine.rs`; `FakeMeta`'s model gains tombstones and releases,
  and can hold or fail answers):
  - `release_deletion_is_off_by_default` (no `ListReleases`);
  - `not_held_is_sent_with_release_deletion_off`;
  - `the_sweep_orders_tombstones_and_releases` (no `ConfirmDeletion` while a release deletion of
    its volume runs);
  - `a_failed_deletion_or_confirmation_is_not_confirmed_and_retried`;
  - `nothing_is_deleted_without_the_service`;
  - `a_stalled_client_holds_a_change_isr_behind_two_calls` (one sweep call, one lookup);
  - `released_chunks_are_confirmed_in_batches_of_118` (a volume's 119 released chunks: two
    `ConfirmRelease` calls, of 118 and 1);
  - with #75 (whichever lands second adds it): a QoS-limited volume's released chunk is deleted
    and its `NotHeld` sent, `FakeMeta` refusing a `GetVolume` without `enforces_qos`, as #75's
    engine tests do for the attach and the refresh;
  - `node.rs`: `delete_released_is_off_unless_given`.
- **S7:** `release.rs` tests: `may_delete` allows the clean case and refuses each `Keep`, a read
  exactly 5 s old allowed. `tests/engine.rs`, `replica/tests.rs`:
  `a_released_chunk_is_deleted_at_the_first_sweep`;
  kept while still a replica, moving, placed by a local view, attaching;
  `a_read_over_5_s_old_defers`; `a_higher_fence_after_the_read_keeps_the_rest`;
  `a_deletion_since_the_read_keeps_the_rest` (`delete VOLUME`, a failed one too; #30's drop by a
  volume of the name created again); `deletion_under_io_lets_other_chunks_complete`;
  `the_fence_and_other_chunks_stay`.
- **S8** (`control.rs`): `list_formats_chunks_and_fences`, `list_refuses_a_bad_volume_id`;
  `LIST_LIMIT` in the limits test.
- **S9** (`replica.rs` tests, `replica/tests.rs`): `a_hello_at_the_fence_creates_nothing`,
  `a_raising_hello_creates_in_one_batch`, `a_failed_hello_restores_the_core_state` (no slot, too
  large; #30's drop stays; `vols` restored), `a_stale_hello_sets_no_shape`,
  `the_shape_is_kept_with_no_chunk`, `a_hello_over_u32_chunks_is_refused`;
  `crash_consistency.rs`: `Op::OpenVolume`, at every crash point.
- **S10** (`store_behavior.rs`, `recovery_edge_cases.rs`, `flaminio-format` tests): the last
  chunk deleted, a restart: a `Hello` below `Stale`; at the fence it records the shape, and a
  `BeginResync` takes the slot; a checkpoint, then a restart, keeps it; another volume's creation
  does not take it; `delete VOLUME` frees it, still free after a restart; #30's drop frees it
  below the first epoch; replay of a `ChunkCreate` onto it and of a `ChunkDelete` of it; bit 2
  refused by a reader without it; `inspect` counts it; `crash_consistency.rs`:
  `Op::DeleteKeepFence`. Refused as corrupt at open, one test each in `recovery_edge_cases.rs`:
  `two_fence_only_slots_of_a_volume`, `a_fence_only_slot_beside_a_chunk`,
  `fence_only_without_bit_2` (a header and a record), `a_fence_only_header_with_sizes`,
  `a_fence_only_record_with_reserved_bits`.
- **S17** (`replica.rs`, `lookup.rs`, `replica/tests.rs`, `frame.rs` tests):
  `not_held_only_when_the_record_does_not_name_the_node`, `out_of_range_is_dropped_unread`,
  `begin_resync_still_creates`, `waiters_join_their_volumes_read`, `waiters_past_the_bound_drop`,
  `one_volume_cannot_fill_the_bound`, `volumes_are_read_in_turn`, `no_payload_is_held`,
  `old_caches_are_pruned`,
  `a_failed_read_or_a_gone_volume_drops`, `a_gone_volume_is_not_read_again_within_3_s`,
  `a_resend_within_3_s_is_answered_from_the_cache`, `only_chunks_not_naming_the_node_are_cached`,
  `a_superseded_session_gets_nothing`, `not_held_round_trips`, `not_held_limits`.
- **S18** (`target.rs` tests):
  - refused: `a_rejected_exact_proposal`, `a_dropped_change`,
    `the_fifth_distinct_evicts_the_oldest`, `after_the_view_moved_on`,
    `a_record_still_naming_the_sender_is_refused`, `a_named_sender_not_matching_fails_the_session`;
  - `a_stale_reply_to_an_older_request_closes_nothing`;
  - `a_drop_with_a_pending_removal_fails_the_session` (the session fails on each `NotHeld`
    while cut off, then the target recovers once a view brings the committed drop and removal),
    `a_joiner_added_is_accepted`;
  - accepted: `after_its_release_was_confirmed`,
    `after_meta_timeout`, `rebuilt_after_a_lost_first`,
    `the_first_distinct_after_five_identical_resends`;
  - requeued, the session kept: `a_read_digest_or_mapped_answered_not_held`,
    `after_the_view_learned_the_drop_first`;
  - S9's reply: `a_reconnect_naming_a_lacked_chunk_keeps_the_session` (a `Fenced` reply at the
    fence without the named chunk's row: the session opens, that chunk gets no `c.fenced`,
    `first_fence` or uncertainty, the others their rows; its first write answered `NotHeld` is
    handled by S18), with #86's validator once both have landed;
  - engine: `a_cut_off_proposer_learns_its_drop_from_not_held` (drop committed, answer lost,
    target cut off, chunk idle, copy deleted; its next write goes on, no acknowledged write lost)
    and `..._after_the_source_restarts`; `adoption_updates_the_view_for_the_next_hello`;
    `a_reconnect_naming_a_deleted_chunk_opens_the_session` (the replica service deletes a
    released chunk; a stale target reconnects at its epoch naming it; the session opens, I/O to
    the node's other chunks goes on, the chunk's next write gets `NotHeld`), under #86's
    validator once both have landed.
- **Simulator** (`sim/tests.rs`, `make sim`): `campaign_with_a_forging_node` (the `forging`
  configuration); `mutation!(catches_not_held_any_epoch, ...)`, `catches_not_held_any_next_version`,
  `catches_not_held_keep_closed`, `catches_not_held_any_sender`, `catches_not_held_other_leavers`,
  on `forging`;
  `scenario_reconnect_after_deletion`, `scenario_old_target_after_restart`,
  `scenario_move_back_before_deletion` and `scenario_move_back_after_forgetting`, each passing,
  and failing (not linearizable) with `hello_creates_at_fence`, `fence_lost_with_last_chunk`,
  `move_while_released` and `forget_without_mark` respectively; `catches_move_while_released` on
  `Config::releases`, under the search rule above; the hinted seeds rerun (above).
- **S13** (`make lint`): `lab-deletion-test.sh` loses its `vol-confirm`, fl-c and per-node delete
  cases and gains: pending for fl-c, it waits as for any node; no `node.sh delete` call; the
  time-out names the pending nodes and `make deploy`; never a `confirm`. `node-scripts-test.sh`
  loses its `confirm.sh` cases and gains `node.sh list` (exit 0, 3, 1; a bad `VOLUME` refused)
  and `up` with `DELETE_RELEASED=1`. `lab-deletion-test.sh` also extracts `cmd_drain_test`:
  without `DELETE_RELEASED=1` it dies before any remote call; with it, the fake fl-c records
  `node.sh down` and then `up` with `DELETE_RELEASED=1`, and the fake metadata service a
  `releases fl-c` that must be empty, all before the first `drain` (one that stays non-empty
  past the limit: it dies naming fl-c).
- **S16** (`drain_test.go`): `TestChunkAtTheBoundWaits` (no move requested, counted in
  `Remaining`, no error, `holding 2 releases of chunk C (nodes N1, N2)` in `Progress.Held` and its
  `String()`; moved once one is confirmed), `TestNoReleasedDestination`,
  `TestOnlyReleasedNodesLeftWaits` (its reason shown), `TestRefusedMoveReasonShown`,
  `TestLateReleaseRefusedThenRetried`, `TestProgressCountsReleases`.
- **Fuzz** (`dataplane/fuzz/regressions/`): `repl_frame`, a `NotHeld`; `journal`, a record 9, and
  one with its reserved field set; `metadata_pages`, a state-4 header, and one with sizes.
- **Targets:** `make lint test race hygiene contracts sim crash-test`. No `unsafe` code changes:
  `miri` and `sanitize` are not needed.

## Lab

On request (`/ship-spec 13 lab`), after the rollout; each wait polls every 5 s, 60 s at most.

**Rollout** (S14), in order:
1. `make meta-up`. From here `agent` cannot list tombstones: no lab deletions until step 3.
2. `make csi-deploy` (no sweep).
3. `make deploy`; `make node-up`; for fl-c, `make node-down VM=c`, then `make data-node-up`.
   Release deletion off.
4. Once every instance and node is new: the same restarts with `DELETE_RELEASED=1`.
5. **The hand cleanup, once,** recorded in the PR, with no drain, move or attach in progress
   (and none started until it ends):
   1. on each node, `node.sh list`: every hash;
   2. for each name in `metactl volumes` and `metactl tombstones`, `node.sh list NAME` gives its
      hash;
   3. for a matched name, just before its delete, `metactl get NAME` (and `metactl tombstones`):
      `delete NAME` only if no record names the node (not a replica, not moving, not released,
      not a fence holder, no tombstone pending for it);
   4. a hash no name matches may still be named (a volume being created or deleted is not
      listed): it stays, listed in the PR. No store is reformatted on that basis.

**Checks,** with `DELETE_RELEASED=1` in the environment:
1. **`make drain-test`:** after fl-b is drained into fl-c, before fl-b's node stops
   (`lab.sh:1288`), fl-b's `node.sh list vol1` holds no chunk and `metactl releases fl-b` none;
   after the move back, the same for fl-c. fio verifies. fl-c is restarted through `node_env`
   first and holds no release before the first drain. `DELETE_RELEASED=1` is required
   (`cmd_drain_test` dies without it): with it off, fl-b's releases stay, and the move back
   cannot take a chunk fl-b released back to fl-b (S2); one with no other free node waits for
   good, its progress naming `chunk C: only nodes releasing it are left (fl-b)`.
2. **`FROM=fl-a TO=fl-b scripts/checks/resync.sh`** (item 3: RF 1, 16 GiB): after the moves,
   fl-a's list for the check volume is a single `HASH:fence:E`, `metactl get` lists fl-a in
   `fenced`, and no release is left. Its cleanup no longer deletes by hand: it waits for the
   tombstone to go, then fl-a's list is empty.
3. **`scripts/checks/releases.sh`** (new): an RF 1 volume on fl-b, written by fio from fl-a; fl-b
   restarted without release deletion; chunk 0 moved to fl-c; the move back refused (`applied:
   false`, `reason` `node fl-b releases chunk 0`); fl-b restarted with it; within 60 s its
   release goes and the move back is taken; fio verifies. Then fl-b's node stopped and the
   volume deleted with `metactl delete` (not `delete_volume`, which would start fl-b): its
   tombstone names fl-b for 10 s; fl-b started, the tombstone goes; the volume created again
   reads zeros.
4. **No CSI sweeps:** `make csi-logs` shows no tombstone line; every `deletion confirmed` and
   `release confirmed` line of the metadata service has a caller `.../node/<node>/io`.

No lab check forgets a node: a forgotten id is forgotten for good, and the lab needs its three
nodes. The service tests and the simulator cover S19.

**What they delete or reformat:** checks 2 and 3 create and delete their own volumes; check 1
moves vol1 and back; the hand cleanup deletes unnamed copies of named volumes only, and
reformats nothing. **Restored after:** vol1 with its usual layout (the drain test's restore),
every node running with `DELETE_RELEASED=1`.

## Docs

- **`docs/replication-protocol.md`:** §2, the release at the drop, the move back refused while
  released, the bound of two releases a chunk (one beside a move) and its reason, and
  forgetting a node; §3, a `Hello` carries its chunk list and creates chunks only when it raises
  the fence, its reply's rows (a named chunk without one not held), the fence-only record, the
  `NotHeld` reply; §7
  (moves), the copy deleted once released, and the target's acceptance of its open proposals
  that drop the sender; §9, the forging configuration, the split read, the adoption invariant,
  the modelled sweep, the four scripted scenarios.
- **`docs/ARCHITECTURE.md` §8** (§8.3 ISR management, §8.4 epochs and fencing): the release at
  a move's drop, the fence kept without a chunk, a target learning a committed drop from a
  replica, and forgetting a node.
- **`docs/format/on-disk-format-v1.md`:** §3 bit 2 and the corrected reserved line; §5.3 state 4;
  §6.3 record 9 and records 1–3 on a fence-only slot; §7 recovery and the corruption rules.
- **`docs/lab.md`:** "Deleting volumes, and fl-c's tombstones" becomes the data planes' sweep;
  `vol-confirm` and the harness's own deletion go; `DELETE_RELEASED`, the rollout and the
  downgrade rule; `node.sh list`, `metactl releases`; the hand cleanup's procedure. **A drain
  with release deletion off:** each chunk it moves keeps its source's release; it never moves
  back to that node (S2), and once it holds two (moved twice) its next move is refused, the
  drain's progress saying `holding 2 releases of chunk C (nodes N1, N2)` or `only nodes
  releasing it are left`, until one deletes (`DELETE_RELEASED=1` there) or, if gone for good
  only, is forgotten; so drain only with release deletion on wherever releases are held, and
  `make drain-test` requires `DELETE_RELEASED=1` (and restarts fl-c with it first). It states
  the two durability residuals (Risks) for the operator. **`metactl forget NODE`:** for a node
  gone for good only, never to free a live node's chunks (release deletion is the cure there),
  once every metadata instance is new; for good; run again while it names volumes being
  created.
- **`docs/followups.md`:** §13 done, with the lab results; the follow-ups decisions names (unnamed
  copies, an index of releases by node, a token for `delete VOLUME`, the
  `ChangeIsr` queue, a `BeginResync` checked against the record); §14 step 1 done.
- **`docs/performance-plan.md`, `README.md`:** unchanged; the steady-state path does not change.
  No benchmark.

## Risks

- **Fencing.** A deletion runs at the epoch read and refuses a higher fence, in the store too
  (`check_epoch`); any deletion of the volume since the read stops it, a failed one too; a
  volume created again fences above every earlier epoch (#30). Between the read and the last
  deletion the release stays, so no move back commits (S2: the simulator's sweep and
  `move_while_released` check it); a forgotten node takes no move
  (S19, `forget_without_mark`); the confirmation comes after the deletion. The harness no
  longer deletes copies itself.
- **Crash points:** a crash in a release deletion leaves the chunk or its absence, unconfirmed;
  the last chunk's `ChunkDelete` and `FenceOnly` share one batch; a `Hello`'s creations and
  `Fence` share one batch; bit 2 is durable before the first `FenceOnly`; a disk breaking the
  fence-only rules is refused at open.
- **Durability.** A target adopts only a record that drops the sender and equals its own open
  proposal at its epoch: committed, or a later target's identical one as a refresh would read it
  (the split read; fencing and leases cover it). Adoption removes the sender after its writes
  forgot its sends (#11 S11). A lying member can at most have itself dropped, every other ISR
  member kept: no worse than acknowledging writes it never stored (review F1, N1).
- **Security, who must not:** the CSI pod (only `agent`) lists and confirms nothing; an
  administrator cannot confirm a release; another node's `io` cannot act for this node; a
  `Hello` refused as stale changes no shape (review F2); one volume cannot take more than 256 of
  the node's 4096 lookup waiters, and a gone volume is not read again for 3 s (review F3). Root
  on the node can still `delete VOLUME` (follow-up). `list` shows name hashes to root and the
  CSI pod. Only an administrator forgets a node; the node's own identity cannot. Two released
  nodes that do not delete hold a chunk they both left at the bound until one runs with release
  deletion on and deletes, or, if gone for good, is forgotten; one such node only keeps the
  chunk from moving back to it. A live node is not forgotten to free a chunk: that retires it
  for good.
- **Liveness only:** a waiter that joined a read sent before its chunk's drop gets a pre-drop
  record and is dropped; a forged or stale `NotHeld` fails a session.
- **S2's bound, the remaining cost** (rounds 2 and 3 after approval; not accepted, decisions): a
  chunk holding two releases moves again only once one of its nodes deletes (one sweep with
  release deletion on) or, gone for good, is forgotten, and a chunk never moves back to a node
  that releases it. With release deletion off (the default) a chunk moves twice, then a drain
  waits on it with its reason, as #45's rebuild will once #45 carries the reasons (decisions,
  round 3). **Two durability residuals, not accepted:**
  1. **Two dead nodes:** a chunk rebuilt off two dead nodes, neither yet forgotten, holds both
     releases; a third loss of its replicas is not rebuilt until an administrator forgets one
     of them. One dead node no longer blocks a rebuild. #45 could close it only by adding an
     administrator's "failed for good" declaration that forgets (a change #45 needs; its
     automatic, reversible Failed cannot forget).
  2. **Two live nodes, release deletion off:** a chunk drained off two live nodes that run with
     release deletion off (the default) holds both releases; after a loss of one of its
     replicas it is not rebuilt until one of those nodes runs with release deletion on and
     deletes. That cure is open once every metadata instance and node is new (S14); forgetting
     a live node is not one.
- **Bounds** (#80's budget): a 17th node to confirm a release of a volume is not named in
  `fenced`, so its fence-only slot is a copy no record names (S8 lists it; #30's drop takes it
  if the name returns): of the kind of the accepted risk "copies no record names", which does
  not yet name this case (decisions, round 1 after approval).
- **Compatibility:** an old target fails its session on `NotHeld` at once; an old binary refuses
  a disk with bit 2; an old metadata instance drops `released` and `fenced` when it rewrites a
  record (release deletion stays off until every instance is new, S14). No rollback once a
  release, fence holder or fence-only slot exists.
- **Accepted risks:** as decisions lists them, among them a `BeginResync` creating a chunk the
  record no longer places (review F4).

## ADRs

- **0001** (Go control plane, Rust data plane, the node agent in Rust): consistent; the sweep is
  in `flaminio-io node`.
- **0002** (etcd): consistent; conditional transactions, no new key.
- **0005** (replica-confirmed leases): consistent. The ISR still comes only from committed
  records, here relayed; chunks that lose a replica during a metadata outage wait as before.
- **0003, 0004, 0006:** untouched.
- No new ADR: a reply type, a journal record and a feature bit follow the existing rules.

## Parallelizable?

Yes. The Go units (domain, service, auth, CLI, drain, CSI, the vectors' replay) build against
the proto and the vector format above; the Rust units against the same proto and the crate
boundaries; the lab scripts against the control-socket line, `node.sh list`,
`--delete-released` and `metactl releases`. Within Rust, `flaminio-repl`, `flaminio-format` and
`flaminio-store` come before `flaminio-node`.
