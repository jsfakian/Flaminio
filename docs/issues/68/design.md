# Issue #68: Instant restore — design

[spec](spec.md) · [decisions](decisions.md) · [ADR, Accepted with the plan, 2026-11-13](adr-proposal.md)

Approved 2026-11-13; not implemented.
Protocol and simulator increment. The existing intent's requirement to accept an ADR
before runtime design remains an approval gate; the alternatives below are proposals.
Settled at approval, 2026-11-13: the ADR and the recommended alternatives are accepted with the
plan (see decisions.md, Settled at approval).
No store, API, networking, credentials or live restore implementation is included.

## The facts it starts from

- `flaminio-repl/src/replica.rs:35–58` actions read bytes, digests and mapped extents;
  `:262` filters resync copies by live writes. None represents a remote reference.
- `flaminio-format/src/meta.rs:450` has only Unmapped, Zero and Cluster. The bottom
  unmapped layer resolves to zeros. It cannot encode this proposal safely.
- `flaminio-repl/src/target.rs:1450` derives resync holes from mapped ranges; `:1644`
  selects a readable ISR source. Treating missing base data as holes loses it.
- `flaminio-repl/src/sim/check.rs:32` checks registers initially zero;
  `sim/mod.rs:1408` calls it for each block. Repository values must become initial values.
- `meta/domain/domain.go:41` records Creating/Deleting but no restore/base state.
  #51/#52/#54's dependency intents describe future snapshot/repository/restore work.
  #54's scoped first step provides no general encrypted restore path.

## Representation and authority (S1, S2, S6)

Add model/core types in `flaminio-repl/src/hydration.rs`:
`BaseId { repository, manifest, source_incarnation }`, `Presence::Remote(BaseId, Block)`
and `Presence::Local`. Replica data is `BlockState::Remote(ref) | Local(Value)` in the
simulator; runtime bytes remain generic. Explicit zero is `Local(0)`. A remote reference
is immutable and range-validated against a per-volume base; it is never an arbitrary URL.
Ordinary chunks initialize Local(0); restored chunks initialize Remote with nonzero base.

Introduce typed state summaries separate from current CRC vectors: `StateDigest::Remote(ref)`
or `Local(checksum)`. Equal content with different presence is still a state mismatch.
A new `StateRanges` describes all local and remote runs, including explicit local zeros.
Reconciliation copies the chosen source's state, not a fabricated byte value. During uncertain
recovery this may discard an unacknowledged write, under the existing source-selection rule;
acknowledged writes are local at every authoritative ISR member and cannot choose Remote.
Restore base identity must match before reconciliation; a mismatch refuses readiness.

Resync sends Local data and Remote references under the same existing live-write filter.
Copies of Remote explicitly install that state and cannot be encoded as ordinary discard.
A destination starts non-ISR; it joins only after the usual copy/filter barrier covers both
kinds. A move can copy Remote without bucket access. The typed state is durable in the model;
real durable format atomicity is a later gate, not inferred from model success.

## Fetch and writes (S3–S5, S7)

Add core outputs `FetchBase { ticket, base, first_block, blocks }` and input
`on_fetch(ticket, Result<VerifiedBase, FetchFailure>)`. A ticket binds volume incarnation,
epoch, chunk, session/generation and a monotonic per-target request id. The core never does I/O.
The simulator's repository returns base values, unavailable, missing or corrupt; verification
failure cannot construct `VerifiedBase`. The later worker authenticates the full repository
object before producing that input. Its byte and concurrency budgets remain runtime gates.

A remote client read locks its logical extent using the existing operation lock, obtains
verified bytes, installs base data durably on the current write set, then uses normal read
lease/readiness checks before replying. A full 4 KiB client write supersedes Remote directly.
The model represents smaller updates as a read/merge/write on that same locked block.
A background fetch need not hold a lock while awaiting the repository: on completion acquire
the block lock, revalidate the ticket/epoch and state, and issue conditional hydration only for
blocks still Remote with that exact reference. If changed, skip rather than overwrite.
Both replica-side acceptance and target scheduling enforce the condition; a target-side check
alone races a delayed network write. Hydration is sent through the current session's ordered
write path, participates in uncertain tracking and resync filters, and receives durable ACKs.
A completed hydration request does not become a client write in the history checker.

Define `HydrateIfRemote { ref, data, version, acked_below }`: install only when the current
state is that exact Remote reference. A Local block replies success-with-skipped, preserving
its bytes. Epoch/session validation precedes it. Repair-state installation is a separate
operation allowed only during reconciliation/resync barriers, not a hydration bypass.
On session loss retry under a fresh ticket; stale replies cannot complete current work.
On restart normal uncertain reconciliation runs before I/O and may re-fetch unresolved base.

Recommended availability policy: unavailable => typed retriable `BaseUnavailable`, missing =>
`BaseMissing`, failed authentication => `BaseCorrupt`. Waiting indefinitely is an alternative
requiring explicit acceptance. The later frontend translates these to failed I/O without
lying about permanence/retry safety; filesystem behavior must be evaluated before release.
Local I/O, attach/detach/delete, and reference-copy moves/resync continue without fetch.
A slow remote reconciliation repair needing only typed references must not initiate fetch.
Fast-read admission additionally requires the entire requested range Local at acquisition;
its normal lock/lease/completion validation remains. No remote content enters the zero path.
Settled at approval, 2026-11-13: the recommended availability policy, typed failure rather than
an indefinite wait (see decisions.md, Settled at approval).

## Completion and base retention (S8, S9)

Add modeled metadata state `restore = Hydrating(base) | Complete` and output
`CompleteHydration { incarnation, epoch, chunk_isr_versions, proof_generation }`.
The target drains hydration requests, collects a no-Remote durable-state acknowledgement for
all chunks from every current ISR member, and records the exact member/version vector.
No operation creates Remote while ordinary I/O is ready; a resync/reconciliation generation
change invalidates the proof. Metadata conditionally compares incarnation, epoch, versions
and Hydrating(base), then marks Complete. The model refuses mixed/stale proofs.
Lost response is read-back/idempotent; a local-only Hydrating volume may safely retry.
A new epoch never reuses the prior target's uncommitted proof.

Base dependency is a durable pin established before exposing Hydrating, released only after
Complete commits or volume deletion commits. Later repository GC must honor pins; this step
adds no GC. Metadata deletion must not wait for the bucket. Only a read-only scoped fetch
worker may resolve the pinned base; replicas/applications receive neither credentials nor
arbitrary fetch capabilities. Model unauthorized/wrong-base inputs as rejected events.
Snapshot/backup/ship requests reject Hydrating before creating a cut or reference.

## Contracts

The named messages above are **sans-I/O model/core contracts**, not wire/proto schemas.
`FetchFailure = Unavailable | Missing | Corrupt | Unauthorized`; failures contain base identity
and logical range, never credentials or plaintext. `VerifiedBase` construction belongs to the
verified repository boundary. Runtime format, frame discriminants, byte limits, proto schemas,
worker authentication, key delivery and frontend status mapping require a later concrete design.
No existing decoder accepts a new meaning for Unmapped. Mixed old/new nodes are refused later.
Completion CAS inputs must remain bounded/batched without permitting partial publication.
Settled 2026-11-13 (user's instruction): the credential distribution protocol is #52's worker
input, its delivery and revocation fixed by the runtime design (see decisions.md).

## Files touched

- `dataplane/crates/flaminio-repl/src/{hydration.rs,lib.rs,msg.rs,types.rs}`: typed contracts.
- `dataplane/crates/flaminio-repl/src/{target.rs,replica.rs}`: S2–S8 state transitions.
- `dataplane/crates/flaminio-repl/src/sim/{mod.rs,check.rs}` and model metadata: S1–S10.
- `docs/replication-protocol.md`, `docs/ARCHITECTURE.md`, followups and issue docs: S11.
  Future store/format/node/net/metadata/CSI changes are explicitly not this increment.

## Tests

| Rules | Concrete planned checks |
|---|---|
| S1–S3 | hydration module units: ordinary zero vs remote nonzero; wrong base; local-only fast permit; mixed state summary; no successful remote-zero read |
| S4–S5 | deterministic simulator: partial merge, full overwrite without bucket, delayed fetch across live write/session/epoch, conditional hydration filter and ACK loss |
| S6 | simulator: crash before/after state ACK, uncertain mixed states, resync Remote reference, move during outage, ISR join before/after durable barrier |
| S7, S9 | scripted repository unavailable/missing/corrupt/unauthorized; local reads continue; lifecycle events finish; no secret in diagnostics; forbid snapshot/backup |
| S8 | stale epoch/version completion, crash around CAS, lost CAS response, delayed Remote repair invalidates generation, post-completion no fetch events |
| S10 | seeded chaos plus mutations listed below, initially nonzero register checker |
| S11 | protocol rule-to-test table and roadmap status distinguish model/runtime/lab |

Extend `check_register` to take an initial value, retaining a zero-start wrapper for existing
callers. A restore block's base supplies that value; hydration events are representation-only.
Add checks for acknowledged state agreement and forbidden repository events after Complete.
Each safety rule has a mutation: erase origin binding; remote-as-zero; unverified fetch;
partial-update zero-fill; hydration overwrite; drop presence during resync; permit stale proof;
serve before lease; authorize wrong base. Dedicated tests must fail with each mutation.
Use `make test sim contracts` for model changes and existing regression behavior; `make lint
race hygiene` for the PR. These tests are planned, not run by this drafting task.

## Lab

None in this model increment. Later authorized #68 lab must measure first pod read before full
hydration, data equality, bucket loss and failover during hydration, then zero bucket requests
after completion. Its setup/destruction/restoration script is a later prerequisite.

## Docs

Update protocol actor state, read/write/copy/completion rules and simulator coverage. Explain
opt-in repository dependence and failure policy in architecture, list later stages in followups.
No README feature-complete claim or undocumented lab/CLI command.

## Risks

The durable layout and worker credential revocation remain open runtime gates, as do #52/#54
repository contracts, per-target memory budgets, partial physical-cluster preservation and
public I/O error mapping. No lab performance/availability evidence exists for instant restore.
A protocol model does not certify durable commit ordering or filesystem behavior. The proposal
retains metadata-free steady-state local I/O; remote I/O explicitly depends on the repository.
Client writes must never inherit repository retries after they have committed.
Settled 2026-11-13 (user's instruction): the credential protocol is #52's worker input; #52's
and #54's repository contracts as their own approved plans settle them (see decisions.md).

## ADRs

ADR0001 consistent: Rust protocol core; worker location requires its own agreed interface.
ADR0002 consistent: lifecycle/completion uses conditional etcd transitions, no per-block metadata.
ADR0003/0004 consistent: no new kernel minimum; explicit remote failure handling.
ADR0005 is not relaxed; current takeover/lease protocol and proposed audit ADR0007 apply.
[Issue-local proposal](adr-proposal.md) is unaccepted; global numbering is deferred for the batch.
Settled at approval, 2026-11-13: the issue-local ADR is accepted with the plan (see decisions.md,
Settled at approval).

## Parallelizable?

No. Settle the proposal and model semantics first. Future Rust storage/fetch and Go lifecycle
work can proceed independently only after exact runtime contracts and formats are designed.
Settled at approval, 2026-11-13: the proposal is accepted with the plan (see decisions.md, Settled
at approval).
