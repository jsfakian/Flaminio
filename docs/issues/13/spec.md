# Issue #13: Replicas left behind by moves — specification

## Terms

- **Copy:** a chunk replica in a node's store, keyed by the volume's hash (`volume_uuid`) and the
  chunk's index.
- **Placed:** the chunk record names the node in `replicas`.
- **Moving:** the chunk record's `moving` names the node, as `from` or `to`.
- **Released:** the chunk record names the node in `released`: the node's copy of the chunk is
  to be deleted.
- **Unnamed copy:** a copy that no record names: not placed, moving or released, and its volume
  on no tombstone pending for the node.
- **Sweep:** one pass of the node agent (S8).
- **Grace:** 10 s. The target that proposed a drop learns whether it committed within
  `meta_timeout`, or from its next poll (`refresh`), and its earlier requests time out after
  `request_timeout`: 2 + 2 + 3 = 7 s (`Timing`, `flaminio-repl/src/types.rs`). 10 s leaves room
  for clock drift and delivery.

## Behaviour

### The metadata service

**S1. A drop releases the source.** A `ChangeIsr` that drops a move's source (`ApplyIsrChange`
clears `moving`) adds the source to the chunk's `released`, in the same chunk record put. No
other change adds one: a move's drop is the only change that takes a node out of a chunk's
replicas (`validMove`). The transaction gains no compare or operation: a `ChangeIsr` stays at
most 64 changes (`MaxChangesPerRequest`), 65 compares and 64 puts.

**S2. A released chunk is not moved back.** `CheckMove` also refuses a `to` that the chunk
releases: `MoveReplica` answers `Applied: false` with the chunk, as for its other refusals. A
release and a move of the same chunk are one record, so they never both commit.

**S3. Confirming a release.** `ConfirmRelease` removes the node from `released`, for chunks of
one volume, and adds the node to the header's `fenced` (S11) if it is not there.
- **Conditional on the revisions listed:** each chunk is given with the revision `ListReleases`
  returned. The write compares those, and the header's revision as read by the confirmation. A
  chunk record changed since (an ISR change, a newer release) fails the whole call: `Aborted`,
  nothing written. So a late confirmation never removes a newer release. The agent does not retry
  it; its next sweep lists again, and confirms at the new revisions.
- **The volume gone or being deleted:** `NotFound`. Its tombstone names the node (S5).
- **At most 119 chunks per call:** the header and 119 compares; 119 puts and the header's
  (`kv.MaxTxnOps` bounds compares and operations each). More: `InvalidArgument`.
- **The header's put** happens once per volume and node, at its first confirmation. It changes the
  header's revision, so a move or ISR change racing it reads again, as after an attach.

**S4. Listing releases.** `ListReleases(node)`: for every chunk record that releases the node,
its volume, chunk and revision, by volume; at most 1024 chunks, the first by key. The rest come
in later sweeps, as these are confirmed. It reads the headers and the chunk records, one range
read each, as `CreateVolume` does for its load. Left out: volumes being created or deleted, and
chunk records without a header (`DeleteVolume`'s leftovers: their nodes are on the tombstone).
For the lab's few hundred chunk records, three nodes every 5 s is a small load; an index by node
is a follow-up (intent).

**S5. Deleting a volume with releases.** `replicaNodes` adds every released node, and the
header's `fenced` nodes. So the tombstone names them; each deletes every copy and fence-only
record of the volume (`delete VOLUME`) and confirms; until then the id is taken, so no new
volume of the name adopts a released copy. The mark fences `ChangeIsr` and `ConfirmRelease`
(docs/issues/8, S3), so none changes the records after the listing.

**S6. A new volume starts above every earlier epoch of its name.** `createVolume`'s last write
(clearing `creating`) sets the header's epoch to the revision of its claim transaction
(`TxnRev`); the first attach takes that + 1.
- Every attach is one header write, so a volume's epoch never passes the revision of its last
  header write, and every earlier volume of the name ended below the claim. Its fences, fence-only
  records (S11) and capability tokens are stale against the new volume: a new target's `Hello`
  raises a stale fence, and an old token gets `Stale` once the new volume has fenced the node.
- This holds for names deleted before this change, and for damaged headers (docs/issues/8,
  S4c). etcd's revision only grows, compaction included; a restore from a snapshot must keep it
  growing (`etcdctl snapshot restore --bump-revision`). The in-memory store's revision grows
  within its process, which holds all its data.

**S7. The contract.**
- **Proto** (`api/proto/flaminio/meta/v1/meta.proto`):
  - `ChunkMeta.released` (`repeated string`, 6): the drain, `get` and the data plane see releases;
  - `rpc ListReleases(ListReleasesRequest{node}) returns (ListReleasesResponse{repeated
    VolumeReleases volumes})`, with `VolumeReleases{volume, repeated ChunkRelease chunks}` and
    `ChunkRelease{chunk, revision}`;
  - `rpc ConfirmRelease(ConfirmReleaseRequest{node, volume, repeated ChunkRelease chunks})
    returns (ConfirmReleaseResponse{})`.
- **etcd** (the key comment at the top of `service.go`): `chunks/<id>/<index>` gains `released`,
  and `volumes/<id>` gains `fenced` (JSON, omitted when empty). No new key.
- **Callers** (`Authorize`), with a new component, `sweep` (`.../node/<node>/sweep`), the agent's
  alone. The CSI node plugin keeps the `agent` certificate, and so loses the sweep's calls:
  - `ListReleases`, `ConfirmRelease`: the node's `sweep` only. Administrators see releases in
    `get`;
  - `ListTombstones`, `ConfirmDeletion`: the node's `sweep` (no longer `agent`), or an
    administrator, through issue #14's command, for a node gone for good;
  - the data plane reads releases with `GetVolume`, open to every caller already: no new case;
  - anyone else, and any method not named: `PermissionDenied`, as now.
  - Every node holds the capability token key (`cmd_pki issue`, `token.key`), so root on any
    node can sign capability tokens. As before; this change adds no token.
- **Old records:** without `released` or `fenced`, none. Old clients ignore the new fields. An
  agent facing an old service (`Unimplemented`) deletes nothing.
- **The reference model and vectors:** `flaminio-repl/src/meta.rs` and the Go domain gain
  `released`, S1, S2 and S3's rule, in step. `examples/meta_vectors.rs` writes `released`, has a
  `confirm_release` op (so releases do not pile up in random runs), and `create` carries its
  starting epoch (today 0, `meta.rs`). The Go test's `vectorChunk` reads `released`
  (`json.Unmarshal` ignores unknown fields, `domain_test.go`). `isr-vectors.jsonl` is regenerated
  (`make contracts`).

### The node agent

**S8. A long-running agent.** `flaminio-agent sweep` runs a sweep every 5 s (the CSI tombstone
sweep's interval) until stopped, as the node's `sweep` identity, with the node's control socket.
- **One per node:** it holds an exclusive lock (`flock`, `/run/flaminio/agent.lock`) while it
  runs; a second one exits.
- **The lab:** the systemd unit `flaminio-agent` on every data node, fl-c included, installed and
  started by `lab.sh agent up [VM]` (`make agent-up`); `cmd_pki issue` gives each data node a
  `sweep` certificate. `ensure_nodes` starts it as it starts the nodes.
- `disks` and `claim` are unchanged.

A sweep, one step at a time:
1. **Tombstones** (`ListTombstones`): as the CSI sweep does today, `delete VOLUME`, then
   `ConfirmDeletion`.
2. **Releases** (`ListReleases`): the agent notes when it first lists each `(volume, chunk)`, by
   its monotonic clock, and forgets those a sweep does not list. Those listed for the grace are
   deleted, per volume, at most 119 per call (`delete-chunks`, S9); then those deleted, or absent,
   are confirmed at their listed revisions (S3).

A sweep reads two lists from the service, and nothing else. The agent keeps only the first
listings, in memory; after a restart they start again. Without the service, it deletes nothing.

### The data plane

**S9. Two control socket commands.**
- **`list`:** every copy in the store, by volume hash and chunk index, and every fence-only
  record (S11). It changes nothing. The hashes are of names (intent, item 5), so a name can be
  confirmed by guessing; the socket is root's only (mode 0600, `control.rs`).
- **`delete-chunks VOLUME CHUNK...`,** at most 119 chunks. The data plane reads the volume
  (`GetVolume`, as `io`, through the metaclient's thread, within the command's deadline). For each
  chunk:
  - **refused** unless its record lists this node in `released`; and while a target on this node
    has a view that places it here. It stays, and the answer names it;
  - **otherwise deleted** from the store at epoch `u64::MAX`, as `delete_volume` does, after local
    I/O in flight; one chunk per engine turn, so replication I/O goes on. No fence refuses it; the
    deletion fences nothing. That is all "a fencing epoch higher than any target's" means here;
  - **forgotten** by the replica core, which keeps the volume's fence (`remove_chunk`). A later
    move to the node creates it afresh, empty;
  - **absent:** counts as deleted.

  It may run while the volume is attached on the node, this node its target included. The
  answer: the chunks deleted and those refused. A volume `NotFound`, or no answer in time: nothing
  deleted.
- **Why the record is enough:** while a chunk releases the node, no move brings it back (S2),
  and only this node's agent, one at a time (S8), confirms, after the delete answers. So between
  the read and the delete, nothing can make the chunk this node's again. A caller that skips the
  grace (a root process with the socket) can only fail the stale target's session early (S12),
  not lose data.
- **Deadline:** a delete carries the socket's limit as its deadline, as an attach does
  (docs/issues/6). Not begun by then, it never begins; the socket's timeout answer says so. A
  timed-out delete never runs later. `delete VOLUME` gets the same deadline, and is otherwise
  unchanged.

**S10. A `Hello` at the fence creates nothing.** A `Hello` creates the chunks it names that the
node lacks only when it raises the volume's durable fence (the store's, or a fence-only record's);
at the fence, it creates none, and the chunks it names that the node lacks are not held. Chunks
are otherwise created only by a `BeginResync`.
- **The core's fence follows the store's:** the core raises its fence before the store writes
  (`ReplicaCore::handle`); if those writes fail (out of space), the core's fence goes back to the
  durable one and the `Hello` is refused.
- **Today's paths keep working:** every target start takes a new epoch (`Attach`;
  protocol §3), so a target's first `Hello` to a node raises the fence and creates. A reconnect
  in the same epoch, and one after the node restarts, finds its chunks: they and the fence are
  durable (S11).
- **A crash in a `Hello`** that raises the fence: its creations and its fence commit in one
  journal batch, so a reconnect finds all of them, or a fence it raises again.
- **A stale view at its epoch** (intent, item 4): the deleted chunk is not created again. The
  target's requests for it are dropped, its session fails, and it learns of the drop. A read
  goes to another ISR member; none returns zeros.

**S11. The fence outlives the last chunk.** Deleting a node's last chunk of a volume leaves a
fence-only record for it, at its fence.
- **Why, with S6:** S6 covers earlier volumes of the name. Within one volume, a node that deleted
  its last chunk and restarted would accept a target of an older epoch, unaware it was deposed:
  its `Hello` would raise the fence from 0 and create the chunks its view places there.
- **Where:** a chunk slot (format §5.3) in a new `state`, 4 = fence only, with `volume_uuid` and
  `fenced_epoch`; no chunk or volume size, and `layer_count` 0. Recovery reads it as the volume's
  fence.
- **Shape:** a `Hello` checks the volume's size and chunk size against the node's only when the
  node holds a chunk of it (`ReplicaService::hello`); a fence-only record has none.
- **Journal:** a new record, `FenceOnly` (type 9: `slot`, `volume_uuid`, `epoch`), not optional
  (§6.3), in the same batch as the last `ChunkDelete`, so a crash leaves both or neither. `Fence`
  (type 3) raises a fence-only slot of the volume too. A `ChunkCreate` of the volume may take a
  fence-only slot, at its fence or higher: that supersedes it.
- **Old binaries refuse the store:** incompatible feature bit 2 (value 4; bit 1 is reserved for
  authenticated checksums, §3), set before the first `FenceOnly` commits. Format 1.0's document
  gains the state, the record, the bit, and the rules above.
- **Bound:** one slot per volume and node, until the volume's deletion. The metadata service knows
  the node (the header's `fenced`, S3), so the tombstone names it (S5) and `delete VOLUME` removes
  the record with the copies. A node whose last release confirms without its last chunk (it still
  holds another, placed) is in `fenced` too: it deletes nothing extra.
- **`flaminio-disk inspect --slots`** shows fence-only slots, counted apart from chunk slots in
  use, and checks them as it checks chunk headers.

### Sweepers, rollout, failure, logs

**S12. One sweeper per node.** Only the node's agent confirms a release (S7), one process per
node (S8). A tombstone's confirmation frees the id; the agent confirms one only between steps,
never during a release deletion of that volume. Others that could confirm one:
- **The CSI node plugin:** its sweep goes, in this change (`SweepTombstones`, started in
  `control/cmd/flaminio-csi`). An old pod still running gets `PermissionDenied` on
  `ListTombstones` (S7), so deletes and confirms nothing.
- **An administrator,** through issue #14's command, for a node gone for good.

**S13. Issue #14's harness.** Issue #14 lands first. This change, in `lab.sh`, `docs/lab.md` and
follow-ups §14:
- removes `vol-confirm` and `scripts/guest/confirm.sh`, and `delete_volume`'s stop for a
  tombstone pending on fl-c: the agent confirms there now;
- moves the harness's `tombstones` calls and the `confirm` command's node caller from `agent` to
  `sweep`;
- changes `bench-writes`' and `vol delete`'s prerequisites from the CSI node plugins on fl-a and
  fl-b to the agents on every data node (`make agent-up`); a pending tombstone that outlasts
  `TOMBSTONE_WAIT` names `make agent-up` as the cause.

**S14. Rollout.** In order:
1. `make meta-up`, every instance. Until all run the new code, an old instance ignores S6 and
   `released`: one that rewrites a chunk record (`ChangeIsr`, `MoveReplica`, with the old
   `domain.Chunk`) drops `released`, and those copies become unnamed (intent, out of scope).
2. The data plane (`make deploy`, the nodes restarted).
3. The `sweep` certificates, the CSI driver without its sweep (`make csi-deploy`), and at once the
   agents (`make agent-up`). Between the last two nobody sweeps: no `bench-writes` or `vol
   delete` then, or its `TOMBSTONE_WAIT` fails.

The agents never run while any metadata instance is old: stop them (`lab.sh agent down`) before
rolling the metadata service back.

**S15. Failure and retry.**
- **A deletion refused, failed or timed out:** not confirmed; retried in a later sweep.
- **A confirmation that fails** (S3's `Aborted`, the network): the next sweep lists again,
  deletes (nothing there) and confirms at the new revisions.
- **The data plane not running:** nothing deleted, nothing confirmed.
- **A node that never comes back:** its releases stay. They keep the chunk from moving to it
  only, and show in the drain's progress. A deletion of the volume names it on the tombstone,
  so the id stays taken until an administrator confirms that tombstone for it (issue #14).

**S16. Logged.** The agent, one line each: per volume per sweep, the chunks deleted and those
refused; each failure, as a warning; an `Unimplemented` service, once per sweep. The data plane:
each `delete-chunks`, with its counts. The service: nothing new.

### Ordering

**S17. Against what else may happen.**
- **A moving chunk:** its source and destination are not released; the drop releases the
  source as the move ends (S1).
- **The target that proposed the drop, before it learns of it:** the grace (S8) outlasts its
  learning and its earlier requests. A remote target that reconnects anyway does not recreate
  the chunk (S10); a local one is refused by the view rule (S9).
- **A move back:** refused while released (S2). After the confirmation the copy is gone, and
  the move's `BeginResync` creates it afresh.
- **An attach:** a new target reads the records after its epoch; its `Hello` names only placed
  chunks.
- **A target of an older epoch:** the volume's fence refuses it, after a restart too (S11).
- **The volume deleted:** its releases and fenced nodes go to its tombstone (S5). A confirmation
  or a `delete-chunks` then gets `NotFound`; the tombstone step deletes the copies.
- **A new volume of the same name:** the id is taken while a tombstone names a released or fenced
  node (S5), and only the node's agent confirms it, between steps (S12). Its epochs are above
  every earlier one (S6).
- **A metadata instance of the old version:** a rewrite that drops `released` makes the data
  plane refuse the deletion (S9): the copy stays, unnamed, and no live chunk is lost.
- **A node down during the move:** the drop does not need it. The release waits, the chunk does
  not move back to it meanwhile, and its sweeps delete the copy once it is back.
- **The drain:** a release committed after its read makes `MoveReplica` answer `Applied: false`
  (S2); the chunk waits for a later step. A node the chunk releases is not a candidate for it;
  with no other, the chunk waits (a chunk with no candidate for another reason still fails the
  step, as now).

**S18. Unchanged.**
- The replication protocol and the target (`drive_move`, resync).
- `ChangeIsr`'s and `MoveReplica`'s rules, but for S1 and S2. `DeleteVolume`'s two steps.
- The drain's end: no replica placed on the node. Its progress adds the releases held for it.
- The simulator, but for one line: as it deletes a move's source at the drop, it confirms its
  release, so moves back work as before.

**S19. Docs.** `docs/lab.md`: the agent's unit, S13, the rollout (S14), and the hand cleanup
(Tests, lab). The format document (S11). Follow-ups §13: done, with the lab check's result; as
follow-ups, unnamed copies (their deletion, a hold on their hash), forgetting a node, an index of
releases, a token for `delete VOLUME`. Follow-ups §14: step 1 done.

## Tests

`meta/service/service_test.go`, against the in-memory store and against etcd:
- **A drop releases the source,** in the drop's put: a move carried out by `ChangeIsr` steps
  leaves the source in `released`. An add, an ISR shrink or an ISR join leaves none.
- **64 drops in one `ChangeIsr`,** against a real etcd (the in-memory store does not enforce the
  limit): applied, with 64 releases.
- **No move back:** `MoveReplica` of the chunk to the released node answers `Applied: false`, and
  `true` after the confirmation. Of another chunk to that node, `true`.
- **Confirmation:** at a stale revision (`interleaveStores`, an ISR change between the listing and
  the confirmation) it fails, and the release stays. A late confirmation after a move back and a
  new drop leaves the new release. 120 chunks: `InvalidArgument`. The first adds the node to
  `fenced`; a second does not write the header.
- **Listing:** by node, with revisions; at most 1024 chunks. Left out: a volume being created,
  one being deleted, and chunk records without a header.
- **Deletion with a release held:** the tombstone names the released and fenced nodes, and a
  creation of the id is refused until they confirm. A confirmation after the mark gets `NotFound`.
- **The starting epoch:** delete a volume attached 7 times, create it again: its epoch is its
  claim's revision, above 7. Again after a damaged header's deletion, and for a name deleted with
  no starting epoch (written as before this change).
- **A move back under a mixed version:** a chunk record rewritten by the old `domain.Chunk` loses
  `released`; the move back is accepted; the node's `delete-chunks` of it is refused.

`meta/domain`: the Go rules against the regenerated vectors (S7).

`meta/service/auth_test.go`: a node's `sweep` lists and confirms releases and tombstones for its
node only; `agent` may no longer list or confirm tombstones; administrators and `io` may not call
the release methods.

`control/drain/drain_test.go`: a node with a release of a chunk is not that chunk's destination;
with no other, the chunk waits, and the step does not fail. A release committed after the
step's read: `Applied: false`, the step goes on, and the next step moves it elsewhere. Progress
counts the releases held.

The agent (`control/agent`, the package behind `control/cmd/flaminio-agent`), against an
in-memory service on a localhost gRPC server and a fake control socket:
- a release is deleted only after the grace; one not listed by a sweep is forgotten, and its
  grace starts again;
- a deletion refused, failed or timed out is not confirmed, and is retried;
- tombstones are deleted then confirmed, in the same loop, never during a release step;
- **two agents on one node:** the second exits; one sweeps;
- **with an old CSI sweep** against the same service, as `agent`: it gets `PermissionDenied`,
  and deletes and confirms nothing;
- with the service unreachable or `Unimplemented`, nothing is deleted.

`flaminio-node` (`replica/tests.rs`, `tests/engine.rs`), `flaminio-store`, `flaminio-format` and
`flaminio-io` (control socket):
- a chunk's deletion leaves the volume's other chunks and its fence;
- **the last chunk:** deleted, then a restart: a `Hello` below the fence gets `Stale`; one above,
  with another volume shape, is accepted;
- **crash atomicity:** a crash at every point of the last chunk's deletion recovers both the
  `ChunkDelete` and the `FenceOnly`, or neither; the same for a `Hello`'s creations and fence;
- **an old binary** refuses a store with bit 2; bit 1 stays unknown (`superblock.rs`'s test);
- **a `Hello` at the fence** names a chunk the node lacks: not created; one above creates it. A
  fresh attach over a placed chunk the node lacks creates it;
- **a `Hello` whose store writes fail:** refused, and the core's fence is back at the durable one;
- **the stale view:** drop committed, answer lost, the grace passed, the session blips, the copy
  deleted, the target reconnects at its epoch with its stale view: the chunk is not created, the
  target's reads of it go to another ISR member, the client gets no error;
- a `BeginResync` of a deleted chunk creates it empty;
- **`delete-chunks` refuses** a chunk whose record does not list this node in `released`, and one
  a local target's view places here; over 119 chunks, a volume `NotFound`, and no metadata answer
  in time: nothing deleted;
- **the deadline:** a delete held behind a busy engine past its deadline never runs; the socket
  says so;
- **after a new creation:** a node with a fence-only record of a deleted volume of the name; the
  volume created again: its first attach fences, creates its chunks and serves; a capability
  token of the old volume gets `Stale`;
- `list`: every copy and fence-only record.

The simulator gets no new check: its `Hello` creates no chunk, so S10's window is the data
plane's; the node tests above cover it.

**The lab, a later step,** after the rollout (S14). Each wait below polls every 5 s, for at most
60 s:
- **The hand cleanup, once,** recorded in the PR: `list` on every node, against the records. A
  volume with no chunk placed on the node, or deleted: `node.sh delete`. A volume with a chunk
  placed there and an unnamed copy: deleted and created again. Afterwards `list` shows only named
  copies.
- **Drain** (`make drain-test`): `drain fl-b` into fl-c under the verified write load, fio
  verified as before. Then fl-b's `list` comes to hold no copy of `vol1`, and `get vol1` no
  release. The same for fl-c after the drain back.
- **Item 3's check again:** an RF 1, 16 GiB volume with 64 MiB written per chunk, its target on
  fl-a, moved chunk by chunk from fl-a to fl-b. Then `list` on fl-a shows no copy of it and one
  fence-only record; `flaminio-disk inspect` (the node stopped) shows as many chunk slots in use,
  and clusters, as before the volume was created, and one fence-only slot.
- **Node down:** fl-b's node stopped, its chunks of `vol1` moved to fl-c; fl-b's node and agent
  started: then no copy of `vol1` on fl-b.
- **Move back:** with fl-b's agent stopped, a chunk moved off fl-b and back: `Applied: false`;
  the agent started: then the move is accepted, and fio verifies the volume.
- **Delete and create again:** with fl-b's agent stopped and a release held for it, delete the
  volume: the tombstone names fl-b, and `create` of the id is refused until fl-b's agent runs.
  The new volume attaches above every old epoch and reads zeros wherever the old one held data.
  Again after a release was confirmed: the same.
- **No second sweeper:** no CSI node pod sweeps; a second `flaminio-agent sweep` on a node exits.
- **Issue #14's checks** pass with the agents (S13): `bench-writes` and `vol delete` with `RF=2`,
  fl-c included, stop for no tombstone.
