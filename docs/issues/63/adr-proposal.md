# Proposal: Treat the storage disruption guard as an availability interlock

- **Status:** Accepted with the plan, 2026-11-13; numbered and moved into docs/adr/ when implemented.
- **Date:** 2026-11-12

## Context

[#63](https://github.com/jsfakian/Flaminio/issues/63) includes shared guard work scoped by the
existing [intent](intent.md), before actual autoscaling. Runtime fencing/ISR protects data;
guard observations can be stale. PDB marker settings do not revoke on controller death and
asynchronous coverage cannot protect a newly labeled node before installation.

## Proposed decision and unresolved choice

Keep guard metadata read-only and serialise admitted disruptions with actual Kubernetes
accounting, identity by Node UID and conservative unknown-state holds. No data authority,
forced minimum reduction or node out-of-service inference. Candidate dual-marker PDBs use
nonoverlapping selectors; one pod must never match multiple PDBs.
Settled at approval, 2026-11-13: this decision is accepted with the plan (see decisions.md,
Settled at approval).

Before implementation choose either an explicitly bounded best-effort availability contract,
or expand scope with synchronous fail-closed eviction admission plus enrollment gating.
No option is accepted here; the current candidate cannot claim absolute S1/S4 compliance.
Lease/readiness alone does not solve stale permission. Final design must include exact API
and race proof for whichever coherent contract is selected.
Settled 2026-11-13 (user's instruction): neither option is chosen in this plan. This ADR chooses
(a) or (b) before S1 or S4 is implemented, with the exact API and race proof above; until then
the intent's promise stands unnarrowed (see decisions.md).

## Options and consequences

PDB markers avoid a synchronous dependency for every eviction but can leave stale grants and
block autoscaler scheduling. A webhook adds TLS, lifecycle and availability dependency and
must serialize admissions/fail closed. Autoscaler-specific annotations do not protect kubectl
drain. All approaches can be bypassed by privileged direct deletion/reboot; those remain failures.
Read-only observation cannot atomically reserve actual replica availability with current APIs.

## Confirmation

[Design](design.md) requires actual API concurrency/controller-death/coverage tests and negative
RBAC checks, not fake PDB status. Accepted replication ADRs remain unchanged. A shared guard
issue can be split later; no runtime implementation, acceptance or #63 completion claimed.
Settled at approval, 2026-11-13: the guard moves to an issue of its own (see decisions.md,
Settled at approval).
