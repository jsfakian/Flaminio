# Creation retains an immutable admitted plan and cooperative progress

- **Status:** Accepted with the plan, 2026-11-13; numbered and moved into docs/adr/ when implemented.
- **Date:** 2026-11-12
- **Scope:** Issue-local proposal; no global ADR number or acceptance is assigned.

## Context

[Issue #76](https://github.com/jsfakian/Flaminio/issues/76) and its [spec](spec.md) require
interrupted creates to finish after service restart and lost commit responses. Current creation
has only a hidden header and an in-memory placement plan. Writing a million-chunk plan in the
claim header would exceed practical etcd request limits. #78's draft admission contract requires
recovery to preserve the original admitted placement and generation. Revision-based first epochs
and deletion tombstones must continue fencing older incarnations.

## Decision proposed for review

Persist an operation-private placement manifest before admission, using bounded transactions and
an optimistic progress root in a separate index from plan values. Admit the sealed manifest and
creating header atomically under name/tombstone absence and #78's placement gate. Retain full
normalized identity in the header. Validate preferred-node IDs and encoded record/identity bounds
before staging; bound discovery keys, fetched values and processing, not only mutation counts.

During cleanup/copy/finalization, compare both the original header claim revision and progress
root revision. Atomically advance progress with each public chunk batch. Identical callers and
service reconcilers may help the same operation without owning a lease or a leader lock. Final
visibility and initial first epoch are committed together. No response, timer or missing process
is authority to restart cleanup or replace an admitted plan.

Private preparation may be fenced before admission by CAS-deleting its root. Orphan plans are
then deleted under root absence and per-key revisions, with no progress Put or lease. These
transactions remain executable under etcd NOSPACE; they never revoke a live admitted creation
solely for quota/time. A fixed 16-shard permanent birth-revision fence in initial root creation
prevents even transparent backend retries from resurrecting a deleted operation root. All later
root/plan writes require the existing positive root revision. No per-operation immortal marker.

Admitted work is completed, explicitly deleted under its incarnation fence, or quarantined on
corruption. Classify Deleting before missing-root checks. Under old-writer exclusion, a recognized
Creating header with epoch0/no target has never passed Attach or issued this incarnation's token:
publication clears Creating and starts a positive epoch atomically. Such exact headers can be
CAS-removed without a new tombstone; bounded orphan metadata cleanup follows under header absence
and per-key revisions. Earlier-incarnation tokens remain below the next first epoch (#30).
Unknown/target-bearing/nonzero-epoch shapes get no exception; existing tombstones and all ordinary
authorization/protection preconditions remain. Published deletion stays conservative. Keep legacy
visible scalar-only matching rather than inventing missing policy identity.

## Options

- **Immutable staged plan (proposed):** straightforward replay and bounded transactions; costs
  temporary duplicate metadata and extra staging/GC writes. No placement-algorithm replay promise.
- **Compact deterministic plan inputs:** potentially less metadata, but needs a durable bounded
  inventory and versioned stable placement replay across upgrades. Current code has no such contract.
- **Rollback every interrupted operation:** less retained plan state, but delayed creators still
  need fencing and each retry may wait for tombstone cleanup. Replanning could defeat earlier
  drain reservations unless rollback is fully complete and fresh admission is obtained.
- **Single-writer lease:** reduces cooperative contention, but does not remove lost-response,
  immutable-plan or stale-writer fencing requirements. Lease expiry cannot supply safety.

## Consequences

The metadata service adds operation manifests and a periodic recovery worker; no data-path I/O,
new RPC, on-disk format, dependency or capability format is introduced. Peak chunk metadata may
approach twice the initial plan size per operation, plus competing preparation. Foreground
requests can be fenced during private preparation and retry. Old writers must be excluded;
rolling mixed-version safety is not claimed. Corrupt plans require explicit administrator action.
Delete-only GC removes eligible live keys but does not automatically reclaim backend MVCC space
or clear its alarm; normal operator compaction/defrag/disarm remains separate. Bounded latest-key
pages need no full Mem snapshot; bounded values reject oversize before decode. Permanent birth
keys have fixed cardinality; roots/orphan plans require no Put to discover or reclaim.

Q1–Q3 in the [specification](spec.md) remain open: duplicate storage cost, legacy scalar matching,
and maintenance rollout. No availability/storage trade-off or legacy-policy ambiguity is accepted
by this draft. Accepted ADRs 0001 and 0002 remain consistent; no accepted ADR is superseded.
Settled at approval, 2026-11-13: Q1–Q3 take the plan's recommendations, accepting the duplicate storage cost (see decisions.md, Settled at approval).

## Confirmation

The [design](design.md) maps each rule to memory/etcd transaction schedules, CSI/auth tests and
a deterministic sans-I/O model. Every transaction boundary is tested before/after commit and
with a lost response; competing helpers/delete/recreate/GC must preserve incarnation authority.
Mutations removing the progress fence, finalization completeness and destructive incarnation
fence, or substituting public by-name deletion, must be caught by the model and real Go CAS
schedules. Embedded-etcd real NOSPACE, root-birth replay, Creating+Deleting, large key-only
inventories and oversized identities are required regression cases, not simulated quota evidence.
The model explicitly asserts NoRootResurrection using retired IDs; missing_birth_compare must
fail initial-commit/lost-reply/GC/backend-replay. Independent publication/Attach/token history
asserts NoUnprovedTombstoneBypass and catches weakened_unpublished_classifier on unsupported or
authorized shapes. Actual Go transitions replay both counterexamples. Four-source partial-page
tests prove no unexamined tail/cursor is skipped under the pass budget, including wraps/restart.
No code or tests have been run to implement this proposal.
