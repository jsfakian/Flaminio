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
