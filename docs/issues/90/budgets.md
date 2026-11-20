# Issue #90: composed representation and capacity budgets

Proposed, unapproved; normative [contracts](contracts.md) appendix. This replaces the
inherited instruction to let #80 tighten caps later. #13/#51/#58/#72/#76/#79/#80/#81 stay
read-only; the last implementation to land installs this composition and boundary fixtures.

## One full-volume envelope

Keep the actual Go/Rust/CSI gRPC receive limit4,194,304 bytes. IDs<=128 ASCII bytes; canonical
UUID strings exactly36. `ReplicaRef` protobuf payload is at most169 bytes: node131+UUID38.
A ref in a field numbered20..31 takes173 bytes including its2-byte tag and2-byte length;
use176 as its conservative allowance. Identity-enabled responses omit every legacy node
projection, including released/fenced. This is required for the formula, not an optimization.

A chunk in its largest move state holds2(R+1) replica/ISR refs,2 endpoints and1 release;
without a move,2R+2 refs is smaller. INITIAL adds at most R pending refs to2R
replica/ISR refs with no move/release;3R<=2R+5 for admitted RF1..5, so the same bound holds. All fixed fields (version/schema/operation/kind), enclosing
lengths and indexed-page wrappers fit128 bytes beyond these refs. Thus use
`704 + 352*(R+1)` bytes/chunk: it exceeds `128 + 176*(2*(R+1)+3)` by48.
Fixed32,768 bytes cover: basic header and #79 initialization/use metadata4096; token/ticket
including protobuf framing4112 (max token4096);16 fenced refs2816; #51's63-entry catalog plus
generation12,549; #58 DR descriptor107; FLCH response allowance138; remaining8950 bytes cover response wrappers and the
bounded extra Snapshot/Incoming/Promotion summary in feature Attach/commit responses.
No manifest pages, root maps, proofs per chunk, unbounded diagnostics or separate operation
history are embedded in a Volume response; their existing separate bounded RPCs remain separate.

The shared conservative admission contract for every new identity profile is exactly:

```text
B90(N,R) = 32768 + N * (704 + 352 * (R+1)) <= 4194304
RF R             1       2       3       4       5
chunk term       1408    1760    2112    2464    2816
maximum N        2955    2364    1970    1688    1477
```

Use checked uint64 arithmetic, R validated first; scalar #80 geometry remains unchanged.
New create/growth admission uses B90 even with no snapshots/roots yet, so later supported
feature metadata does not invalidate the envelope. #51 snapshot requests also retain their
geometry/encryption/retention checks. #58 root per-session chunk bound4096 still applies.
Worst-view fixtures at all five caps include maximal IDs/UUIDs/versions, temporary R+1 sets,
a move plus release,16 fence holders,63 snapshots, initialization, DR descriptor and max ticket;
Create/Get/Attach/ChangeIsr and feature wrappers must each satisfy proto.Size<=B90<=4MiB.
Boundary+1 refuses before provisioning/epoch mutation. No test-only transport-limit increase.

## Existing volumes: bounded maintenance and paged operation

A formerly supported scalar-valid volume can exceed B90 after identity fields are added.
[Migration](migration.md) enrolls it as `paged_legacy=true`; it is never silently stranded by
activation. This applies only to an immutable existing inventory entry, N<=65,536 and valid
old scalar geometry, not new provisioning. Its pending snapshot/cut/creation operations must
finish under their existing supported implementation before the maintenance barrier/inventory;
uncertain operations are resolved, not discarded; an unresolvable affected volume and its charges
remain explicit quarantine under migration.md. Existing committed snapshots/roots are
retained and verified. Activation requires the following readers/Attach adapter installed.
Growth or a new snapshot/DR reservation which would exceed B90 is refused; existing committed
views, ordinary I/O, takeover, authorized replacement/move and exact deletion remain operable.
A replacement/move additionally respects root Q<=4096 and all normal replica/release bounds.

New read-only `ReadIdentityVolumePage` is open to authenticated readers as GetVolume. Messages:
- Request: string volume1; uint64 first_epoch2; uint32 next_chunk3,limit4(1..128);
  int64 expected_header_revision5,expected_replica_gate_revision6;
  string accepted_encryption_profile7,accepted_snapshot_profile8,accepted_dr_profile9;
  bytes legacy_incarnation10(empty or16).
- Response: Volume header1(with chunks empty); repeated IndexedChunk chunks2;
  int64 header_revision3,replica_gate_revision4; uint32 next_chunk5; bool complete6.
  `IndexedChunk {uint32 index1; ChunkMeta chunk2;}`.
First page requires next_chunk=0 and revisions=0; first_epoch=0 may discover the current
incarnation. It reads mode, header and gate, then at most128 contiguous current chunk records,
then compares header/gate/mode with an empty-operation transaction. Missing/malformed rows
refuse, never skip. It returns exact first_epoch/header/gate revisions, summary/catalog and the
next contiguous index. Subsequent pages require all returned incarnation/revision predicates including legacy_incarnation when first_epoch0,
read current rows and perform the same final compare; zero/mixed predicates are InvalidArgument,
a changed/missing predicate is Aborted, never an automatic newer page. Final complete=true only
at N after its final3-compare validation. Revisions are predicates against current state, not
caller-selected historical KV reads. No cursor/snapshot registry or unbounded server session is
needed. Never-deleted gate plus revision CAS prevents change-and-revert ABA across pages.
Every page includes the same bounded header/catalog and has B90(128,5)=393,216 bytes maximum,
under a512KiB per-page cap. Target accumulates exactly N rows by index, validates geometry,
refs/profiles and identical predicates before installing anything; a failed page discards the
candidate view. Any authority mutation after the last page follows normal epoch/version/core
fencing; paging itself is read evidence, never permission to bootstrap or join.

New `AttachIdentityVolume` uses the same authorization and conditional Attach logic, including
#79 use/initialization checks and `unless_target`/`only_if_target`, but only for paged_legacy.
Request fields: string volume1,node2; bool unless_target3,only_if_target4; UseProfile use_profile5;
uint64 expected_first_epoch6; string accepted_identity_profile7(exact `identity-v4`),
accepted_encryption_profile8,accepted_snapshot_profile9,accepted_dr_profile10;
bytes legacy_incarnation11(empty or16, required for first_epoch0),target_nonce12(empty or32 nonzero).
Response: Volume header1(chunks empty); bytes token2; int64 header_revision3,replica_gate_revision4;
bytes target_handoff5(empty or exact134).
It validates scalar geometry, paged flag and all feature capabilities before changing epoch,
compares header/tombstone absence/gate/mode and #79's existing use-authority predicates, then
returns the committed summary/token. Compose #83 on this same transaction: a nonempty nonce
must be exactly32 nonzero bytes, nonce+unless_target is InvalidArgument before mutation, and
purpose1 FLCH134 binds that nonce and SHA256(exact returned FLCTv2/v3) only after an actual
new-epoch CAS. Empty nonce returns no handoff and cannot start a strict daemon. Auth/use gates
are unchanged; no node/io issuer or old-epoch read-only handoff mint. Strict callers fsync nonce/
intent and returned epoch before daemon attach. This additive handoff uses the existing fixed
32KiB allowance; header/gate field numbers3/4 remain unchanged. It returns no token if the existing initialization rules
forbid one. The target starts page0 pinned to those returned nonzero predicates (the one
exception to discovery's zero predicates), assembles the full view and only then opens sessions.
Lost Attach reply means no daemon attach was sent: retain intent and obtain a fresh OnlyIfTarget
epoch/handoff. Page conflict discards the candidate before target start and requires another
fresh Attach. Partial pages never start I/O. The exact normal/paged local target, FE0/LID
device tuple and CSI restart contract is [local-target-contract.md](local-target-contract.md).
Normal full-volume Get/Attach on these records returns FailedPrecondition naming these APIs;
no malformed empty chunk list is handed to an old client. This path performs no resize, split,
format, new metadata receive override or freshness inference. Maintenance can inspect/delete
unsupported raw records even when scalar validation fails; such records are not reclassified as
previously supported. The explicit paged path is the implementation obligation of #90.

## Reports and replacement charges (#72, #51)

Keep #72's sole per-node ledger `capacity/<node>` and its checked Room/releaseCharge/InitCapacity
rules. Do not create a second per-store admission authority. Count copies by exact ref before
aggregating by node: placed refs, pending destination not yet placed, released refs and one slot
per exact fence holder. Two UUIDs on one node cost two copies. A chunk j costs
`(m(j)*chunk_bytes,m(j) slots)`, where #51 m(j)=1+snapshot_count, less1 for chunks at/after the
current snapshot_charge_next. The compared header supplies m(j). A completed replacement moves
oldRef into released_refs without releasing its charge; failed/resumed resync neither doubles
the pending destination charge nor cancels the old one. Future snapshots charge the exact-copy
union, aggregated into the same node ledger, preserving #51's bounded charge cursor.

ReplaceReplicaStore reads placement-gate before destination node/report, then current header,
chunk, ledger, replica-gate and ACTIVE mode. New ref/G must match the node and its valid capacity
report. It requires Room(ledger,report,HardPPM=950000,chunk_bytes)>=m(j), so both
byte and slot inequalities cover the full m(j) charge (checked multiplication),
compares all7 keys and writes pending operation/chunk, increased ledger and gate atomically.
Missing report/readable ledger is FailedPrecondition; insufficient Room ResourceExhausted; a
compare conflict rereads/rechecks, at most32 attempts then Aborted. Snapshot charge, create,
move, InitCapacity and exact release serialize on their common ledger/header/gates. No charge
is forgiven because the old disk is inaccessible, smaller or has a different UUID. A smaller
replacement can consequently lack Room; choose another destination or safely clean retained
refs. This is visible conservative accounting, never inheriting the old disk's capacity budget.

`CapacityReport` appends string store_uuid20; uint64 store_generation21. Stored alive value is
schema2 with all #72 figures plus those fields. Heartbeat authorization remains #45 own-node IO
or administrator; agents are PermissionDenied even without a report. Reports are own-node IO
only, exact current UUID/G; mismatched/delayed identity is FailedPrecondition with no write.
Administrator heartbeat supplies no report. Invalid figures preserve #72's no-report value1.
The request without a report still appends store_uuid20/store_generation21, requiring the IO's
actual opened lifetime; an administrator may omit these and renew the read current node lifetime.

Use #45's multi-operation `KV.Txn` with optional `Op.TTL`, not a new single-put TxnTTL API.
The one heartbeat transaction compares node, node-state (absence included) and identity-mode;
it puts alive with TTL and optionally records Missing→Active/ever_heartbeat, preserving Failed.
UUID/G must match that compared node; a registration change atomically updates node/lifetime/
placement-gate and deletes alive, so old heartbeats cannot reinstall liveness or capacity.
#45 Fail/Admit/Forget races remain ordered by node-state/alive comparisons and preserve its
Failed admission policy. No heartbeat renews another node or promotes Failed to Active.

Keep #45's etcd lease contract exactly: use cached lease, retry with a fresh lease only on
ErrLeaseNotFound, and renew after commit. Register (lease,committed ModRevision) under the key
only after a committed TTL put; replace cached registration only with a newer committed revision,
so overlapping answers cannot restore the older lease. Failed CAS leaves old registration and
revokes only a lease allocated by that attempt; renew never republishes an absent key. Mem applies
TTL put plus optional state write atomically under its mutex. NodeCapacity exposes only reports
whose UUID/G matches the node. InitCapacity retains #72 marks/prev/release races and reads the
bounded tombstone root/ref pages: a BUILDING root is ignored while its frozen
Deleting header/sources supply the charge; only after atomic header removal/PENDING publication
do pending ref rows supply it. CONFIRMED rows contribute zero. #72 InitCapacity marks still
block new charges and make any raced release invalidate its final ledger write. Replace its
full List(v1/) with bounded per-volume scans: classify header/root once, stream source pages
and sum by node, then compare the same header/root/gate/mode before accepting that volume total.
A phase/gate conflict discards/retries only that volume (<=32 attempts); no header plus its
tombstone is counted together. Enumerate names/pages with bounded ScanKeys, not a whole-map
snapshot; current node totals are bounded by existing node-ledger enumeration limits.
#45/#72/#90 last-to-land hook installs this exact transaction/lease/authorization combination.

Creation's capacity-claims schema2 keeps its aggregate charges and adds
`ref_charges:{node:{uuid:{bytes,slots}}}` from #76's immutable exact-ref plan; sums equal node
charges. Tombstone creation takes these exact claims on interrupted creation. No new disk's
report or aggregate node key can identify an old claimed copy. Missing exact legacy claims
block initialization/enrollment until resolved by the explicit inventory procedure.

## Cleanup storage, transport and migration envelopes

The bounded root/ref/index schema in contracts.md supports <=458878 distinct exact refs for
N65536/RF5; no list or transaction materializes that union. Root<=4096, row<=512, index<=128
encoded bytes, key<=512, and one build mutation <=64KiB encoded TxnRequest including compares,
keys, values and protobuf overhead. Service rejects an over-budget constructor before submission;
KV tests enforce the same check in Mem/Etcd. The supported etcd MaxRequestBytes is1,572,864;
new #90 transactions stay<=512KiB, cleanup<=64KiB, leaving backend overhead headroom. This does
not require a raised server quota or RPC limit. At most one8KiB source chunk plus7 row/index
pairs and one4KiB root is retained per build step; parsed-object allocation is bounded separately.
A maximal row includes its128-byte node,36-byte UUID, maximal uint64 charges/state and fits512.

ListTombstones performs one keys-only index scan<=32 keys/32KiB, then <=32 root reads of4096
and32 ref reads of512, plus its node/mode/revision checks: <=68 backend calls/call. It returns
<=32 summaries of512 bytes plus<=1024-byte cursor and protobuf overhead, hard32KiB response;
response producers stop before count or byte cap (whichever first). Transport uses real4MiB
limits; each root/ref GetBounded has max4096/512 and per-RPC cap payload+16KiB. An unpublished
BUILDING root consumes the same work budget but returns no cleanup authority. Many volumes or
one legacy volume's hundreds of thousands of ref rows cannot force a larger value/message.

Existing legal monolithic legacy tombstones/claim values are the sole bounded source-adapter
exception: GetBounded<=1,572,864 bytes, per-RPC receive<=1,589,248 (value+16KiB), one raw buffer
per service; no whole-map/array JSON unmarshal. A streaming parser projects at most32 typed
node/charge entries into a<=16KiB inventory page, binding source key+revision+SHA256+field-kind+
entry ordinal. Continuation may reread and parse the same frozen bounded source; it never
claims etcd can read a byte range within one value. Identity-mode ENROLLING excludes source
writers; each projection/conversion validates source revision/hash and operation. Build shadow
root/ref rows under a fresh deletion operation, persist source page cursor, and replace the old
root only once every node and charge projection is reconciled and exact totals verified.
No giant source item/binding/Txn is required. Unknown identity retains conservative quarantine/
charge; source beyond the supported backend envelope is explicit unsupported/corrupt status,
not truncation or an asserted valid16KiB object. This also covers the existing64KiB bounded
initial claim record. Native schema3 migration inventories root and ref rows independently.
Migration correction/verification uses the same pages/cursors, as migration.md defines.

## Combined transaction limits

All rows fit120 comparisons and120 writes. Counts include each unique key once; absent compares
count too. All live authority rows include ACTIVE identity-mode and a compared/bumped replica-gate;
ENROLLING uses migration's exact barrier. Placement-gate covers multi-node UUID eligibility
reads, avoiding one extra Node comparison per new ISR member. ForgetNode's final mark is
permanent; it never grants cleanup/source authority and never deletes store-lifetime/gates.

| Operation | Comparisons | Maximum writes / bound |
|---|---|---|
| ConfirmRelease b | header+b chunks+ledger+node+gate+mode = b+5 | b chunks+header+ledger+gate <=b+3; b<=115 |
| Tombstone mark | header+root absent+gate+mode =4 (plus existing tenant/reference protection keys) | header+root+gate =3 (plus existing protection writes) |
| Tombstone build one chunk, t<=7 refs | header+root+source chunk+t rows+t indexes+gate+mode =5+2t<=19 | root+t rows+t indexes+gate =2+2t<=16; <=64KiB |
| Tombstone publish | header+root+gate+mode =4 | header deleted+root+gate =3 |
| ConfirmDeletion | root+ref row+ledger+node+gate+mode =6 | root+ref row+ledger+gate <=4 |
| Tombstone reclaim b<=32 pairs | header absent+root+b rows+b indexes+gate+mode =4+2b<=68 | root+b row deletes+b index deletes+gate =2+2b<=66 |
| Tombstone final root delete | header absent+root+gate+mode =4 | root delete+gate =2 |
| Heartbeat (#45) | node+node-state+mode =3 | alive TTL+optional node-state =1..2 |
| Forget released batch b | header+b chunks+gate+mode = b+3 | b chunks+gate =b+1; b<=117 |
| Forget fence-holder cleanup | header+gate+mode =3 | header+gate =2 |
| Forget tombstone ref waiver | root+row+gate+mode =4 | root+row confirmed+gate =3; no usable-node ledger credit |
| ReplaceReplicaStore | header+chunk+node+ledger+placement-gate+replica-gate+mode =7 | chunk+ledger+gate =3 |
| MoveReplica | header+chunk+at most R+1 #18 node records+ledger+placement-gate+replica-gate+mode <=12 | chunk+ledger+gate =3 |
| CompleteInitialReplicas b<=32 chunks | header+b chunks+node+gate+mode =b+4<=36 | b chunks+gate =b+1<=33; no ledger change |
| ChangeIsr b (#81) | header+b chunks+placement-gate+replica-gate+mode =b+4 | b chunks+gate =b+1; b<=64 |
| #76/#78 creation claim, k nodes, tenancy active | sealed root+header absent+tombstone absent+placement-gate+replica-gate+mode+k ledgers+authorization+owner absent+policy+quota =10+k | header+root+claim+gate+k ledgers+owner+quota =6+k; k<=110 |
| #76 admitted public copy/cleanup b | claim/header+root+replica-gate+mode =4 | b chunks+root+gate <=b+2; b<=32 |
| #51 charge b chunks,d nodes,last batch | header+b chunks+d ledgers+snapshot+placement-gate+replica-gate+mode =b+d+5 | d ledgers+header+snapshot+gate =d+3; b+d<=115 |
| #51 CommitSnapshot | header+snapshot+replica-gate+mode =4 | header+snapshot+gate =3 |
| #72 InitCapacity mark b nodes | b ledgers+mode =b+1 | b marks; b<=119 |
| #72 InitCapacity write | ledger-own-mark+mode =2 | ledger =1 |
| Identity page final check | header+gate+mode =3 |0 |

Snapshot nonfinal charge omits snapshot compare/write but keeps the same safe bound. Uniform
MaxPlacementNodes becomes110, so a tenant activation cannot invalidate a claim's fixed cap.
A permanent ForgetNode may remove refs without a physical deletion proof; its node is forever
ineligible and its old ledger is not credited for a usable replacement. Normal confirmations
are the sole automatic release path. #70 growth keeps its longest-prefix batching algorithm:
add2 comparisons and1 write for mode/gate, and retain placement-gate for store eligibility;
count all quota/ledger/chunk keys before choosing the prefix. At minimum one chunk fits.
Other existing authority transactions (including deletion/#79 initialization/#58 root barriers)
add exactly the missing mode/gate compares and gate write to their existing key sets; if they
read destination UUID/report eligibility they also compare placement-gate. Each constructor
checks actual deduplicated counts before Txn; bounded batch producers shrink before submission (enrollment uses the exact prefix of at most16
whole inventory items fitting both120 limits, as migration.md defines),
never send an oversized Txn and hope the backend retries it. Deletion post-header cleanup is
bounded<=117 keys with header absence+gate+mode, <=118 writes including the gate,
except the exact delete-only unpublished/orphan paths below.

## Delete-only NOSPACE exception (#76)

Etcd3.7.3 rejects a transaction containing any Put under NOSPACE. Preserve #76's exception
only for these proof-scoped metadata deletes, with no progress/tombstone/gate/ledger Put:

| Eligible action | Exact comparisons | Deletes |
|---|---|---|
| Proven unpublished rollback | exact header revision+existing tombstone absence+identity-mode =3 | only that header1 |
| Eligible private create-root revocation/GC | exact root revision+observed header revision/absence+mode =3 | root1 |
| Private-plan orphan batch b<=32 | root absence+b listed key revisions+mode =b+2<=34 | b keys |
| Public-chunk orphan batch b<=32 | header absence+b listed key revisions+mode =b+2<=34 | b keys |

Rollback requires #76's recognized monotone unpublished shape: Creating, epoch0, empty target,
matching observed operation (or the exact supported legacy shape under ENROLLING writer
exclusion); new-format cancellation is already Deleting or an explicitly authorized scoped
DeleteVolume. All existing protection/reference/authorization checks remain. Never classify a
published first_epoch0 legacy volume by zero epoch alone. Any unknown/nonzero-epoch/target-bearing
shape takes conservative published deletion. A changed comparison returns changed/Aborted;
never reread a replacement and delete it by name. Header removal irrevocably prevents that
claim's late publication. Root eligibility stays #76: complete with matching visible header;
preparing/sealed unchanged twice with no admitted matching header; or admitted whose header is
absent/Deleting/another operation. Unknown evidence is ineligible. Private root deletion revokes
all later staging/admission because they compare its positive revision; permanent create-birth
CAS prevents resurrecting the operation. Orphan deletes compare every selected key revision,
so a same-name header absence ABA cannot delete a newly written chunk/plan. Discover keys with
#76's bounded keys-only scans; retain its32-key batches, work budgets and unconsumed-tail rules.

No grant depends on these deleted private manifests: INITIAL authority after publication is
its explicit durable pending-ref state in the chunk/header, not retained create_operation or
private root existence. Creating/absent headers cannot authorize a grant at Open/Finish;
removing an unpublished header invalidates its exact header comparison. A fresh same-name claim
has a different header/incarnation and its ordinary gated admission still compares mode/gate.
Thus these deletes cannot revive or invalidate live earned authority, and need no gate Put.
Published chunk/ref changes, initial earning, releases, tombstone phases/confirmations, ledger
changes and materialized-store cleanup remain normally gated; this is not a global cleanup
exemption. The exception is used regardless of whether NOSPACE is currently raised, so no
quota observation can race the safety classifier.

Planned real embedded-etcd3.7.3 NOSPACE regression first proves a control with gate/progress Put
is refused, then executes eligible rollback, root revocation, private/public orphan batches
unchanged under the alarm. Inject lost replies/restart, late publication/authorization, root
birth replay and same-name recreation between read/delete; no published data/authority may
change. Mem replays the same transactions. Independent model assertion NoLiveAuthorityGC
compares published/materialized incarnation history against every exception deletion; mutation
`identity_gc_weak_classifier` skips the Creating/operation/key-revision proof and must be caught
by the published-legacy0/late-recreation schedule (clean counterpart passes). Existing #76
NoRootResurrection/missing_birth_compare and NoUnprovedTombstoneBypass mutants remain required.
Compaction/defrag/alarm disarm occur only inside that isolated future fixture, never shared lab.

Planned both-backend boundary tests fill every table row, including115/116 confirmations,
117/118 Forget batches,119/120 Init marks,110/111-node claims with tenancy and #81's64/65.
Cleanup fixtures include maximal root/row/index key and byte caps, 32/33 summaries, cursor1024/
1025 bytes, exact32KiB/+1 response/64KiB/+1 build rejection, maximum458878-ref legacy union
streamed in bounded batches, repeated UUID replacements and many pending volumes for one ref.
Both-backend tests assert bounded keys/value bytes/visits/retained pages, source-adapter1.5MiB
boundary/+1, interrupted build/publish/confirm/GC and phase/name-reuse races. Real Go/Rust
production transports consume every page without increased limits; lost replies never double
release and a newer root/ref revision forces relist. Heartbeat tests on both backends cover
Missing→Active, Failed preservation, Fail/Admit/Forget races, agent refusal, delayed old UUID/G,
invalid reports, lease expiry, failed TTL CAS and overlapping commits answered in reverse order.
Production sweep producer emits115+1 for116 chunks, model adapters use the same constants.
Capacity races cover replacement/create/move/snapshot charge/InitCapacity/release, exact old-A/
new-B charges, repeated operations, larger-old/smaller-new reports, delayed A heartbeat after B
registration, and snapshot m(j) at both sides of a charge cursor. Proto-size/Go-Rust transport
fixtures and coherent paged-legacy migration/attach tests use production limits. No runtime or
lab check ran for this draft; `make proto contracts test race lint sim` are implementation checks.
