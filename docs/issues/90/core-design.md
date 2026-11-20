# Issue #90 — bootstrap, replacement and executable proof

Normative proposed appendix to [design](design.md); **unapproved, not implemented**.
[Public types/wire](contracts.md) · [wire](wire-contracts.md) · [budgets](budgets.md).
This appendix owns bootstrap RPCs/KV interfaces; do not infer them from the inherited #61 ABI.

## Pure bootstrap machine

Add `Bootstrap` to `flaminio-repl/src/replica.rs`, invoked for remote **and synthetic local**
Hello. `BootstrapId=(process_generation,connection_id,attempt)` is nonreused checked u64s;
overflow closes admission. `SessionRef` adds exact receiver StoreId/store_generation, volume
first_epoch, legacy_incarnation (empty or UUID16), epoch, session and BootstrapId. Each store callback and reply carries that identity
plus action ID; no callback routes by node name or connection ID alone.

States: `Absent`, `Authorizing`, `Cleaning`, `Fencing`, `Creating`, `ConfirmingInitial`, `Ready`, `Cancelled`.
Inputs and emitted actions are concrete:

| Input | Guard/transition and action |
|---|---|
| `HelloAccepted(id,peer,claims,expected_ref,generation,sorted_chunks,profile)` | Validate identity/shape/token locally and opened store/ref first. Reject missing/changed identity before any store call. Supersede any lower `(epoch,session)` candidate, cancel old action IDs, then `Authorizing`; emit `OpenGrant(id,…)`. Never activate merely because TLS/token passed. |
| `GrantPage(id,grant,page,rows)` | Only matching authorizing attempt; accumulate exact immutable allowed roles, consume reserved bytes and request next page. Duplicate page must byte-match; unexpected/missing rows refuse. No store action. |
| `GrantFinished(id,proof)` | Check complete rows and digest against Hello, exact first_epoch/epoch/ref/lifetime/profile and local durable fence not above epoch. Positive-incarnation profiles enter `Cleaning` and emit `DeleteEarlierIncarnation(id,first_epoch)` only for chunks below first_epoch. Legacy profiles3/4 require exact signed/header LID and enter `Fencing` directly, never deleting preserved FE0 data. Existing higher fence refuses before any mutation. |
| `CleanupDone(id,result)` | Matching success enters `Fencing`; emit `PersistFence(id,first_epoch,epoch)` through #13 retained-volume-fence adapter, including the empty-volume case. Error closes. Cleanup occurs before fence raises old chunks' epochs, preserving #30's bounded-delete predicate. |
| `FenceDurable(id,epoch)` | Require matching current action, exact identity and durable epoch; enter `Creating`. Missing MEMBER chunks refuse (data-loss/corruption), never become empty. Emit bounded `CreateAuthorizedChunk` only for INITIAL/MOVE_DEST/REPLACEMENT_DEST rows. |
| `ChunkDurable(id,chunk,role,operation)` | Verify matching granted row and current action before adding the chunk to core memory; after every creation succeeds, INITIAL rows enter `ConfirmingInitial` and emit `CompleteInitialReplicas`; they cannot emit FencedIdentity or credit source/ack/lease. Other rows enter `Ready`, publish session and emit FencedIdentity with exact ref and the preexisting uncertain sets. A destination's absent/empty map is explicitly destination state, never earned ISR state. |
| `InitialCompleted(id,result)` | Only matching successful completion may consume local pending state; close with `Refused("initial_complete_reopen")` and require new Hello/session. No INITIAL session becomes a MEMBER in place. Stale/error reply closes without credit; fresh metadata grant determines retry role. |
| `Cancel(id,reason)` / disconnect / superseding session / store close / process restart | Enter Cancelled, remove queued grant pages, cancel RPC, release unused reservations and emit `CancelGrant`. Drop every later answer as an authority candidate. Already submitted disk work may finish; pins/buffers stay alive until its actual completion, and no stale success/session publication follows. Persisted fences remain monotonic. |

A new Hello does not erase the last durable fence. Serialize per-volume fence/cleanup actions
so cancelling an old attempt cannot let its cleanup delete a newer incarnation. #13 store
actions check the expected first_epoch/epoch and durable state again at submission; restart
replays completed durable effects and repeats authorized cleanup/fence/create idempotently.
No Ready state survives a process restart. Failure after cleanup or partial destination creation
leaves no granted source; a fresh bootstrap must authorize again before continuing.

An accepted session stores `Allowed[chunk]={role,ref,operation,chunk_version,frontier,pending_initial_count}`.
Role enum values: UNSPECIFIED0, INITIAL1, MEMBER2, MOVE_DEST3, REPLACEMENT_DEST4, REJOIN5. INITIAL binds
#76 initial-placement operation **and a durable unconsumed initial_pending_refs entry** as defined below; MEMBER requires the exact
earned current ISR ref and existing chunk. REJOIN is an earned same-UUID placed ref outside
ISR: existing chunks only, ordinary dirty resync allowed, never source/lease/old-ISR credit.
A REJOIN with a missing chunk refuses rather than inventing data. Destinations permit only
their operation's BeginResync/copy/
discard/history/root writes plus explicitly fenced live forwarding. Read/Digest/Mapped as
**authority**, ordinary client data/acks and lease credit require earned membership; prospective
newRef lease-set confirmation follows the existing expansion rule only after its frontier is
complete, and never substitutes for oldRef. A destination may expose only its own verification
state to the target, not become a repair source. Check grants before every mutation, including
BeginResync on an absent chunk. Frames have no power to choose a different operation.

**Dynamic move:** after metadata records a new move/replacement/chunk role, target closes the
old connection and opens a new monotonically numbered Hello containing the full required chunk
set. No in-place grant extension exists in this version. A session predating the operation
refuses BeginResync before any allocation. A changed operation ID, epoch, store lifetime or
chunk set cancels the old attempt; delayed pages/callbacks cannot authorize the new session.
Existing immutable accepted sessions use no per-I/O/heartbeat metadata lookup. Target view
application closes sessions whose allowed operation/ref changes, as ordinary membership refresh
does; cached grants are not reusable across another Hello.

## Consumable initial materialization (R1-C1, SEC90-R1-01)

On new-volume publication #76 writes `initial_pending_refs` equal to the initial replica refs
in each chunk (stored JSON; `ChunkMeta.initial_pending_refs=28`, at most R), plus immutable
`initial_operation` equal to the published create operation (`ChunkMeta.initial_operation=29`,
UUID16). An implemented growth owner sets its own operation only for a newly added chunk,
never for an existing chunk. The retained header
create_operation identifies the plan but grants no creation by itself. The set is consumed only,
never refilled: growth creates a new chunk and its own pending set, never resets an existing
chunk. Until a chunk's set is empty, reject its client read/write/repair, snapshot/root cut,
move/replacement and ordinary ISR change; only bootstrap materialization proceeds. The published
ISR is the planned initial ISR, not a claim that pending copies have earned data. Target and
replica apply this pending barrier independently; SessionAssignment.pending_initial_count
carries the snapshot count (0..R), and a count change requires refreshed view/new Hello before
the cached barrier can clear. No pending initial set coexists with move or
released refs, preserving the response bound (3R refs <=2R+5 for RF<=5).

Grant role selection is deterministic at each snapshot: exact placed ref in pending set plus
matching published chunk initial_operation => INITIAL; otherwise exact ISR ref => MEMBER; exact placed
non-ISR ref => REJOIN; otherwise exact current move/replacement destination => its destination
role. Ref/lifetime mismatch or unsupported combination refuses. Finish's gate/header compares
revalidate that choice. INITIAL is forbidden for enrolled legacy chunks. A partially materialized
pending chunk may resume only if its valid existing geometry, empty/zero map, no historical
layers and durable fence match that original operation; never discard unexpected blocks/history.
No application data can have been acknowledged while the pending barrier holds.

`ChunkDurable` for INITIAL is emitted only after real Store create/replay and the checked volume
fence have completed durably on the exact open UUID. Existing store chunk/fence records suffice;
no volatile flag or empty map alone is completion. The own-IO adapter then calls:

`CompleteInitialReplicasRequest={volume1:string,first_epoch2:u64,epoch3:u64,ref4:ReplicaRef,
store_generation5:u64,grant6:bytes32,initial_operation7:bytes16,rows8:repeated InitialCompletion}`;
`InitialCompletion={chunk1:u32,expected_version2:u64,durable_evidence3:bytes32}`;
response `{results1:repeated InitialResult}`, where InitialResult is
`{chunk1:u32,result2:enum(APPLIED1,ALREADY_EARNED2),version3:u64}`. Rows1..32, unique; request and
response<=8KiB. Evidence is SHA256(`FLINITIAL1` || UUID16 || u64 first_epoch || u64 epoch ||
u64 store_generation || initial_operation16 || u32 chunk || u64 durable_fence). It is the
**authenticated trusted IO adapter's attestation** after durable completion, not a hash that
proves media contents to metadata; tests/model independently inspect those durable effects.

Authorization is own-node IO only, insecure/admin/agent/other IO refused. Method validates the
finished unexpired private grant belongs to that exact caller/ref/lifetime/epoch and each row
was INITIAL, current target/epoch/incarnation, chunk initial_operations and expected chunk versions still
match,
current Node UUID/lifetime match and no pending conflict exists. Read current gate then atomically
compare header+Node+gate+ACTIVE mode plus b chunk revisions (**b+4 compares**, max36); consume
only that ref's pending entries, increment each chunk version and gate (**b+1 writes**, max33).
Capacity was charged by create; consume never releases or charges bytes. Expired/lost grant or
stale epoch/version => Aborted and new Hello. The metadata transition is the earning point;
no durable-create evidence means no request and no earning. A same-epoch lost-reply retry whose
exact ref is already consumed and still placed returns ALREADY_EARNED without restoring a bit;
removed ref, changed operation/incarnation or takeover cannot be reported earned by that request.

After completion, reopen under a new Hello; metadata now issues MEMBER/REJOIN, which requires
physical presence and refuses missing data. No caller may reuse consumed INITIAL to recreate an
earned absent chunk even while header create_operation remains. Partial-batch crash leaves
exactly the committed consumed refs; a new process/grant resumes only pending refs. A new target
epoch repeats fence/verification of pending chunks, never resets consumed refs. All initial
refs of a chunk must be earned and re-fenced before its normal source/lease/ack rules admit I/O.
Losing a pending disk does not authorize aliasing its UUID; ordinary unavailable admission applies.

## Pinned metadata read and server-owned grant protocol

New `meta/kv.KV` interfaces (revisions int64, positive):
`BeginSnapshot(ctx,ttl) (Snapshot{Revision,Handle},error)`,
`GetAt(ctx,snapshot,key) (Entry,bool,error)`,
`ListPage(ctx,snapshot,prefix,afterExclusive,limit,maxBytes) (entries,next,complete,error)`,
`EndSnapshot(ctx,handle)`, and `Check(ctx,[]Cmp) (bool,error)`.
BeginSnapshot linearizably selects one current revision after expiring TTL keys. Etcd uses a
linearizable revision read then WithRev for **all** GetAt/ListPage calls, sorted key range and
WithLimit; entry values and mod revisions come from that revision. Range continuation is the
last complete key, never a caller-selected revision. A compacted/expired snapshot returns
Aborted; no retry against a newer revision within that grant. Check is a linearizable
read-only compare transaction with zero writes and checks the same absent-key revision0 rules
as Txn. It does not invent a durable grant or advance the fence.

Mem implements the same API with one immutable revision image shared by handles to that
revision (including TTL visibility), bounded to64MiB total/128 handles, TTL≤30s. Do not clone
an unbounded database to service an attacker: compute required image bytes under the Mem mutex,
refuse ResourceExhausted before allocation; release on EndSnapshot/expiry/cancellation. Mutations
never alter an existing image. Snapshot storage capacity failure blocks bootstrap, not existing
I/O. Mem restart invalidates handles; etcd service restart also invalidates all process grants.
GetAt rejects a raw revision or unknown handle even if the backend still has that history.

All RPCs below are own-node **IO only**, enforce mTLS and refuse insecure mode. Tokens are
validated again by metadata, including signature, rights, authenticated target, volume,
first_epoch and exact current epoch. Admin/agent/read-only/other IO cannot call them. Fields
use types/validation in contracts.md; message names are distinct from ordinary GetVolume.

| RPC | Request fields (tag:type) | Response fields (tag:type) |
|---|---|---|
| OpenReplicaGrant | node1:string,store_uuid2:string,store_generation3:u64,volume4:string,first_epoch5:u64,epoch6:u64,session7:u64,token8:bytes,chunks9:repeated u32,profile10:u32,legacy_incarnation11:bytes16-or-empty | grant1:bytes32,cursor2:u32,request_digest3:bytes32,expires_in_ms4:u32 |
| ReadReplicaGrantPage | grant1:bytes32,cursor2:u32 | assignments1:repeated SessionAssignment,next_cursor2:u32,complete3:bool,request_digest4:bytes32 |
| FinishReplicaGrant | grant1:bytes32,request_digest2:bytes32 | proof1:ReplicaGrantProof |
| CancelReplicaGrant | grant1:bytes32 | empty |

`SessionAssignment={chunk1:u32,role2:Role,ref3:ReplicaRef,operation4:bytes16,
chunk_version5:u64,snapshot_generation6:u64,root_set_digest7:bytes32,pending_initial_count8:u32}`. Empty operation only
for MEMBER/REJOIN; INITIAL uses the chunk initial_operation bytes, destinations exact current
move operation.
No catalog/root lists per row: digests bind the coherent header's catalog/root frontier.
`ReplicaGrantProof={grant1:bytes32,request_digest2:bytes32,floor_epoch3:u64,
store_generation4:u64,snapshot_revision5:u64,replica_gate6:u64,profile7:u32,legacy_incarnation8:bytes16-or-empty}`.
The proof uses profile0 ordinary,1 snapshot and2 root as wire-contracts.md. It is an opaque
response on authenticated metadata RPC to the IO adapter, never a bearer capability accepted from a target or reusable across processes. All grant IDs use OS
random32 bytes and remain private. Service instance owns a bounded map of grant records;
unknown IDs after restart return Aborted. One record binds exact caller SPIFFE identity,
full token SHA256, node/ref/lifetime, volume/first_epoch/legacy_incarnation/epoch/session/profile, **complete sorted
unique chunk vector**, snapshot handle/revision, header/node/gate/mode revisions, page offset,
request digest, state and deadline. Client supplies neither snapshot nor gate generation.

Digest is SHA256 of ASCII `FLRSG1` followed by big-endian u32 length-prefixed UTF8 caller,
node, UUID and volume, then u64 generation/first_epoch/epoch/session, u32 profile, u32 legacy_id_len and legacy_incarnation bytes (0 or16),
token hash32,
u32 chunk_count and sorted u32 IDs. No ambiguous concatenation or partial-set digest. Chunks
must exactly match authorized assignment requests, be in geometry and ≤65536 (ordinary) or
≤4096 (root profile); Open≤512KiB including token≤4096. One row≤512 encoded bytes; page≤256
rows and≤136KiB. Validate the bounded request **before** allocating a snapshot/grant.

Open captures header/node/gate/mode at its snapshot and requires ACTIVE. An empty chunk vector
requires an explicit exact-ref retained fence-holder assignment; it is not a general cleanup
permission for any token holder. Each page reads only
requested chunk IDs at that snapshot, validates the complete assignment/role/operation and
stores compact rows. Cursor is the exact next row offset from the private server record, not
authentication by itself. Retry of the immediately previous cursor returns the byte-identical
cached page; cursor jumps, mixed IDs/callers or changed digest abort. Partial pages cannot be
installed. Finish requires all pages complete and performs exactly **4 compares**: header,
Node, `replica-gate/<volume>` and `identity-mode`. A false compare aborts the entire grant;
no unbounded per-chunk compare vector. This read-only Check is the authorization linearization
point, not a promise that future epochs cannot supersede it. Normal durable fences/leases
handle a later takeover. Repeated Finish after a lost reply returns the same cached proof only
until the grant deadline; caller still validates its local attempt before any action.

`replica-gate/<volume>` is a service-owned nonzero u64 under `/flaminio/v1/`, created by CAS,
never deleted even at volume deletion, incremented without wrap on every authority mutation:
create-plan/chunk publication, membership/ref/version/move/replacement/release, fence-holder,
cleanup/ForgetNode/tombstone/deletion and migration conversion; #81 bounded batches too.
The narrowly proof-scoped unpublished delete-only GC exception is defined in budgets.md;
its `NoLiveAuthorityGC` check and caught `identity_gc_weak_classifier` mutant run with this
same model. The exception cannot delete an admitted header or a pending INITIAL ref. It may reclaim a completed private manifest while a matching visible header has pending INITIAL refs: those refs, not the private manifest, independently retain bootstrap authority. Snapshot
cut/catalog and root commitment/pin/frontier writers bump it as well when those features exist.
Every such transaction compares the old gate and writes its increment with the changed keys;
header changes also change header revision. Overflow refuses before mutation. Node observation/
forget writes change Node revision and retained store lifetime atomically; source lifetime,
replica gate, snapshot revision and page offset are **different** values. Gate initialization
is part of create/enrollment, not a lazy unguarded write by authorization. No gate deletion/ABA.

## Bounded resources and fair metadata work

Numbers are hard aggregate caps, enforced before queueing; elapsed timers limit resources only.
Per node:64 TLS/pre-Hello connections,8 per authenticated peer (unidentified share the64 cap),
5s TLS and3s Hello deadlines. Pending attempts128 globally,2 per peer+volume; active sessions
1024 globally,16 per peer,64 per volume, plus64MiB total immutable assignment storage. Pending
authorization storage≤33MiB/grant and≤64MiB aggregate, charged **before** Open by checked row/
request allocation; metadata service has the same128-record/64MiB aggregate caps. Grant lifetime
30s, one in-flight page per attempt, no speculative page queue; expiry/cancel reclaims map,
snapshot and cached reply reservations. No grant timer creates membership/fencing authority.
An oversized legacy session can consume a large reservation and block another bootstrap; it
cannot consume reserved control-worker resources. Successful session admission transfers compact
rows to the separately bounded accepted-session pool, then releases the grant reservation.

Replace metaclient unbounded FIFO with bounded control(128) and bootstrap(128) queues,4 dedicated
concurrent control slots and2 bootstrap slots. Control covers CompleteInitialReplicas/ChangeIsr/Attach completion,
refresh, registration/heartbeat and exact cleanup. Round-robin by volume/peer within each class,
weighted4 control dispatches:1 bootstrap when both eligible; per-volume at most1 active control
mutation as today. Bootstrap cannot borrow all control slots; stalled pages cannot block a
ready control call. Total RPC time≤6s,≤2 endpoint attempts with100/200ms backoff within the
remaining grant deadline, no unbounded retry/resubmission. Caller retains bounded retry state
and explicit Busy result rather than sending to an unbounded channel. Cancel removes queued
calls and drops late answers by BootstrapId; retry starts a fresh snapshot after Aborted.
Bound pre-ready network reply/event channels to4 entries/512KiB aggregate per attempt; no data
frames are accepted before Ready. Full channel closes that attempt without engine blocking.
No mutex or metadata scheduling enters the steady I/O path; owned engine state and bounded
nonblocking channels manage these control transitions.

## Independent simulator checks and caught mutants (S2–S6, S9)

`LostStore(node)` removes all durable blocks/fences/chunks, chooses a fresh UUID and increments
its observed lifetime through the model registration transition. Crash alone retains store UUID
and durable state. The simulator uses **the same** Bootstrap events/actions as runtime; do not
precreate replacement chunks outside that machine. Scheduler can delay authorization, cleanup,
fence, create and metadata callbacks independently. Checker history retains acknowledged blocks,
actual UUID/ref provenance and grant linearization records independently of target/core flags.

Each row names an assertion in `sim/check.rs`, a `Bugs` mutation and a deterministic bounded
case in `sim/tests.rs`; clean counterpart must pass and mutant must produce a counterexample.
Existing write linearizability/lease/minimum checks stay enabled, not replaced by these checks.

| Check / independently observed rule | Mutation | Bounded witness |
|---|---|---|
| `initial_durable_before_earned`: each consumed pending ref has independently observed durable chunk and floor | `earn_before_initial_durable` | First attach RF2/min1, delay create/fence completion and crash at each callback; no initial lease/ack/source success |
| `initial_consumed_never_reissued`: INITIAL only while its exact pending bit exists | `reissue_consumed_initial` | Earn original empty chunk, acknowledge nonzero write, erase that chunk on same UUID and reopen with retained create_operation; missing MEMBER must refuse |
| `earned_ref_only`: read/source/repair success comes from exact earned ref | `identity_alias` | RF2/min1, acknowledged nonzero block, lose one store, detached attach on its replacement; no blank-source read/repair |
| `ack_lease_ref_isolation`: every credited ack/confirmation has exact pending ref/session | `transfer_old_ack_lease` | RF2/min1 pending write and queued heartbeat, lose/reformat, deliver old reply after new session |
| `bootstrap_current_authority`: every cleanup/create/fence has a complete grant current at its Check point | `bootstrap_old_token` | RF3/min1 takeover plus new UUID; old target Hello first, and delete/recreate first_epoch mismatch |
| `fence_before_ready`: Ready/Fenced implies matching durable floor and completed action | `ready_before_fence` | Delay FenceDurable, deliver grant/create callbacks; crash between cleanup/fence/reply |
| `grant_role_before_create`: creation/reconstruction matches exact granted operation and role | `begin_without_grant` | Open member session before move; BeginResync after move on same connection; stale replacement operation |
| `callbacks_cannot_resurrect`: cancelled/superseded BootstrapId never publishes success | `accept_cancelled_grant` | Cancel and replace Hello while page/fence callback queued; deliver late callbacks in both orders |
| `replacement_current_version`: replacement/join bound to current epoch/version/operation/newRef | `replace_without_version` | Competing replace and move; takeover between full copy and join; A→B→A registration |
| `full_range_before_join`: fresh destination proves every current block/hole, not dirty set | `replacement_dirty_only` | RF2/min1 untouched acknowledged block plus one post-loss write; only dirty block copied |
| `history_before_join`: all committed snapshot stages verified before join/current forwarding | `replacement_current_only` | One snapshot with overwritten/deleted old block, new current content; copy only current |
| `frontier_current_at_join`: join evidence equals catalog/root frontier used by winning CAS | `join_stale_frontier` | Commit cut/pin root after completion before join; delay old stage callback |
| `roots_receipts_before_join`: every committed+pinned #58 root/receipt exists durably on joiner | `replacement_skip_pinned_root` | Current root copied, older pinned ship root omitted; takeover/replay before join |
| `no_authoritative_source_no_success`: no reconstruction/ISR join from non-ISR or empty store | `promote_blank_source` | Lose every ISR store; retain stale out-of-ISR disk; read/write/resync must remain unavailable |

Profile-specific witnesses run when #51/#58 models are composed; ordinary identity model does
not claim those absent features. Final membership CAS also retains independent existing
minimum-ISR and all-ISR durability checks/mutants. Snapshot/root frontier mutations are added
by the second-to-land implementation with these exact scenarios, never deferred generic proof.

## Production adapter and concurrency tests

- `flaminio-node/tests/engine.rs`: parameterize actual remote TLS Hello and **synthetic local**
  engine Hello through identical grant/fence states. Counting real Store adapter proves wrong
  UUID/old token/ungranted BeginResync/missing MEMBER chunk produces no cleanup/create/read;
  actual durable floor exists before Fenced and survives restart/partial creation.
- `replica/mod.rs`/`net.rs` tests: delayed metadata/fence/create completions after disconnect,
  supersession and UUID/process change; cancelled pending work releases memory but keeps I/O
  pins until completion. Local fast-read admission **and completion** verify exact StoreId,
  lifetime, epoch/session/membership; queued old bytes/acks never count for newRef.
- Actual remote and synthetic local first-attach tests cover all-zero initial data, partial
  creation/restart, completion CAS lost reply, takeover, mixed pending/earned refs, expired grant
  and post-earned missing-chunk refusal; both-backend CompleteInitialReplicas tests check32/33
  rows, b+4/b+1 bounds, role/auth/epoch/version races and no pending-bit resurrection.
- Memory **and embedded real etcd** `meta/service/replica_grant_test.go`: forged/mixed cursor,
  chunk-set substitution, partial-page Finish, cross-peer ID, lost Finish reply, metadata restart,
  old token, header/node/gate/mode mutation across pages, generation ABA and overflow, compaction,
  max page/request/reservation and +1. `meta/kv/snapshot_test.go` proves shared revision/TTL,
  absent keys, stable pages under mutation, expiry/cancel reclamation and Mem cap refusal.
- Controlled node/metaclient test: valid-token Hello flood, stalled metadata endpoints, one
  65536-row legacy volume and cancellation; caps remain bounded and a ready control call is
  dispatched within one scheduler cycle through a reserved slot. If metadata itself is stalled,
  completion is bounded failure by6s rather than a false progress claim. Existing sessions
  continue data/heartbeat work without bootstrap metadata dependence.
- Same-node replacement adapter tests use real current/snapshot/root reconstruction where
  profiles exist, full holes, concurrent writes, interrupted stages and each join/drop crash;
  verify each newly joined store independently. Shared Go/Rust vectors exercise grants and
  exact replacement/ChangeIsr CAS, not model-only mocks.
