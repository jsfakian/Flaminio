# Issue #58: Requested snapshot shipping and promotion — specification

[intent](intent.md) · [decisions](decisions.md) · [design](design.md)

Historical approval:2026-11-13; review revisions are unapproved. Not implemented.
The existing intent's first increment: explicit pairing, requested snapshot ships,
standby and promotion. Periodic scheduling/RPO target, failback, groups and operator resources
remain later steps. The issue stays open; a manual ship cannot prove the full Done-when.

## Terms

- **Cluster identity:** durable random UUID plus trust domain; different from volume name/epoch.
- **Pairing:** administrator-installed peer UUID, exact trust domain, CA bundle and endpoint,
  isolated from local metadata and replica trust stores.
- **Relationship:** destination-owned UUID binding one source cluster/volume incarnation to one
  standby incarnation, with a revision/generation and lineage.
- **Ship:** durable operation ID, immutable source snapshot/base and destination relationship.
- **Committed point:** a completely received, validated, durable destination snapshot published
  under the relationship. An in-progress diff is not a recovery point.
- **Promotion:** local durable fork from the last committed point, fencing ship publication and
  making that point an ordinary attachable volume. It does not fence the remote source.

## The invariant

An attachable promoted volume contains exactly its last committed destination point, never a
partial later ship. Foreign cluster trust cannot grant local administration or replica access.
Application writes on the source never wait for the peer or destination metadata.

## Behaviour

**S1. Pairing.** Explicit action by an administrator on each cluster installs isolated peer
trust. Require exact CA-chain/trust-domain/cluster/role identity together; reject a peer with
the local trust domain or UUID. Never merge peer roots into local service/replica trust.
Pair-record generation CAS revokes new batches/publication in O(1); already admitted bytes may
finish only in isolated roots. Relationship epoch/session cleanup is resumable fallout.

**S2. Least authority.** A peer can submit authenticated snapshot data only for relationships
bound to its exact source incarnation, and query its own ship progress. It cannot read standby
data, invoke local metadata APIs, obtain local replica tokens, attach or affect another volume.
Requests carry protocol version, relationship generation and ship identity; unknown versions
or over-limit/malformed messages fail before allocation or mutation.

**S3. Standby.** Local administrator creates a separately placed standby with explicit RF,
minimum ISR and failure-domain policy. Every ordinary attach is refused while standby, including
admin/CSI requests. Only a purpose-bound local receiver ticket can run the specialized device-less target;
ordinary/maintenance/control/CSI shortcuts reject it and Standby/Promoting independently. Resize/encrypted volumes and chained relationships are refused in this increment.

**S4. Immutable shipment.** An administrator requests one source snapshot of an attached volume.
First ship is full; later ships use the last committed common base of the same incarnation and
lineage. Reject stale/out-of-order/conflicting ships. Pin source layers/keys until durable
completion or explicit safe cancellation; process death does not drop the pins.

**S5. Isolated prepare.** Receive into durable isolated staging state. The previous committed
point remains intact during partial apply, crash, replica loss, resync and takeover. No point is
published until every selected byte, explicit zero and geometry is verified and durable on
the current required ISR/joiners; prepared roots are immutable and every join holds all pinned
roots/receipts first. The current target emits a model-checked barrier, not a staged ISR vector.

**S6. Commit.** A conditional epoch/incarnation/relationship transition publishes the destination
point and lineage once. Lost responses are resolved by read-back of the same operation ID;
conflicting reuse is refused. Commit includes a durable destination snapshot and immutable
source/destination generation mapping, suitable for later failback. Source records completion
only from authenticated committed evidence, never an upload ACK alone.

**S7. Resume and resources.** Restart/link loss resumes verified durable ranges without resending
them. Corrupt/wrong-incarnation receipts cannot prove completion. Bound buffers, frames, active
ships and staging space; capacity/layer exhaustion refuses a new prepare without deleting the
last good point. Local application I/O/snapshots continue during peer outage; no network wait on
the source data path. Retention pressure is explicit, never silently destroys unshipped data.

**S8. Promotion.** Local admin may promote with no peer contact. Promotion conditionally freezes
the last committed point, closes the relationship and raises the local fence before any
application I/O becomes ready. Pending staged generations are excluded; later ship/retry/commit
cannot alter the promoted volume. Repeated identical promotion is idempotent. No completed point
means refusal. A competing receiver/commit is ordered by metadata CAS, not a timeout.

**S9. Divergence.** Promotion records the selected point and a new durable lineage. It cannot
promise the old source has stopped. This first profile requires explicit acknowledgement of possible dual primaries; it supplies
no source-exclusion provider. Report that state. Future failback must refuse
silent merging or overwrite of divergent histories. Removing/recreating a name never reuses an
old relationship, snapshot receipt or authorization.

**S10. Status.** Report last restorable committed point/time, in-flight backlog, failures and
recovery-point age with clock uncertainty. Configured interval is not actual RPO; outages can
make age unbounded. No schedule or interval-derived RPO claim in this requested-only increment.

**S11. Model and evidence.** Simulator represents prepare/commit/promotion and lineage during
chaos; each new safety rule has an injected mutation it catches. Lab uses two separately named
clusters, cuts only the peer path, promotes during a partial ship and verifies exact last-point
data on each authoritative replica. Exercise all peer/standby refusals. No lab result yet.

## Tests

- S1–S3: separate trust-store/session and RPC authorization tests, wrong CA/domain/role,
  local identity spoof, revoke, unknown version, ordinary attach, bounds/decoder fuzzing.
- S4–S7: shared snapshot diff-reader vectors; crash/retry at each staged range, snapshot and
  publication boundary; base mismatch, lost receipts, capacity/layer limits and local progress.
- S8–S9: model promotion races with commit/failover/delayed ship; last complete point only;
  mutation tests for partial exposure, stale commit, premature pin drop and lineage reuse.
- S10–S11: status uncertainty/backlog fixtures, seeded chaos, authorized two-cluster checks.

## Proposed review resolution

Exact proposed metadata/lifecycle, source reader, root format and DR bytes are fixed in
[contracts](contracts.md), [lifecycle](lifecycle.md), [root-format](root-format.md) and
[wire-contracts](wire-contracts.md). There is no unspecified future coordinated-ABI gate.
System-owned standby creation uses #73/#76; source/destination modes fail closed to older
attachment/header writers. Unsupported legacy deletion/drain during a prohibited mixed rollout
is explicit, not claimed repaired by a marker. Paired-source growth is refused; source IO and
snapshots continue. Active #83 blocks VM mutation; legacy exploration is non-production only.
Historical settlements remain in decisions.md; these detailed revisions require fresh approval.
