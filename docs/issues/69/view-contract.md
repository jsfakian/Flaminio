# Issue #69: observational view contract

**Proposed, unapproved R1 correction**, normative for [design](design.md) S2–S6. #69 owns
this status consumer/reducer, not #90 identity authority or #91 strict service/authorization.
The whoever-lands-second implementation installs these hooks; none is installed by this plan.

## Acquisition and installed adapters

`control/status.Client` exposes only ListNodes, ListVolumes, GetVolume, ListTombstones and,
when installed, ReadIdentityVolumePage. Collect uses four fixed workers, no unbounded queue,
30s total, at most32768 application RPC invocations (failures/pages included),4096 nodes and
4096 listed IDs; stock retry details are in the security contract.
All live application allocations and output share the [security](security-contract.md) cap.
No automatic restart/retry of the whole collection; service-internal #77 retries stay bounded.

Always send `ListVolumesRequest.require_complete20=true`, with candidate_metadata_id21 empty.
Require response complete20=true before accepting any ID, including an empty list. Consume
[#91 reader-contract](../91/reader-contract.md) strict candidate validation as its sole owner:
sole/mixed damaged headers, old-server default false and unsupported modes fail with empty
stdout. Valid Creating/Deleting omissions occur inside that complete service enumeration.
#69 adds no guard bracketing, candidate-history request, quarantine bypass or eviction rule.

Choose exactly one installed adapter from the strict response's identity_mode/revision:
`UNINSTALLED`/0 is the explicit pre-#90 legacy adapter; `ACTIVE`/positive is identity-v4.
An identity-aware server cannot downgrade to UNINSTALLED. LEGACY/ENROLLING, missing/unknown
mode, corrupt/quarantined or unsupported profiles fail. Validate supported encryption,
snapshot and DR profiles through their installed owning validators; no unknown-field fallback.
Validate node IDs/address/labels, unique IDs and UUID/generation pairs before retaining them.
An observed node may have no store (empty UUID/G0), or a canonical UUID with positive G.

**Legacy:** coherent full GetVolume uses #77, the installed legacy header/chunk validators,
node membership and move projections only. Do not invent UUIDs, positive first_epoch or
initial-pending state: schema1 renders identity/initial fields null for this adapter.

**Identity:** require Volume.identity_schema22=2, every ChunkMeta.identity_schema27=2 and
supported identity-v4; legacy membership/move/release/fenced projections must be empty.
Canonical exact refs and all move/set/profile rules come from [#90 contracts](../90/contracts.md)
and [core-design](../90/core-design.md), never legacy projections. Incarnation is positive
first_epoch with no LID, or first_epoch0 with an exact nonzero16-byte legacy_incarnation.
Validate canonical sorted unique refs, ISR subset of placed refs, distinct ISR node IDs,
RF1..5,1<=minISR<=RF, checked nonzero geometry and exact chunk count. Committed ISR below
minISR or contradictory move/set/operation fields are invalid metadata, not liveness states.
Use the owning validator for legal temporary RF+1 membership and same-node replacement refs.
Pending INITIAL is a placed/ISR subset with its exact operation, no move/released refs.

Full GetVolume uses #77+#90 coherence and B90 RF-specific limits (4MiB receive). Only #90's
defined paged_legacy FailedPrecondition redirects to ReadIdentityVolumePage; any other error
fails. Page0 sends volume, next_chunk0,limit128, first_epoch0, empty LID, both revisions0 and
exact accepted encryption/snapshot/DR profile names. Validate returned header/gate revisions
positive, incarnation/geometry/profiles and header with no chunks. Later requests copy exact
first_epoch/LID/header revision/replica-gate revision and profile predicates. Require identical
relevant header/profile summaries, predicates and exactly contiguous indexed chunks. Each
nonfinal page advances; complete=true iff next_chunk=N. Missing/duplicate/gapped/out-of-range
indices, empty nonfinal page, changed predicates or late failure discard that whole volume
attempt and fail collection. Never splice attempts. #90's final header/gate/ACTIVE compare
is required on every page. At most65536 chunks,512 pages at128 and512KiB receive/page;
aggregate/time/attempt/credit limits may refuse an otherwise legal large view.

A listed volume whose named/full/page read returns NotFound is one bounded identified
omission; discard every earlier page for it. It is not an empty volume. All other errors,
including Aborted, refuse the entire command. One returned same-name incarnation is shown
as actually read; enumeration and liveness still are not an aggregate snapshot.

## Exact schema1 nested fields

Top-level fields are `schema_version:1,started_at,finished_at,snapshot:false,nodes,volumes,
tombstones,omitted_volumes,unsupported_fields,tombstone_coverage`. Arrays are always arrays,
including empty ones; nullable values are JSON null, never fabricated zero. Integers are
canonical decimal JSON integers with checked arithmetic; timestamps are UTC RFC3339Nano.
No raw token/key/proto unknown fields enter the View. Objects have exactly these fields:

| Object | Fields |
|---|---|
| Ref | `{node,device_uuid}`; UUID null only in legacy node-ref adapter |
| Incarnation | `{first_epoch,legacy_incarnation}`; LID null for positive epoch; both null when legacy adapter cannot establish them |
| Node | `{id,address,labels,alive,draining,store_uuid,store_generation,totals}`; store fields null when uninstalled/absent |
| Node.totals | `{placed_copy_count,earned_isr_copy_count,initial_pending_copy_count,served_volume_count,complete,scope:"listed_volume_reads"}`; initial count null in legacy |
| Volume | `{id,identity_profile,incarnation,size_bytes,chunk_bytes,replicas,min_isr,epoch,target,chunks,under_replicated_chunks,initial_pending_chunks,redundancy,availability_hint}` |
| Volume.target | null or `{node,liveness_hint:"alive"|"dead"|"unknown"}` |
| Chunk | `{index,isr_ver,replica_refs,isr_refs,initial_pending_refs,released_refs,move,committed_isr_count,earned_isr_count,planned_initial_count,redundancy,availability}`; initial fields null in legacy |
| Chunk.move | null or `{from_ref,to_ref,kind,operation}`; kind=`move`/`replacement`; legacy kind=`move`, operation=null |
| Chunk.availability | `{known_alive,known_dead,unknown,hint}`; hint=`at_least_min`/`below_min`/`unknown`/`blocked_initial` |
| Omitted volume | `{id,reason:"not_found_during_collection"}` |

`identity_profile` is `legacy-node-v1` or `identity-v4`; `redundancy` is `full` or
`under_replicated`. Legacy released_refs is [] only when the installed profile establishes
absence; otherwise unsupported_fields names `legacy.released_refs` and value is null.
Legacy initial_pending_chunks, Node.totals.earned_isr_copy_count and Chunk.earned_isr_count
are null: this profile cannot establish earned versus planned initial copies. Its availability
counts validated committed ISR node observations but hint is unknown; it never asserts an
initial copy earned. Identity availability uses earned refs as defined below. Exact tombstone
fields/coverage are defined in
the [inspection contract](tombstone-inspection-contract.md). Unsupported_fields always names
disks,physical_capacity,rebuild_progress and any unsupported legacy fields; no zero substitutes.

## One pure reduction

For identity chunks, C=number of committed ISR refs, P=number of pending INITIAL refs and
E=number of ISR refs excluding pending refs. Publish C,E,P separately. Committed redundancy
is under_replicated iff C<RF, otherwise full; it never compares placed-copy RF+1 with ISR.
This is [#71's existing under-replicated meaning](../71/contracts.md): a volume counts once
iff any chunk is under-replicated. Pending full planned ISR can be committed-full while its
initial_pending_chunks>0 and availability is blocked_initial; never label it earned healthy.

Classify every earned exact ref once: known_alive iff its registered node has alive=true,
matching current UUID and positive observed generation; known_dead iff that observed node
has a different valid current UUID, or matching UUID and alive=false; unknown if the node
is absent or has no observed store. Generation is an observation predicate, not membership:
original A returning at fresh G can be observed alive, without reviving old sessions/grants.
Node draining does not change these liveness hints. Same-node copies never add two ISR votes.
Legacy committed ISR uses node alive/dead, absent node unknown, without UUID/earned claims.

For identity-v4 let A,D,U be those counts (A+D+U=E). If P>0, hint=blocked_initial. Otherwise if A>=minISR,
hint=at_least_min; else if A+U<minISR, hint=below_min; else hint=unknown. For RF3/min2,
1alive+1dead+1unknown is unknown;2alive+1unknown is at_least_min. These are hints, not I/O
permission or assurances against failures after collection. An empty target is null; a
registered target's flag gives alive/dead, an unregistered target is explicitly unknown.

Volume counts under-replicated and pending chunks exactly; redundancy is under_replicated
iff any chunk is. Availability reduces chunks in priority below_min,blocked_initial,unknown,
at_least_min (first present); retain every chunk/count so mixed reasons stay visible.
Each registered node counts placed refs by exact copy (two UUIDs mean two copies), earned
ISR refs and pending refs separately, and one served_volume per observed volume targeting it.
Released/fenced refs are not placed holdings or earned ISR and add no such count. These are
recorded-placement observations, not physical disk/capacity totals. Missing-node refs remain
on chunks with unknown hints; they do not synthesize a registry node.

If omitted_volumes is nonempty, every node totals.complete=false: counts exclude exactly
those named omissions and are observational lower bounds. Otherwise complete=true only for
listed_volume_reads, never all volumes at one time. No omission becomes healthy/zero.
Sort nodes/volumes/omissions by byte ID, chunks by index, refs by(node,UUID), labels by key,
unsupported fields by name; tombstone order follows its contract. Human output consumes only
this View, including nulls, counts, reasons and the snapshot/coverage disclaimer.

## Future fixtures (not executed)

Pure table tests cover RF/min/geometry/set/move invalidity, temporary RF+1, mixed chunk
reductions, both threshold examples, unknown target, no/all/some omissions, observed totals,
empty legacy projections, alive B/earned A, A return with fresh G and pending INITIAL.
Service-to-status tests cover sole/mixed bad strict-list headers, valid Creating/Deleting,
old-server complete=false and valid empty cluster; full/paged equivalence, FE0/LID, churn,
wrong predicates, missing/duplicate/gapped pages, premature completion and late failure.
Golden human/JSON cases derive from the same View and no error publishes stdout. Use actual
Mem/etcd service and generated ABI/auth fixtures, not fake-client success alone; implementation
runs `make proto contracts test race lint`. No tests/runtime ran during this draft.
