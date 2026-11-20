# Issue #91: Shared storage disruption guard — specification

[intent](intent.md) · [decisions](decisions.md) · [design](design.md) ·
[admission contract](admission-contract.md). Proposed; unapproved and unimplemented.

## Terms

- **Node identity:** immutable Kubernetes Node UID and exact name==metadataID under
  [identity-contract](identity-contract.md); no alias/address/label inference.
- **Anchor:** ordinary controlled pod bound by `spec.nodeName`, annotated with its enrolled
  Node UID; its immutable Pod UID identifies one admission generation.
- **Enrolled:** durable UID-owned record and corresponding anchor exist; Node UID has been
  observed after commit. Staging coverage precedes Node commit and grants no admission.
- **Admission:** durable single-slot CAS for one enrolled node and anchor UID, before the
  validating webhook returns allow. In-flight admitted operations may finish after failure.
- **Unavailable:** dead, draining, cordoned, admitted, missing/ambiguous identity or unknown.
- **Eligible:** removal leaves every relevant observed chunk at least minISR distinct live
  available earned stores, using [reader-contract](reader-contract.md).

## The invariant

This is an availability interlock. Replication/fencing still protect acknowledged writes.
It cannot mutate metadata authority, membership, epochs, ISR or node state.
At most one Node UID owns a durable admission slot, independent of pod counts/readiness.

## Behaviour

**S1. Automatic coverage.** Establish anchor coverage before admitting Node CREATE or a
storage-label change. No UID affinity fiction: placement uses supported `spec.nodeName`;
identity checks use immutable UIDs. Missing/uncommitted/mismatched enrollment holds. Existing
cluster installation verifies coverage before claiming the profile active. No manual per-node
Flaminio enrollment step is required. Existing-name CREATE returns actual AlreadyExists
without doomed-UID state; durable same-name contention retains only the original claim/anchor.

**S2. Complete observation.** Every new admission performs complete bounded strict metadata
collection, coherent full/paged per-volume reads and actual Node/anchor reads. Any malformed,
unsupported, incomplete or unavailable input holds. Exact store identity never falls back to
legacy node projections; external nodes can count only under the reader's explicit rules.
Distinguish Storage from ProvenNonStorage using permanent exact-store-history absence and
complete no-reference predicates, never missing ListNodes alone. Unsupported/ambiguous joins hold.

**S3. One node at a time.** Kubernetes resourceVersion CAS reserves the single durable slot
before allow. Different-node contention holds; an unchanged same-node/anchor retry rechecks
metadata. Pod surge/replacement/termination never creates admission capacity. Stable enrolled
UID membership is independent of readiness. A missing/cordoned/admitted peer counts gone.

**S4. Fail closed.** Synchronous eviction admission covers Pending, terminal, deleting and
replacement anchors; webhook failure/timeout rejects. No label or namespace selector can skip
an anchor. The guard issues no stored permissive PDB. Failed metadata reads never use a cache
as authorization. A committed in-flight admission is not retroactively revoked by an outage.

**S5. Ordinary drain and boundaries.** Standard eviction-enabled drain needs no Flaminio CLI,
force flag or fake PDB status. A held drain can already have evicted application pods. Direct
pod/node deletion, reboot, drain filters/disable-eviction and privileged webhook/config changes
remain bypasses; no autoscaler compatibility is claimed.

**S6. Restricted authority/status.** Use a separate Restricted guard namespace and dedicated
metadataReader identity. Only owned resources are writable; nodes and tenant workloads are
read-only. Publish bounded reasons/observations/status and rate-limited Events under exact
[security-contract](security-contract.md) permissions. No admin data-plane credentials.

**S7. Durable recovery.** Lost response, overlap or restart retains the slot. Time/Lease expiry
cannot release it. Release requires the exact admitted anchor UID permanently absent, a fresh
healthy replacement for the same Node UID, schedulable/live recovered node and complete fresh
metadata using the exact Storage/ProvenNonStorage recovery branches. Deleted UID retirement
uses saved exact old-store UUIDs and returned-store/moved-away predicates, not nonexistent
metadata KubernetesUID fields. Node recreation gets independent enrollment and no inherited grant. Staging Node
creates are never timeout-collected while a delayed commit could still occur.

Resource limits are explicit in [resource lifecycle](resource-lifecycle-contract.md):64MiB is application-controlled reader credit, not all stock gRPC/runtime reachability. Actual process containment requires verified cgroup memory.max≤256MiB and swap0. Known terminal reader/resource failure quiesces intake and replaces the process; durable slots/enrollments are never reset. Old remote writes may still commit and must be re-read conservatively. Held/unavailable drains and restart/backoff are Proposed operational costs, not successful admission or an accepted tradeoff.

**S8. Evidence.** Run the pinned real-API counterexamples and original RF2 stop/return/drain/data
sequence according to [qualification](qualification.md). VM refusal after #83 is unmet evidence;
API qualification alone is not storage acceptance. All runtime checks are future work.

## Tests

- S1/S4/S7: real Node CREATE UID timing, delayed commit, label update, new same-name UID,
  Pending/terminal/unready anchor, existing-name kubelet POST, overlapping/delayed creates,
  nonstorage refresh/drain/recovery, exact-name/external collisions, saved-store return/move,
  restart and overlap; no absent-anchor drain success.
- S2: strict service-to-mTLS reader malformed/partial/oversized and #90 exact/paged fixtures.
- S3/S7: actual API concurrent A/B drains with unrelated C rollout; lost responses, paused
  request, cancellation, same-name anchor replacement and CAS conflict fixtures.
- S5/S6: ordinary drain admitted/held sequence; exact authorizer and pod-spec negatives.
- S8: separate API and qualified physical-host data evidence matrix.

## Open questions

No mechanism alternative is deferred. Proposed synchronous admission and its explicit costs
need human acceptance after fresh plan review; source #63 approvals do not provide it.
