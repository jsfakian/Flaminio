# Issue #91 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-20, of the published clear snapshot (`docs/issues/publication-2026-11-20.json`, content_sha256=`f9af95f4b851d6bb8d9cfeedfa0b8cabe01584c2b380c2f4c9fecc347687a94c`).
- **Design:** approved by Ioannis Sfakianakis, 2026-11-20, of the same published clear snapshot.

The 2026-11-20 approval covers the published clear post-review text (including every plan-review fix recorded after the earlier 2026-11-13 approval, where present). Nothing is implemented yet.

## Current status

Dedicated tracker move authorized. Spec and design of the published clear snapshot are
approved 2026-11-20 (see Approvals). No #63 approval or reviewer round is transferred as a
new #91 approval/round.

## Historical source provenance

Source63 at `1a4a14df1a8b1ae6c2762bbaabf4ba5489305d31` was approved2026-11-13.
Its guard-only first half of step1 and dedicated-owner move were approved; later autoscaler,
evacuation and cloud work remain63 scope outside this owner. The source decisions/ADR
explicitly left neither(a) bounded best effort nor(b) synchronous fail-closed admission chosen
in that plan, requiring the owning ADR/later plan to supply exact APIs/race proof before
implementation while the original intent promise stands unnarrowed.
This owner may supply a concrete Proposed ADR mechanism/research; it must not invent human
acceptance or quietly weaken requirements. A truly irreducible choice remains explicit and
not review-clear; unaffected batch work continues.

Original source approval command/namespace and PDB candidate are provenance, not endorsement
of the concrete R1 defects. Actual63R1 findings remain transferred and unresolved until this
owner completes its own real full fresh review/address loop.

## Proposed technical choices (not approvals)

- **2026-11-20, mechanism draft:** propose option(b), synchronous fail-closed admission, in
  this new owner. The source #63 user's neither-option-selected instruction remains history;
  no approval is inferred. [ADR](adr-proposal.md) supplies actual APIs and race/lifecycle proof.
- **Coverage/accounting:** Node validating precommit coverage, ConfigMap-controlled finalizer
  anchors with supported nodeName placement and separate immutable UID checks; single durable
  Kubernetes CAS slot, no dual-PDB/Deployment counts or node writes. UID preconditions fence
  old admitted evictions after retirement. Finalizer removal and slot release are distinct.
- **Isolation and reader:** separate `flaminio-disruption-guard` Restricted namespace, dedicated
  metadataReader, strict bounded enumeration and #90 exact/full/paged adapter. These technical
  corrections are unapproved; source command/namespace decisions are not silently rewritten.
- **Operational costs:** global serialization, staging/resource bounds, no timeout reclamation,
  maintenance-window installation/uninstall and management-node repair boundary are explicit.
  No runtime safety mechanism is deferred to a future unspecified release gate.
- **Evidence:** pinned k3s `v1.36.5+k3s1`/Kubernetes `v1.36.5` API qualification is separate from
  the original RF2/minISR1 data proof on #83-qualified physical hosts. Existing VM refusal is
  unmet storage evidence, never reason for a profile bypass.

## Proposed R1 corrections (unapproved)

- Existing-name kubelet CREATE returns exact AlreadyExists before request-UID allocation.
  One durable same-name claim serializes absent-name creates; overlapping different UIDs
  hold without ghost Pods, while delayed original creates retain coverage without timeouts.
- [Identity contract](identity-contract.md) owns exact metadataID==KubeName, sticky everStorage,
  saved exact old-store UUID evidence, and Storage/ProvenNonStorage recovery. Proven absence
  requires #90 permanent history proof, complete no-reference inputs and no prior storage;
  lack of a metadata Node alone is never a downgrade. Returned/moved-away exact old stores
  have concrete retirement paths across same-name UID recreation and restart.
- These address actual #91 R1 inputs; no review clearance, human acceptance, source #63
  approval transfer or runtime result is inferred from this revision.

- **R1-C1 / SEC91-R1-01:** both pinned admission plugins use explicit per-control-plane client credentials matched to the exact service DNS/port. The listener verifies dedicated client trust, identity and EKU before HTTP/decoded review allocation; server caBundle and NetworkPolicy are not client authentication. HA/rotation and forged-client zero-allocation fixtures are explicit in security-contract.md.
- **SEC91-R1-05:** the guard uses bounded two-pass production response decoding with structural preflight, conservative charged direct construction and transient reservations in the same process-wide pool. Generated response Unmarshal is not called; existing wire limits remain. The reader appendix owns exact types, nested limits and real gRPC allocation fixtures.
- **SEC91-R1-06:** HTTP1-only serving follows the pinned API-server client, with bounded connections/handshakes/workers and finite TLS/header/body/whole-request/write/idle waits, including unwind. Prehandler decoded header storage is budgeted; health checks use local exec rather than an unauthenticated network path. These are proposed fixes, not executed transport qualification.

## Earlier R2 transport proposal — superseded by the R4 resource choice

- **SEC91-R2-01:** pin public static HTTP2 window/header/buffer controls, bounded single-call transport leases, pre-parser frame admission and the actual pinned queue-compaction/control-buffer behavior. Reserve lease, queued DATA/frame/HPACK/compaction/transient and decoded/retained memory in one64MiB invariant before calls. Full-response concurrency may shrink and connections are not reused. These are proposed implementation/qualification requirements; fresh R3 must verify tiny-frame allocation and cleanup against the pinned source rather than infer them from payload arithmetic.

## Earlier R3 allocation proposal — superseded by the R4 resource choice

- **SEC91-R3-01:** replace the implicit default response pool with the exported guard-local exact-capacity noncaching pool, retaining capacity charges through actual last-reference cleanup. The direct pinned-peer profile rejects PADDED DATA before forwarding its header to gRPC; its stock writer is unpadded. Re-derive component bounds from actual capacities and add alternating-tier/padding/credit-reuse fixtures. The source trace disproved the old component proof; it did not by itself prove a whole64MiB overflow. All changes remain proposed/unapproved.

## Proposed R4 resource correction (unapproved)

**SEC91-R4-01/02:** the earlier R2/R3 whole-stock-library64MiB proof and custom pool/PADDED/frame proof mechanisms above are historical review proposals, not the current contract. Apply cut-before-add: keep stock gRPC and ordinary supported receive/header/admission controls, with bounded typed application data, actual verified cgroup memory.max≤256MiB/swap0 containment and explicit fail-closed Quiescing/exit75/supervised replacement. Remove the maintained-fork/terminal-cleanup-API candidate; no dependency source or module replacement is selected.

Stock internal cache references and abandoned receive roots may survive until process death. No timer, Put/ConnEnd assumption or forced root retirement releases their ownership. Known terminal resource/reader failure stops new admission; replacement re-reads durable UID/resourceVersion/token state and preserves uncertain remote writes/slots. The later healthy-call fixture crosses observed process termination and restart.

The resource/restart/maintenance cost is Proposed and needs human acceptance; it does not quietly relax automatic coverage, fail-closed controller/metadata failure, read-only metadata or durable single-slot accounting. [Resource lifecycle](resource-lifecycle-contract.md) records the exact operational limits, alternatives and evidence.

## Proposed R5 alignment (unapproved)

- **R5-C1 / SEC91-R5-01:** one header-completion+24s whole-handler deadline and+23s action
  cutoff govern body/decode, initial reads, reader and API/CAS work. Clamp each actual
  subdeadline to remaining time; reserve the last≤1s for reply/unwind, dispatch no durable
  write/retry after cutoff and retain unknown already-submitted writes. The qualification
  fixture includes nonzero earlier work and a reader exhausting its remaining allowance.
- **SEC91-R5-02:** qualification accepts supported compressed stock-gRPC responses within
  encoded/decompressed/cardinality caps; unsupported encoding, malformed compression and
  decompressed boundary+1 hold/recycle. No blanket compression ban or custom transport.
  These are draft corrections, not fresh review clearance or executed qualification.
