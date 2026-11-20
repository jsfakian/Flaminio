# Issue #51: snapshot contracts (settled with the plan, 2026-11-13; changed after approval)

**Pending the coordinated byte review (round 1, R1-C5).** #58's settled decision fixes the bytes
#51, #52 and #58 share "in one coordinated review with #51 and #52 before implementation"
(`docs/issues/58/design.md`, in #58's plan). Until that review, the bytes below that #52 and #58
read are proposed, not final: the `Snapshot` message and the catalog record, the store views and
`changed_ranges`, the Wire-v2 view requests and replies (`ReadView`, `DigestView`, `MappedView`,
`Changed`, `LayerState`, `Layers`, `Changes`, the view encoding) and the completeness evidence
(last section). The review is deferred, not settled here; none of those bytes is built before it.
The rest of this file is settled with the plan and changed after approval as decisions.md lists.

## Metadata and lifecycle

Under `/flaminio/v1/`: header optional JSON `snapshot_generation`, `snapshot_next_ordinal`,
`snapshot_count`, `snapshot_charge_next` (a chunk index, present only while a request's node
charge is in progress, R2-C1; a `*uint32` with `omitempty`, so a cursor of 0 is present and only
nil is absent, R3-C1); record `snapshots/<volume>/<first_epoch>/<name>` v1 fields
operation_id (16 random bytes encoded lowercase32hex), ordinal, state, requested_unix_ms,
generation, commit_epoch. IDs use existing1…128 metadata identifier syntax. No deletion API.
`<first_epoch>` is the volume's first epoch in 20 decimal digits, zero-padded, so one
incarnation's prefix is never a prefix of another's.

**One catalog per header revision (R1-C9).** While the header exists, every catalog write is in a
transaction that also writes the header: a request's mark (count, next ordinal), its last charge
batch (the record `PENDING`; every batch writes `snapshot_charge_next`) and a cut (generation);
deletion clears the catalog only once the header is gone (Deletion, below). So a request moves the
header's revision, as a growth step does: a `ChangeIsr` or `MoveReplica` racing it fails its header
compare and retries; #77's pinned Attach whose second header read sees it answers #77's `Aborted`
(committed-superseded), and its caller attaches again. A volume view lists
`snapshots/<volume>/<first_epoch>/` inside #77's header-to-header window (after the chunk prefix,
before the final header read), so the catalog it returns is that header's. Every read lists the
current incarnation's prefix only, never `snapshots/<volume>/`.

Proto append-only Volume fields: uint64 snapshot_generation=30; repeated Snapshot snapshots=31.
`Snapshot {string name=1; bytes operation_id=2; uint64 ordinal=3; enum State{PENDING=0;
 COMMITTED=1; CHARGING=2;} State state=4; uint64 generation=5; uint64 first_epoch=6;
 uint64 commit_epoch=7;}`.
**Size, for #80's bound (R1-C1):** a maximal `Snapshot` encodes in 195 bytes (`name` 131,
`operation_id` 18, the four `uint64` 11 each, `state` 2); an entry of field 31 in 199 (a 2-byte
tag, a 2-byte length); 63 entries and field 30 (12) in 12,549 bytes.
GetVolumeRequest string accepted_snapshot_profile=3. The gate: `GetVolume` answers
`FailedPrecondition` to an `io` caller whose node is the record's target, for a volume with
pending or committed snapshots, when `accepted_snapshot_profile` is not `"snapshot-v2"`; every
other caller (a replica-side `io` read, the agent, CSI, `flaminio-meta`) gets the record, as
#50's gate does.
Max63 entries, in ordinal order; operation_id exactly16 bytes. Unknown state never appears
as committed. Header/cut counters use checked arithmetic. New methods:
- RequestSnapshot: request(volume1,name2,expected_first_epoch3), response(Snapshot snapshot1).
  Caller: the roles table below. Idempotent current-incarnation name. Refused before any write:
  an invalid id or name (`InvalidArgument`); a volume unknown, being created or deleting
  (`NotFound`); `expected_first_epoch` not the volume's, 63 snapshots counted, a later state
  that forbids snapshots (#68: hydrating), and an encrypted volume: #50's `Volume.encryption`
  (field 20) with a profile other than `none`, or #50's mark in the header's `id`
  (`FailedPrecondition` `volume <id> is encrypted: no snapshot until its keys are pinned`, R1-C11,
  R1-S3; whichever of #50 and #51 lands second adds this refusal and its two test cases, R2-S3);
  a volume outside the snapshot-capable bound (design, Budget; `FailedPrecondition`); the
  retention charge (below): a node holding a named copy without a valid report or a readable
  ledger (`FailedPrecondition`), no room at #72's soft watermark or no quota
  (`ResourceExhausted`). A request these checks admit completes the volume's `CHARGING` snapshot
  first, if it has one (Retention charge), then makes its own mark; a refused request writes
  nothing, completion included. A request naming the `CHARGING` snapshot itself is checked as an
  answer only (id and name, the volume's lifecycle, `expected_first_epoch`), completes it and
  answers it.
- ListSnapshots: request(volume1), response(repeated Snapshot snapshots1). One of #73's open
  reads, as GetVolume. No clear keys or snapshot payload.
- CommitSnapshot: request(volume1,epoch2,expected_generation3,operation_id4,first_epoch5),
  response(bool applied1,Volume volume2,Snapshot snapshot3). `auth.go`: `allowed = id.Node != ""
  && id.Component == "io"`, as ChangeIsr. The method, as ChangeIsr (`service.go:794-800`): an
  `io` of a node that is not the volume's target, or at another epoch, generation or first epoch,
  gets `applied=false` and the current volume (epoch and generation), nothing written, which tells
  a deposed target it is deposed (R1-C8). Oldest Pending exact op required; a `CHARGING` record
  is not cut (`applied=false`).

**Retention charge (R1-S2, R1-C3; round 2: R2-C1 to R2-C3, R2-S1, R2-S2).** A volume with *k*
snapshots (pending ones counted) is charged (1 + *k*) times what #72 and #73 charge it without: a
layer holds at most a chunk size and one page table, so a copy holds at most (1 + *k*) chunk
sizes of data and (1 + *k*) slots' page tables (design, Roles, audit and charges). A request:
1. **checks** room before any write: one chunk size and one slot per named copy of #72's
   `VolumeCharges` (replicas, pending move destinations, #13's released copies; fence holders
   none) on each such node, by #72's `Room` at `SoftPPM`;
2. **marks**, in one transaction: the record `CHARGING`, the header's count and ordinal, and
   `snapshot_charge_next` = 0; for a tenant owner once #73 is active, the volume's size to
   `quota/<ns>/<class>.bytes` and the owner record's `bytes`, as #73's growth does (a system
   owner, nothing; an unassigned volume, nothing until #73's `AssignOwner` charges it, below);
3. **charges** the nodes in batches, chunks in index order from `snapshot_charge_next`: each
   batch adds one chunk size and one slot per named copy of its chunks, rechecking `Room` at
   `SoftPPM`, and advances `snapshot_charge_next`; the last removes it and writes the record
   `PENDING`. A batch without room answers `ResourceExhausted`, and a batch that finds a named
   copy's node without a valid report or its ledger not readable (#72's InitCapacity mark)
   answers `FailedPrecondition` naming the node (R3-C3); either keeps earlier batches, as #70's
   steps do, and the snapshot stays `CHARGING` until a later request of the volume completes it.

Every site that charges or releases a copy of chunk *j* uses **m(*j*)** = 1 + `snapshot_count`, less
one while `snapshot_charge_next` is present and *j* is at or after it, read from the header its
transaction compares. So a copy named before the batch that reaches its chunk gets its extra chunk
size and slot from that batch, and one named after (a move, growth) is charged at the new count by
its own transaction (R2-C1). The quota uses (1 + `snapshot_count`), charged at the mark. A
`CHARGING` snapshot is never cut (CommitSnapshot, the target), so nothing is retained before its
charge completes. #73's `AssignOwner` of a tenant charges (1 + `snapshot_count`) times the volume's
size, from the header it compares, and #73's owner `bytes` reads "a tenant's size at its claim or
assignment plus every growth step's, each times (1 + `snapshot_count`)", so snapshots taken before
activation or while unassigned are counted once assigned (R2-C3, R2-S2). Each part is applied by the
last to land of the plans it names: the request's node charge, #51 and #72; its quota charge and
`AssignOwner`'s, #51 and #73; the other sites, as the design lists (Roles, audit and charges).

Strings/integers above use their proto scalar types (string volume/name, uint64 epoch/gen).
**Transactions (120 compares and 120 operations, `meta/kv/kv.go:33`).** Commit: compares the
header and the pending record (2), writes both (2). Request's mark: compares the header and the
record's absence; once #73 is active, `owners/<id>` at the revision read, absent included, and
`tenant/authorization`, as #70's step does, so an `AssignOwner` racing the mark makes it read
again and charge the quota (R3-B2): 4 for an unassigned volume or a system owner, and for a
tenant the quota and the policy too (6); under #73's `inactiveRules`, the authorization record
absent (3). It writes the header and the record, and for a tenant the quota and the owner
record (2 or 4). A charge batch of *b* chunks on *d* nodes: compares the header, the *b* chunk
records (a move, drop or release of one of them retries the batch) and the *d* ledgers, and on
the last batch the record: 2 + *b* + *d* ≤ 120; writes the *d* ledgers, the header and on the
last the record. So any volume is charged, whatever its number of nodes (R2-C2: no node limit).
Lost compare retries at most32 with current service backoff, then Aborted; stale epoch predicate
returns applied=false current view. PermissionDenied comes only from Authorize. No cut RPC to
admin/agent.

**Roles (#73's "Methods and roles", R1-C3, R1-S1).** Whichever of #51 and #73 lands second adds
these rows. In #73's Authorize, RequestSnapshot joins the existing methods (`inactiveRules`
before activation), not #73's new methods, which inactive insecure instances refuse;
CommitSnapshot passes by its node rule, as ChangeIsr; ListSnapshots joins the open reads.

| Method | Before activation | Once active | Event |
|---|---|---|---|
| `RequestSnapshot` | administrator name | `cluster-admin` | – (follow-up) |
| `ListSnapshots` | any identity | any identity | – |
| `CommitSnapshot` | the target's data plane | the target's data plane | – |

Insecure instances (`--insecure`): before activation no caller is authenticated, so every call
passes Authorize, as today (CommitSnapshot then checks no caller, as ChangeIsr); once active they
refuse every call but the open reads (#73): ListSnapshots is served, RequestSnapshot and
CommitSnapshot are refused. Audit: no event in this step; `docs/followups.md` gains "Audit events
for snapshot operations" (#73 S7: the issue that adds them owns its hook), issue to be filed.

**Deletion (R1-C4, R1-S6).** The catalog goes after the header, as chunk records do: once the
transaction that deletes the header commits (today's, or #73's `finishDeletion`), `deleteChunks`
also lists `snapshots/<volume>/` and deletes it in batches of at most 120 deletions, each
conditional on the header being absent. DeleteVolume's header-absent branch and a creation's
claim do the same, under the same compares as for chunk records. No operation joins the header's
transaction. #72's tombstone charge takes each copy's bytes and slots times m(*j*) of the header
it deletes (a fence hold, one slot); #73's owner record already holds the snapshot charge it
releases.
CLI `snapshot request V NAME [--wait SECONDS]`, `snapshot list V`; timeout returns nonzero with
Pending operation identity, never cancels an uncertain commit or prints success. No CSI surface.

## Store and generation views

`View::Current | Before(u64)`; Before(g) requires g≥1 and resolves layers generation<g.
`changed_ranges(key,from,to:View,range)` unions actual map/zero references at generations≥from
and below Before(g), or through current. Internal from0 is the empty baseline. Return sorted,
disjoint, clipped extents covering every changed block; physical cluster overapprox allowed.
Plans retain existing pins and do not create layers or bypass node/session authorization.
Existing records unchanged: LayerCreate(type4,slot/gen), top-only MapUpdate/DeferredWrite/
Discard; no LayerMerge, under-layer write, new version or feature bit is added.

## Wire-v2 bytes

Existing `[u32le body_len][u8 tag][body]`, checked total≤MAX_BODY=1MiB+128. New tags below
are explicitly fixed version2; coordinated upgrade only, no negotiation fallback. Strings use
existing u16le length bytes. Extent is start:u32le,len:u32le; Version epoch:u64le,seq:u64le.
All counts validated before allocation, no encoder assertion as admission.

| Tag | Body after tag (in order) |
|---|---|
|0x10 HelloV2|u16 version=2; existing Hello req/epoch/session/volume/token/size/chunk_size/chunk-list encoding unchanged thereafter|
|0x11 WriteV2|req:u64,chunk:u32,extent,generation:u64,Version,prune_present:u8 plus Version if1,data exactly4096×len|
|0x12 CopyV2|req:u64,chunk:u32,extent,generation:u64,data exactly4096×len|
|0x13 ReadView|req:u64,chunk:u32,extent,view|
|0x14 DigestView|same as ReadView|
|0x15 MappedView|same as ReadView|
|0x16 Changed|req:u64,chunk:u32,extent,from:u64,to_view|
|0x17 BeginCatchup|req:u64,chunk:u32,stage:u64,known_current_generation:u64|
|0x18 DiscardAt|req:u64,chunk:u32,extent,generation:u64|
|0x19 LayerState|req:u64,chunk:u32|
|0x48 Layers|req:u64,count:u32,ascending generations[count]:u64 (count≤64)|
|0x49 Changes|req:u64,count:u32,extents[count] (bounded by request length and MAX_UNCERTAIN)|

Reply tags 72 and 73 (R1-S5): today's end at 70 (`frame.rs:130-148`), #13 (clear) takes 71
(`NOT_HELD`); no clear plan claims 72 or 73 as a frame tag (#72's 72 is a protobuf field; #75's
`LaneOpen` was cut). A decode test feeds each tag to the other's decoder and gets
`DecodeError::Invalid`, never the other frame.

view encoding: kind:u8 (0 Current,1 Before), generation:u64; Current requires generation0,
Before1…63. Write/stage generations0…63; extent.len for data≤256, Digest existing bound,
Mapped existing bound, Changed≤4096 blocks; all extents inside chunk. Existing Data/Digests/
Mappings/Ok/Stale correlate by req/session, so no new payload echo is assumed. `Refused` carries
no `req` and closes the connection (`frame.rs:110-113`): a write, copy or discard below the
chunk's top generation (S12's h < t) is answered by `Refused` and the close, the session ends
with no effect on the store, and the target takes the replica as failed, as on a store failure
(`replica/mod.rs:11-12`) (R1-C7).
Heartbeat/Settle remain existing tags within a v2 session. Legacy Hello/read/write/copy/mapped/
discard requests cannot enter v2 sessions. Decode old/new incompatibility as controlled refusal
before core/store action, never interpret one format as another. Token encoding unchanged.

BeginCatchup applies only on epoch/session-valid outside-ISR catch-up as driven by target;
replica does not independently trust metadata liveness. It resets stage filter state only
under a valid session. Live forwarding at current stage begins after all lower stages checked;
old sessions/replies cannot advance frontier. Existing larger top with differing lower view
is refusal, not top-only repair/erase. Join stays ordinary target ChangeIsr under epoch and
per-chunk versions; serialize cut/join mutations to avoid publishing an unverified frontier.

## Offline seam and bounds

`snapshot-read --device PATH --volume V --first-epoch E --cut G --chunk C
 --offset BYTES --length BYTES [--digests]`: stopped device lock required, checked4KiB geometry,
no format/create/fence. Data on stdout only when explicitly selected; errors/stats stderr.
The store key is the node's: `flaminio_node::volume_uuid(V)` (SHA-256, `flaminio-node/src/
lib.rs:21`), never the MVP name padding of `flaminio-io/src/cli.rs:41-48`. A chunk whose
`fenced_epoch` is below E is an earlier volume's of the same name (#30 S5, the rule
`Store::delete_volume(uuid, Some(first))` applies): refused, exit 1, nothing printed; an absent
chunk is refused too, never created (R1-C10, R1-S7).
Catalog's committed cut evidence is captured before stopping services. No command infers a
committed snapshot solely from layer existence. A future external reader API needs separate
authorization/pins, and encrypted retention remains unavailable pending #50 policy.

## Completeness evidence (for #52's `SourceProofV1`, R1-C5)

A replica holds the snapshot at *g* of a chunk in full when a volume view validated by #77
(header-to-header window, catalog within it) has the snapshot at *g* Committed and names the
replica's node in the chunk's ISR at its `isr_ver`. Why: a member in the ISR across the cut got
every write acknowledged before it (writes are acknowledged once durable on every ISR member, and
the cut drained them, S8); a member that joined after it held every committed snapshot (S18); and
nothing lands below a cut afterwards but catch-up copies (S12, S15). `SourceProofV1`'s
`node_id`, `chunk_index`, `isr_version` and `snapshot_ref` are these. The evidence is about that
view: that the copy is still the same store's (#61's store lifetime, which `SourceProofV1` also
carries) and still held (a drop or deletion since) is checked by the reader, #52's. A replica
outside that ISR is no evidence, whatever layers it holds.
