# Issue #58 — metadata and operation contracts

Historical approval/settlement: 2026-11-13. This review revision is **proposed/unapproved**.
Nothing is implemented. [Lifecycle](lifecycle.md) · [wire/reader/manifest](wire-contracts.md) ·
[root format](root-format.md). These three appendices replace the old unresolved ABI gate with
concrete draft contracts; clear dependency documents remain read-only. Whoever lands second
adds the listed #15/#17/#48/#50/#51/#52/#70/#73/#76/#77/#80 integration and golden vectors.

## Types and common validation

Independent protobuf messages use the field numbers below. UUID=bytes exactly16 and nonzero;
hash=bytes exactly32; IDs use domain.ValidID (ASCII1–128); revisions/epochs/generations are u64,
nonzero except explicit absence0. Unknown enum/field, duplicate singular field, invalid length,
overflow and mismatching immutable identity fail before mutation. JSON metadata serialization
is strict schema1 with the same logical fields; each record≤8192 bytes, CA-bearing pair≤98304.
Caller never supplies an etcd key/path. Error codes: malformed InvalidArgument; wrong role
PermissionDenied; unknown NotFound; changed UUID request AlreadyExists; stale CAS Aborted;
unsupported mode/phase FailedPrecondition; caps ResourceExhausted; no metadata Unavailable.
Retries of a committed operation read the same durable outcome; no success inferred from timeout.

`DrPair={id1:uuid,peer_cluster2:uuid,trust_domain3:string,ca_bundle4:bytes,
endpoint5:string,config_generation6:u64,revoked7:bool,shipper_uri8:string,
receiver_uri9:string,receipt_public_key10:bytes}`. Domain canonical ASCII≤253, endpoint≤512
and explicitly host:port (no URL credentials/path), CA≤65536, each URI≤512 canonical SPIFFE,
receipt key32. Exact CA/domain/role/cluster/key binding is explicit administrator configuration.

`VolumeRef={cluster1:uuid,volume2:string,first_epoch3:u64}`.
`Geometry={size1:u64,chunk_bytes2:u64,logical_block_bytes3:u32}`; logical block4096; chunk/size
use #80's validated geometry/snapshot-capable bound. `SourceRef` is SnapshotRefV1 in wire-contracts.
`GrantRef` is #52's `{volume,first_epoch,operation,generation}`.
`StandbyDescriptor={source1:VolumeRef,destination2:VolumeRef,relationship3:uuid,
generation4:u64,destination_pair5:uuid,pair_generation6:u64,geometry7:Geometry,
endpoint8:string,lineage9:uuid}`. Its authenticity is checked through the paired endpoint;
a copied descriptor never configures trust or supplies a local target capability.

`Relationship={id1:uuid,destination2:VolumeRef,source3:VolumeRef,pair4:uuid,
pair_generation5:u64,geometry6:Geometry,generation7:u64,state8:DrState,
last_point9:ShipPoint,receiver_node10:string,promotion11:uuid,lineage12:uuid,active_ship13:bytes}`.
DrState UNSPECIFIED0,STANDBY1,PROMOTING2,PROMOTED3,REVOKED4,SOURCE_PAIRED5;
SOURCE_PAIRED appears only in a source Volume descriptor, never as relationship state.
`ShipPoint={ship1:uuid,relationship2:uuid,source3:SourceRef,destination_cut4:u64,
manifest5:hash,lineage6:uuid,source_time_unix_ns7:i64,clock_uncertainty_ns8:u64,
clock_known9:bool}`. Source UTC/uncertainty is diagnostic; unknown remains unknown.

Proposed `Volume.dr=32:DrDescriptor{profile1:string,relationship2:uuid,state3:DrState,
selected_ship4:uuid,lineage5:uuid,manifest6:hash}`. Profile is dr-roots-v1 for destination,
dr-source-v1 for a paired ordinary source. Maximum addition107 bytes, including outer field32;
#80's worst-volume allowance gains107 bytes. Proposed `GetVolumeRequest.accepted_dr_profile=4`
is the feature capability string dr-roots-v1 for this DR-aware client version; it supports both
marker modes. Existing snapshot accepted profile3 and encryption profile2 remain unchanged.
No field claims #51's30/31, #75's40 or an occupied volume field9.

## Exact local RPC messages

Every response includes revision at its explicitly numbered field where stated. Empty request
means no fields. Tickets/keys never appear in Get/List/status output. Full metadata auth/role
matrix, activation comparisons and transactions are in lifecycle.md.

| RPC | Request fields in tag order | Response fields in tag order |
|---|---|---|
| EnableDr | empty | revision1:u64 |
| ConfigureDrPair | pair1:DrPair,expected_revision2:u64 | pair1:DrPair,revision2:u64 |
| RevokeDrPair | pair1:uuid,expected_revision2:u64 | pair1:DrPair,revision2:u64 |
| CreateStandby | operation1:uuid,relationship2:uuid,name3:string,pair4:uuid,expected_pair_generation5:u64,source6:VolumeRef,geometry7:Geometry,replicas8:u32,min_isr9:u32,failure_domain10:string | descriptor1:StandbyDescriptor,revision2:u64,creating3:bool |
| SelectDrReceiver | relationship1:uuid,node2:string,expected_revision3:u64 | relationship1:Relationship,revision2:u64 |
| StartShip | operation1:uuid,source_volume2:string,source_first_epoch3:u64,destination_pair4:uuid,expected_pair_generation5:u64,destination6:StandbyDescriptor,target_snapshot7:string,expected_base_manifest8:bytes,worker_uri9:string,shipper_uri10:string | operation1:Outgoing,revision2:u64 |
| GetOutgoing | operation1:uuid | operation1:Outgoing,revision2:u64 |
| ReportDrProgress | operation1:uuid,bytes_done2:u64,error_class3:string,check_closure4:bool | operation1:Outgoing,revision2:u64 |
| BindDrManifest | operation1:uuid,expected_revision2:u64,manifest_root3:bytes | operation1:Outgoing,revision2:u64 |
| RecordDrCompletion | operation1:uuid,receipt2:bytes | operation1:Outgoing,revision2:u64 |
| CancelShip | operation1:uuid,expected_revision2:u64 | operation1:Outgoing,revision2:u64 |
| CancelIncomingShip | relationship1:uuid,ship2:uuid,expected_revision3:u64 | incoming1:Incoming,revision2:u64 |
| EndDrSource | volume1:string,first_epoch2:u64,expected_revision3:u64 | revision1:u64 |
| AdmitDrShip | relationship1:uuid,ship2:uuid,node3:string,pair_generation4:u64,source5:SourceRef,base6:SourceRef,manifest7:hash,manifest_pages8:u32,expected_relationship_generation9:u64,manifest_root10:bytes | incoming1:Incoming,revision2:u64 |
| AttachStandby | relationship1:uuid,ship2:uuid,node3:string,expected_relationship_generation4:u64,pair_generation5:u64 | volume1:Volume,epoch2:u64,ticket3:bytes,incoming4:Incoming |
| CheckDrAdmission | relationship1:uuid,ship2:uuid,node3:string,epoch4:u64,pair_generation5:u64 | permit1:AdmissionPermit |
| CommitShip | relationship1:uuid,ship2:uuid,epoch3:u64,relationship_generation4:u64,pair_generation5:u64,manifest6:hash,barrier_digest7:hash,destination_cut8:u64 | point1:ShipPoint,revision2:u64,receipt3:bytes |
| PromoteStandby | operation1:uuid,relationship2:uuid,expected_revision3:u64,expected_manifest4:hash,node5:string,accept_possible_dual_primary6:bool | promotion1:Promotion,revision2:u64 |
| AcquireDrPromoter | operation1:uuid,node2:string,expected_revision3:u64 | volume1:Volume,epoch2:u64,ticket3:bytes,promotion4:Promotion |
| CompletePromotion | operation1:uuid,node2:string,epoch3:u64,selected_manifest4:hash,barrier_digest5:hash | promotion1:Promotion,revision2:u64 |
| GetShip | relationship1:uuid,ship2:uuid | incoming1:Incoming,point2:ShipPoint,revision3:u64 |
| ListRelationships | cursor1:bytes,limit2:u32 | relationships1:repeated Relationship,next_cursor2:bytes |

`Outgoing={operation1:uuid,source2:VolumeRef,destination3:StandbyDescriptor,
local_pair4:uuid,pair_generation5:u64,target6:SourceRef,base7:SourceRef,
target_grant8:GrantRef,base_grant9:GrantRef,worker_uri10:string,shipper_uri11:string,
state12:OutgoingState,manifest13:bytes,receipt14:bytes,bytes_done15:u64,error_class16:string,
manifest_root17:bytes,outcome18:DrOutcome,required_grants19:u32,closed_grants20:u32,
expected_base_manifest21:bytes}`.
OutgoingState UNSPECIFIED0,QUEUED1,READING2,SENDING3,COMMITTED4,CLOSING5,CLOSED6,CANCELLED7.
Absent base/grant/manifest/receipt is empty optional message/bytes, never zero-as-a-valid-hash.
DrOutcome UNSPECIFIED0,COMPLETED1,CANCELLED2; retained when state reaches CLOSED.
required_grants is bit0 target plus bit1 when base exists; closed_grants is a subset. Bind stores
full canonical ManifestRootV1≤842 bytes plus its32-byte hash in manifest; expected_base_manifest
is empty for full ship or the immutable32-byte StartShip input. Error class empty or≤64 `[a-z0-9_]`; receipts≤2048; bytes_done≤geometry size.

`Incoming={relationship1:uuid,ship2:uuid,source3:SourceRef,base4:SourceRef,
manifest5:hash,pair_generation6:u64,destination_cut7:u64,receiver_node8:string,
receiver_epoch9:u64,state10:IncomingState,bytes_done11:u64,error_class12:string,
manifest_root13:bytes}`.
IncomingState UNSPECIFIED0,CHARGING1,RECEIVING2,PREPARED3,COMMITTED4,CANCELLED5. Admit creates
CHARGING with destination_cut=header.snapshot_next_ordinal and advances that ordinal and reserves #51's snapshot charge before RECEIVING; no
root write/foreign received ACK precedes completed reservation. Repeating Admit resumes it.
Admit validates/persists full canonical ManifestRootV1≤842, its hash, refs/geometry/page count
in the same incoming claim; hash-only admission is refused. Thus a crash before any replica bind
can recover root initialization from local metadata without the sender. Prepared is a durable replica/root fact, not peer claim; Commit requires the current core barrier.
Destination snapshot catalog name is `dr-` plus ship hex; its pending reservation/catalog entry
is published Committed with the same destination_cut in CommitShip.

`Promotion={operation1:uuid,relationship2:uuid,destination3:VolumeRef,point4:ShipPoint,
lineage5:uuid,node6:string,epoch7:u64,state8:PromotionState,accepted_dual_primary9:bool}`.
PromotionState UNSPECIFIED0,PENDING1,READY2. This first increment supports only explicit
accept_possible_dual_primary=true, recorded with actor and exact source/point; no provider of
verified remote exclusion is invented. False refuses FailedPrecondition. This narrows the old
unused exclusion_evidence option, preserves no-peer promotion and makes divergence explicit.
AcquireDrPromoter always CASes a fresh epoch for the elected local node; lost response/retry
supersedes earlier unknown tickets. Complete compares its exact current epoch and core barrier.

`AdmissionPermit={relationship1:uuid,ship2:uuid,epoch3:u64,pair_generation4:u64,
relationship_generation5:u64,max_ranges6:u32,max_bytes7:u32}`; max8/8MiB. The local receiver
binds it to the current connection and a one-second monotonic issuance interval; no serialization
to the peer, reusable bearer token or wall-clock fencing claim. Every batch revalidates.

## CLI and worker handoff

CLI exact families: `dr enable`; `dr pair --pair UUID --expect-revision N
--peer-cluster UUID --domain TD --ca PATH --receipt-key PATH --endpoint HOST:PORT --shipper-uri URI
--receiver-uri URI`; `dr revoke --pair UUID --expect-revision N`; `dr standby --operation UUID
--relationship UUID --name V --pair UUID --pair-generation N --source-cluster UUID --source-volume V
--source-first-epoch N --size N --chunk-bytes N --replicas N --min-isr N --failure-domain KEY`;
`dr select-receiver --relationship UUID --node N --expect-revision N`; `dr ship --operation UUID
--source V --first-epoch N --pair UUID --pair-generation N --destination-descriptor PATH
--snapshot NAME --worker-uri URI --shipper-uri URI [--base-manifest HEX]`; `dr cancel --operation UUID
--expect-revision N`; `dr cancel-incoming --relationship UUID --operation UUID --expect-revision N`; `dr end-source --volume V --first-epoch N --expect-revision N`; `dr promote
--operation UUID --relationship UUID --expect-revision N --manifest HEX --node N
--accept-possible-dual-primary`; `dr status --relationship UUID --operation UUID`.
All integers strict unsigned decimal, hashes64
lowercase hex, UUIDs32 lowercase hex. Paths are local regular files opened safely, never shell
interpolated or forwarded. Outputs schema_version1 JSON, exit0success/1failure/2arguments,
no tokens/keys/payload; nonterminal status is not fabricated completion.

Worker config is strict JSON≤16384 bytes: local metadata endpoints (≤8), separate local backup
and foreign shipper certificate/key/CA paths, local node control path, reader port, DR endpoint
and operation/relationship UUID. No embedded secret bytes or executable field. Credentials live
in separate0700 user-owned directories with0600 keys. Node control/root writes remain Rust;
Go CLI/service carries lifecycle metadata only (ADR0001). No arbitrary data over control lines.

CancelIncomingShip is a local administrative cancellation: compare header/relationship/incoming,
mark incoming Cancelled, clear relationship.active_ship and allocate a newer header epoch with
no receiver. Committed already won: return its unchanged committed outcome. The inverse commit
then fails its epoch/active-ship CAS. Cancelled cannot restart; a new operation reserves the next
monotonic destination ordinal. Retained cancelled snapshot charge/root slots are not reused.


## R2 source manifest and grant transactions

BindDrManifest belongs only to the exact outgoing backup worker (same auth row as progress).
It parses canonical ManifestRootV1, requires exact authorized target/base SnapshotRefs and
geometry, and computes its hash over those exact bytes. Compare outgoing Reading at the
expected revision, source header/guard naming this latest operation, unrevoked source pair at
its bound generation, and each required grant Active (at most2): at most5 compares, one outgoing
write. It atomically stores full bytes/hash and enters SENDING before any foreign Begin. An
identical bound SENDING retry returns the same record with UNSPECIFIED outcome; an identical
Committed/Closing/Closed retry with COMPLETED outcome returns its stored completed outcome.
Different bytes/hash are AlreadyExists. Cancelled/CANCELLED outcome refuses.
A stale first bind is Aborted. RecordDrCompletion requires the exact bound manifest/root;
progress cannot install or alter them. Restart reads the bound bytes and resumes the same ship.

Internal DR AuthorizeSnapshotRead is #52's grant claim with one extra outgoing compare and an
extra outgoing write. Require outgoing QUEUED/READING, exact current source header guard
(latest_operation=S, same incarnation/destination), and the deterministic per-kind grant key.
Atomically create the grant/pin and publish that exact GrantRef in outgoing; never expose a
committed grant without its durable outgoing reference. #52's at most7 comparisons/4 writes
become at most8 comparisons/5 writes, or9 comparisons when the collected older closed
DR grant needs its own outgoing-CLOSED eligibility compare. This includes inactive authorization,
cluster and collection cases. Header is already compared by #52; guard validation adds no separate record. A repeated
claim returns only the same still-active grant/ref; it cannot reopen a closed/cancelled grant.
Reading requires both references when a base is required; no foreign Begin occurs in QUEUED.

CancelShip first CASes the outgoing to CANCELLED outcome/state (a committed/completed outcome
is never changed to cancellation). Every CancelShip call/retry also invokes the closure reconciler below for either terminal outcome,
so an administrator can finish cancellation after worker death without worker credentials. This blocks every subsequent grant claim/bind. It then computes both
required deterministic IDs from S even if a reference response was lost; reads each exact grant
key and closes any present one aborted through #52. Absent is safe only after the cancellation
CAS, because the outgoing predicate then forbids creation. A pre-cancel claim either committed
its ref atomically and is found, or loses its compare. A crash resumes from outgoing, never from
an ephemeral reference list. No timeout bypasses reader-source acknowledgements/exclusions.

ReportDrProgress(check_closure=true), and the same reconciler called by CancelShip, first
check outgoing CLOSED: return its unchanged durable outcome and all-required-bits-closed mask,
without re-closing or requiring records that #52 may already have collected. This is an
idempotent read-only fast path; an inconsistent CLOSED/mask record is Internal, never repaired
by inference. Otherwise reconcile Committed/Cancelled/Closing operations: close both
deterministic grants with the already recorded completion/cancellation outcome, then read
their actual states. At most3 compares (outgoing plus2 exact grant revisions/absence) write one
outgoing with closed_grants; enter CLOSING while any barrier is pending and CLOSED only when
all required grants are closed/absent under frozen claims. Before closure is durably CLOSED,
a missing grant for a COMPLETED outcome is Internal, never fabricated completion; a cancelled
never-created grant may be absent. Lost final closure replies use the CLOSED fast path even if
grant collection has since removed the closed records. EndDrSource compares header/guard, latest outgoing CLOSED and
the same required closed/absent grant records: at most4 compares, one header write. New StartShip
must establish the previous latest operation CLOSED before replacing guard.latest_operation;
its header CAS serializes with EndDrSource. Earlier operations are transitively closed.

#52's closed-grant collection may collect a DR grant only after its bound outgoing is CLOSED
with that required bit recorded closed; compare that outgoing revision in the collection
transaction. Until then retain the closed record, so a completed grant cannot disappear before
closure reconciliation observes it. A completed operation already CLOSED may therefore see
collected/absent grant records on EndDrSource; its durable all-closed mask remains evidence.
