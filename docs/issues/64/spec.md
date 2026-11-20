# Issue #64: Erasure-coding feasibility study — specification

[intent](intent.md) · [decisions](decisions.md) · [design](design.md)

Approved 2026-11-13; not implemented.
Step 1 only: evidence-backed study and outcome (Accepted with the plan, 2026-11-13), no production
or simulator code.
No coding profile/library or new risk is accepted. Steps2–4 and actual m-loss tests keep #64 open.

## Terms

- **k+m:** k data and m parity fragments on k+m independent failure domains; any k matching
  intact fragments of one valid version decode a stripe under the chosen code assumptions.
- **Fragment version:** immutable stripe/write identity; a checksum alone does not identify it.
- **D:** additional whole-fragment failures tolerated after a write is acknowledged.
- **Write hole:** interrupted data/parity update leaves no provably consistent version or loses
  unrelated previously acknowledged data during degraded reconstruction.

## The invariant

The study cannot reuse replicated-member assumptions for fragments without proof. A proposal
must preserve per-block linearizability and acknowledged data under its declared failure model,
including untouched neighbors during partial writes and permanent target-member loss.

## Behaviour

**S1. User/placement scope.** Identify actual use cases and alternatives, whether capacity/cold
classes exist, and minimum independent domains including rebuild spare. No claim that three
lab VMs on two physical hosts demonstrate three independent data failures.

**S2. Capacity comparison.** Compare RF2/RF3 with2+1,4+1,2+2,4+2 using (k+m)/k, m losses and
required domains, plus metadata, retained versions, allocation/stripe padding and rebuild reserve.
Same nominal m is not identical end-to-end protection under different min-ack policies.

**S3. Consistency.** Define what an in-sync fragment means, q acknowledgement and D, read/rebuild
source set, epoch/session fencing, takeover and writes degraded to k. Separate theoretical
minimum k from an accepted risk policy; do not reuse ADR0006 singleton-write permission.

**S4. Crash/write-hole analysis.** Work through partial new/old fragment sets, target-member
permanent loss and partial-stripe updates. Explain durable version/old-data/commit evidence
needed to select one complete version. A scalar highest sequence, equal CRCs or arbitrary
fragment majority is insufficient. An unresolved proof remains a blocker, not pseudocode safety.

**S5. Cost model.** For proposed stripe units, estimate reads/writes/round trips for small/full
writes, healthy/degraded reads, repair and conversion. Label estimates, assumptions and unknown
measurements; existing slow-link RF numbers cannot prove coded performance. State frontend
alignment/atomicity implications and no unsupported multi-block atomicity claim.

**S6. Containment alternatives.** Compare freely writable coded volumes, full-stripe-only
workloads and epoch-fenced cold conversion. Identify what each still needs for crash safety,
metadata identity, concurrent writes, snapshots and rollback. A restriction is not a waiver of
unaligned CSI application behavior unless explicitly validated/rejected at the interface.

**S7. Dependency contracts.** List required changes to #45/#46/#50/#51: multi-source rebuild,
fragment-aware scrub, authenticated identity/version, immutable snapshot references, capacity
and distinct-domain placement. Distinguish current code from dependency drafts.

**S8. Model plan.** Specify fragment/version/durable-decision model, allowed failures, permanent
loss and per-block history checks before coding. Every new rule needs a caught injected bug;
random failures with all nodes eventually returning are insufficient m-loss evidence.

**S9. Evidence limits.** State lab limits and hardware/domain/space requirements for m2 and
rebuild. No new benchmark/lab/format/network action in this study increment.

**S10. Recorded outcome.** Recommend now or not-yet with named prerequisites and reopening
criteria. Any new architecture decision is Proposed, never inferred accepted. Align issue steps,
roadmap/architecture status and study artifact, preserving requirement history.

## Tests

This docs-only increment uses checked arithmetic tables, code-reference verification and
correctness/security review against all S1–S10 outcomes. It does not add tests mirroring prose.
Later simulator/runtime proposals must include the explicit S8 mutations and m-loss lab evidence.

## Open questions

[Study](study.md) recommends not yet: no actual workload need, coding commit protocol, model or
qualified failure-domain lab evidence exists. A human has not accepted that priority decision.
No replicated-volume rule or accepted ADR is changed by recording the recommendation.

Settled at approval, 2026-11-13: not yet, with the study's reopening criteria (see decisions.md,
Settled at approval).
