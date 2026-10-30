# Issue #13: Replicas left behind by moves — design

Builds on issue #30: a new volume's first epoch is its claim's etcd revision (header
`first_epoch`), and a node drops a name's leftovers fenced below it at `Hello`, with
`Store::delete_volume` (below). Rule numbers are the [spec](spec.md)'s; settled choices are in
[decisions](decisions.md).

## Releases in the chunk record (S1–S5)

**The fields** (`meta/domain/domain.go`):
- `Chunk.Released []string` (`released`, omitted when empty).
- `Header.Fenced []string` (`fenced`, omitted when empty): the fence holders.

**The drop** (`ApplyIsrChange`): when a change clears `moving` because the source left the new
replicas, it appends the source to `Released`. It is the chunk record's existing put: a
`ChangeIsr` stays at most 64 changes (`MaxChangesPerRequest`), 65 compares and 64 puts, under
`kv.MaxTxnOps` (120, compares and operations counted apart, `etcd.go`).

**No move back** (`CheckMove`): also false when `to` is in `Released`. `MoveReplica` then answers
`Applied: false` with the chunk, as for its other refusals. Release and move are one key, so the
record's compare orders them.

**`ConfirmRelease(node, volume, chunks[{chunk, revision}])`** (`service.go`):
- reads the header (`readHeader`: `NotFound` if gone or being deleted);
- compares the header's revision and each chunk record at the listed revision; puts each chunk
  without the node in `Released`; if the node is not in `Fenced`, puts the header with it;
- at most 119 chunks (header + 119 compares; 119 puts + the header's). More: `InvalidArgument`;
- a failed compare: `Aborted`, no retry inside the call;
- the header put changes its revision, so a racing move or ISR change reads again, as after an
  attach. It happens once per volume and node.

**`ListReleases(node)`:** one range read of `volumes/`, one of `chunks/`. For each chunk whose
volume has a header that is neither `creating` nor `deleting`, and whose `Released` has the node:
`{volume, chunk, revision}`, grouped by volume, the first 1024 by key. Chunk records without a
header (a deletion's leftovers) are skipped.

**Deletion** (`replicaNodes`): adds every `Released` node of every chunk and the header's `Fenced`.
`DeleteVolume`'s mark changes the header, so `ChangeIsr` and `ConfirmRelease`, which compare it,
cannot change either after the listing (docs/issues/8, S3).

## The contract (S12)

- **Proto** (`meta.proto`): `ChunkMeta.released` (6); `Volume.fenced` (next free number);
  `rpc ListReleases(ListReleasesRequest{node}) returns (ListReleasesResponse{repeated
  VolumeReleases volumes})`, `VolumeReleases{volume, repeated ChunkRelease chunks}`,
  `ChunkRelease{chunk, revision}`; `rpc ConfirmRelease(ConfirmReleaseRequest{node, volume,
  repeated ChunkRelease chunks}) returns (ConfirmReleaseResponse{})`.
- **etcd:** the key comment at the top of `service.go` gains `released` and `fenced`. No new key.
- **`Authorize`:** component `sweep` (`.../node/<node>/sweep`). `ListReleases`, `ConfirmRelease`,
  `ListTombstones`, `ConfirmDeletion`: the node's `sweep`; the last two also administrators, as
  today. `agent` loses the tombstone methods. Unnamed methods stay denied.
- **The data plane** reads with `GetVolume` through the metaclient (`Call::GetVolume`, already
  there); its generated code gains the fields.
- **The reference model** (`flaminio-repl/src/meta.rs`): `released`, the drop's append,
  `CheckMove`'s refusal, a `ConfirmRelease` transition. `examples/meta_vectors.rs` writes
  `released` and adds a `confirm_release` op (so releases do not pile up in random runs);
  `create`'s first epoch is #30's. The Go replay (`domain_test.go`) reads `released` into
  `vectorChunk` (`json.Unmarshal` ignores unknown fields otherwise) and replays
  `confirm_release` with `domain.ConfirmRelease`, which `ConfirmRelease` uses too. `make
  meta-vectors` regenerates `isr-vectors.jsonl`; `make contracts` checks it.

## The agent (S6, S11)

- **`flaminio-agent sweep`** (`control/cmd/flaminio-agent`, logic in `control/agent/sweep.go`):
  the `sweep` certificate, the metadata endpoints, the control socket. It takes `flock` on
  `/run/flaminio-agent/lock`, non-blocking; held elsewhere, it exits 1. The directory, which the
  CSI pod does not mount, is made if missing (`mkdir`, 0700, not `MkdirAll`: a missing `/run` is
  an error), then checked with `lstat` (a
  root-owned 0700 directory, not a link) before the file is opened with `O_NOFOLLOW`.
- **The loop,** every 5 s: tombstones (`ListTombstones`, `delete VOLUME`, `ConfirmDeletion`), then
  releases. One step at a time; tombstone confirmations only between steps.
- **The grace:** `first map[(volume, chunk)]time` (monotonic). Each sweep drops keys it did not
  list, keys it confirmed, and keys of a volume its tombstone step handled. A key older than 10 s
  goes in that volume's `delete-chunks`, up to 119 per call; then `ConfirmRelease` of those
  deleted or absent, at their listed revisions.
- **Logged:** per volume and sweep, the chunks deleted and refused; failures as warnings. The
  data plane logs each `delete-chunks` with its counts.
- **The CSI node plugin:** `SweepTombstones` and its goroutine go (`control/cmd/flaminio-csi`,
  `control/csi/tombstones.go`).
- **The lab:** unit `flaminio-agent` on every data node; `lab.sh agent up|down [VM]`, `make
  agent-up`; `ensure_nodes` starts it; `cmd_pki issue` issues `sweep.crt`.

## Deleting released chunks (S7)

**The command** (`flaminio-io/src/control.rs`): `delete-chunks VOLUME CHUNK...`, at most 119
chunks, under the socket's 64 KiB line. It sends `Input::DeleteChunks{volume, chunks, deadline,
reply}`; the deadline is the socket's limit, as an attach's (docs/issues/6). On the socket's
timeout it answers that the request may have begun; the agent confirms nothing for it.

**The engine** (`flaminio-node/src/engine.rs`):
1. **One per volume:** a `delete-chunks` for a volume with one pending (waiting for its
   `GetVolume`, or with chunks queued, until its last chunk or its deadline) is refused at once.
   That is the guarantee: once a request has read the records, no per-chunk check would stop a
   chunk moved back in the same epoch, so nothing may confirm meanwhile. The next agent's request
   is refused, so it does not confirm; only a later one runs, with a fresh read. **Not begun by
   the deadline:** dropped. Begun: `Call::GetVolume` (a `MetaWait`, as attach's), bounded by the
   deadline.
2. **The answer:** `NotFound` or none in time: nothing deleted. Otherwise keep `E` (the volume's
   epoch) and the chunks whose record has this node in `released`, and not in `replicas`,
   `moving_from` or `moving_to`. The others are refused.
3. **One chunk per engine turn,** so replication I/O goes on; `next_wakeup` returns now while
   chunks are queued. Before each: the deadline; no local target's view places the chunk here;
   `core.fenced(vol) <= E`; no `ReplicaService::delete_volume` of the volume since (a tombstone's
   `delete VOLUME`, or #30's drop: either cancels the queue). Any failing: that chunk and the rest
   refused.
4. **The deletion,** in the same engine turn as the check: `Store::delete_chunk_keep_fence(key)`
   (its `guard` settles every commit in flight first: one commit's stall per chunk, accepted);
   `core.remove_chunk`; drop it from `vols[uuid].chunks`. No fence argument: check and deletion
   share the turn, so nothing raises the fence between them.

**The store** (`flaminio-store`):
- **`delete_chunk_keep_fence(key)`:** one batch: `ChunkDelete`, and when it is the volume's last
  chunk, `FenceOnly` at the volume's highest fence (as `fence()` computes it), in that slot.
- **`delete_volume(uuid, below)`:** #30's (it deletes the volume's chunks fenced below `below`,
  in batches of whole chunks within `max_batch_bytes`; `u64::MAX` for `delete VOLUME`). #13 adds
  the fence-only slot, freed only when its fence is below `below`, in the last batch. #30's
  `would_drop`, and its "forget the volume when the core's fence is below `first`", count it.
- **`fences()`:** the fence-only slots, for recovery (`ReplicaService::new`) and `list`.

**`list`:** every chunk slot and fence-only slot, by volume hash, index and fence.

## Opening a session (S9)

`ReplicaService::hello` (`flaminio-node/src/replica/mod.rs`):
- **The shape check** (size, chunk size) only when the node holds a chunk of the volume. A volume
  known by its fence only gets its shape from the `Hello`, in memory (`vols`), so a
  `BeginResync` can create a chunk after a restart.
- **Raises:** the `Hello`'s epoch above the store's durable fence (chunks and fence-only slot).
  At the fence: nothing created; chunks it names that the store lacks are not held, and requests
  for them are dropped (`ReplicaCore::handle`, as today for unknown chunks).
- **Missing** comes from the store, not `vols`.
- **`Store::open_volume(uuid, shape, chunks, epoch)`:** `ChunkCreate` for each missing chunk and
  `Fence` at `epoch`, in one batch. A `Hello` whose batch exceeds `max_batch_bytes` (4 MiB: about
  65 000 creations of 64 B) is refused.
- **On failure** (out of space, I/O, too large): the volume's whole core state (fence, session,
  chunks, filters) is snapshotted after #30's drop of leftovers (which commits first, and stays)
  and restored; the chunks added to `vols` are removed, and so are the `vols` and core entries
  made for a new `VolumeId` (`vol_id`, the core's `entry().or_default()`). The `Hello` is
  refused.

## The fence-only record (S10)

**Format** (`docs/format/on-disk-format-v1.md`, `flaminio-format`):
- **Chunk header state 4 = fence only** (§5.3): `volume_uuid`, `fenced_epoch`; chunk and volume
  size 0; `layer_count` 0.
- **Journal record 9, `FenceOnly`:** `slot u32`, reserved u32, `volume_uuid [16]`, `epoch u64`;
  not optional (§6.3). `Fence` (3) raises fence-only slots of its volume too. `ChunkCreate` may
  take a fence-only slot of its volume, at its fence or above: that supersedes it.
- **Freeing it:** `ChunkDelete` (2) on a state-4 slot frees the slot; it has no clusters or pages
  to free. Replay does the same. §6.3 says so.
- **An allocated slot everywhere** (`flaminio-store`): `chunks[s]` holds it, marked fence only,
  so every place that treats an empty slot as free sees it taken:
  - the allocator (`create_chunk`) does not give it to another volume;
  - replay refuses a `ChunkCreate` onto it, but for its own volume (below);
  - the checkpoint writes its state-4 header, not a free one;
  - opening the store loads state-4 headers into `chunks[s]`.
- **Hidden from chunk lookups:** opening the store does not add a state-4 slot to `by_key`;
  `chunks()` filters fence-only slots out of `chunks.iter().flatten()`. So `ReplicaService::new`
  does not recover a 0-block chunk 0; `fences()` lists them. `fence()` raises them, and counts
  them in its highest and lowest.
- **Replay:** a `ChunkDelete` of a fence-only slot frees it without touching `by_key`. A
  `ChunkCreate` onto a fence-only slot of its own volume is allowed although the slot is taken,
  and the new chunk keeps the higher of the two fences.
- **One per volume:** a `ChunkCreate` of a volume that has a fence-only slot takes that slot
  (`open_volume`'s first creation, a `BeginResync`'s), at its fence or above. So a volume never
  has two.
- **Incompatible feature bit 2 (value 4)** (§3): set in the superblock before the first
  `FenceOnly` commits. `KNOWN_FEATURES_INCOMPAT` gains it. Bit 1 stays reserved for authenticated
  checksums, so `superblock.rs`'s unknown-bit test (value 2) stands. §3's "reserved for later"
  line, which still names bit 0 an external journal, is corrected.
- **Recovery:** a fence-only slot sets the core's fence for its volume, with no chunk.
- **Slots:** one per volume and node until `Store::delete_volume` frees it. They come out of
  `chunk_slots` (at least 64, sized for 2× overcommit); `flaminio-disk inspect` counts them apart
  from chunk slots in use. The last chunk's `FenceOnly` takes the slot its `ChunkDelete` frees,
  in the same batch, so it never fails for want of a slot.

## Crash and ordering arguments

- **Release vs move:** one key; each compares it (S2).
- **Deletion vs confirmation:** the only confirmer is the node's one agent (`flock`, `sweep`),
  and it confirms after `delete-chunks` answers; while a request is pending, a second for the
  volume is refused, so no agent confirms meanwhile. Between the data plane's read and its last
  deletion, the release stays, so the chunk cannot be moved back.
- **A delete outliving its answer:** a new volume of the name (#30) or any newer attach fences
  above `E`, and a `delete_volume` of the volume cancels the queue; the per-chunk check catches
  both.
- **The drop's proposer:** learns within `meta_timeout` (2 s) or its next `refresh` (2 s); its
  requests time out after `request_timeout` (3 s) (`Timing`, `types.rs`). The 10 s grace exceeds
  the 7 s. If it reconnects with its stale view anyway, its `Hello` is at the fence and creates
  nothing; its requests for the chunk drop, its session fails, it reads the drop.
- **Last chunk:** `ChunkDelete` and `FenceOnly` in one batch: both or neither.
- **A `Hello`:** creations and `Fence` in one batch.
- **A request outliving its agent:** one per volume (S7); the next agent's is refused until it
  ends.
- **An attach:** a new target reads the records after its epoch; its `Hello` names only placed
  chunks. A target of an older epoch is refused by the fence, after a restart too (S10).
- **The volume deleted:** releases and fence holders go to its tombstone; a confirmation or a
  `delete-chunks` then gets `NotFound`, and the tombstone step deletes the copies.
- **A new volume of the name:** the id is taken while a tombstone names a released node or fence
  holder; its epochs are above every earlier one, and its first `Hello` drops the leftovers (#30).
- **An old metadata instance** that erased a release: `delete-chunks` refuses; the copy stays,
  unnamed.
- **A moving chunk** is never released: the drop releases the source as the move ends.

## The drain (S16)

`control/drain/drain.go`:
- **`leastLoaded`** also excludes the chunk's `GetReleased()`.
- **No candidate because only released nodes remain:** the chunk counts as waiting, instead of
  `stepVolume`'s "no node to move to" error, which stays for other causes.
- **`Progress`:** `Released`, releases held for the node; `WaitingRelease`, its chunks waiting
  because the only candidates are released nodes. Both shown by `String()`.

## Rollout (S13, S14)

1. `make meta-up` (every instance). From here `agent` cannot list tombstones: no sweeping until
   step 3.
2. `make deploy`, the nodes restarted.
3. `cmd_pki issue` (`sweep.crt`), `make csi-deploy` (no sweep), `make agent-up`.

Rollback: unsupported once a release, a `fenced` entry or a fence-only slot exists; the agents
stop first in any case.

**Issue #14's harness** (it lands first), in `lab.sh`, `docs/lab.md` and follow-ups §14:
- `vol-confirm`, `scripts/guest/confirm.sh` and `flaminio-meta confirm` go; so does
  `delete_volume`'s stop for a tombstone pending on fl-c;
- its `tombstones` calls move from `agent` to `sweep`;
- its prerequisite becomes `make agent-up` on every data node; a tombstone outlasting
  `TOMBSTONE_WAIT` names it.

**Docs:** `docs/lab.md`: the agent, the rollout, and the hand cleanup of unnamed copies, allowed
for a volume only when no record names the node for it (not a replica, not in a move, not
released, not a fence holder, no tombstone pending for it). The format document. Follow-ups §13
(done, and the follow-ups in decisions); §14 (step 1 done).

## Tests, per rule

- **S1–S3** (`service_test.go`, in-memory and etcd): a move's `ChangeIsr` steps leave the source
  released; an add, shrink or join leaves none. `MoveReplica` to the released node `Applied:
  false`, `true` after the confirmation; another chunk `true`. A confirmation after an ISR change
  (`interleaveStores`) fails and the release stays; one after a move back and a new drop leaves
  the new release; 120 chunks `InvalidArgument`; the first adds `fenced`, the second writes no
  header.
- **S4:** by node; at most 1024; volumes creating or deleting and header-less records left out.
- **S5:** the tombstone names released nodes and fence holders; creation refused until they
  confirm; a confirmation after the mark `NotFound`. A mixed-version rewrite (old `domain.Chunk`)
  drops `released`; the move back is accepted; the node's `delete-chunks` is refused.
- **S6, S11** (`control/agent`, an in-memory service over localhost gRPC, a fake socket): the
  grace; a key unlisted or confirmed starts again; refused, failed or timed-out deletes not
  confirmed; tombstones in the same loop, not during a release step; a second agent exits; an
  old CSI sweep as `agent` gets `PermissionDenied`; nothing deleted without the service.
- **S12** (`auth_test.go`): `sweep` for its node only; `agent` loses tombstones; administrators
  and `io` cannot call the release methods. `flaminio-meta` has no `confirm` command any more.
- **S7** (`flaminio-node`, `flaminio-io`): refused for a chunk not in `released`, in `replicas` or
  `moving`, placed by a local view; over 119; `NotFound`; no answer in time. A delete held past
  its deadline never runs. **A delete held while the volume is deleted and created again:** it
  deletes nothing of the new volume. **A request held at its `GetVolume` wait, a second one
  refused, the release confirmed, the chunk moved back:** the held one reads the record and
  deletes nothing. **A request past its read, chunks queued:** a second one is refused until its
  last chunk or its deadline, then runs with a fresh read (no confirmation out of band meanwhile).
  The other chunks and the fence stay. **I/O during a 119-chunk delete:** reads and writes of
  the volume's other chunks complete between deletions. **With no other I/O,** the queue still
  drains, one chunk per turn.
- **S9:** a `Hello` at the fence creates nothing; above, it creates; a fresh attach over a
  placed, missing chunk creates it. A failed or too large store write leaves the core state
  (fence, session, chunks, filters) and `vols` as before (#30's drop, already committed,
  excepted), with no entry left for a new volume. Crash at every point of `open_volume`.
  The stale view: drop committed, answer lost, grace passed, session blip, copy deleted,
  reconnect: not created, reads go elsewhere, no client error.
- **S10** (`flaminio-store`, `flaminio-format`, `flaminio-node`): last chunk deleted, restart: a
  `Hello` below the fence `Stale`; at the fence it records the shape, and a `BeginResync` creates
  the chunk in the fence-only slot. Crash atomicity of the last delete. A checkpoint, then a
  restart, keeps the slot. Another volume's creation does not take it. `delete VOLUME` frees it,
  and it stays free after a restart. A `Hello` with a first epoch above 0 keeps it; one of a later
  volume of the name (#30) drops it. Replay of a `ChunkCreate` onto it, and of a `ChunkDelete` of
  it. An old binary
  refuses bit 2. `inspect` counts fence-only slots.
- **S8** (`flaminio-io`): `list` shows chunk slots and fence-only slots by hash; the socket is
  0600, root's.
- **S16, drain** (`drain_test.go`): a released node is not a destination; with no other the
  chunk waits, the step does not fail; a release after the step's read: `Applied: false`, next
  step elsewhere; progress counts releases.
- **The simulator:** it confirms each release as it deletes the source at the drop. No new
  check: its `Hello` creates nothing, so S9's window is the data plane's, covered above.

## Risks

See [decisions](decisions.md), accepted risks. The largest: copies no record names (from before
this change, or erased by an old metadata instance) are adopted by a later volume of the name
until their follow-up.
