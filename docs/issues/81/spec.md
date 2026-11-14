# Issue #81: Bounded ISR updates — specification

[Intent](intent.md). Settled choices: [decisions](decisions.md). How: [design](design.md).
Approved 2026-11-13; not implemented.

## Terms

- **Batch limit:** the largest change count accepted by one metadata `ChangeIsr` request;
  currently 64. This bounds chunks changed by a transaction, not volume size.
- **Pending change:** a chunk's necessary ISR or replica-set transition not yet confirmed by
  authoritative metadata, whether queued, in flight, refused or awaiting a retry.
- **Effective write quorum:** the existing ISR plus joiners that the protocol already requires
  to acknowledge. A member being removed counts until authoritative removal is known.
- **Current view:** metadata for the target's epoch, with a version for each chunk.

## The invariant

Every completed block write is durable on its effective write quorum. Batching cannot permit
an uncommitted shrink, admit an unsynchronized joiner, remove a move source prematurely, or
commit under an obsolete epoch. Each metadata transaction remains atomic for its named chunks;
there is no new requirement for a volume-wide all-chunk transaction.

## Behaviour

**S1. Bounded requests.** Every emitted and forwarded `ChangeIsr` contains between one and the
batch limit changes, names each chunk once, and is otherwise subject to the existing RPC rules.
The Rust producer, adapter and Go receiver agree on the supported bound. A volume above that
count is not rejected or made unrecoverable solely because it needs several transactions.

**S2. Preserve deferred work.** Changes excluded from a batch remain pending, with their
uncertain extents, dirty sets, membership state and required acknowledgements intact. Queueing
or omission is not a metadata commit and cannot release a client acknowledgement early.

**S3. Authoritative per-chunk progress.** Apply successes only to the named chunks using their
expected versions and the current target epoch. Partial completion across separate batches
may leave other chunks waiting; those chunks must not inherit another batch's result. Batches
may contain shrink, expansion or move transitions only when each passes the existing rules.

**S4. Recover ambiguous outcomes.** A lost or delayed reply, refused compare, or retry after
commit obtains or adopts the authoritative state. A transition already committed is not
applied twice. Older versions cannot replace newer ones, a stale batch answer cannot answer a
different batch, and a transport failure does not erase pending work. Every retry also fits S1.

**S5. Epoch fencing.** An attach or takeover that advances the epoch prevents the earlier
target's remaining batches from committing. Observing a higher epoch deposes it as today.
No batch spanning a timeout or restart reuses another target's ownership or acknowledgement.

**S6. Membership safety.** A joiner counts in leases and write acknowledgements at the same
point as in the existing protocol, even while its metadata change waits for a batch slot.
Removed members count until removal is authoritatively committed. Move source removal waits
for the destination's completed synchronization and preserves the minimum ISR. Batching does
not relax readiness or reconciliation.

**S7. Eventual progress.** With a live current target, sufficient reachable replicas and
metadata service, finite pending transitions eventually complete through bounded requests.
Repeatedly changing early chunks cannot starve an otherwise eligible later chunk indefinitely.
No request waits for all volume chunks to fit one transaction before it can progress.

**S8. Production-model agreement.** Simulator and engine metadata fixtures enforce the real
batch bound and the same atomic refusal behavior. Boundary and interoperation tests must fail
if the producer emits an oversized proposal again. A new safety rule introduced by the chosen
batching design requires a simulator check and a deliberately broken variant it catches.

**S9. Existing service and compatibility rules.** Oversized external requests remain refused
without a write; this plan does not raise the service limit. Authentication, target-only ISR
mutation, conditional read-dependent etcd writes, durable all-quorum acknowledgements and
steady-state metadata independence remain unchanged. No protocol, token or disk-format
version is introduced by batching alone.

## Tests

- **S1–S3:** core tests at 1, 64, 65, 128 and 129 pending chunks; capture each emitted batch,
  commit it independently, and verify its bound, unique chunk IDs and deferred state. Cover
  peer-loss takeover, ordinary shrink, rejoin and mixed moves, not only new target startup.
- **S2, S6:** issue writes while their required member's shrink waits behind another batch;
  withhold the acknowledgement and verify no completion before that chunk's commit. Delay
  join commits and move-source removal; check minimum ISR and lease/readiness invariants.
- **S3–S5:** lose an applied reply, refuse one compare, reorder old replies, time out, restart,
  and advance the epoch between batches. Verify no cross-batch reply association, duplicate
  transition, version rollback or obsolete-owner success.
- **S7:** a 129+ chunk workload with recurring early-chunk transitions eventually completes
  every continuously eligible late-chunk transition once faults cease.
- **S8:** production-bound engine metadata fixture and simulator campaign; preserve the
  audit's 65-chunk counterexample. Catch oversized-proposal and any new safety-rule mutations.
- **S1, S4, S9:** Go service tests accept the boundary, atomically reject limit+1, and retain
  authorization/epoch/version refusal. Rust-client/real-Go-service interoperation drives
  multiple batches and verifies eventual recovery and the expected chunk state.
- `make test race contracts sim` proves the affected code, metadata vectors, client/server
  boundary and simulation. `make lint hygiene` remains required before a pull request. No
  destructive lab run is needed for this issue's acceptance.

## Open questions

None requiring a new architectural decision. Scheduling and shared-bound enforcement are
implementation choices for the design; they must satisfy fairness and the existing safety
rules rather than silently changing them. Proposed ADRs 0007–0011 remain unaccepted.
