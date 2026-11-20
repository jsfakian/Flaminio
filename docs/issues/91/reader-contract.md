# Issue #91: bounded complete metadata observation

Proposed, unapproved appendix to [design](design.md). This owns the guard's read adapter,
including the last implementation to land with #69/#77/#90; those dependency plans stay
read-only. Observations never authorize replica I/O, membership, bootstrap or fencing.

## Strict enumeration at the metadata service

Actual `meta/service/service.go:845-857` silently skips malformed header JSON. A successful
empty legacy `ListVolumes` is therefore insufficient. #77 supplies coherent **named** reads,
not this missing enumeration check. No guard process receives etcd credentials or reads KV.

Add `bool require_complete=20` and `string candidate_metadata_id=21` to `ListVolumesRequest`;
add `bool complete=20`,
`string identity_mode=21`, `int64 identity_mode_revision=22` to `ListVolumesResponse`.
The exact candidate-history tags23/24, permanent-history absence check and classification
are owned by [identity-contract](identity-contract.md), not inferred from an empty ID list.
IDs retain tag1. The new fields are additive. `require_complete=false` preserves old caller
behavior; a guard always sends true and accepts only complete=true. An old server ignores
the request and returns default false: hold `MetadataIncomplete`, never retry as an admin.

The strict branch reads one linearizable bounded snapshot of **all candidate header keys**
under `/flaminio/v1/volumes/`, before filtering publication flags. Add internal
`KV.ListBounded(ctx,prefix,maxEntries,maxValueBytes,maxTotalBytes) ([]Entry,error)`:
sorted entries from one read point, or an error with no partial result. Limits are4096
candidate headers including unpublished/deleting ones,16,384 bytes/value and4,194,304 total
key+value bytes. Checked arithmetic includes key bytes. Mem visits its ordered keys under
one mutex, checking before cloning each value; it must not call unbounded List/sort/clone.
Etcd uses one linearizable prefix Range with limit4097, ascending key order and a per-call
receive cap5,242,880 bytes (4MiB payload plus1MiB for bounded per-key protobuf/revision
framing;4097 entries at≤128 bytes overhead fit that allowance); count/`More`/bytes overflow
returns ResourceExhausted. An oversized transport frame is an error, never a partial list.
This is a new bounded internal read, not a new metadata backend or direct client KV access.

Before JSON decoding validate key length/suffix with `domain.ValidID`; reject nested or
noncanonical keys. Reject duplicate JSON keys, trailing values, oversized/deep nesting
(depth16), malformed types and contradictory publication flags. Header ID must equal the
key suffix, geometry must use checked nonzero sizes/chunk counts and valid RF/minISR, and
the header must satisfy the installed schema/profile validator. Do not calculate ChunkCount
before checking nonzero chunk size and overflow. Unknown authority/schema fields refuse;
recognized feature fields are validated by their owning adapter. Stable malformed header,
ID/publication/schema contradiction is DataLoss, backend/context failure preserves its code,
unsupported profile/mode is FailedPrecondition. Every case returns no IDs and complete=false.

A valid Creating header is an unpublished claim and is deliberately omitted; a valid Deleting
header is no longer a published volume and is deliberately omitted. Missing keys and separate
tombstone records are not published volumes. This does not discard their storage/cleanup
authority. Malformed records are never omitted by guessing they were unpublished/deleted.
Only after every candidate validates does strict enumeration return sorted unique published
IDs and complete=true. A mixed good/bad list fails just as a sole damaged header does.

Before #90 exists, absent identity-mode is reported `identity_mode="UNINSTALLED"`, revision0;
this guard's admission profile refuses it. Admission requires ACTIVE identity-v4 under
[identity-contract](identity-contract.md). With #90 installed, read its mode before
the bounded prefix read and compare its exact revision after validation using a read-only
conditional transaction; require ACTIVE and report `"ACTIVE"` with positive revision.
LEGACY/ENROLLING, corrupt mode, quarantine/unsupported header or a changed mode refuses.
Mode absence is accepted only by the pre-#90 implementation, never as downgrade fallback
from an identity-aware server. Strict enumeration retains quarantined header candidates and
refuses them; it cannot silently remove affected volumes from the guard's universe.
In ACTIVE, also perform a bounded keys-only existence query on `store-quarantine/` (limit1,
key≤512 bytes); any retained quarantine record makes the whole strict request
FailedPrecondition. This deliberately conservative rule includes quarantined nodes/chunks
whose ordinary projections cannot expose their provenance. It needs no quarantine details
or maintenance RPC privilege. Reuse #76 ScanKeys when present; otherwise add the same bounded
keys-only primitive to both KV backends, never a whole-prefix value read/sort. Future explicit
quarantine resolution must remove/reclassify the active record before relaxing this rule.
For a supplied candidate ID, include its bounded exact never-deleted store-lifetime read and
conditional revision/absence comparison from identity-contract in this strict operation;
the final compare includes mode and history. Both bracketing enumerations carry that same ID.

## Collector and compatibility

`control/disruption/reader.go` exposes only ListNodes, strict ListVolumes, GetVolume and
ReadIdentityVolumePage. It does not call ListTombstones, Attach, DrainNode or any writer.
Each admission collects anew; no persisted eligibility bit or cache authorizes an eviction.
The collector has20s total,2s per RPC, four in flight,8192 total RPC attempts,4096 distinct
metadata nodes,4096 volume IDs,4096 Kubernetes nodes and64MiB aggregate encoded+retained
decoded data. Check budgets before allocation/append and count retries/failed pages too.
The existing4MiB full-RPC receive limit remains; page receive is512KiB. No unbounded worker
queue, goroutine fan-out or automatic restart on churn. An exhausted budget is Unknown.
The process shares one nonblocking collection semaphore between admission and reconciliation:
at most one collection, four metadata RPCs and64MiB application reader credits exist, even with
eight active HTTP handlers. A competing request holds `MetadataBusy`; it does not queue or
multiply the memory allowance. Noncollecting handlers retain only bounded request/API state.

Start with strict enumeration and ListNodes; validate duplicate IDs, lengths and node/store
fields. Obtain a complete #77 coherent GetVolume for each published ID. A listed ID returning
NotFound, malformed/partial response, TLS/RPC error or any unsupported profile makes this
whole attempt Unknown; a later admission may start a fresh attempt. Never copy #69 status
CLI's omitted-volume success rule into this admission reader. Repeat strict enumeration and
ListNodes after volume collection; require identical ID sets, identity mode/revision and
node lifetime/schedulability inputs used by the reducer. Churn refuses this attempt rather
than reusing earlier candidates. This catches observed changes; it is not an atomic snapshot
across volumes and cannot prevent a failure after observation. Existing replication/fencing
remains the acknowledged-write authority.

## Cardinality checks before application construction

Ordinary generated protobuf Unmarshal is **not used for guard responses**: grpc1.85.0's
size check precedes codec.Unmarshal, but protobuf1.36.12 can allocate millions of repeated
empty messages before the collector can reject their counts. Implement production
`control/disruption/boundedproto.go`, passed via per-call `grpc.ForceCodecV2` with Name
`"proto"`; request Marshal remains generated only after small request budgets validate.
Unmarshal handles exactly ListNodesResponse, ListVolumesResponse, GetVolumeResponse and
ReadIdentityVolumePageResponse. Other destination types or unsupported fields refuse.
Scan the supplied CodecV2 BufferSlice fragments directly; the application does not make a
second whole-message copy before checking cardinality.
This is guard-local and preserves ordinary server/client protobuf compatibility.
Keep this mechanism because S2 requires exact node/volume/chunk cardinality and typed-data
bounds before application construction: a valid-size repeated-empty-message input must be
rejected as Unknown without building millions of child objects or relying on an OOM restart.
It enforces the application's finite view, not a proof about stock-library heap ownership.

Before a call, reserve `R + 1MiB + 64KiB` application credits: bounded incoming message
length R, scan-plan capacity and outgoing request. R remains4MiB for full RPCs and512KiB
for pages. This is a work/owned-data admission budget, not complete accounting of gRPC's
private transport allocations. At most four calls share64MiB credits with retained decoded
views. A call that cannot reserve credit fails before invocation; no unbounded wait queue.
Release only application-owned data/credits after call/codec unwind. The separate actual
memory limit and restart behavior are [resource lifecycle](resource-lifecycle-contract.md).

### Stock transport and failure lifecycle

Use stock pinned grpc1.85.0 and its supported per-call4MiB/512KiB receive caps, plus
`grpc.WithMaxHeaderListSize(4096)` for received initial/trailer headers. Header-list size is
name+value+32 per field, not encoded HPACK size. Use a bounded shared metadata client set
(at most four configured endpoint ClientConns) and four total in-flight RPCs; no client per
volume/page or unbounded application retry fan-out. Preserve configured mTLS/peer checks.
Ordinary gRPC HTTP2 flow control, framing, padding, compression and buffer caches remain
library behavior. No exact-capacity pool, post-TLS frame parser, padding restriction, custom
static-window proof or dependency fork is part of this guard. The [public header option](https://github.com/grpc/grpc-go/blob/v1.85.0/dialoptions.go#L688-L704)
and receive limits provide input bounds; they do not promise whole-process memory usage.

Stock caches can retain incomingSettings arrays and cleanupStream closures pointing into
old streams/transports. A1025-byte DATA frame advertising R+1 can leave an unread root after
ordinary stream Close. No public cleanup/ConnEnd/Free event proves that every such reference
has gone, and the application neither tracks nor force-frees those internal roots. They count
against the verified256MiB cgroup memory.max/swap0 containment described by resource lifecycle,
with its documented kernel reclaim/OOM and temporary-overage semantics. The64MiB application
credit budget is explicitly separate from total stock-gRPC reachability or heap/RSS.

A metadata receive/transport/decode error, cancellation/deadline or application-budget failure
holds the request and initiates Quiescing→exit75 under resource lifecycle. New admissions stop;
occupied/unknown durable slots remain. No failed-call backing is released or credit reused
by inventing a Put callback. The replacement process has a fresh heap and must reload durable
state and make fresh complete observations. Recovery may cross a supervised restart; neither
same-process recovery nor a bounded restart delay is promised. Repeated failures may cause
repeated held drains/restarts, an explicit Proposed operational cost.

The one normal protocol exception is the defined #90 GetVolume FailedPrecondition directing
ReadIdentityVolumePage, with no protobuf response/codec invocation. That branch proceeds to
validated paging; an unexpected status/hint, received response or any page error still holds
and recycles. It cannot become a healthy/empty view or an authorization fallback.

Pass1 is a nonallocating wire scan over those fragments using fixed depth32 state, checked
varints/length arithmetic and a pre-reserved≤1MiB flat allocation plan. Reject malformed wire
types, groups, duplicate singular fields, unknown/uninstalled schema fields, more than262,144
field occurrences or32,768 nested message/map entries before constructing generated children.
Count every occurrence, including empty messages and packed scalars. Plan exhaustion refuses;
it never spills into an unbounded map/recursion. Unsupported fields are not retained unknowns.

| Response/submessage | Structural bounds checked before allocation |
|---|---|
| ListNodes | ≤4096 Nodes; each ID≤128 bytes, address≤512, labels≤128 entries, each key≤256/value≤1024; scalar lifetime fields |
| strict ListVolumes | ≤4096 IDs≤128 bytes each; mode≤16; installed mode/history/completeness scalars |
| full GetVolume | Exactly one Volume; checked RF1..5/B90 geometry; chunks≤RF-specific B90 maximum, never65,536 full rows |
| identity page | One header with no chunks; ≤128 IndexedChunks, exactly one ChunkMeta each; later semantic index validation |
| every ChunkMeta | replica/ISR refs each≤RF+1≤6; pending≤RF≤5; released≤1; ≤one each move endpoint; refs node≤128/UUID36; legacy projections empty |
| header/profile children | fenced refs≤16; snapshot catalog≤63; installed fields only; each string/opaque field≤4096 bytes and owning profile's stricter bound |

These also consume global message/field totals, including map-entry envelopes; wire-sized
but more expensive input may conservatively hold. Exact UUID/operation/LID/profile byte
lengths still apply. No opaque field gets an implicit unbounded allocation. Perform complete
existing semantic/schema/profile validation after these structural checks.

Pass1 calculates application credit before child construction: each concrete generated message costs
`align16(unsafe.Sizeof(type))+64`; every exact-capacity slice/byte/string backing costs
`align16(bytes)+64`; string maps reserve1024 bytes per entry plus charged key/value backing,
including empty entries (conservative for pinned Go1.27.4). Plan and transient reservations
remain charged. Atomically reserve the whole calculated amount from remaining process credit;
failure discards the response. Pass2 constructs generated structs **directly** with a bounded
decoder, exact-size slices and capacity-planned maps, then fills them. It must not call
generated Unmarshal/proto.Unmarshal/protojson or grow collections with append. Every
allocation uses the charged allocator; feature adapters cannot allocate past the plan.
These are conservative application charges, not a complete allocator/RSS model. Type/runtime
changes require charge-table regeneration and cardinality fixtures before use; actual process
containment is the separately verified cgroup, including stock allocations outside this model.

Transfer decoded charge to the retained candidate; wire/scan credit is released only after
the application codec/call completes and drops its owned references. Page-header duplication,
accumulation and all four call reservations count together. Error/cancellation drops the
candidate and initiates the defined process recycle; it never manufactures a complete view
or claims stock caches were purged. Only complete views publish eligibility.

## Exact-store adapter (#90)

ACTIVE requires Volume.identity_schema22=2 and every ChunkMeta.identity_schema27=2, supported
`identity-v4` and installed encryption/snapshot/DR profile validators. Legacy membership,
move/release/fenced projections must be empty; never fall back to them. Require the exact
incarnation `(first_epoch, legacy_incarnation)` under #90: positive first_epoch and empty
LID for native data, or first_epoch0 plus exact nonzero16-byte LID for proven legacy data.
Unknown, unsupported, unproven or quarantined input is Unknown for the entire attempt.

The canonical metadataID/Kubernetes-name join, saved-store set/generations, same-name UID
replacement, nonstorage class and old-store retirement are defined once in
[identity-contract](identity-contract.md). No address, hostname or label establishes an alias.
Refs are canonical `(node,device_uuid)` under #90's ID/UUID grammar. Count only earned
`isr_refs` which are valid placed members, are not in `initial_pending_refs`, and match that
node's observed current `store_uuid` and positive `store_generation`. Generation must remain
equal across the bracketing node observations. It is an observed lifetime predicate, not part
of earned ref identity: A→B→A may make original A present again with a fresh generation,
but cannot revive old sessions or grants. INITIAL/pending materialization is not a survivor;
a chunk with unresolved initial authority holds admission. Same-node multiple UUIDs never
count as two independent surviving members. Deduplicate by metadata node ID, validate its
unique Kubernetes Node UID mapping if present, then exclude the candidate, every consumed/
ambiguous admitted UID, metadata-dead/draining, Kubernetes cordoned or identity-unknown node.
External non-Kubernetes members count only from valid observed metadata store identities.
Installer supplies their exact IDs using repeated `--external-node NODE_ID` (unique,≤4096,
existing ID grammar), including fl-c in the historical lab. An ID must be either an enrolled
Kubernetes UID mapping or an explicitly external ID, never both; absence of a Kubernetes
object cannot silently turn a deleted/unknown Kubernetes node into an external survivor.
Unknown/conflicting mapping holds the pass. Merely absent Kubernetes objects do not mark
these explicitly configured external members unavailable.

Validate each active move's exact endpoints, operation and kind together; include source/
destination in affected-node detection even before destination placement. A destination,
released or fenced ref earns no survivor credit unless independently an earned exact ISR
member. Never infer earned data from a live node name or a new replacement UUID. For every
chunk affected by removing the candidate, remaining distinct qualified nodes must meet
minISR; contradictory/missing refs or invalid geometry makes the whole attempt Unknown.

## Full and paged coherent views

For normal full identity volumes use #77+#90 GetVolume, with all schema/profile validation.
For `paged_legacy=true`, ordinary GetVolume's defined FailedPrecondition directs the adapter
to #90's `ReadIdentityVolumePage`; no other FailedPrecondition gets a legacy fallback. Page0
uses volume ID, next_chunk0, limit128, zero discoverable incarnation/revisions and the exact
accepted encryption/snapshot/DR profile names. Validate its nonzero header/gate revisions,
incarnation, bounded header/catalog, geometry and contiguous indices. Later requests copy
the same first_epoch/LID/header revision/replica-gate revision and profile predicates.
Require byte-identical relevant header/profile summaries, identical revisions/incarnation,
exactly contiguous indices with no duplicate/gap, progress on every nonfinal page and complete
only at N. Server final read-only compare of header/gate/ACTIVE mode is #90's contract.
At most65,536 chunks/volume,512 pages at128 and512KiB/page; the stricter aggregate/call/time
budgets still apply, so a legal but expensive volume can conservatively hold admission.

Accumulate one complete candidate view before derivation; an error/Aborted/changed predicate,
missing index, wrong LID, mode change or premature complete discards the entire pass. Do not
splice pages from different attempts or raise gRPC receive limits. Full or paged observation
is never a bootstrap grant, snapshot handle, source proof or token.

## Authorization and implementation scope

#69 Reader URI is `spiffe://<trust-domain>/flaminio/reader/disruption-guard`. #91 owns the
whoever-lands-second change to its explicit `meta/service/auth.go` Reader allowlist: add
**only** ReadIdentityVolumePage alongside ListNodes/ListVolumes/GetVolume/ListTombstones when
the #90 method exists. Page request predicates retain the same read-only validation as
GetVolume. Reader must still be denied every present/future writer, token/grant issuer,
replica-grant/page/finish/cancel method, AttachIdentityVolume and replica network session.
An unsupported server/role returns a visible hold; never substitute admin/agent/io identity.

Implementation touches `api/proto/flaminio/meta/v1/meta.proto`, generated `api/gen`,
`meta/{service,kv}` and their tests, Go guard client and Rust `flaminio-metaclient` compatibility
bindings/interop, plus the per-method auth fixtures. Run `make proto contracts test race lint`
when implemented. No replication/format/epoch/ISR rule changes; no runtime check ran here.

## Required service-to-client fixtures

- `TestStrictListVolumesMalformedHeader`: Mem and real etcd, actual service through real gRPC
  into the guard collector; sole malformed header and mixed valid/malformed headers both hold.
  Cover invalid JSON/duplicate keys, key-ID mismatch, invalid geometry/flags/schema, backend
  error, valid Creating/Deleting omission and malformed claims/deleting records refusing.
- `TestStrictListVolumesBoundsAndCompatibility`: count/individual/aggregate boundary and+1,
  cancellation, backend transport overflow, old server complete=false, descriptor field tags
  and unchanged ordinary ListVolumes callers, ACTIVE quarantine outside the header prefix;
  no mutation and no partial success.
- `TestGuardIdentityAdapter`: healthy schema2 with empty legacy fields, alive replacement B
  while earned A is unavailable, A return with new G, A→B→A churn, duplicate physical-node
  refs, pending INITIAL, quarantine/unsupported mode and exact moving endpoint eligibility.
- `TestGuardPagedIdentityView`: full/paged equivalent derivation, FE0/LID, all pages coherent,
  changed header/gate/mode/LID, missing/duplicate indices, premature complete, late page error,
  aggregate/call/deadline exhaustion and no mixed-attempt or partial eligibility.
- `TestGuardPredecodeBoundsRealGRPC`: actual codec receives malicious wire from a real gRPC
  server: boundary/+1 empty repeated Nodes/IDs and nested empty Volume/ChunkMeta/ReplicaRef,
  IndexedChunk/header/profile/map entries. Rejection precedes generated child construction;
  instrument application-credit/owned-allocation high-water≤64MiB across four full/page
  receives plus a retained view, separately recording actual cgroup memory and restarts.
  Cover wire-length/field/depth/plan/+1, fragmented/supported compressed input,
  cancellation/unwind and valid exact-capacity response construction. A service-generated
  well-formed response alone does not prove the malicious-transport boundary.
- `TestGuardStockTransportAndRestart`: real mTLS gRPC/HTTP2 responses hit full4MiB/page512KiB
  and header/trailer-list4096 boundary/+1; four concurrent calls and application-credit
  exhaustion respect their caps. Exercise ordinary stock pooling,1025-byte/16KiB DATA,
  valid PADDED frames (including255 padding), queued/partial cancellation, malformed wire,
  codec rejection and R+1 advertised length in a1025-byte frame. Errors Quiesce/exit75;
  no custom protocol rejection, internal Free or fabricated cache-purge receipt is required.
  Cached incomingSettings/cleanupStream references and unread roots are contained by the
  actual cgroup controls. After four hostile attempts across supervised restarts, a healthy
  peer serves a fresh call with durable slot state unchanged. Include expected GetVolume
  page redirection without restart, and page/hint failure with restart. Record64MiB
  application credits separately from cgroup events/peak; do not infer whole64MiB stock heap.
- `TestGuardReaderMTLSAllowlist`: real certificates/service RPCs allow exact reads including
  the page method, descriptor-driven denial for all writers/grants and replica IO identity
  refusal; wrong CA/domain/server SAN/expired certificate cannot become an empty view.
