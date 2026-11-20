# Issue #68: Instant restore — design

[spec](spec.md) · [decisions](decisions.md) · [ADR, Accepted with the plan, 2026-11-13](adr-proposal.md)

Approved 2026-11-13; not implemented.
Protocol and simulator increment. The existing intent's requirement to accept an ADR
before runtime design remains an approval gate; the alternatives below are proposals.
Settled at approval, 2026-11-13: the ADR and the recommended alternatives are accepted with the
plan (see decisions.md, Settled at approval).
No store, API, networking, credentials or live restore implementation is included.

Review revision, 2026-11-20: the delivery boundary and independent partial-write/pin
oracles below are proposed changes after approval; they have not been approved.

## Model delivery boundary (S10, S11)

Deliver a separate `hydration` sans-I/O facade and `sim::hydration` driver in `flaminio-repl`.
Its own target, replica, metadata, request/response and action types model Remote state;
existing `target::Target`, `replica::ReplicaCore`, `msg::{Request, Response}` and
`replica::Action` stay unchanged. No conversion to their ordinary byte/hole operations is
provided. Production node/net adapters cannot construct, send or accept this model protocol.
The facade may reuse extent/identity types, but owns hydration state transitions and runs the
existing fencing, session, acknowledgement, lease and resync scenarios against those transitions.
Runtime integration must later replace this isolated boundary with reviewed typed adapters.
Ordinary/default workspace consumers must still compile; both simulator paths remain covered.

## The facts it starts from

- `flaminio-repl/src/replica.rs:35–58` actions read bytes, digests and mapped extents;
  `:262` filters resync copies by live writes. None represents a remote reference.
- `flaminio-format/src/meta.rs:450` has only Unmapped, Zero and Cluster. The bottom
  unmapped layer resolves to zeros. It cannot encode this proposal safely.
- `flaminio-repl/src/target.rs:1450` derives resync holes from mapped ranges; `:1644`
  selects a readable ISR source. Treating missing base data as holes loses it.
- `flaminio-repl/src/sim/check.rs:23–25,113–116` supports only whole-value replacement from
  zero; `sim/mod.rs:322–328,1370–1407` records only scalar writes. Partial writes need an
  independent original-operand history/checker, not a recorded implementation merge.
- `flaminio-node/src/engine.rs:2361–2411` and `replica/mod.rs:355–444,954–962` exhaustively
  adapt the existing core enums; adding hydration variants would require runtime changes.
- `meta/domain/domain.go:41` records Creating/Deleting but no restore/base state.
  #51/#52/#54's dependency intents describe future snapshot/repository/restore work.
  #54's scoped first step provides no general encrypted restore path.

## Representation and authority (S1, S2, S6)

Add isolated model/core types in `flaminio-repl/src/hydration.rs`:
`BaseId { repository, manifest, source_incarnation }`, `Presence::Remote(BaseId, Block)`
and `Presence::Local`. Replica data is `BlockState::Remote(ref) | Local(ModelBlock)`, with
four symbolic lanes per block ([model checks](model-checks.md)). Explicit zero is `Local([0; 4])`. A remote reference
is immutable and range-validated against a per-volume base; it is never an arbitrary URL.
Ordinary model chunks initialize Local([0; 4]); restored chunks initialize Remote with nonzero base.

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

Add facade outputs `FetchBase { ticket, base, first_block, blocks }` and input
`on_fetch(ticket, Result<VerifiedBase, FetchFailure>)`. A ticket binds volume incarnation,
epoch, chunk, session/generation and a monotonic per-target request id. The core never does I/O.
The simulator's repository returns base values, unavailable, missing or corrupt; verification
failure cannot construct `VerifiedBase`. The later worker authenticates the full repository
object before producing that input. Its byte and concurrency budgets remain runtime gates.

A remote client read locks its logical extent in the facade, using the current target's
operation-lock admission/order rules, obtains
verified bytes, installs base data durably on the current write set, then uses normal read
lease/readiness checks before replying. A full 4 KiB client write supersedes Remote directly.
The facade implements smaller updates as read/merge/write on that same locked block.
Its driver records original `Patch { mask, lanes }` at invocation; the separate checker
merges it into its own state, with concrete overlap cases in [model checks](model-checks.md).
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
The independent per-transition [pin monitor](model-checks.md) binds admission and release to
the immutable requested destination incarnation/base and actual successful durable commits.
It checks the modeled durable pin state even while the repository retains every object.

## Contracts

The named messages above are **sans-I/O model/core contracts**, not wire/proto schemas.
They belong only to `hydration::{Target, Replica, Metadata, Request, Response, Action}`.
The existing runtime enums have no new variants, conversions, feature flags or wire mappings.
`FetchFailure = Unavailable | Missing | Corrupt | Unauthorized`; failures contain base identity
and logical range, never credentials or plaintext. `VerifiedBase` construction belongs to the
verified repository boundary. Runtime format, frame discriminants, byte limits, proto schemas,
worker authentication, key delivery and frontend status mapping require a later concrete design.
No existing decoder accepts a new meaning for Unmapped. Mixed old/new nodes are refused later.
Completion CAS inputs must remain bounded/batched without permitting partial publication.
Settled 2026-11-13 (user's instruction): the credential distribution protocol is #52's worker
input, its delivery and revocation fixed by the runtime design (see decisions.md).

## Files touched

- `dataplane/crates/flaminio-repl/src/hydration.rs`: separate facade types and target,
  replica, metadata state transitions, including modeled durable pins (S1–S9).
- `dataplane/crates/flaminio-repl/src/lib.rs` and `src/sim/mod.rs`: expose the separate modules.
- `dataplane/crates/flaminio-repl/src/sim/hydration/{mod.rs,check.rs}`: driver, original
  operand history, independent register/pin checks, scenarios and caught mutants (S1–S10).
- `dataplane/crates/flaminio-repl/examples/sim.rs`: run both ordinary and hydration seed paths
  through `make sim`; ignored mutation tests remain discovered by its existing second command.
- `docs/replication-protocol.md`, `docs/ARCHITECTURE.md`, followups and issue docs: S11.
  Future store/format/node/net/metadata/CSI changes are explicitly not this increment.

## Tests

| Rules | Concrete planned checks |
|---|---|
| S1–S3 | hydration module units: ordinary zero vs remote nonzero; wrong base; local-only fast permit; mixed state summary; no successful remote-zero read |
| S4–S5 | independent operand-history checker: nonzero neighbors, concurrent partial/full/disjoint updates; full overwrite without bucket, delayed fetch across live write/session/epoch, conditional hydration filter and ACK loss |
| S6 | simulator: crash before/after state ACK, uncertain mixed states, resync Remote reference, move during outage, ISR join before/after durable barrier |
| S7, S9 | repository unavailable/missing/corrupt/unauthorized; local I/O/lifecycle progress; forbid snapshot/backup; independent exact destination/base pin monitor, crash/retry, failed/lost completion and deletion/recreation |
| S8 | stale epoch/version completion, crash around CAS, lost CAS response, delayed Remote repair invalidates generation, post-completion no fetch events |
| S10 | seeded chaos, independent register and per-transition pin checks, paired clean/mutant scenarios below |
| S11 | protocol rule-to-test table and roadmap status distinguish model/runtime/lab |

Implement the separate bounded lane-register and pin monitors in [model checks](model-checks.md).
The existing scalar checker and ordinary history stay unchanged. Add acknowledged state
agreement and forbidden repository events after Complete in the hydration driver.
Each safety rule has a mutation: erase origin binding; remote-as-zero; unverified fetch;
`partial_update_zero_fill`; hydration overwrite; drop presence during resync; permit stale proof;
serve before lease; authorize wrong base; `omit_initial_pin`; `early_release_pin`. Dedicated
paired clean/mutant tests must pass clean and fail each mutation against its independent check.
Use `make build test sim contracts` to compile ordinary/default workspace consumers and cover
both model paths; `make lint race hygiene` for the PR. These checks are planned, not run here.

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
The isolated facade does not establish runtime enforcement; its fencing/lease/session and
resync regressions must remain explicit, and later typed runtime integration is a separate gate.
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
