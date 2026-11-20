# Accepted with the plan, 2026-11-13: Node failure requests ordinary moves and preserves earned wait credit

- **Status:** Accepted with the plan, 2026-11-13; numbered and moved into docs/adr/ when implemented.
- **Date:** 2026-11-12
- **Issue:** [#45](https://github.com/jsfakian/Flaminio/issues/45)

## Context

The existing [spec](spec.md) scopes this slice to node-level failure state and automatic
ordinary moves. Metadata currently has liveness TTL and drain marks, no disk lifecycle or
capacity ledger. A wrong failure declaration must never change authoritative data membership.
A durable missing timestamp alone would count metadata outage time, contradicting S5. Local
health probes also cannot prove every instant of service availability or client reachability.

## Decision

Settled at approval, 2026-11-13: the lifecycle records and the wait policy below are accepted as stated (see decisions.md, Settled at approval). S5's authority was settled the same day at the user's instruction, and rounds 1 to 7 of the plan review (2026-11-14) changed this text after approval (see decisions.md).

Propose separate v1 etcd node lifecycle records: Active, Missing, sticky Failed, with diagnostic
wall timestamps, authenticated actor and revision. Heartbeat recovery and timeout failure must
serialize through transactions with alive TTL and state; state revisions are not disk epochs.
State can influence destination admission and requests for replacement, never sessions, ISR,
fencing, minimum or client target ownership. Reuse #78's shared placement-gate, bumped only into
and out of Failed; preserve ordinary MoveReplica and maintenance-attach contracts. Keep
unsupported minimum-ISR progress explicit. An administrator's failed-for-good declaration is
#13's forgetting of the node, and the only way to forget one: only a Failed node that is not
alive. The automatic, reversible Failed never forgets, and is no evidence that a node is gone.

Propose one cluster wait policy (default600 s) and a persisted service-opportunity credit clock,
fenced by one TTL observer identity. Credit only consecutive bounded successful observations;
restarts, failed reads, ticks more than 2 s after the previous and ownership gaps earn zero
credit. Conditional clock writes, made only while a node is missing, prevent duplicate credit; missing records retain their credit origin across restart. Display timestamps do not
authorize failure. Metadata outage never provides storage-side fencing evidence.

**S5's authority** is the observed-opportunity definition: the wait counts only the credit
clock's credited intervals: each 1 s tick of the live owner that saw an active node's heartbeat
commit at most 5 s before, at most 1 s, none for a tick more than 2 s late. A heartbeat
committing is the opportunity itself, so the probe writes nothing and checks no certificate or
wall clock. Probes cannot see up to 6 s of each outage (the interval of the tick that sees the
last heartbeat, at most 1 s, plus the 5 s after that sighting), nor reachability from each node
(a change
to the user-settled definition, round 3, flagged in decisions.md); that is the
definition, not a proof of exact availability. Automatic timeout failure runs on it while the
cluster's policy is on: off by default, turned on by an administrator once every metadata
instance is new, with an implementation whose credit clock has passed its review and tests.

Only this early slice is proposed. Many-to-many copier, adaptive rate/background priority and
largest-failure-domain reserve remain #45 steps3–5. #72's ledger admits each rebuild move, as
it does a drain's (a dependency). No private controller reserve, per-disk placement or capacity
claim is introduced here.

## Options

- **Wall timestamp deadline:** simple and durable but counts service outages and wall steps;
  reject as an implementation of S5.
- **Controller memory deadline:** loses elapsed wait on restart and permits divergent observers;
  reject as the persistent contract.
- **Automatically admit a returned Failed node:** can oscillate rebuilds and reuse unstable
  nodes; retain the spec's proposed explicit administrator admission, pending approval.
  Settled at approval, 2026-11-13: explicit administrator admission (spec Q2 (a)) (see decisions.md, Settled at approval).
- **Edit ISR on failure:** makes a policy observation authorize data loss/stale reads; prohibited.
- **Use an external continuous availability authority:** might provide stronger evidence but
  its definition, trust and implementation are not supplied; do not invent it.

## Consequences

More metadata CAS/TTL state and rollout coordination are required; no older metadata instance
may admit allocations after lifecycle policy is enabled. A partially observed legacy node may
require an operator declaration before failure history is known. Quorum/probe outages delay
replacement. Manual/incorrect declarations can cause copies, but must not authorize I/O safety.
Same-node replacement-disk identity and held/minimum-ISR blocked volumes remain known limits.
Where RF equals the number of data nodes (ADR 0006's 2-node edge site), a failed node's chunks
have no destination, and once back the node stays failed, so creations of that RF are refused
until an administrator admits it: such a site keeps the policy off or the wait long. No proposed
policy or risk is accepted by creating this document.
Settled at approval, 2026-11-13: this proposal, its policy and its stated limits are accepted with the plan (see decisions.md, Settled at approval). Settled 2026-11-13 (user's instruction): S5's authority is the observed-opportunity definition (see decisions.md).

## Confirmation

Memory/real-etcd transaction interleavings, cross-language transition vectors, ordinary move/
maintenance tests, lifecycle-invariant simulator checker and fail_removes_isr mutation in the
design. Explicit known-gap tests do not certify unknown inter-probe availability, as the
observed-opportunity definition accepts. Shared lab runs (`make rebuild-test`) remain separately
authorized, where #83's admission lets volumes attach. Issue-local numbering avoids collision
with other batch proposals.
