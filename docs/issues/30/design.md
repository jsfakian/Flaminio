# Issue #30: a volume created again starts above every earlier epoch — design

## The first epoch (S1)

**The header** (`meta/domain/domain.go`): `Header` gains `FirstEpoch uint64
json:"first_epoch,omitempty"`. A header without it reads as 0 (S3).

**The domain:** `(h *Header) Start(first uint64)` sets `Epoch` and `FirstEpoch` to `first`.
`NewVolume` is unchanged: the claim's revision is known only after the claim.

**The service** (`createVolume`, `meta/service/service.go`):
- **The claim** is unchanged: `TxnRev` writes the header marked `creating`, and returns its
  revision, `claim`.
- **The last write,** which clears `creating`, calls `v.Start(uint64(claim))` first. So the
  header that becomes visible has its first epoch, and `CreateVolume` returns it as its epoch.
- **No other write** sets either field. `Attach` keeps `h.Epoch++`; `DeleteVolume`'s mark keeps
  them as read.

**Why every earlier volume ended below the claim.** The invariant: a header's epoch is at most
the revision of the write that stored it.
- **At creation:** the epoch is `claim`, stored by a later write.
- **At an attach:** the write compares the header's revision `r`, and stores `epoch + 1` at a
  revision above `r`. Since `epoch ≤ r`, `epoch + 1 ≤ r + 1`, which is at most the new revision.
- **Other header writes** (the deletion's mark) keep the epoch, at a higher revision.
- **Volumes created before this change** started at 0, so it holds for them too.
- **The earlier volume's last header write** came before its deletion's transaction: that
  transaction compares the header's revision. The claim compares the header's absence and the
  tombstone's, so it comes after the deletion, at a higher revision. So every epoch the earlier
  volume reached is below `claim`.
- **Fences and tokens:** every token is signed with an epoch an attach stored (`Attach`, after
  its write). A target fences at its token's epoch. So both are below `claim`. Root on a node
  can sign other tokens (S8).

**Not read:** the deleted header, the tombstone, any history. The proof needs only revisions,
which is why names deleted before the change and damaged headers are covered (S3).

## The revision (S2)

**The interface** (`meta/kv/kv.go`): `KV`'s comment states the requirement: revisions only
grow, across restarts too. A future store (ADR 0002) must keep it.

**etcd** (`kv.Etcd`): nothing changes. `TxnRev` returns the transaction's header revision.
Compaction removes history, not the counter. A restore from a snapshot needs
`--bump-revision` (Documents).

**The in-memory store** (`kv.NewMem`): `rev` starts at `now().UnixMicro()` (0 if negative).
- **Why:** a restarted service loses every record, but nodes keep replicas and fences. Starting
  at the clock puts the next service's claims above the last one's, unless it made more than one
  revision per microsecond on average, or the clock went back. Both are accepted for a store
  meant for development and tests.
- **Tests with a fake clock** start at its time. No test compares a revision with a constant
  that could collide (`kv_test.go` uses `a.Rev + 100` and `1 << 40` as revisions that must not
  match; the clock in microseconds is about 2^50).
- **Range:** about 1.8 × 10^15 now, below 2^53, so `jq` in the lab scripts reads epochs exactly.

**The flag's help** (`flaminio-meta serve --store`): "memory (development and tests: nothing is
kept; revisions start at the clock)".

## Existing volumes (S3)

- **A header without `first_epoch`** keeps its epoch. `Attach` signs today's token (S4).
- **A name deleted before the change:** S1's proof holds; its claim is above the old volume's
  last write. Its leftover replicas are fenced below the new first epoch, so S5 deletes them.
- **A damaged header:** `DeleteVolume` deletes it without parsing it, as today (docs/issues/8,
  S4c). The next claim is above every write it got.
- **No migration step.** Nothing is rewritten.

## The token (S4)

**Go** (`pkg/captoken`):
- `Claims` gains `FirstEpoch uint64`; `MaxLen` grows by 8.
- **`Sign`** writes version 1 when it is 0, unchanged. Otherwise version 2: version 1's fields,
  then `u64 first epoch`, before the signature. It refuses a first epoch not below the epoch.
- **`Verify`** accepts both. Version 1 gives 0. Version 2 refuses a first epoch of 0 or not
  below the epoch.
- The package comment documents version 2.

**The service's `Attach`** signs `FirstEpoch: h.FirstEpoch`.

**Rust** (`flaminio-net/src/token.rs`):
- `Claims` gains `first_epoch: u64`. `MAX_LEN` grows by 8.
- **`verify`** checks the signature, then parses with Go's rules.
- **`decode_unverified(tok)`** parses without the signature. It is behind a crate feature,
  `unverified`, that only `flaminio-node` enables, for the local session (S6).

**`session::accept`** is unchanged: it compares volume, epoch and node. The first epoch rides in
`Accepted.claims`.

## Dropping an earlier volume's chunks (S5)

**`Store::delete_volume(uuid, below)`** (new, `flaminio-store`), shared with #13, which adds
the volume's fence-only slot to it. It deletes, in one plan, every chunk of `uuid` whose fence
is below `below` (`u64::MAX` for the tombstone's `delete VOLUME`), and returns how many.
- **One commit:** the records are `ChunkDelete`s, 8 bytes each plus the record header. A plan
  larger than one batch (`max_batch_bytes`, 4 MiB: over 100 000 chunks) is split in batches of
  whole chunks.
- **`ReplicaService::delete_volume`** (`delete VOLUME`) uses it too, instead of one commit per
  chunk.

**`ReplicaService::hello`** (`flaminio-node/src/replica/mod.rs`), right after the chunk-range
checks and the `uuid`, before the geometry check, when the token's `first` is above 0:
1. **The store decides:** `store.delete_volume(uuid, first)`. Not `vols` or the core, which a
   failed drop could leave out of step with the store.
2. **Only if it succeeds,** and the core's fence for the volume is below `first` (it was
   fenced only by chunks now deleted, or by chunks moved away), the volume is forgotten: removed
   from `vols` and from the core (fence, sessions, filters).
3. **On an error,** the `Hello` fails, and nothing in memory has changed. The store holds only
   chunks fenced below `first` or none, so the next `Hello` drops the rest.

Then the geometry check finds no volume, so a new size or chunk size passes. Then as today:
`vol_id` gives a new `VolumeId`, the core's `Hello` sets the fence, the placed chunks are
created at the epoch, and the fence is durable before the reply.

**The engine settles first.** `NetEvent::Hello` and the local `Hello` call `settle_local`, not
only `flush_local`, when `ReplicaService::would_drop(&accepted)` (new: the same test, without
the deletion) is true. That is what `DeleteReplicas` does before `delete VOLUME`.
- **Why:** local commits in flight or staged for the old chunks land, and get their replies,
  before the chunks go.
- **A zombie target:** `DeleteReplicas` refuses while the volume is attached here. The drop
  cannot: this node may still run a target of the earlier volume. That target is of an older
  epoch, so a newer one deposes it anyway. After the drop its local requests get "unknown
  volume", as a remote replica's would, and its I/O fails.

**Why the condition is right.**
- **The `Hello` passes the core's fence check:** what is left is fenced at or above `first`,
  below the token's epoch (S4).
- **Nothing of the current volume is fenced below `first`.** Chunks are created at a session's
  epoch (`hello`, `create_moved_chunk`), and fenced at one. Every session of the volume is at
  one of its epochs, above `first`.
- **Everything of an earlier volume is below `first`** (S1).
- **A forged `first`** needs the token key (S8). A session with a valid current token already
  controls the replica's data.

**Crashes.** The drop's batches commit before the first `create_chunk`. The journal replays in
order and a crash loses a suffix. So after a crash, the store holds chunks fenced below `first`
(dropped by the next `Hello`), or new chunks fenced above it, never a mix that the drop misses.

**The cost:** one scan of the store's chunk slots and one commit, once per node and volume
created again. A `Hello` without a drop costs a comparison.

## The target's own replica (S6)

**`Engine::start_target`** (`flaminio-node/src/engine.rs`), after the geometry checks:
`decode_unverified`, then it refuses the attach unless `claims.volume` is the volume,
`claims.node` is `cfg.node` and `claims.epoch` is `v.epoch`: "the token is for X epoch E on N,
the volume is at E′". It keeps `claims.first_epoch` in `TargetRt` (new field). `send_from_target`'s
local `Accepted` takes its claims from there.

**An empty token is refused.** A node always loads the token key (`node.rs`), and a remote
replica refuses a session without a token. Only a local-only volume under `flaminio-meta
--insecure` attached without one. The flag's help says the data plane needs tokens.
- **The callers pass the token of the attach they just made:** the CSI node plugin
  (`only_if_target`, which returns one) and `lab.sh vol attach`. The controller's
  `unless_target` attach never reaches the data plane.

**Why decode the token, not ask the metadata service:** the first epoch is in no proto message,
and the token holds it, signed. #15 replaces `decode_unverified` with `verify` (its S12).

## Mixed versions (S9)

- **An old node** refuses version 2 at `accept` (the version byte). The new volume's target
  fails that session, and the chunk's ISR shrinks as for an unreachable replica.
- **So nodes are upgraded before the metadata service** (the upgrade notes say so). Volumes
  created before the metadata service's upgrade keep version 1 tokens.
- **An old metadata service** rewriting a header at attach drops `first_epoch`: Go's
  `json.Unmarshal` ignores unknown fields. The epoch is kept, so S1 holds for later volumes.

## The contract (S10)

**The reference model** (`flaminio-repl/src/meta.rs`): `create_volume(vol, first, min_isr,
blocks_per_chunk, placement)` sets `epoch: first`. `VolumeMeta` is unchanged. Callers (the
simulator, tests) pass 0.

**The vectors** (`examples/meta_vectors.rs`):
- **`create`** writes `"first_epoch"`: 0 for one seed in four (a volume from before the
  change), otherwise `r.range(1, 1 << 52)`.
- **The stale epochs** drawn for `ChangeIsr` then include epochs below the first one.
- `meta/testdata/isr-vectors.jsonl` is regenerated (`make contracts` checks it).

**The Go replay** (`TestVectors`, `domain_test.go`): `create` reads `first_epoch` and calls
`Start` after `NewVolume`.

**Token vectors:** a version 2 token signed with the fixed test key, in `captoken_test.go` and
in `token.rs`'s `go_compatible_vectors`, with the same bytes.

## Tests

**S1** (`meta/service/service_test.go`):
- create: `Epoch == FirstEpoch > 0`; the first attach gives `FirstEpoch + 1`;
- delete after 7 attaches, create again: the new first epoch is above the old epoch;
- the same after a damaged header is deleted;
- a name deleted before the change (a header written without `first_epoch`, at epoch 7, then
  deleted): created again, above 7;
- an interrupted creation (claim only) deleted, then created: above its claim.

**Rewritten to take the epoch from the attach** (they assert absolute epochs):
- `service_test.go`: the attach sequence (epochs 0 to 4), the ChangeIsr at epoch 1 (`:546`), the
  move test (`:944`), and "epoch equals the number of attaches";
- `auth_test.go` (2 and 3);
- `flaminio-metaclient/tests/interop.rs` (epoch 1 after the first attach, and its ChangeIsr at
  epoch 1), which `make contracts` runs through `meta-interop`;
- the lab's `meta-smoke` (`== 2`): the first attach's epoch + 1.

**S2** (`meta/kv/kv_test.go`): a `Mem` built with a clock at T starts above T in microseconds; a
second one with a later clock starts above the first one's last revision.

**S3:** the legacy header above attaches with `FirstEpoch` 0, its epoch + 1, and a version 1
token.

**S4:**
- Go: `Sign` with 0 gives today's bytes (the existing vector); version 2 round-trips; `Sign`
  refuses a first epoch equal to or above the epoch; `Verify` refuses version 2 with a first
  epoch of 0, equal or above; the longest version 2 token passes `MaxLen`. The service's `Attach`
  returns a token carrying the header's first epoch.
- Rust: the shared vector; the same refusals; `decode_unverified` of a token with a bad signature
  succeeds.
- **The fuzz target** `capability_token` also builds version 2 bodies, and runs
  `decode_unverified` on its input.

**S5** (`flaminio-node/src/replica/tests.rs`, on a `MemDevice` store):
- leftover chunks 0 and 3 fenced at 9, data written; a `Hello` with first 20, epoch 21, placing
  chunks 0 and 1: chunk 3 is gone, chunks 0 and 1 read zeros, the fence is 21;
- leftover fenced at 30: a `Hello` with first 0 at epoch 21 gets `Stale`, as today; with first
  40, epoch 41: accepted, zeros;
- leftover with another chunk size: accepted after the drop;
- a live volume (first 20, fenced at 21, data written), the store reopened: a `Hello` at 22
  deletes nothing, and the data reads back;
- first 0 (an existing volume): nothing deleted;
- an old session's write after the drop: "unknown volume"; an old token's `Hello` (epoch 9):
  `Stale`;
- **an error mid-drop:** a device that fails the drop's commit (`DeferredDevice` completing it
  with an error): the `Hello` fails, `vols` and the core are unchanged, and after the store is
  reopened a `Hello` drops the chunks;
- a crash between two batches (a store with a small `max_batch_bytes`, reopened after the first):
  the next `Hello` drops the rest;
- a drop with an old session's commit in flight, and with one staged (`deferred_service`, hold
  on): the writes land and are answered, then the chunks go.

**S6** (`flaminio-node/tests/engine.rs`, whose model signs tokens):
- the model creates `v` at first epoch 0, a target writes a pattern on fl-a and fl-b and
  detaches; the model deletes `v` and creates it again at first epoch 1000 (nothing deletes the
  replicas). The new target attaches on fl-a: it reads zeros, writes and reads back, and is not
  deposed;
- an attach with the old token, another volume's, another node's, or none: refused;
- an old token's session to fl-b, which the new volume has fenced: `Stale`;
- a drop while fl-b still runs a target of the earlier volume: its local I/O fails, nothing
  panics.

**S9:** no automated test: it needs the old binaries. Today's verifiers refuse any version but
1 (the version byte).

**S10:** `make contracts`; `go test ./meta/... ./pkg/...`; `cargo test -p flaminio-repl`.

**The lab** (spec, Checks), by hand, recorded in the PR:
1. `make vol-create`, `make vol-attach VM=a`; fio writes a verify pattern on 1 GiB; then
   `lab.sh vol detach a`.
2. Keep the attach's token (`metactl attach` prints it). `make vol-delete`, then remove the
   tombstone with `etcdctl del` on an etcd VM (a stand-in for a node the tombstone does not
   name).
3. With another `VOL_SIZE`, `make vol-create vol-attach VM=a`: the epoch is above the old one.
4. fio reads zeros over the first GiB, then writes and verifies; `vol show`: no degraded chunk.
5. `make vol-attach VM=b` (fl-b reads its own replica): fio reads back the new pattern, not the
   old one.
6. `lab.sh vol detach b`, then `node.sh attach` on fl-a with `TOKEN` set to the old token:
   refused, naming both epochs.

## Documents

- **ARCHITECTURE §8.4:** a volume's epochs start at its first epoch, the revision of its claim;
  the token carries it; a replica fenced below it drops the volume's chunks.
- **ARCHITECTURE §5.4 and §14.3 (DR runbooks):** restore etcd with `etcdutl snapshot restore
  --bump-revision 1000000000 --mark-compacted`. The bump must exceed the revisions made since the
  snapshot; 10^9 does for any plausible gap, and keeps epochs far below 2^53. A wiped etcd, with
  nodes keeping their data, is the same hazard: nodes must be wiped too.
- **ARCHITECTURE §18 (upgrades):** nodes before the metadata service (S9).
- **docs/replication-protocol.md,** at `Hello`: S5's rule and why it keeps the volume's own
  chunks.
- **`service.go`'s key comment:** the header's `first_epoch`.
- **`flaminio-meta --insecure`'s help:** no tokens, so the data plane refuses its volumes.
- **`lab.sh`'s `delete_wbench` comment** ("epoch 1 again … deposed"): a new wbench now starts
  above the old one; the replicas are deleted to free their space.
- **The issue** is closed by the PR, with a comment on S8.
