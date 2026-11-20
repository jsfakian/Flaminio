# Proposal: Admit storage drains synchronously with UID-owned durable accounting

- **Status:** Proposed; unapproved, unimplemented; local issue draft, numbered only on acceptance.
- **Date:** 2026-11-20

## Context

[#91](https://github.com/jsfakian/Flaminio/issues/91) owns the guard-only first half of #63 step1.
The unchanged promise is automatic coverage, conservative observed minimum-ISR availability,
serialized jointly unsafe drains and denial on controller/metadata failure. The predecessor's
2026-11-13 approvals explicitly chose neither bounded best effort nor synchronous admission.
They are provenance, not acceptance of this decision. The former PDB candidate cannot meet
that promise: permissive settings survive process death, Pending/terminal bypass PDBs, and
pod rollout/readiness counts cannot establish one credit per immutable Node UID.

## Decision proposed

Use synchronous mutating/validating eviction admission and validating Node enrollment with
failurePolicy Fail, under the exact [admission contract](admission-contract.md). This proposes
option(b) here, expanding the source webhook exclusion while keeping its coverage/failure
promise. It does not change what #63's historical user selected.

Existing-name POST returns exact AlreadyExists without allocating its never-committing UID;
absent-name CREATE first wins a durable same-name claim, so overlapping attempts create no
extra anchor. Before that winning Node CREATE commits, create its ordinary anchor with nodeName and
its enrollment ConfigMap controller ownerReference. Cover every Node, including unlabeled
nodes, to close subsequent storage-label/registration races without a metadata writer or
node mutation. Anchors carry a custom finalizer from creation. Only the owning controller
removes it after reserving the exact anchor UID, including image/terminal refresh; otherwise
kubectl's old Pod snapshot could finish without ever observing a replacement.

Mutating admission adds the actual Pod UID delete precondition omitted by stock kubectl.
Validating admission performs complete fresh strict read-only metadata observation and
atomically reserves a single slot in an owned Kubernetes ConfigMap. It never derives credit
from Pod readiness, PDB status, controller timing or Lease expiry. All overlap/retries use
resourceVersion CAS and immutable ownership. In-flight admitted work may finish after failure;
no cached observation or persisted permissive PDB independently authorizes a new request.

An occupied slot stays unavailable while cordoned/dead/draining. The exact admitted anchor
can disappear while its node remains cordoned, allowing the original drain to finish; slot
release separately requires old Pod UID absent, same Node UID recovered schedulable/Ready,
live matching store/full earned ISR for Storage, or fresh permanent no-history/no-reference
proof for ProvenNonStorage, plus a fresh Ready finalizer-bearing anchor. [Identity contract](identity-contract.md)
defines exact metadataID==KubeName join, sticky everStorage and saved old exact-store UUID
retirement/return predicates; metadata is never asked to distinguish a Kubernetes UID it lacks. UID precondition
makes an arbitrarily delayed old request harmless to its replacement. Staging Node creates
are never collected by timeout, because a paused API-server create can still commit later.
Bounds, failure/refusal and maintenance-window installation are explicit in [design](design.md).

Metadata remains read-only and service-mediated. The reader uses strict bounded enumeration,
coherent full/paged views and exact earned-store membership; no ISR/epoch/replica/drain mark
changes and no I/O authority arise. Separate Restricted namespace and minimal dedicated
identities replace the unsafe source namespace/credential arrangement. No node writes.

## Options

- **Dual PDB candidate:** smaller synchronous dependency, but unbounded stale permission,
  uncovered states and pod-count credits violate the unchanged promise; rejected as this
  proposal's mechanism, without rewriting predecessor acceptance history.
- **Bounded best effort:** can state weaker observation/coverage guarantees, but would need
  an explicit changed promise. This proposal preserves the source promise instead.
- **Synchronous admission with native Deployment/StatefulSet anchors:** can serialize with
  durable state, but autonomous terminal/replacement deletion adds coverage races. A custom
  controller with ConfigMap-owned finalizer pods keeps lifecycle and admission in one place.

## Proposed resource-lifecycle choice

Use the stock pinned gRPC library and a bounded application reader budget, with actual cgroup-v2 memory.max≤256MiB/swap0 verification and fail-closed process recycling under [resource lifecycle](resource-lifecycle-contract.md). Withdraw the earlier whole-gRPC64MiB reachability/cleanup claim and the custom pooling/padding/transport proof mechanisms introduced solely for it. Standard receive/header/admission limits and typed cardinality/owned-message validation remain.

A maintained dependency cleanup patch could clear internal caches and drain abandoned roots, but is unnecessary for the actual availability-interlock requirement when process containment/replacement is honest. The selected proposal has explicit held-drain/restart cost and no bounded time-to-recovery promise; no risk is accepted for the user. Process death does not cancel submitted remote writes or reset the durable CAS. Human acceptance remains required before implementation.

## Consequences

Admission/controller outage blocks eviction requests and Node CREATE/UPDATE; nodes/status
and storage I/O do not depend on it. Management-node loss can require explicit privileged
repair. One global slot is conservative; refresh may wait for safe membership. Finalizers
and unresolved Staging creates consume bounded durable resources and may require an explicit
maintenance procedure rather than time-based reclamation. Installation on an already-running
unguarded cluster requires a no-drain maintenance window; retroactive coverage is impossible.
These availability/operational costs are proposed and need human acceptance.

This does not atomically reserve actual replica availability against unrelated future failure,
metadata mutation or direct privileged bypass. Existing epochs/fencing/linearizable membership
remain data authority and no accepted replication ADR is weakened. No autoscaler, evacuation,
cloud or claims stage is added. #63 awaits #91's actual clear review and later runtime evidence.

## Confirmation

[Spec](spec.md) rules map to exact tests in [design](design.md) and [qualification](qualification.md).
Actual pinned Kubernetes/k3s API fixtures must cover Node precommit UID, Pending/terminal/GC,
old-client Pod snapshots, concurrent drains with unrelated rollout, lost response, delayed old
allow after replacement, cancellation and leader overlap. Real authorization/service-mTLS
fixtures prove narrow authority and complete metadata. API evidence is distinct from #83-qualified
physical-host RF2 stop/return/drain/data proof. No runtime evidence or human approval is claimed.
