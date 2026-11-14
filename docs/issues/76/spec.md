# Issue #76: Interrupted volume creation — specification

[Intent](intent.md). How: [design](design.md). Approved 2026-11-13; not implemented.

## Terms

- **Create identity:** the volume name and normalized full create specification: size, chunk
  size, replicas, minimum ISR, preferred node, failure domain and placement policy.
- **Operation:** a unique durable identifier for one attempted creation; never reused.
- **Plan:** the immutable initial chunk placement/ISR values persisted for that operation.
- **Admission:** the transaction claiming the absent name and binding its operation and plan.
- **Claim revision:** admission's header modification revision; the volume's eventual first epoch.
- **Helper:** an identical requester or internal recovery worker advancing conditional progress.
- **Legacy claim:** a creating header without a recoverable operation and full create identity.

## The invariant

A name has at most one admitted creation. Only the plan admitted for that incarnation can be
published. Every public chunk write, cleanup and visibility transition is fenced against
concurrent progress and deletion/recreation. A volume becomes visible only after all of its
initial chunks are durable. Successful retries never reset an epoch, ISR or later placement.
`NoRootResurrection`: once GC removes an operation root, it never exists again for that ID.
`NoUnprovedTombstoneBypass`: skipping a new tombstone requires the exact scoped incarnation
never to have published or authorized Attach/tokens, and the recognized-writer/exclusion/
Creating/zero-epoch/empty-target proof. Earlier incarnations' tokens are distinct.

## Behaviour

**S1. Canonical identity.** Normalize API defaults before comparing full create specifications.
Equivalent default spellings are identical. A creating or completed operation with another
identity returns AlreadyExists without changing it. Node liveness, load and current topology
are not part of retry identity. Preferred node is empty or a valid bounded node ID; validate all
identity/encoded-size bounds before any staging. Persisted oversized/malformed identity fails
closed. New operation IDs distinguish delete/recreate incarnations and are never resurrected
after root removal, including through an ambiguous initial transaction's backend retry.

**S2. Durable admission.** Before claiming the name, persist a complete immutable plan in bounded
transactions. Admission binds that plan and identity, requires name and tombstone absence, and
validates #78's placement generation when integrated. Incomplete preparation cannot publish
public chunks or block a drain as an admitted reservation. Preparation fenced before admission
may be safely discarded and replanned under fresh eligibility.

**S3. Resumption.** An identical request first looks for an admitted/completed operation, before
new placement or current capacity checks. Helpers resume exactly its persisted plan and admission
generation. They neither erase progress nor silently replan under old admission authority.
No original creator, lock, service affinity or expiring ownership lease is required.

**S4. Conditional progress.** Helpers safely cooperate through cleanup, publication and
finalization. A committed but unanswered transaction is discovered by durable reread and cannot
be replayed as destructive cleanup. Concurrent helpers cannot rewrite already visible chunks,
reset ISR/placement, or clear a deletion mark. Batches respect operation and byte limits.

**S5. Visibility and epochs.** Incomplete volumes remain absent from normal reads/listings and
unattachable. Finalization atomically makes the complete plan visible and records the original
claim revision as first epoch and initial epoch. A successful repeat returns the existing
volume without mutation, including after attach or a move. Concurrent deletion/recreation may
produce Aborted/NotFound, never a mixed successful response; compose with #77's read validation.

**S6. Deletion.** Deleting an incomplete operation first revokes every helper's publication
rights, then preserves normal tombstone/replica cleanup. Stale helpers and reconcilers cannot
delete or publish into a replacement. Re-creation waits for the tombstone and starts at a newer
claim revision, preserving first-epoch fencing. A recognized never-published creating claim may
instead use S8's proof-based rollback; existing tombstones are never removed by that exception.

**S7. Reconciliation.** Service startup and periodic reconciliation resume admitted operations,
reclaim safely fenced preparation/completed plan copies, and retry recoverable errors. Every
pass and contention loop is bounded and cancellation-aware; eventual completion assumes an
available store and fair scheduling. Discovery, value decoding and orphan cleanup are bounded
too, including headers and interrupted legacy cleanup; no full-prefix materialization precedes
the budget. Reclaim eligible private roots/plans using delete-only transactions that work under
etcd NOSPACE; a stalled admitted operation is not silently abandoned to free space. A timeout
alone cannot abandon admitted work. Classify Deleting before Creating/missing-root validation;
an interrupted deletion remains a deletion even when both flags are set. Invalid or
missing admitted plan data fails closed with an actionable operation identifier; never invent
replacement placement or report success. Persistent corruption requires explicit administrator
repair/deletion and is not disguised as a healthy permanent hidden record.
Page cursors advance only through examined keys; retain bounded unconsumed tails or rescan them.
Exhausting a pass cannot skip a fetched tail or wrap a source before its tail was examined.

**S8. Compatibility and authority.** Create/delete remain administrator-only; internal recovery
exposes no new remotely callable privilege. Retain full identity on new completed headers.
Legacy visible volumes retain their existing behavior. Legacy creating records cannot be
adopted by guessing missing policy/placement. With old writers excluded, a validated Creating
header at epoch 0 with no target, whose recognized writer only clears Creating on publication,
has never authorized Attach/tokens for that incarnation. Fence/remove only that exact header;
page away metadata leftovers without a new tombstone. Earlier-incarnation tokens remain below
the next first epoch (#30). Unknown/nonzero-epoch shapes are not eligible: retain conservative
deletion/diagnostics. Existing tombstones still block new admission. Scalar-incompatible retries
are refused; incomplete bounded cleanup returns Aborted and resumes. Document this proof and
exception, the upgrade barrier and legacy-policy limitation.

**S9. CSI retries and evidence.** Metadata returns success for an identical new-format complete
or resumed create. CSI must preserve AlreadyExists for full-identity conflicts rather than
turning a numeric-property match into success. Test crashes and ambiguous responses at every
state transition on both stores; document identity, recovery and operator diagnostics.

## Tests

- **S1–S5:** service/backend suites cover normalized defaults and each conflicting field,
  differing current topology, multi-batch plans, two creators and helpers, every pre/post-commit
  failure, hidden reads, successful finalization and repeats after attach/ISR/move changes.
- **S6:** suspend each helper transaction, delete/confirm/recreate, then resume it; no old write
  or cleanup lands and the replacement's first epoch exceeds every prior epoch. Inject omitted
  deletion-incarnation/header fences and substitution of public by-name deletion: the model
  and real Go CAS schedule must both catch replacement damage.
- **S7/S8:** restart without a client; concurrent workers, abandoned preparation, lost final
  replies, corrupted/missing plan records, cancellation, bounded work, legacy empty/partial
  claims, legacy visible headers and maintenance-barrier migrations.
- **S1/S7/S8:** oversized preferred node/identity before any write; million-key manifests and
  headers with bounded page/RPC/allocation/work counters; Creating+Deleting after root GC;
  initial-root commit/reply loss/GC/backend replay; embedded-etcd NOSPACE root/orphan reclamation,
  read/delete failures and compaction/alarm handling, with no false progress or silent rollback.
- **S8/S9:** authorization matrix and CSI tests for full-spec conflicts, resumed success and
  legacy behavior. A deterministic sans-I/O recovery model checks the invariant and catches
  injected missing-progress-compare and early-publication bugs; Go schedules test real CAS.
- **S1/S7:** the model retains retired operation IDs and original backend transaction compares.
  Initial commit → lost reply → root GC → replay asserts NoRootResurrection. Inject
  `missing_birth_compare`, require a counterexample, and replay the schedule through actual
  Go transactions on memory/etcd; a functional no-resurrection test alone is insufficient.
- **S6/S8:** model publication/Attach/token history per logical incarnation independently of
  mutable header flags. Assert NoUnprovedTombstoneBypass on every shortcut. Inject
  `weakened_unpublished_classifier` (e.g. Creating alone) against published/authorized or
  unsupported shapes; require a counterexample. Replay Go transitions with replacement and
  delayed prior-incarnation tokens; valid never-published rollback remains allowed. The Go
  replay performs actual publication/Attach/token issuance before unsupported-shape injection,
  rather than inferring authorization history from seeded current flags. Include schema-decodable
  Creating headers with positive epoch/target so the weakened classifier itself is exercised.
- **S7:** populate all four sources with >=64 stable eligible keys each, cap each pass at64
  examined items, pause mid-page and run through wraps/restart. Every retained key eventually
  gets its work attempt; assert no unexamined tail is skipped and page memory stays bounded.
- Run appropriate `make test race sim contracts lint hygiene` checks during implementation;
  no runtime or lab checks are claimed by this draft.

## Open questions

- **Q1:** durable staged plans temporarily duplicate chunk metadata. The proposed ADR selects
  that design for review; a compact replayable plan would require a stable placement algorithm
  and a bounded durable input inventory. No storage/availability trade-off is accepted yet.
  Settled at approval, 2026-11-13: the immutable staged plan, accepting its temporary duplicate metadata (see decisions.md, Settled at approval).
- **Q2:** old visible headers lack full policy identity. Preserve their current scalar matching
  only as a documented compatibility exception; #76 cannot recover historical policy intent.
  Settled at approval, 2026-11-13: scalar matching stays as a documented compatibility exception (see decisions.md, Settled at approval).
- **Q3:** the draft requires a maintenance barrier excluding old writers. A rolling upgrade
  needs an explicit version fence beyond this scope. Unproved legacy states need conservative
  cleanup; pre-existing tombstones still gate reuse. No below-proof cleanup exception is accepted.
  Settled at approval, 2026-11-13: a maintenance barrier excluding old writers; no rolling mixed-version safety is claimed (see decisions.md, Settled at approval).
