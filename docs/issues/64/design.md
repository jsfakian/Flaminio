# Issue #64: Erasure-coding feasibility study — design

[spec](spec.md) · [decisions](decisions.md) · [study](study.md)

Approved 2026-11-13; not implemented. Review revisions dated 2026-11-20 are unapproved.
Docs-only first increment. The **not yet** outcome and reopening criteria were settled at that
approval. No coding library, format, simulator rule or replicated-volume behavior is implemented.
Settled at approval, 2026-11-13: not yet, with the study's reopening criteria (see decisions.md,
Settled at approval).

## The facts it starts from

`meta/domain/domain.go:186` validates replica count/minimum, no coding scheme. `target.rs:1400`
sends identical extent data; `:1644` selects one readable source; `:1450` computes resync holes.
`flaminio-repl/src/replica.rs` stores no durable per-block version in its core; uncertain state
is volatile/restart-wide. `flaminio-format/src/meta.rs:450` maps locations/zeros, not fragment
version+commit evidence. `sim/mod.rs:1355` expects replicas to agree on values, not decodable
fragment sets. #45/#46 first increments do not supply coded multi-source recovery. #72's reviewed
increment plans a per-node capacity ledger and create/move admission, not device classes or cold
classification; none is implemented. The lab has3VM data nodes on2physical hosts.

## Study artifact and outcome (S1–S10)

[study.md](study.md) answers the eleven existing intent outcomes. Keep formulas and assumptions
adjacent to numbers. It explicitly separates code evidence, estimates, candidate mechanisms,
unknown proof and new measurement requirements. The approved outcome is to revisit after real
workload/class need, coding recovery model and independent-domain evidence exist. The review
revisions clarify prerequisites and estimates; they accept no coding protocol and keep #64 open.

Use no proposed production pseudocode as proof. The write-hole worked cases are counterexamples
to directly reusing replication, not a proof every erasure scheme fails. Candidate versioned
COW/commit evidence must be modeled before implementation and must preserve untouched blocks.
ARCHITECTURE §8.5 supplies the intent's single-source lease rationale; ADR0005 contains the older
immediate-read wording. Current protocol §6 and `Target::advance_chunk` require fencing the
effective ISR and reconciliation before serving. Fragment recovery must extend those stricter
source, fencing and lease requirements; this study does not change the accepted ADR.

If reviewers/user later select a protocol/library/profile, create an issue-local **Proposed**
ADR with exact failure/ack policy, stripe geometry, identities, format and model contracts.
Do not fabricate an accepted ADR to satisfy issue step1. Until then no new global ADR number
or byte layout is reserved; study is the permitted recorded not-yet branch of the intent.

## Files touched

- `docs/issues/64/{intent,spec,design,study,decisions}.md`: scoped requirements, study and review
  corrections; historical approvals and the settled not-yet outcome are preserved.
- Future implementation of this docs increment aligns architecture §20.6/§11.4, README phase6
  row and followups with the approved not-yet outcome and reopening criteria, without rewriting
  accepted ADRs or claiming coded storage exists. GitHub issue evidence is appended, original scope kept.
- No Rust/Go/manifests/Makefile dependency or production source changes in this increment.

## Contracts

None added. Existing replica, disk, API and frontend contracts remain unchanged. Study formulas:
raw/usable=(k+m)/k, ideal post-ACK failure budget≤q-k for q coherent surviving fragments,
subject to the actual commit/read/failure-domain protocol. These are necessary arithmetic
conditions, not sufficient crash-consistency or adversarial-integrity proofs.

## Tests

| Rules | Review evidence |
|---|---|
| S1/S2 | study use case/domain/capacity table, check ratios and reserve/version exclusions |
| S3/S4 | q/k/D policy and explicit interrupted-write/target-loss cases; unknown proofs remain blockers |
| S5/S6 | RF2/RF3/coding operation/byte/transfer-round and conversion estimates; declared assumptions and confinement table, no invented timing |
| S7 | named dependency contracts including identity/integrity/retention and rebuild source sets |
| S8 | model/check/mutation plan precedes data-plane work, permanent loss explicitly included |
| S9/S10 | independent-domain lab requirements and approved not-yet/reopening criteria; review revisions unapproved |

Settled at approval, 2026-11-13: not yet and its reopening criteria (see decisions.md, Settled at
approval).

Verify links, changed-line references, arithmetic and whitespace. No runtime tests that merely
assert document contents; make lint/test become required for later source changes, not proof
of a paper coding protocol. No benchmark or lab run for this draft.

## Lab

None. Future m2 evidence requires≥k+2 independent data domains plus a spare for rebuilding,
with power-loss/target-member fault control and preserved acknowledged histories. Three VMs
on two hosts cannot satisfy it. Existing disks/VMs/networks are untouched.

## Docs, risks and ADRs

Do not repeat “same protection as three copies” without defining m, post-ACK D, rebuild window,
correlated failures and malicious-fragment assumptions. Ratios exclude retained old versions,
metadata and reserve. Both lower space and higher performance are unproved workload outcomes.
ADR0001/0002/0003/0004 remain consistent; ADR0005/current takeover proof needs extension for
fragments, not a weakened old immediate-read rule; ADR0006 singleton-write policy applies only
to full replicas and cannot be imported into k+m. Proposed audit0009/0011 constrain identity,
key/snapshot lifetime and commit boundaries. A future conflicting design needs superseding ADRs.

## Parallelizable?

No implementation units. Review cost arithmetic and consistency/security assumptions separately;
any later runtime split waits for the model, proof and exact versioned contracts.
