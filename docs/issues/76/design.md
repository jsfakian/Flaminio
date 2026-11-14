# Issue #76: Interrupted volume creation — design

[Specification](spec.md). Approved 2026-11-13; not implemented.

The facts it starts from:
- Creation's plan is volatile; batches/finalization compare its claim (`service.go:382–473`).
- Deletion preserves Creating when setting Deleting (`service.go:539–542`); its public helper
  lists all chunks and runs all cleanup batches (`:548–577`). Recovery must not call that loop.
- Attach rejects hidden headers before signing tokens (`:275–289,696–744`). Publication clears
  Creating and sets a positive claim-based epoch atomically (`:461–473`; #30 first-epoch rules).
- etcd3.7.3 `apply/capped.go:37–40` rejects transactions with any Put under NOSPACE;
  `storage/quota.go:151–173` charges Puts even alongside deletes. Mixed GC cannot recover space.
- CSI's AlreadyExists fallback checks only four scalar fields (`controller.go:252–264`).

## Identity, roots and immutable plans (S1, S2)

Canonical `domain.CreateSpec` fields: size_bytes,chunk_bytes,replicas,min_isr,prefer_node,
failure_domain,failure_domain_policy; normalize node/strict defaults. Before **any** KV write,
require PreferNode empty or domain.ValidID (ASCII1–128), all other scalar/ID checks, canonical
identity<=1024 bytes and every encoded root/header/chunk<=8192 bytes. Do not echo oversized input
in errors. Strict persisted decoding enforces the same bounds/schema; malformed data is DataLoss.
Header retains create_operation/create_spec/admission_generation after visibility and mutation.

Use separate `/flaminio/v1/create-roots/<op>` and `create-plans/<op>/<index>` keyspaces; the root
index contains no plan values. Op is fresh random128-bit lowercase hex, never reused. Root schema1
contains volume/spec,state,chunk_count,staged,next,admission_generation; states preparing,sealed,
cleaning,copying,complete. Staged/next describe contiguous plan/public prefixes. Sealed plan is
immutable; later root writes change progress only. Index formatting matches current chunk keys.

Initial root creation also writes permanent `create-birth/<shard>` value `1` (16 shards, first
nibble SHA256(volumeID)); compare its previous revision plus root absence in the same transaction.
Never delete/revert birth keys. A committed initial CAS advances birth revision, so even the
backend's transparent retry cannot resurrect the root after GC. On uncertain/false response,
reread that root; if absent abandon this op ID, never retry it with a fresh birth revision. Every
subsequent write compares a positive root revision. This uses existing TxnRev, not blind absent-Put.

Read/classify raw header before planning (Deleting first below). Identical live creates help,
visible matches return current validated data, conflicts refuse. Read #78 gate before eligibility,
compute plan, create root, stage up to32 chunks plus root progress per transaction, then seal.
Gate conflict before admission discards unadmitted work and replans; no old authority is reused.

Admission compares sealed root, name/tombstone absence and #78 gate; write creating header plus
cleaning root atomically. Header epoch=0,target empty until finalization; original modification
revision C is claim authority. Lost replies discover the same root/header. Without #78, omit its
gate/field but claim no drain serialization. Never infer uncommitted admission from a failed reply.

## Conditional helpers (S3–S5)

Keep header byte-for-byte at C until visibility/deletion. Every helper compares C and the root
revision it read, checking operation/identity/phase and not-deleting. Use bounded APIs below:
- Cleaning reads at most32 public chunk keys, deletes them with root progress. Empty prefix
  transitions root to copying. A delayed cleaner's old root compare cannot erase copied chunks.
- Copying fetches at most32 consecutive bounded plan values, validates indices/initial ISR and
  atomically writes their public chunks plus next. Missing/bad admitted plan fails closed.
- Finalization at next==chunk_count atomically writes visible header, Epoch=FirstEpoch=C and
  complete root. Retain identity/generation; never reset placement/ISR on an identical retry.
- Response uses #77 coherent read and verifies operation/identity. Replacement/churn gives a
  bounded retry/Aborted/NotFound; do not return the cached initial plan after attach or moves.
Every false/ambiguous result rereads durable state; context failure leaves it for another helper.
Foreground work respects context/retry bounds; no helper restarts cleaning once copying began.

## Deletion classification and legacy rollback (S6–S8)

Decode/classify Deleting **before** Creating/missing-root checks. For a recognized still-Creating
unpublished shape, resume its deletion below even if root GC already removed the root. Published
Deleting headers need no create root; leave their conservative public deletion path in charge.
Create/recovery returns Aborted for that case, never calls a by-name deleter on a header it read.
Malformed/unsupported deletion shape is diagnosed as such, never falsely as a missing-create-root.
Only a nondeleting new-format Creating header with no root is missing-root corruption.

**Unpublished proof:** recognized writers never set Creating again after publication; initial
claim has epoch0/no target, and the visibility transaction clears Creating while setting C>0.
readHeader hides Creating/Deleting, so Attach cannot issue this incarnation's token. With old
writers excluded, a validated Creating,epoch0,target-empty header (including both flags) therefore
never authorized materialization for this incarnation. Old-incarnation tokens may still create
old copies, but their epochs precede this claim; the next claim/first epoch is newer and #30
fences/cleans those copies. This proof depends on monotone KV revisions and #30 node support,
not timeouts or assuming old tokens vanish. No existing tombstone may be removed/bypassed.

`stepUnpublishedRollback(ctx,expectedHeader)` is scoped to ID, observed revision and operation
(or exact recognized legacy shape). Legacy automatic rollback requires maintenance writer
exclusion; new-format cancellation requires an already Deleting header or authorized explicit
DeleteVolume. Apply all existing authorization/protection/reference preconditions. DeleteVolume routes proven
unpublished headers to this bounded step; published deletion stays conservative. One delete-only
Txn compares exact header revision and tombstone absence, then deletes **only that header**.
A conflict returns changed; never reread a replacement and start deleting it. Header removal
fences all delayed publication before cleanup. No mark/inventory/new tombstone is required for
this never-published incarnation. Unknown/nonzero-epoch/target-bearing shapes are ineligible:
retain conservative deletion or actionable repair, never guess that they have no replicas.

Public chunk leftovers are discoverable by bounded keys-only chunks/ scans after any restart.
Delete <=32 selected keys only if header is absent **and each key still has its listed revision**.
Thus a stale cleanup cannot cross header-absence ABA/recreation. Existing/new tombstones and
replica sweeps are untouched; normal published deletion still inventories before removing header.
No recovery call invokes public by-name DeleteVolume or deleteChunks' inherited unbounded loop.
A compatible legacy retry may finish a small rollback/cleanup then freshly admit; budget/deadline
exhaustion returns Aborted, subsequent retry/worker resumes. Incompatible scalar identity refuses.
Legacy visible matching remains scalar-only in metadata, allowing CSI's weaker fallback removal.

## Delete-only private GC and bounded recovery (S7)

GC eligibility is exact root+observed header: complete with matching visible header; preparing/
sealed unchanged in two observations with no admitted matching header; or admitted whose header
is absent, Deleting or another operation. Unknown/corrupt evidence is not eligible. Delete root
in a **delete-only** Txn comparing its revision and that observed header revision/absence.
Preparing/sealed admission races on the same root revision, so only admission or revocation wins.
Root removal is the publication fence. No discarded-state/progress Put, GC lease or TTL is needed.

Scan create-plans/ keys for orphan groups; when root is absent, delete <=32 plan keys comparing
root absence and their individual revisions. This stays delete-only under NOSPACE, even for
oversized/corrupt plan values: discovery/deletion never fetch those values. Root creation's birth
CAS prevents resurrection; delayed staging requires its old positive root revision and fails.
Restart discovers any orphan without a persistent Put cursor. A live admitted creator is never
silently revoked for quota/timeout. If only admitted work occupies space, report ResourceExhausted;
explicit scoped deletion or operator quota recovery is required. Deleted keys do not themselves
clear NOSPACE: compaction/defrag/alarm disarm follow normal operator procedure, not worker privilege.

RunCreateRecovery runs immediately/every5s, one nonoverlapping pass with a 2s context. Four
rotating sources: roots, raw headers, private-plan orphans, public-chunk orphans. At most one
64-key discovery page/source/pass; process at most64 items total round-robin, max8 mutation
attempts and512 total KV calls/item **including retries/point reads**. Share counters for the
same operation encountered through roots/headers/plans; foreground legacy rollback uses the same
per-invocation limits. `kv.WithWorkBudget` charges every backend wire attempt (also retries),
not just the outer call; exhaustion returns Aborted without another send. Each source retains
its <=64-key page/tail plus fetched next/done. Advance its cursor only through examined keys;
do not use fetched next or wrap until the tail is consumed. Cancellation before examination
leaves that key pending; missing/error/skipped keys count toward the64-item budget when examined.
Do not refetch/overwrite an unconsumed tail. Restart may rescan from the beginning, never save
fetched-but-unexamined progress. Chunk/plan batches max32; root.next/remaining keys preserve
safety state. Retain <=64 root key/revision observations for second-observation GC, never all
roots/headers/values; advance the cohort after checking it, even on error. Rotate first source
on each pass so slow failures cannot always exclude the same kind. Cancellation ends work before
next RPC; join worker before KV Close. Eventual progress assumes store availability/fair execution.

## Bounded KV contract and memory (S1, S7)

Add `ScanKeys(ctx,prefix,after,limit)->{entries:[Key,Rev],next,done}`: limit1..64, exclusive
byte-lexicographic after, latest linearizable page, no cross-page snapshot/MVCC clone. Empty page
is complete only with done=true. Service keys<=256 bytes, wire reply<=32KiB, values never returned.
Etcd uses keys_only+limit range, prefix end/after+NUL start and a per-RPC receive cap, no compression.
Add `GetBounded(ctx,key,maxBytes)->(Entry,found,error)` for bounded current values (max8192 here):
server-side exact-key Range with per-RPC receive<=16KiB; oversized/invalid values fail closed
before JSON decode. Use direct bounded Range call options, not a full Get/List then truncation.
Mem maintains an ordered key index (seek O(logN), visit <=limit), copies only selected keys/
bounded values, expires examined keys only in these APIs and never sorts/clones the whole map.
Backend tests cover byte/order/continuation/error semantics, cancellation and allocation bounds.
Recovery holds one <=32-record batch, <=256KiB value payload plus <=512KiB encoding; bound/measure
parser/object allocations separately (independent of inventory size), not as a claimed RSS figure.
cohorts/cursors/pages are bounded. New discovery/cleanup/headers/helpers use these APIs exclusively;
creation's load/eligibility enumeration also pages rather than listing private manifests.
#77's separate atomic ReadSet contract is unchanged; this API needs no historical Mem snapshots.

## Files and contracts

- domain Header/CreateSpec; service create.go/create_recovery.go, existing lifecycle/CSI hooks;
  main worker lifecycle; kv/{kv,mem,etcd}.go for bounded APIs; dedicated backend/recovery/auth tests.
- Pure sim/create_recovery.rs, sim/mod.rs and shared meta/testdata/create-recovery.json schedules.
No new RPC/proto/token/data-frame/disk record. New private keys/states/birth records are above.
Each Txn<=120 compares/ops and<=512KiB encoded writes: root birth2/2, admission<=4/2, copy/clean
2/<=33, finalize2/2, root GC2/1, orphan GC<=33/<=32. Never split an atomic admission. Invalid input
is InvalidArgument before any write; oversized persisted values DataLoss, quota ResourceExhausted,
contention/incomplete rollback Aborted; preserve other context/placement mappings. Stored data
and identity bounds apply at admission and reread. Diagnostics identify bounded IDs, not raw values.

## Tests

- S1/S2: every identity/default/conflict, PreferNode empty/128 valid/invalid/300KiB, duplicate/
  oversized persisted identities; zero KV writes on rejection, exact atomic byte/op bounds.
- S3–S5: both stores, every stage/claim/clean/copy/finalize pre/post-commit failure/reply loss,
  helpers and cleaner races; unchanged C/generation, hidden partial state and no ISR/epoch reset.
- S6/S8: suspend exact deletion CAS after read, legitimately remove old header, recreate/publish,
  then release stale worker. Inject missing-header/incarnation-compare and public-by-name-delete
  substitution. Model per-incarnation publication/Attach/token history, assert NoUnprovedTombstoneBypass;
  weakened_unpublished_classifier must fail on unsupported/authorized shapes. Replay actual Go
  transitions with replacement/old delayed tokens; retain progress/early-finalize mutants too.
- S7: embedded-etcd3.7.3 real NOSPACE alarm rejects a Put-containing GC control, while eligible
  root/orphan delete-only GC succeeds. Lose root-delete/plan-delete replies, restart, race initial
  root commit/lost reply/GC/original backend replay. Model retired IDs: NoRootResurrection must
  catch missing_birth_compare; replay Go transactions on both stores. Inject failed reads/deletes;
  no false completion. Compact/defrag/disarm only isolated fixture, then verify creation resumes.
- S7/S8: million plan keys plus many roots/headers, instrument range/value/byte/call/allocation
  ceilings above fixture baseline. Four populated64-key sources, only64 examined items/pass,
  mid-page pause/restart/wrap: all stable eligible keys get work, no tail skip/early cursor advance.
  Bound memory/max8 mutations; legacy partial chunks resume. Creating+Deleting resumes as deletion;
  published Deleting is not missing-root corruption; unproved shapes fail closed, no tombstone loss.
- S9: full CSI policy conflicts refuse; compatible legacy/new resumed creates succeed; mTLS
  admin-only matrix unchanged. Implementation checks make lint/test/race/sim/contracts/hygiene.
Only document checks run; planned model/runtime/etcd tests remain implementation work.

## Lab, docs, risks and ADRs

No shared VM/destructive lab required. Document proof-based legacy exception, bounded discovery,
quota recovery and writer exclusion in architecture/follow-ups/lab guidance. Q1 duplicate plan
cost, Q2 scalar-only legacy identity and Q3 maintenance remain open; no risk/approval is invented.
Settled at approval, 2026-11-13: Q1–Q3 take the plan's recommendations (see decisions.md, Settled at approval).
Unknown state fails closed, not silently repaired. Accepted0001/0002 consistent, 0005/0006 unchanged;
audit0007–0011 remain Proposed. [ADR](adr-proposal.md) includes these R1 protocol refinements.

## Parallelizable?

Go contracts precede model/fixture/CSI integration; no independent Rust runtime implementation.
