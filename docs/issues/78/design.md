# Issue #78: Placement admitted after drain marking — design

[Specification](spec.md). Approved 2026-11-13; not implemented.
Protocol choice: [proposed ADR](adr-proposal.md).
Settled at approval, 2026-11-13: the protocol ADR is accepted with the plan (see decisions.md, Settled at approval).

The facts it starts from:
- `createVolume` reads node/drain listings before its claim; that claim compares only volume
  and tombstone absence (`meta/service/service.go:382–417`). Its `claim` revision fences chunk
  batches and final visibility (`:435–473`). Preserve that incarnation fence.
- `nodes` reads node, alive and drain prefixes separately (`:241–270`). Registration conditionally
  updates only the node key (`:211`); marking writes only a separate drain key (`:827–846`).
- `MoveReplica` compares chunk/header only (`:907–910`). `ChangeIsr` may add only a recorded
  destination (`meta/domain/domain.go:257–268`); the pending move already represents admission.
- `ListVolumes` hides creating/deleting headers (`service.go:848–862`); `drain.Run` completes
  on its visible scan alone (`control/drain/drain.go:172–185`). The KV interface offers
  linearizable prefix lists and revision CAS, with `MaxTxnOps=120` (`meta/kv/kv.go:1–59`).
- #17 adds maintenance targets; #18 adds failure-domain validation but its draft S15 explicitly
  allows draining direct destinations. This issue replaces that narrow assumption. #72 consumes
  lifecycle admission and adds capacity accounting later; it is not required for this fix.

## Admission fence and generation (S1, S5, S6)

Add `placement-gate` under `/flaminio/v1/`; its value is `1`, its modification revision is the
placement generation. Generation0 belongs only to legacy headers; new admissions require a
positive initialized gate. No writer deletes it. Bump it atomically with changed registered
node record or drain mark/unmark. Heartbeats do not bump it: liveness remains a placement
observation, not fencing authority. Registration no-ops do not advance an existing gate.

Before enabling creation/move admissions during the writer-exclusion rollout, call
`ensurePlacementGate(ctx)`: read/validate it; if absent, CAS absence and Put1 via TxnRev.
Retry a lost compare/response by reread. Its committed revision G is newer than every retained
legacy marker D by the KV monotonic revision contract. No marker is rewritten/restarted merely
for initialization. Every mutating entry point, including unchanged registration/idempotent
mark, ensures initialization before returning or planning; absence cannot yield a new generation0.
A failed initialization holds admission unavailable. Readers may remain available. CheckDrain
still uses each marker's exact original D, so legacy creating generation0 remains conservatively
older, while later unrelated claims with G>D are not misclassified as pre-mark reservations.

- **Creation:** read gate first, then nodes/load and compute placement. Claim compares gate at
  the read revision, volume absent, tombstone absent; one header write. Gate conflict means
  bounded retry of the entire planning pass, never reuse of the stale node list. Distinguish
  gate conflict from a taken id before returning today's AlreadyExists handling.
- **Header:** add `AdmissionGeneration int64` as `admission_generation,omitempty`, recording the
  gate revision read by the winning claim. It remains unchanged through creation/recovery,
  finalization, attach and deletion; legacy absence is 0. A pending creation retains its
  original plan. #76 must not relabel an old reservation as new or silently recompute its plan.
  If recovery needs a different plan, fence every old writer and resolve its published chunks
  before obtaining fresh admission under the current gate; never acquire new publication rights
  using the old generation. This is a required cross-issue contract, not an automatic resume.
- **Publication:** retain the original claim compare on every cleanup/chunk/finalization batch.
  Do not recheck the drain mark: earlier admissions may finish, and completion waits for them.
- **Move:** in each retry read gate before destination/node/domain state, then header/chunk.
  Reject an existing destination drain mark with `FailedPrecondition`, `destination "b" is
  draining`. Commit includes gate, chunk and header compares, plus #18's node-record compares.
  Thus even drain→undrain ABA invalidates a stale admission pass. Check lifecycle eligibility
  before the no-op `CheckMove` response so callers never receive an accepted draining proposal.
- **Mark:** ensure the gate, then read node, existing marker and gate. An existing marker is
  idempotent: return its modification revision after the initialization check. Otherwise compare node/gate/marker absence, write marker `1` and gate
  `1` with `TxnRev`; its revision is the drain generation. Lost compares retry boundedly.
- **Undrain:** compare the marker and gate revisions read, delete marker and write gate together.
  Already absent is a no-op. It does not delete or cancel any creation. A future mark gets a new
  generation. All these loops use existing bounded retries/backoff and `Aborted` on exhaustion.

The fence precedes the eligibility reads. A concurrent changed registration or mark advances
it, so a stale claim/move cannot commit, without a compare for every selected node.

## Inventory and completion (S2–S4, S6)

Add administrator-only `CheckDrain`. It never moves data or deletes reservations.
1. Validate node id and positive generation; read its drain marker. Missing/different revision:
   `FailedPrecondition`, `drain generation is no longer current`.
2. List raw `volumes/` first. Count every `creating` header with `admission_generation < D`,
   including legacy generation 0, regardless of its intended nodes or `deleting` flag. Invalid
   JSON, negative generation or inconsistent schema returns `DataLoss`, never zero work.
3. List raw `chunks/` afterwards. Count chunks with the node in `replicas`, and separately
   chunks whose `moving.to` is the node, including when it is already a replica. No visible
   header join may hide a chunk. Invalid chunk JSON/schema fails closed with `DataLoss`.
4. Execute `Txn` with compare `{drainKey(node), D}`, no operations. A failed compare returns
   the stale-generation error. Set `complete` only if all three counts are zero.

**Why the order works:** while D remains marked, no new claim or incoming move can admit the
node. Earlier creations either appear in the first list, or finished publication/fenced deletion
before it. Their chunks therefore appear in the later list if still active. An older incoming
move remains represented in `moving.to` or `replicas` until removed; it cannot disappear and
then legally re-add the destination. After zero inventory, work for this node cannot increase
without ending D. The final marker compare is completion's linearization point. Full lists
are linearizable individually; no atomic multi-prefix snapshot or unbounded compare set is used.

Deleting a hidden creation first marks its header, fencing every old batch, then publishes its
tombstone/removes its header before removing chunks, as today. Reservation/chunk scanning remains
conservative during every phase. Tombstones continue tracking physical cleanup independently;
completion does not certify erased stale source copies (#13).

`DrainNodeResponse` carries its generation. `drain.Run` retains that token, runs ordinary `Step`
to request work, and calls `CheckDrain` each iteration before any success. It succeeds only on
`complete=true`; old `Remaining==0` alone never succeeds. Add `Progress.PendingCreations` and
`Incoming` counters (uint64) from the check, and print them when nonzero; report after updating.
If counts exist only in hidden/deleting records, continue waiting rather than treating the
volume as attachable. Cancelled contexts return their error and leave the marker set.

An interrupted creator can block every pre-existing drain until incarnation-fenced explicit
`DeleteVolume` or #76 recovery resolves it. No timer reaps a reservation. After restart, repeat
`DrainNode(true)` to recover the same D. Undrain/re-drain invalidates the old controller token;
its next check fails, requiring restart of that drain invocation.

## Files touched

- `meta/domain/domain.go`: immutable admission generation in Header (S2, S7).
- `meta/service/service.go`, new `admission_test.go`: fence, conditional transitions, raw
  inventory and validation (S1–S6); preserve `RegisterNode`'s label-merge CAS.
- `api/proto/flaminio/meta/v1/meta.proto`, `api/gen/`: response generation and CheckDrain (S4).
- `meta/service/auth.go`, `auth_test.go`: CheckDrain administrator rule (S7).
- `control/drain/drain.go`, `drain_test.go`: generation, completion and progress (S3, S4).
- `dataplane/crates/flaminio-repl/src/sim/placement_admission.rs` (new), `sim/mod.rs`: executable
  metadata-admission schedules and invariant checks with injected bugs (S8).
- `meta/testdata/placement-admission.json` (new), Go admission tests: replay common schedules
  against actual service CAS. Keep existing replication-transition vectors intact (S8).
- Protocol/architecture lifecycle text, `docs/lab.md`, `docs/followups.md`: semantics/rollout (S8).

## Contracts

- **Proto:** `DrainNodeResponse { uint64 generation = 1; }` (0 only for undrain);
  `rpc CheckDrain(CheckDrainRequest) returns (CheckDrainResponse);`
  `CheckDrainRequest { string node = 1; uint64 generation = 2; }`;
  `CheckDrainResponse { bool complete = 1; uint64 pending_creations = 2;
  uint64 placement_chunks = 3; uint64 incoming_moves = 4; }`. Generation is an etcd revision,
  must fit positive int64; otherwise InvalidArgument. Caller: administrator only.
- **Stored records:** gate and marker keys/value above; header admission field above. Existing
  drain value `1` is retained. Legacy markers retain their modification revision D; before
  admissions resume, initialized gate G must be newer than every retained legacy D. Only legacy
  header absence represents generation0; new requests never record0.
- **Errors:** invalid arguments `InvalidArgument`; absent registered destination/mark target
  `NotFound`; draining destination/stale generation `FailedPrecondition`; corrupt inventory
  `DataLoss`; contention `Aborted`; KV/context errors retain current mappings.
- **Bounds:** gate initialization1compare/1op; create claim3compares/1op; registration at most
  2/2; mark3/2; unmark2/2;
  completion 1/0. Move adds one gate compare to existing/#18 compares, well below 120. Chunk
  batches keep existing bounds. No node-count-sized reservation transaction.
- **Clients:** `make proto`; Rust metadata client has no drain RPC today and gains none.
  `make contracts` verifies unchanged calls/unknown-field interoperability. No token, control
  socket, replication frame or on-disk format changes. CLI drain progress adds counters above;
  its success `{}` and exit 0, errors exit 1, remain.

## Tests

- **S1/S5:** `admission_test.go`, both stores: audit F03 at node/load-list boundary; drain
  before/after claim CAS; registration label/witness change; direct move racing mark and
  undrain/re-drain. Assert fresh replanning or refusal and no forbidden chunk/move committed.
- **S2/S3/S6:** both stores: two creators with multi-batch publication; pauses before first,
  middle and final batch; lost claim/finalize replies; restart; deleting a hidden claim; stale
  batch after cleanup/recreate. Verify reservation→chunks handoff, original admission generation,
  exact compare/op bounds and no zero check while any earlier writer can still publish.
- **S3/S4:** service and `drain_test.go`: hidden older reservation on this/another node, later
  unrelated creator, incoming move before replica addition, ChangeIsr completion, empty visible
  listing, deleting leftovers, list failures/corruption, undrain during each list/final compare,
  repeated mark/restart and cancelled Run. No complete result on an invalidated D.
- **S7:** auth matrix refuses agent/io/other-node/no-cert CheckDrain; admin allowed. Legacy
  markerD with absent gate and unchanged registrations: initialize G>D, repeat mark retains D,
  admit unrelated creator with generationG, crash/lose init response/restart and assert that
  creator does not block D. Legacy generation0 creation still blocks. No new request records0.
  Additive proto fields interoperate with Rust client.
- **S8:** model plan/admit/batch/publish/mark/unmark/move/delete/restart plus separate check-start,
  header-scan, chunk-scan and final-validation events, independently scheduled. Check no post-mark
  admission, no premature inventory completion and that every successful check's original D is
  still current at its validation instant. Inject omitted admission compare, omitted reservation
  count and omitted final marker compare; each must produce a bounded counterexample. For the
  last, scan empty D then unmark (alone, and with admit/re-mark D2) before validation: success
  for D is forbidden even if inventories were empty. Serialize/replay those schedules against
  actual service CAS; the clean model/service refuses stale generation and the mutant fails.
- **Later targets:** `make lint test race sim contracts hygiene`. No lab or runtime tests ran
  for this documentation draft; implementation must execute those checks.

## Lab

None required for this metadata race. Do not run shared VM targets. Existing drain smoke tests
may be run later only on request; no new destructive lab procedure is introduced.

## Docs

Document mark/admission/completion linearization, conservative blocking and old-writer exclusion
in architecture/protocol and lab drain guidance. Link #78 as planned in follow-ups; mark it
resolved only after implementation/verification. No README commands or performance claims change.

## Risks

- **Open Q1:** a global gate contends on changed topology, and unrelated old creating volumes can
  block drain. Per-node reservations remain an alternative, not an accepted availability cost.
  Settled at approval, 2026-11-13: the global gate is chosen and its availability cost accepted (see decisions.md, Settled at approval).
- **Open Q2/#76:** preserve admitted plans/generations on resume; explicit fenced deletion is
  today's recovery. #72 must extend this fence with capacity reservations rather than bypass it.
  Settled at approval, 2026-11-13: explicit fenced deletion until #76; no timeout reaping (see decisions.md, Settled at approval).
- **Open Q3:** before upgrade, quiesce create/move/drain mutations, stop all old metadata writers
  and drain controllers, deploy all new instances, inventory old claims/markers, initialize
  the gate G above every retained legacy D, verify admission refuses0, then resume.
  Mixed writers are unsafe; new controllers fail closed if generation is 0/CheckDrain Unimplemented.
  Settled at approval, 2026-11-13: this maintenance-barrier procedure is the rollout (see decisions.md, Settled at approval).
- **Scale:** full-prefix inventory uses current KV List memory costs. Pagination/operating bounds
  are #72 work; this issue promises bounded transactions, not production-scale certification.
- **Security/safety:** labels remain trusted as today; admission changes no epoch/ISR/lease rule.
  Completion authorizes no destructive host action. No accepted risks or approvals are recorded.
  Settled at approval, 2026-11-13: the plan is approved and Q1's availability risk accepted (see decisions.md, Settled at approval).

## ADRs

ADR 0001 (Go control plane) and 0002 (dedicated linearizable etcd) are consistent. ADRs 0005/0006
remain unchanged. Main-checkout proposed ADRs 0007–0011 are not accepted prerequisites and none
supplies placement serialization. [Issue-local ADR](adr-proposal.md) is Proposed, pending Q1–Q3.
Settled at approval, 2026-11-13: the issue-local ADR is accepted, with Q1–Q3 settled (see decisions.md, Settled at approval).

## Parallelizable?

No independent Rust runtime work. Implement the Go contracts/fence before controller integration;
the model/fixture can be developed against those transitions, then replayed against the service.
