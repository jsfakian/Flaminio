# Issue #69: Read-only cluster view before the UI — specification

[Intent](intent.md) · [Design](design.md). Original spec approved 2026-11-13; not implemented.
R1 corrections below and the linked contracts are **Proposed, unapproved**. This is the
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
trust domain. The original reads are ListNodes, strict ListVolumes, GetVolume and node-scoped ListTombstones.
Consume #91's sole page-read allowlist extension when #90 is installed; the proposed node-history
tombstone inspection is read-only. All other/future methods default-deny; replicas refuse it.
Require the configured exact metadata-server URI/role before accepting responses, as specified
in [security-contract](security-contract.md); generic same-domain trust is insufficient.

**S2. Command.** A single bounded status command renders nodes, volumes, redundancy, requested
moves and registry-enumerated pending tombstones for people and JSON consumers. It needs only
reader credentials; it reads no node control socket or token-signing key and performs no write.

**S3. Shared derivation.** One pure derivation computes node holdings/targets and per-volume
redundancy from its chunks, with the exact nested schema and reductions in [view-contract](view-contract.md).
Committed ISR, earned ISR and pending INITIAL counts are distinct; exact-store availability
never aliases a replacement UUID. A move's additional replica does not increase configured RF. Below
minimum ISR is invalid metadata, not an invented liveness-derived permission. Liveness labels
remain explicit hints; no consumer may use them to authorize recovery or deletion.

**S4. Observation boundary.** The view carries start/end times and explicitly is not a
cross-volume or liveness snapshot. GetVolume consistency depends on #77. Concurrently deleted
volumes are identified as omitted observations. Tombstones are enumerated only through the
registered-node list, including retained UUIDs within each node prefix. Its cursor/row scope
and unknown totals are fixed in [inspection-contract](tombstone-inspection-contract.md).
Omissions make derived holdings/served-volume totals observed lower bounds, never complete zero.

**S5. Failure and bounds.** RPC/authorization/decoding errors, deadline expiry and configured
resource bounds return failure, without successful partial JSON or an empty-cluster fallback.
Strict ListVolumes must explicitly certify complete candidate validation; old-server default
false cannot yield success. Full/paged volume attempts and every inspection page must complete
within all limits. [Security-contract](security-contract.md) fixes before-typed-allocation
collection/derivation credits and bounded encoding; excess refuses before allocation/publication.
A vanished named volume may be S4's bounded omission. Unsupported fields are absent or explicit
null according to schema, with a declared unsupported-fields list; zero does not mean unknown.

**S6. Compatibility.** Existing node/admin identities and their method rules remain unchanged.
Deploy metadata readers and required shared adapters before issuing reader credentials.
The proposed composition consumes #90's read-only page RPC through #91's shared authorization
owner and adds only a bounded inspection request mode, no writer, data frame, on-disk record,
CRD or web listener. Old or unsupported servers fail explicitly; legacy reads use an explicitly
validated pre-#90 adapter, never a fallback from ACTIVE or an unknown schema.

**S7. Evidence and docs.** Test every denied mutation and future-method default, Go/Rust identity
agreement, malformed/multiple-SAN/foreign-CA identities, derive cases, partial observations,
timeouts and resource exhaustion. A later authorized lab check shows stopped/returned members
as hints without mutation. Keep #69 open for UI and integrate #71's summaries/CLI later.

## Tests

- S1/S5/S6: [security-contract](security-contract.md) exact-role TLS, hostile predecode and bounded
  allocation/output fixtures; table over metadata method descriptors, actual mTLS denials, Rust parser and
  replica handshake refusal; unknown method and unknown role deny by default.
- S2/S3: [view-contract](view-contract.md) fixtures define exact-store/INITIAL, availability thresholds,
  chunk→volume aggregation, JSON/human equivalence and omission totals; also moves, no target, missing/expired nodes,
  invalid ISR and permanently unregistered tombstone nodes; no network calls during derivation.
- S4/S5: strict damaged-header/old-server/empty enumeration and all [inspection-contract](tombstone-inspection-contract.md)
  service-to-status cases; full/paged delete/recreate (#77/#90), changing liveness, bounded omission,
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
