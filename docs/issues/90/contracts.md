# Issue #90: exact store identity contracts

Proposed, unapproved. Normative appendix to [design](design.md). #90 owns this identity ABI;
#61 is the cloud-policy tracker and #72 supplies capacity accounting, not `ReplicaRef`.
Bootstrap RPC/KV/grant bytes are defined once in [core-design](core-design.md); maintenance
RPCs and activation are defined once in [migration](migration.md). [Wire](wire-contracts.md)
and [budgets](budgets.md) fix their composed transport and accounting limits. Historical #61
approvals do not approve these contracts or the new #90 split.

## One identity and one source lifetime

`ReplicaRef { string node=1; string device_uuid=2; }`. Node/volume IDs use the existing ASCII
1..128-byte metadata grammar. UUID is exactly36 lowercase ASCII bytes in canonical
8-4-4-4-12 form, nonzero; no arbitrary UUID spellings. Binary wire uses those same UUID's raw16
bytes in network UUID order, not host-endian GUID words. Ref order is `(node bytes, UUID bytes)`;
sets are sorted, unique exact refs. Distinct UUIDs on one node are distinct copies but never
independent ISR/domain members. A replacement may temporarily name both in replicas/move/release.
Store generation is not part of earned membership: an original UUID returning retains its data
identity, while every previous session and external source proof still needs fresh authority.

Durable `/flaminio/v1/store-lifetime/<node>` is schema1 JSON
`{"schema":1,"generation":G,"last_uuid":UUID-or-null}`. It is service-owned and never deleted,
including ForgetNode, node-record removal, restart and enrollment. G is uint64, starts at1 on the
first verified data-store enrollment/registration, increments on every UUID change including
A→B→A and on an explicit new enrollment lifetime, and otherwise stays unchanged. A node without
a verified store has no positive lifetime. Checked overflow refuses the change with
`FailedPrecondition`; no wrapping, reset or UUID reuse restores an old generation.
`Node.store_uuid=20:string`, `Node.store_generation=21:uint64` project that committed lifetime.

Registration's store fields are writable only by that node's authenticated `io`, from the
actually opened validated superblock, or by the administrator-only enrollment operation in
ENROLLING mode. A normal administrator/agent/address/label registration cannot set/clear store
identity. Callers never supply a generation: nonzero request generation is `InvalidArgument`.
An omitted UUID preserves the lifetime; new data-plane registration requires the real UUID.
Address/label-only updates, same-UUID registration, daemon restart and heartbeats do not increment
G. Existing #45 role/health fields and #72 capacity projections remain service-owned.

On a lifetime change, one transaction compares node, lifetime, placement-gate and identity-mode;
it puts node+lifetime, bumps #78 placement-gate and deletes `alive/<node>`. New-node zero-ledger
creation adds the ledger-absence compare/write. Same-UUID retries read the committed G, never
increment twice. Node revision changes atomically with every lifetime change, so bootstrap and
cleanup compare the node revision; no separate lifetime compare is needed there. Registration
never rewrites earned refs or clears an inaccessible old copy's capacity charge.

## Authoritative metadata and additive protobuf

Identity records use JSON `identity_schema:2`. Chunk fields are `replica_refs`, `isr_refs`,
`moving_from_ref`, `moving_to_ref`, `released_refs`, `move_operation` (lowercase32hex nonzero
UUID), `move_kind` (`move` or `replacement`). Header fields add `fenced_refs` and `paged_legacy`.
Absent move has no endpoints/operation/kind. A move has all four. `initial_pending_refs` and nonzero `initial_operation` are initialized only for new #76
chunks (or genuinely new chunks of an implemented growth step), never reset on existing chunks.
The durable, consumable authority is defined by
[core-design](core-design.md); a chunk with pending initial refs has no move/release. Existing first-epoch, current
epoch, ISR version, creation operation and header revision remain their authorities. A nonempty
legacy membership field in a schema2 stored record is corruption, not an additional authority.

The strict local target/device and normal CSI intent extension for FE0/LID is defined once in
[local-target-contract.md](local-target-contract.md); paged Attach nonce/handoff tags are in
[budgets.md](budgets.md).

Append-only protobuf allocations (all other fields keep their owners):

| Message | New fields |
|---|---|
| `ChunkMeta` | repeated ReplicaRef replica_refs20,isr_refs21; ReplicaRef moving_from_ref22,moving_to_ref23; repeated ReplicaRef released_refs24; bytes move_operation25(exact16 when moving); enum MoveKind move_kind26(NONE0,MOVE1,REPLACEMENT2); uint32 identity_schema27=2; repeated ReplicaRef initial_pending_refs28; bytes initial_operation29(empty or16) |
| `Volume` | uint32 identity_schema22=2; repeated ReplicaRef fenced_refs23; bool paged_legacy24; uint64 first_epoch25; bytes legacy_incarnation26(empty or16) |
| `IsrChange` | repeated ReplicaRef new_isr_refs20,new_replica_refs21; bytes expected_move_operation22(empty without move, otherwise exact16) |
| `MoveReplicaRequest` | uint64 first_epoch20; ReplicaRef from_ref21,to_ref22; uint64 expected_ver23,expected_store_generation24; bytes operation25(exact16),legacy_incarnation26(empty or16) |
| `ReplaceReplicaStoreRequest` | string volume1; uint64 first_epoch2,epoch3; uint32 chunk4; uint64 expected_ver5; ReplicaRef old_ref6,new_ref7; uint64 expected_store_generation8; bytes operation9(exact16),legacy_incarnation10(empty or16) |
| `ReplaceReplicaStoreResponse` | bool applied1; ChunkMeta chunk2; string reason3(max256 UTF8 bytes) |

`ReplaceReplicaStore` caller is current target node's `io`, checked again by current target/epoch
and expected first epoch/version in the method. Old/new refs must share a node, differ in UUID;
old is placed and excluded from ISR, new is the observed store at the requested generation.
No pending move, no conflicting retained new ref, release bound and authoritative surviving ISR
checks precede the transaction. The pending operation is immutable/idempotent: same operation and
arguments returns its existing move; reusing it with different arguments is `InvalidArgument`;
a different pending operation returns applied=false and current chunk. Final ChangeIsr joins
only that operation's new ref after the core's complete current+historical frontier proof.

Ordinary MoveReplica retains its administrator authorization and domain checks, now requiring
exact refs and first epoch/version. Every new destination is admitted under [budget charges](budgets.md).
ChangeIsr retains own-target `io` authorization and #81's64-change limit; it cannot name a
node-only ISR, substitute a destination UUID or omit/change the saved operation. In ACTIVE mode
legacy node fields (`ChunkMeta`1,2,4,5,6; `Volume.fenced`15; `IsrChange`3,5) are omitted in
responses and rejected when nonempty in mutation requests. They appear only in read-only legacy
inventory before enrollment. A schema2 reader never falls back to them. CLI derives display
node names from refs. Wire/metadata upgrades are coordinated, with no mixed authority writers.

Wrong role is `PermissionDenied`; bad syntax/counts `InvalidArgument`; absent identity/profile/
mode/required historical proof `FailedPrecondition`; changed revision/epoch/generation `Aborted`
(or applied=false with a current chunk where the existing method specifies it); no Room
`ResourceExhausted`. Refusal precedes store mutation or ledger writes.

## Exact cleanup authority (#13 and #72 composition)

Tombstones use bounded schema3 roots and ref-charge rows, never one volume-wide ref array.
`tombstones/<volume>` is a <=4096-byte JSON root: schema3, volume/incarnation
(first_epoch plus optional legacy_incarnation canonical UUID36), nonreused random deletion_operation UUID,
phase BUILDING/PENDING/DRAINED/RECLAIMING, frozen_header_revision, source cursor,
ref_count/pending_count and reclaim cursor (all checked uint64). Per-ref keys
`tombstone-refs/<volume>/<operation>/<node>/<uuid>` hold <=512-byte schema1 rows
`{ref,bytes,slots,state:PENDING|CONFIRMED}`; index keys
`tombstone-index/<node>/<uuid>/<volume>/<operation>` hold <=128 bytes. Keys are <=512 bytes.
A row aggregates every charge of that exact ref; it never deduplicates different UUIDs by node.
Root/row decoder bounds are checked before JSON allocation; unknown/extra forms are DataLoss.

DeleteVolume first freezes the header as Deleting and creates BUILDING root atomically under
header/root-absence/gate/mode compares, retaining all original deletion authorization/protection
checks. No grant can Finish against that header. One bounded build step consumes one chunk's
exact-copy union (<=R+2<=7 refs), or one fence/claim source entry; it atomically updates row
charges, creates each new row's index and advances root cursor. Header/root/source/row/index
revisions, gate and mode are compared. Snapshot m(j) comes from the frozen header. Source
chunks/claims/fence holders remain until build completes. Only after every source is accounted
for does one CAS delete the header and publish PENDING (or DRAINED if empty); partially built
rows/indexes are never cleanup authority. A lost step reply rereads the durable root cursor,
so no contribution is charged twice. Header Deleting freezes all source writers; source hashes/
revisions are verified, not inferred from a current UUID. BUILDING resumes after restart.

The union contains at most `N*(R+2)+16+110` refs, including interrupted creation claims; the
largest supported legacy union is458878. This bounds total work, not one allocation or value.
Every phase scans/holds bounded pages. Old claims/tombstones are converted through bounded
source projections in [migration](migration.md); no legal union must fit a16KiB inventory item.

| Message | Identity additions to #13/current messages |
|---|---|
| `VolumeReleases` | uint64 first_epoch3; ReplicaRef ref4; uint64 observed_store_generation5; int64 header_revision6,replica_gate_revision7; bytes legacy_incarnation8(empty or16) |
| `ListReleasesRequest` | ReplicaRef ref20; uint64 observed_store_generation21 (node1 must equal ref.node) |
| `ConfirmReleaseRequest` | ReplicaRef ref20; uint64 first_epoch21,observed_store_generation22; int64 expected_header_revision23,expected_replica_gate_revision24; existing chunks3 are `{chunk1:uint32,revision2:int64}`; bytes legacy_incarnation25(empty or16) |
| `Tombstone` | ReplicaRef ref20; uint64 charged_bytes21,charged_slots22,first_epoch23; int64 root_revision24; bytes deletion_operation25(exact16),legacy_incarnation26(empty or16); int64 ref_revision27; bool pending28 |
| `ListTombstonesRequest` | ReplicaRef ref20; uint64 observed_store_generation21; uint32 limit22(1..32,default32); bytes cursor23(<=1024); node1 equals ref.node |
| `ListTombstonesResponse` | existing repeated Tombstone tombstones1(<=32); bytes next_cursor20(<=1024); bool complete21 |
| `ConfirmDeletionRequest` | ReplicaRef ref20; uint64 first_epoch21,observed_store_generation22; int64 expected_root_revision23,expected_ref_revision24; bytes deletion_operation25(exact16),legacy_incarnation26(empty or16); existing volume1,node2 retained |

Each Tombstone result describes only the requested exact pending ref, its charge, incarnation,
operation and revisions (<=512 encoded bytes). It does not repeat all other refs/confirmed rows.
ListTombstones scans only that ref's index prefix, at most32 keys/32KiB per call, then bounded
root/row reads; non-PENDING roots and confirmed/stale rows consume scan budget but are not
returned. Response <=32KiB including cursor; it can be empty with complete=false. No full
List(tombstones/) or full-prefix Mem sort/clone is permitted. Reuse #76 ScanKeys/GetBounded:
Etcd keys_only+limit and bounded exact-key RPCs; Mem's ordered index visits only selected keys.

Cursor is signed bytes `FLTSCP01`8, caller_URI_SHA25632, node:string, uuid16, G:u64,
after_index_key:string, Ed25519_signature64 (all integer/string encodings LE, <=1024 bytes).
The signature uses the existing metadata token signer with this distinct magic/domain and
covers every preceding byte. Each metadata instance verifies scope/caller/ref/G and key-prefix
before its current linearizable scan. Forged/mixed/oversize cursors are InvalidArgument. Cursor
is exclusive last **examined** key, including skipped entries; it is not an authority snapshot.
One final read-only Txn compares all returned root/row revisions plus node/mode (<=66
compares); failure returns Aborted without advancing the response cursor. Confirmation requires both exact
revisions/operation/incarnation, so concurrent changes or name reuse cannot alias a listed row.
Entries inserted behind the cursor are visited on the sweep's next completed wrap. Sweep keeps
one bounded page/tail, advances only through examined entries, never overwrites an unconsumed
tail, and restarts safely from the prefix after process loss. Per turn max32 examined index keys,
68 backend calls and one confirmation in flight; backpressure/cancellation retains that tail.

Lists are read-only: own-node `io` or administrator; own-node requests require its opened UUID
and current G. Administrator may supply G=0 to inspect any exact ref. ConfirmRelease is own-node
`io` only. ConfirmDeletion is own-node `io` or administrator, but administrator authorization is
not evidence that a missing old disk was erased: the method still requires the exact current
observed ref/G and matching listed incarnation/revision. Agents and another node's IO cannot
confirm. Old node-only requests are `FailedPrecondition` in ACTIVE mode. Tombstone's legacy
nodes/pending fields are omitted; all empty/unknown UUID forms refuse.

Before deletion the adapter pins its open Store instance and verifies UUID/ref, first epoch,
release row's age and #13's no-target/no-placement/fence guards. It repeats UUID/instance and
session/deletion checks at completion; a callback for another Store instance cannot confirm.
An unplugged A cannot be deleted/confirmed by B at the same node. A returning A must register
its actual UUID, obtaining a fresh G, relist exact A rows and perform safe cleanup. It gets no
old session/source/ISR authority from returning or from B's completion. A missing/reformatted
old disk remains pending and charged; only #13's explicit permanent ForgetNode can waive it.

ConfirmRelease accepts1..115 unique listed chunks, same ref/first epoch/header/gate, and compares
header, every listed chunk, node, ledger, replica-gate and ACTIVE identity-mode (b+5<=120).
It removes only the exact ref, bumps the gate and applies #72 releaseCharge once; it retains
one fence slot iff it newly adds this exact ref to the bounded fence-holder set. Snapshot bytes/
slots use m(j) from the compared header. A stale listed revision is Aborted; retry relists and
never reuses a chunk revision with a different ref. Replaying a committed confirmation cannot
subtract again. ConfirmDeletion compares the exact tombstone revision/incarnation, node, ledger,
gate and mode plus the exact ref row (six comparisons), records only that row CONFIRMED,
decrements root pending_count and subtracts only its charge (root+row+ledger+gate, <=4 writes).
A lost reply/replay already CONFIRMED returns success without a second release. Root/operation/
incarnation mismatch is Aborted; root absence is success only when no newer same-name tombstone
can be mistaken for that operation. pending_count=0 changes root to DRAINED. Reclamation deletes
at most32 confirmed row/index pairs per transaction while updating root cursor and gate; only
a final empty scan under the unchanged root revision permits root deletion. Root remains a
name-reuse barrier until every row/index is gone. Gates/lifetimes are never deleted.

## Gates, retained keys and tests

`replica-gate/<volume>` is a never-deleted schema1 uint64 counter, initialized to1 before the
first identity claim, compared and checked-incremented atomically by every authority mutation.
No writer resets it on delete/recreate; overflow refuses. Writers: creation claim/public chunk
copy/cleanup/finalize and abort; placement/move/replacement/ChangeIsr (including #81 batches);
release confirmations; fence-holder additions/removals; deletion mark/tombstone creation/chunk
cleanup/final deletion and tombstone confirmation; ForgetNode per-volume cleanup; snapshot
charge/cut/catalog changes and #58 committed/pinned-root/frontier authority changes; migration
enrollment/quarantine conversion. Header epoch/target/initialization mutations and CompleteInitialReplicas consumption also bump it.
TTL liveness/report renewal and label changes do not alter earned authority; node revision and
#78 placement-gate separately bind observed-store changes. All runtime authority writers compare
ACTIVE `identity-mode`; enrollment writers compare their exact ENROLLING mode revision.
The only gate-Put exception is proof-scoped #76 delete-only reclamation: unpublished header
rollback, eligible private-root revocation and private/public orphan deletion, under exact
identity-mode and root/header/key revision conditions in [budgets](budgets.md). No published
chunk/ref/cleanup authority or ledger is changed by that exception; all live mutations stay gated.
No other helper, older deletion path or name-reuse cleanup can bypass these hooks.

Planned proof: both-backend service/auth/registration/cleanup/capacity tests and real Rust-client
interop check same-node wrong-UUID confirmations, old return, A→B→A source-proof replay, label
CAS/no-op/restart preservation, identity/gate overflow, delayed old heartbeat, max115/116 release
batches, Forget/deletion/enrollment bounds and lost replies. Pure model/adapters must emit the
same refs/limits; [core-design](core-design.md) owns safety assertions and caught mutants.
`make proto contracts test race lint` plus the specified simulator checks are future checks,
not results of this documentation revision. The last implementation to land among #13/#51/#52/
#45/#58/#72/#76–#83 and #90 installs these exact hooks/fields/limits and fixtures; dependent plans
remain read-only and their historical approvals do not settle this new composition.
