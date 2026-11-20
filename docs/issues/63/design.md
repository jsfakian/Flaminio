# Issue #63: Storage disruption guard — design

[spec](spec.md) · [decisions](decisions.md) · [ADR, Accepted with the plan, 2026-11-13](adr-proposal.md)

Approved 2026-11-13; not implemented.
Shared first slice only. This candidate makes unresolved coverage/fail-closed contracts
explicit; it is not implementable as a claim of absolute denial without resolving those gates.

## The facts it starts from

`control/failover/kube.go:20` is a small authenticated REST client, no client-go in go.mod.
It currently can list/patch nodes; guard requires a separate narrower identity/client.
`meta/service/service.go` lists volumes separately from per-volume views; #77 fixes each view,
not a cluster-wide transactional snapshot. ISR does not imply node liveness. #69 proposes a
metadata Reader role, not implemented; this controller must not reuse CSI admin credentials.
#17/#13/#45 handle drain/copy/failure later; this slice sends no mutation RPC to them.

External primary sources checked 2026-11-12: [Kubernetes PDB API](https://kubernetes.io/docs/reference/kubernetes-api/policy/pod-disruption-budget-v1/)
defines IfHealthyBudget behavior, [PDB configuration](https://kubernetes.io/docs/tasks/run-application/configure-pdb/)
limits availability expressions for arbitrary controllers, and [upstream eviction code](https://github.com/kubernetes/kubernetes/blob/master/pkg/registry/core/pod/storage/eviction.go)
rejects a pod matching multiple PDBs. [Autoscaler FAQ](https://github.com/kubernetes/autoscaler/blob/master/cluster-autoscaler/FAQ.md)
identifies unschedulable node-bound pods as scale-down blockers. Pin qualification to actual
Kubernetes release, not assumptions from moving upstream code or fake clients.

## Read-only decision and observation (S2, S6, S7)

New `control/disruption` reducer takes node records, Kubernetes Node UID/schedulability,
complete volume views and owned guard/disruption states. Returns per-node Eligible|Held|Unknown
with bounded sorted reasons. Count distinct surviving node IDs, including non-Kubernetes
nodes; exclude metadata-not-alive, draining, cordoned/admitted, missing identity. Account for
all chunk replicas/ISR and active move source/destination before allowing removal.
Read all list pages under a bound; truncation/partial error is Unknown. #77 gives coherent
per-volume view; concurrent creation/deletion between views remains an observation race.
No false claim of an atomic cross-volume metadata snapshot. Eligibility is a conservative
availability estimate, never authorization to read from a stale replica.

Controller runs one bounded reconcile at a time, repeat5s, total30s timeout, max4096nodes/
4096volumes/64MiB aggregate and per-RPC existing cap. On limit exceed hold all, not silently
omit data. Coalesce reasons≤64 per node with omitted count; never put tokens/payload in Events.
Persist observation revision/hash and state by immutable Node UID. A successful pass may
propose permission only after every required guard/accounting resource is observed ready.
Metadata outage/unknown state revokes proposed permissions when controller can act.
No metadata mutation, node patch, pod eviction, force delete or data-plane control credential.

## Candidate marker/PDB mechanism (S1, S3, S5)

For each storage Node UID, create two small owned Deployments, each replicas1 and required
node affinity to that exact node identity: `flaminio-disruption-slot-UID` and
`flaminio-disruption-permit-UID`. They are ordinary controlled pods so kubectl drain must
process them. No host access or data credential; restricted nonroot idle container, no API
token by default. This is intentionally conservative and may prevent real autoscaler scale-down.

Slot pods share **one** global PDB selector and integer minAvailable=N-1 for N registered
ready slot deployments, allowing at most one unavailable slot. Permission pods each match
one node-specific PDB: initial minAvailable1; candidate eligible setting0. They never match
the slot PDB; **no individual pod matches more than one PDB**. Use IfHealthyBudget, never
AlwaysAllow. Creation/removal adjusts integer budget only after coverage is reconciled; unknown
count clamps denial. A1-node cluster needs a separate proven zero-disruption default; this
slice cannot silently use minAvailable0 as fail-closed protection.

When kubectl drain evicts a slot, its replacement cannot run on the cordoned node, retaining
the global unavailable count. This serializes later slot evictions until the node returns
schedulable/alive with healthy replacement. A denied permission can consume the global slot
before other guard pods finish; report this conservative in-progress hold rather than reset
accounting and admit another node. Terminal/pending/unhealthy pod eviction bypass behavior
must be exercised against the real API server. Global and per-node budgets do not create an
atomic storage transaction; they only bound admitted marker disruptions under their model.

UID/resourceVersion test preconditions protect writes. Re-read API status.observedGeneration,
disruptionsAllowed/disruptedPods before claiming installed behavior; don't set PDB status in
controller tests as a substitute for API accounting. Record cordoned/admitted nodes separately
until recovery, not until a replacement object's name reappears. Node recreation gets a new
resource set and no inherited grant. Never delete unrelated matching-label objects to repair
coverage; fail with ownership reason. Drift changing selectors/budgets holds, no broad adoption.

## Required gate: coverage and stale permission (S1, S4)

Plain PDB settings cannot satisfy both absolute rules as stated:

- A newly labeled node can be drained before an asynchronous controller installs its marker.
- minAvailable0 persists if the controller dies after granting it. A Lease expiry does not
  atomically revoke a PDB grant. Unhealthy/terminal/pending marker rules may also bypass it.
- Data/service state may change between the observation and eviction, even with one leader.

Therefore **do not enable/claim this candidate as full S1/S4 compliance**. Resolve by either:
(a) explicitly narrow the availability guard to observed/installed best effort with bounded
staleness, retaining all documented bypasses; or (b) add a synchronous fail-closed admission
mechanism and enrollment gate, expanding the existing intent's webhook exclusion. A webhook
must consult durable admission/UID state and serialize unsafe concurrent evictions, fail closed
on missing controller/metadata, and have an exact Kubernetes match/failure policy; it is not
implemented by the PDB design above. No option is silently accepted.

If a synchronous option is selected, update intent/spec/ADR and exact admission contracts
before coding. Do not hide this choice behind a readiness probe or a passing fake test. PDB
and leader Lease alone are insufficient proof. The proposed architecture can still be reviewed
for its limits, but final zero-comment clearance requires a coherent chosen requirement/design.
Settled 2026-11-13 (user's instruction): neither option is chosen in this plan; the ADR chooses
(a) or (b) before implementation, as this section requires (see decisions.md).

## Process, security and Kubernetes contract (S6, S7)

Proposed `flaminio-disruption-guard --namespace flaminio-system --meta ENDPOINTS --cert PATH
--key PATH --ca PATH --trust-domain TD`, one controller Deployment and leader Lease. A Lease
avoids ordinary duplicate reconciles, not fencing a paused stale writer. CAS resourceVersion
and never release unknown slot accounting; leader takeover first clamps/reads all owned state.
If this cannot prevent stale grant writes under the selected semantics, it is another blocker.
Settled at approval, 2026-11-13: this command, Deployment and Lease as proposed (see
decisions.md, Settled at approval).

ClusterRole nodes get/list/watch only. Namespace Role deployments/pods/PDBs/Lease get/list/watch/
create/update/patch/delete only in dedicated protected guard namespace; restrict named leader
Lease and deny tenant/service-account impersonation. No writes to nodes or tenant workloads.
Kubernetes RBAC cannot restrict dynamic-name create by label, so the namespace boundary is
part of the trusted scope. No broad Secrets read; dedicated reader certificate via mounted
Secret. Metadata Reader allowlist and replica-role refusal must exist (#69). Server identity
and CA verified, projected Kubernetes token reloaded on rotation, request body/output caps.

Owned ConfigMap status keyed Node UID reports state/reason codes, affected volume/chunk,
metadata observation revision/time, coverage and unknown/bypass state. Codes MissingCoverage,
MetadataUnknown,WouldBreachMinISR,OtherDrainInProgress,ForeignResource,StaleObservation,
MechanismUnqualified. Events are rate-limited. No status field is fencing authority.
No new metadata RPC, proto, storage or control-socket contract in this guard increment.

## Files touched

`control/disruption/{guard,kube,status}.go` and tests; command under `control/cmd/`; restricted
`deploy/disruption/` manifests; scripts/checks/disruption-guard.sh and script fixtures; Make
help/lab dispatch. Reuse small REST transport primitives only after removing node-write authority,
not by sharing the current failover identity. No new controller-framework dependency proposed.
If synchronous admission is selected, this file list/contracts must expand before implementation.

## Tests

| Rules | Planned checks |
|---|---|
| S1/S7 | Node UID recreation, foreign resource, missing/stale coverage, replica count drift, lost update/leader overlap; never adopt or reset unknown admission |
| S2 | per-chunk distinct-node count, move endpoints, dead/draining/cordoned/outside-Kubernetes members, malformed/partial/oversize lists, stale coherent per-volume view |
| S3 | actual API simultaneous two-node evictions; one global slot remains unavailable across cordon; no pod matches two PDBs |
| S4/S5 | kill controller after grant, API/metadata outage, pending/terminal/unhealthy marker; demonstrate candidate gap rather than bless it; chosen replacement mechanism must close it |
| S6 | metadata/RBAC positive+negative auth, no node/tenant/metadata mutations, secret redaction and reason caps |
| S8 | real drain sequence, held reasons, recovery/re-admission and verified data; actor bypass explicitly outside guarantee |

`make lint test race hygiene contracts` for future code; real Kubernetes PDB tests required.
Docs-only drafting ran no runtime test and cannot qualify the mechanism.

## Lab, docs, risks and ADRs

Only after explicit authorization and mechanism resolution: proposed `make disruption-guard-test`
uses dedicated RF2/minISR1 data, stops/restarts one data plane, cordons/drains/uncordons nodes,
then verifies data and restores owned workloads/guards. It may move application pods; require
no unrelated workloads before running. No VM shutdown, raw format, forced taint or bypass to
get green. #82/#83 constraints remain. Clean only exact owned UIDs and restore prior cordon
state. If drain did not complete, report held state, not success.
Settled at approval, 2026-11-13: `make disruption-guard-test` as proposed, run only after
authorization and the mechanism's resolution (see decisions.md, Settled at approval).

Document §10.4's actual fraction, PDB/privileged bypasses and autoscaler incompatibility in lab,
README/followups. Shared issue split is still tracking work. Accepted replication ADRs unchanged;
[Proposed ADR](adr-proposal.md) records availability-only nature and unresolved mechanism.
No new data-loss risk accepted. Scale-up/claims/rebalance/capacity remain #61/#72/later #63.
Settled at approval, 2026-11-13: the ADR is accepted with the plan; the guard moves to an issue of
its own (see decisions.md, Settled at approval). Settled 2026-11-13 (user's instruction): the
mechanism is left to the ADR, chosen before implementation (see decisions.md).

## Parallelizable?

No until S1/S4 mechanism/semantics settle. Read-only reducer/status tests can then be separate
from Kubernetes integration, but fake accounting cannot certify admission concurrency.
