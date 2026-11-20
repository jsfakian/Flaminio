# Issue #69 ADR (Accepted with the plan, 2026-11-13): a distinct read-only metadata identity

- **Status:** Accepted with the plan, 2026-11-13; numbered and moved into docs/adr/ when implemented.
- **Date:** 2026-11-12

## Context

A dashboard/status collector must not hold a metadata administrator identity. Node identities
currently permit generic reads but are neither a named read-only role nor sufficient for the
registry-based tombstone view. #71 now owns general observability/CLI foundations.

## Decision proposed

Use `spiffe://<trust-domain>/flaminio/reader/<name>`, parsed equivalently in Go/Rust. Metadata
permits exactly ListNodes, ListVolumes, GetVolume and node-scoped ListTombstones. All future or
mutation methods default-deny; replicas continue to accept only node/io identities plus a
matching valid capability token. Issue read-only credentials only after server support exists.
No admin-credential fallback or tenant access is introduced.
Settled at approval, 2026-11-13: this decision is accepted with the plan (see decisions.md,
Settled at approval).

## Consequences

New URI parsers and CA tooling must roll out together; old implementations can refuse safely.
The identity exposes administrative read data and requires admin credential distribution.
It does not supply Kubernetes/browser authorization or a consistent cluster snapshot.

## Confirmation

Descriptor-driven every-method denial tests, Go/Rust URI vectors, actual TLS tests and replica
handshake refusal; no tests claim the unimplemented browser dashboard exists.

## Proposed follow-on read composition (2026-11-20; unapproved)

The accepted ADR text above is preserved as historical approval. This **Proposed** follow-on
would supersede only its exactly-four-read-method restriction when #90/#91 are installed;
it does not supersede the accepted Reader URI, default deny, credential or replica boundaries.
#91 is the sole owner of adding ReadIdentityVolumePage to that explicit Reader allowlist;
#69 consumes that shared contract and never grants a token/grant/Attach or writer method.
[#69's inspection mode](tombstone-inspection-contract.md) adds only bounded read-only
node-history enumeration to the existing ListTombstones method, with no confirmation rights.

Exact store identity/paging, strict complete enumeration and the local observational schema
are composed in [view-contract](view-contract.md); exact metadata-role TLS and application
allocation/output credits are fixed in [security-contract](security-contract.md). Their
R1-C1–C4/SEC69-R1-01–02 provenance is recorded in [decisions](decisions.md). All additions
remain unapproved future implementation contracts, with later approval and a numbered
superseding ADR required before changing the accepted authorization policy. Existing accepted
ADRs0001/0002 remain constraints. No new runtime, toolchain, disk/frame, dashboard or mutation
authority is chosen here.
