# Issue #78: Placement admitted after drain marking — specification

[Intent](intent.md). How: [design](design.md). Approved 2026-11-13; not implemented.

## Terms

- **Admission:** durable acceptance of a creation's planned placement, or of a move destination.
  Planning and listing alone confer no right to publish placement.
- **Reservation:** a durable creating incarnation that can still publish its admitted chunks.
- **Drain generation:** an identity for one continuously marked draining interval of one node.
  Undrain ends it; a later mark starts a different interval.
- **Completion:** validated absence of active placement, incoming moves and unresolved earlier
  reservations for that interval. It is a metadata result, not proof of physical erasure.

## The invariant

While one drain generation remains marked, no admission ordered after its mark can target the
node. Work admitted earlier is either represented by a reservation, by chunk placement/move
state, or fenced from publication. Completion cannot certify zero work while any such work
can still publish to that node. A stale creator or completion check cannot cross an incarnation
or drain-generation boundary.

## Behaviour

**S1. Atomic admission.** Creation admits its planned placement at a conditional durable claim;
move admission is the conditional publication of its requested destination. Each commit validates
lifecycle state used to admit it. Drain marking and admission cannot both succeed in the order
where marking wins but the admission uses pre-mark eligibility. A conflict retries from fresh
planning or returns a bounded contention error; polling is not a substitute for a compare.

**S2. Earlier reservations.** A creation admitted before marking may finish publishing its
original plan. Its reservation remains visible to completion checks until all chunk batches are
published and the volume is visible, or until a durable fence prevents every delayed writer.
Crashes, cancellation and identical concurrent requests cannot silently discard that reservation.
No partial creation becomes an attachable volume.

**S3. Complete accounting.** A drain cannot complete while earlier reservations might publish
there, while a chunk names the node as a replica, or while a recorded move can add it. Hidden
creating/deleting records and corrupt inventory cannot be treated as an empty visible listing.
It may wait conservatively for unrelated earlier reservations (Q1); reports distinguish this
wait from visible replicas waiting for a target. Separate tombstone cleanup remains tracked.

**S4. Validated completion.** Completion validates the same drain generation after accounting.
An undrain or undrain/re-drain during its scan invalidates the result. Controllers use this
validation, including when their ordinary volume scan is empty. Cancellation leaves admission
closed until an explicit undrain. A result certifies its generation at its validation instant;
it is not authority to shut down a host after that generation ends.

**S5. Direct move destinations.** A currently draining destination is refused with a failed
precondition, for every caller and policy, and no move is recorded. If marking wins a concurrent
commit, the move retries and sees the mark. A move admitted first may finish under existing
ISR/epoch rules; its incoming destination is counted until it is present as a replica or the
move is safely removed. #18's domain rules compose with this lifecycle check.

**S6. Bounded and restartable.** Claims, publication batches, marking, inventory and validation
respect documented transaction limits; the number of selected nodes never becomes an unbounded
compare list. Durable state suffices after service/controller restart. A lost response causes a
safe retry. Contention and corrupt state return errors, never successful empty completion.
There is no timeout that abandons a live reservation.

**S7. Compatibility and authority.** Admission and drain mutation remain administrator-only.
Completion inventory has an explicit administrator caller rule. Existing volume/chunk epochs,
capability checks, minimum ISR and failure-domain placement constraints are preserved. A rollout
must exclude writers that do not enforce this protocol before its guarantee is advertised.
Existing in-progress creations remain conservatively accounted for. Before new admissions,
initialize a positive gate newer than retained legacy drain markers; only legacy headers may
retain generation0. Idempotent mark/unchanged registration cannot skip that initialization.

**S8. Executable checks and documentation.** Deterministic schedules cover both commit orders,
publication phases, incoming moves, cleanup, restarts and cancelled/restarted drains. A sans-I/O
admission model checks the invariant and catches injected missing-admission-check and
missing-reservation-accounting and missing-final-generation-validation bugs. Model scan and
validation as separate phases. Document the linearization points, blocking reservations,
generation meaning and the limitations of metadata completion.

## Tests

- **S1, S2, S6:** Go service tests on memory and etcd with intercepted reads/transactions;
  reproduce audit F03, then vary creator count, chunk batch boundaries, claim/finalization
  response loss, registration changes and interrupted creation/deletion. Assert bounded compares
  and operations and no publication by a fenced incarnation.
- **S3–S5:** service and drain integration tests: hidden creator, pending incoming move with no
  destination replica yet, undrain during scan, repeated mark, re-drain, bad JSON, failed lists,
  successful cleanup and controller restart. Zero visible volumes alone never certifies success.
- **S7:** mTLS authorization matrix; generated API and unchanged Rust-client interoperability;
  legacy creating headers and legacy drain marks during the documented rollout barrier.
- **S8:** simulator model with fixed seeds/exhaustive short schedules; each injected bug yields
  a counterexample within the same budget. Go schedules exercise actual CAS rather than relying
  on the model alone. Run relevant `make test race sim contracts lint hygiene` targets later.

## Open questions

- **Q1. Reservation precision:** draft design uses a global placement revision and conservatively
  waits for all earlier creating headers, even if their plans use other nodes. A per-node index
  avoids that coupling but requires bounded multi-stage reservation admission. No availability
  trade-off is accepted yet; see the issue-local proposed ADR.
  Settled at approval, 2026-11-13: the global gate and conservative scan, accepting unrelated drain blocking (see decisions.md, Settled at approval).
- **Q2. Recovery:** #76 owns resumable abandoned creates. Until it lands, a stranded reservation
  needs explicit incarnation-fenced `DeleteVolume` cleanup and may block a drain indefinitely.
  There is no automatic timeout cleanup in this issue.
  Settled at approval, 2026-11-13: explicit fenced `DeleteVolume` until #76 lands; no timeout cleanup (see decisions.md, Settled at approval).
- **Q3. Rollout:** draft requires a maintenance barrier excluding old metadata writers/controllers;
  a rolling mixed-version guarantee would require an additional capability/version fence.
  Settled at approval, 2026-11-13: the maintenance barrier; no rolling mixed-version guarantee (see decisions.md, Settled at approval).
