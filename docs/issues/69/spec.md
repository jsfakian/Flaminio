# Issue #69: Read-only cluster view before the UI — specification

[Intent](intent.md) · [Design](design.md). Approved 2026-11-13; not implemented. This is the
intent's read-only identity/status-command slice; the dashboard remains open under #69.

## Terms

- **Reader:** authenticated, explicitly read-only Flaminio identity, not an administrator/node.
- **View:** collected nodes, volume records and registry-enumerated tombstones, with timestamps.
- **Redundancy:** committed ISR count compared with the configured RF, not temporary move size.
- **Availability hint:** observed liveness of ISR members; never permission to serve I/O.
- **Unavailable field:** information not represented by current records, such as physical capacity.

## The invariant

Rendering or reading a view grants no mutation, token issuance, fencing or deletion authority.
An unknown, incomplete or failed observation cannot be represented as a healthy empty cluster.

## Behaviour

**S1. Explicit identity.** The reader has a distinct validated SPIFFE identity in the cluster's
trust domain. Exactly ListNodes, ListVolumes, GetVolume and node-scoped ListTombstones are
allowed. All mutation methods and future methods default-deny, and replica sessions refuse it.

**S2. Command.** A single bounded status command renders nodes, volumes, redundancy, requested
moves and registry-enumerated pending tombstones for people and JSON consumers. It needs only
reader credentials; it reads no node control socket or token-signing key and performs no write.

**S3. Shared derivation.** One pure derivation computes node holdings/targets and per-volume
redundancy from its chunks. A move's additional replica does not increase configured RF. Below
minimum ISR is invalid metadata, not an invented liveness-derived permission. Liveness labels
remain explicit hints; no consumer may use them to authorize recovery or deletion.

**S4. Observation boundary.** The view carries start/end times and explicitly is not a
cross-volume or liveness snapshot. GetVolume consistency depends on #77. Concurrently deleted
volumes are identified as omitted observations. Tombstones are enumerated only through the
registered-node list; completeness outside that registry is explicitly unknown, not asserted.

**S5. Failure and bounds.** RPC/authorization/decoding errors, deadline expiry and configured
resource bounds return failure, without successful partial JSON or an empty-cluster fallback.
A vanished volume during collection may be recorded as S4's bounded omission. Unsupported
fields are absent with a declared unsupported-fields list; zero does not stand for unknown.

**S6. Compatibility.** Existing node/admin identities and their method rules remain unchanged.
Deploy metadata readers before issuing reader credentials. The change adds no new RPC, data
frame, on-disk record, CRD or web listener; old servers refusing readers fail explicitly.

**S7. Evidence and docs.** Test every denied mutation and future-method default, Go/Rust identity
agreement, malformed/multiple-SAN/foreign-CA identities, derive cases, partial observations,
timeouts and resource exhaustion. A later authorized lab check shows stopped/returned members
as hints without mutation. Keep #69 open for UI and integrate #71's summaries/CLI later.

## Tests

- S1/S6: table over every metadata method descriptor, actual mTLS gRPC denials, Rust parser and
  replica handshake refusal; unknown method and unknown role deny by default.
- S2/S3: pure derivation fixtures for moves, mixed chunks, no target, missing/expired nodes,
  invalid ISR and permanently unregistered tombstone nodes; no network calls during derivation.
- S4/S5: deleted/recreated volumes (#77), changing liveness, failed reads, bounded omission,
  registry-limited tombstone coverage, cap/deadline/cancellation; stdout stays empty on failure.
- S7: script argument/identity handling and reader credential permissions; explicitly authorized
  lab verifies read-only behavior while a replica goes down and returns, without a health action.

## Open questions

The reader identity URI/API extension and initial CLI/resource limits are proposals in the
design, not accepted security policy. Dashboard authorization/toolchain is deliberately deferred.

Settled at approval, 2026-11-13: the reader URI and four-method allowlist, and the initial CLI and
limits, as the design proposes (see decisions.md, Settled at approval).

Settled 2026-11-13 (user's instruction): dashboard authorization and toolchain are decided by
#69's later dashboard stage, the toolchain in an ADR, none in this step (see decisions.md).
