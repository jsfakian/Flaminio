# Issue #63: Storage disruption guard — specification

[intent](intent.md) · [decisions](decisions.md) · [design](design.md)

Approved 2026-11-13; not implemented.
The existing intent's guard-only first slice. Replica draining, autoscaler scale-down,
scale-up, disk claims and CSI capacity are later work. The complete #63 stays open. Existing
intent proposes moving this shared work to a separate issue; tracking that split is not delivery.
Settled at approval, 2026-11-13: the shared guard moves to an issue of its own, as the intent
recommends (see decisions.md, Settled at approval).

## Terms

- **Guarded node:** Kubernetes node labeled storage, identified by immutable Node UID and the
  matching metadata node ID; external data nodes still count as possible surviving ISR members.
- **Known unavailable:** not alive in metadata, cordoned/draining/admitted for disruption, or
  missing/ambiguous identity. A heartbeat is an availability observation, never I/O authority.
- **Eligible:** for every relevant chunk, removing this node leaves at least minISR members on
  distinct other nodes observed alive and not being drained/admitted. Include move endpoints.
- **Admission:** eviction of the required guard marker(s) through Kubernetes eviction API.

## The invariant

The guard is an availability interlock, not data authority. It never changes ISR, epochs,
replicas, metadata drain flags or source selection. Ordinary replication/fencing still protect
acknowledged writes if observations become stale or a guard is bypassed.

## Behaviour

**S1. Coverage.** Automatically reconcile required owned guards for every storage node and
remove them only for the same deleted Node UID. Never adopt an unrelated object by name.
Report coverage pending until actual pod/PDB/controller status is observed, not just created.
A design must explicitly close or disclose the node-appearance-before-coverage race.

**S2. Eligibility.** Compute per-node blocking volume/chunk reasons from complete bounded
metadata reads, coherent per-volume views and node observations. Exclude every other node
already admitted/cordoned/draining or not alive. Unknown/malformed/missing data denies a new
permission. A metadata error cannot be treated as an empty/safe volume list.

**S3. Concurrent drains.** One observation cannot allow a jointly unsafe pair of drains.
Kubernetes atomic disruption accounting must serialize admission; a controller poll/Lease
alone is insufficient. An admitted node counts unavailable until schedulable+alive again and
its replacement guard is observed healthy. Conservative serialization is allowed and visible.

**S4. Controller/metadata failure.** Default guard state denies admission. Existing intent's
absolute denial while controller is absent requires a mechanism preventing stale permission;
plain mutable PDB settings do not revoke themselves. A bounded-observation variant is a
proposed clarification only, with its exact failure window and bypasses stated. Do not claim
this rule implemented by a controller Lease or fake test alone.
Settled 2026-11-13 (user's instruction): neither is chosen in this plan; the ADR chooses the
bounded-observation variant or a synchronous admission mechanism before this rule is
implemented (see decisions.md).

**S5. Boundaries.** A held drain may already have cordoned the node and evicted application
pods. Direct pod/node deletion, reboot, --disable-eviction and privileged administrator changes
can bypass the guard and remain explicit. Node-bound marker pods may prevent a real autoscaler
from scaling down even when kubectl drain can progress; no autoscaler compatibility claim.

**S6. Status and authority.** Held reason names node UID, volumes/chunks and observation
revision/time/unknown state in owned Kubernetes status/events and logs. Service holds only a
read identity at metadata and rights to its own guard/controller/PDB/Lease objects plus node
read. No admin data-plane/token credential, node taint/cordon mutation or tenant pod deletion.

**S7. Recovery.** Lost response/restart/leader change resumes UID-owned state conservatively.
Conflicting resources, unknown admitted state and stale generation hold rather than reset to
safe. Node recreation cannot inherit old permission. A forced override is outside controller
and must be explicit/documented; the controller never issues one to make a test pass.

**S8. Evidence.** On RF2/minISR1 A/B, stop A data plane and drain B: held with a reason. Return
A/full ISR: same drain can progress. While B remains cordoned, drain A is held; after B returns
schedulable/alive/guard-ready it may progress. Include simultaneous drain and controller/metadata
loss with real eviction API behavior, then data verification. Fake clients alone do not prove
PDB concurrency. No lab result exists yet.

## Tests

- S1/S6/S7: ownership/UID recreation, restricted auth and incomplete coverage/resource/status
  cases with fake API; no metadata mutation and foreign-object modification.
- S2/S3: eligibility fixtures, partial GetVolume failures, moved chunks, live-but-draining peer,
  simultaneous real API evictions and retained unavailable accounting.
- S4/S5: controller death with stale permit, metadata failure, pending/unhealthy guard behavior,
  direct-delete/eviction bypasses and documented limitations.
- S8: explicitly authorized real-Kubernetes drain sequence and data check.

## Open questions

The [design](design.md) explores a PDB marker approach but cannot claim the absolute S1/S4
contract without an additional synchronous admission/coverage mechanism or explicit narrowing.
No accepted choice exists. Shared guard tracking, fail-closed mechanism and Kubernetes version
qualification must settle before implementation; the draft is not review-clear.

Settled at approval, 2026-11-13: shared guard tracking, in an issue of its own (see decisions.md,
Settled at approval).

Settled 2026-11-13 (user's instruction): the fail-closed coverage and stale-permission mechanism
is chosen by the ADR before implementation, not in this plan; qualification is on the lab's k3s
release, `K3S_VERSION` v1.36.5+k3s1 (see decisions.md).
