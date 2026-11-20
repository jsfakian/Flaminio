# Issue #91: Shared storage disruption guard — intent

[Issue #91](https://github.com/jsfakian/Flaminio/issues/91) owns the original guard-only first
half of #63 step 1; #48 and #63 compose it later. [spec](spec.md) · [design](design.md) ·
[decisions](decisions.md). This new technical proposal is unapproved and unimplemented.
The 2026-11-13 #63 approvals are provenance, not approval of this mechanism.

## Why

A normal drain evicts Kubernetes pods but leaves Flaminio's host service and DaemonSets.
The last synchronous maintenance interlock must therefore be an ordinary controlled pod.
The former dual-PDB candidate has real gaps: Kubernetes v1.36.5 ignores PDBs for Pending,
terminal and deleting pods; permissive PDB settings survive controller death; readiness and
Deployment surge count pods rather than distinct Node UIDs. Source findings from #63 R1
are inputs to this issue, not completed #91 reviews.

Current `meta/service/service.go:159` registers nodes, `:374` places new volumes and `:845`
lists volume headers. The list silently skips malformed JSON. A complete read-only guard
therefore needs the strict reader contract, not a client-side assertion of completeness.

## What we want

1. Every Kubernetes storage Node UID automatically has a controlled anchor which an ordinary
   `kubectl drain NODE --ignore-daemonsets` must evict. A new Node cannot become visible before
   its anchor exists; storage-label changes do not create a coverage gap.
2. A drain is held if removing its node leaves any relevant observed chunk below minISR,
   counting only distinct other live, available earned stores, including external storage nodes.
   Exact metadataID==Kubernetes name joins and permanent history distinguish storage from
   proven never-storage nodes; both have an executable drain/recovery path.
3. Kubernetes atomic resourceVersion accounting serializes admitted nodes. An admitted node
   remains unavailable until recovery; rollouts, readiness and name reuse cannot mint credits.
4. Controller absence, failed metadata reads, missing coverage or unknown durable state deny
   new admission. No permissive PDB or cached grant survives as independent authorization.
5. Kubernetes status and logs identify held node UID, volume/chunk and bounded reasons.
6. The guard reads metadata and writes only its own Kubernetes resources. It changes no
   metadata drain mark, ISR, epoch, replica, lease authority, node taint or cordon.
7. Real pinned-API tests and the original RF2/minISR1 stop/return/drain/data proof establish
   acceptance on an authorized qualified environment; current VMs cannot certify #83's profile.

The [Proposed ADR](adr-proposal.md) concretely proposes synchronous admission instead of
narrowing these outcomes to best effort. This necessarily expands the predecessor's webhook
exclusion. #63's historical neither-option-selected decision remains unchanged.

## Out of scope

Autoscaler integration, evacuation, scale up/down, cloud classification, disk claims, CSI
capacity and replication/fencing changes. Privileged bypasses (direct deletion, reboot,
webhook removal, drain filters/disable-eviction) are explicit boundaries. Admission observes
availability; it cannot atomically reserve replicas against unrelated subsequent failures or
metadata changes. Already admitted operations can finish after a controller failure.
No runtime, deployment, lab action or ADR acceptance is authorized by drafting these files.

## Proposed resource-exhaustion cost

The owner uses bounded application reader credits plus verified whole-process cgroup containment, rather than maintaining a gRPC fork solely for a self-imposed live-memory proof. Known terminal reader/resource failure stops admission and triggers supervised process replacement; actual lost/uncertain writes remain durable-state recovery cases. Held/unavailable maintenance and restart/backoff are explicit Proposed costs requiring human acceptance before implementation, not an exception to fail-closed S1/S4 or data authority. See [resource lifecycle](resource-lifecycle-contract.md).
