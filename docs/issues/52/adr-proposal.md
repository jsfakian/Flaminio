# Accepted with the plan, 2026-11-13: immutable snapshot-reader grants and standalone versioned backup archives

- **Status:** Accepted with the plan, 2026-11-13; numbered and moved into docs/adr/ when implemented.
- **Date:** 2026-11-12
- **Issue:** [#52](https://github.com/jsfakian/Flaminio/issues/52)

## Context

The existing [intent](intent.md) scopes manual single-snapshot incremental backup, verification
and raw file/device restore. It requires accepting an ADR before design. This provisional
[design](design.md) is drafted under the explicit batch instruction; acceptance remains a gate
before implementation. Schedules, retention, PVC restore and broad recovery are still future work.
Settled at approval, 2026-11-13: accepted with the plan (see decisions.md, Settled at approval).

Current target-only Hello/token sessions fence the volume, so a backup reader cannot reuse them.
A signed offline token with no proven elapsed-clock/expiry/revocation contract cannot establish
that its read authority ends at deletion. Store layers exist; #51's committed cut/root/diff and
#50's stored encryption/key identity are unimplemented dependency contracts. Audit Proposed
ADR0009/0011 require retained keys/pins and publication after durable exact-version objects.

## Decisions proposed

- Rust standalone worker/library in dataplane/ uses a pinned aws-sdk-s3 adapter and proposed
  blake3/zstd/XChaCha20-Poly1305/HKDF/zeroize dependencies. Future Go controllers use gRPC. Validate
  dependency licences/MSRV and choose exact versions before implementation.
  Settled at approval, 2026-11-13: this worker and these dependencies; versions pinned after the checks (see decisions.md, Settled at approval).
- A distinct backup identity and snapshot-reader/v1 listener serve only a pinned committed
  SnapshotRef. Registered source grants bind worker, incarnation, complete store/replica lifetime
  and chunk proof. They never issue target tokens or permit writes/current-data reads/fences.
- Metadata registers sources before issuing grants. Revocation closes admission and drains
  every registered source before pin release or externally absent deletion; restarted sources
  validate online. Offline holders block unless their node is forgotten (#13 S19), the one
  exclusion proof (round 1, R1-C10). Expiry/liveness alone is not proof. Already disclosed data
  cannot be recalled.
- A versioned repository stores 4 MiB content-addressed encrypted blocks and a complete encrypted
  sparse manifest graph per archive. No base manifest is needed to read an archive. Exact object
  version IDs, authenticated lengths/positions and durable verified publication define completion.
  Key derivation/domains/envelope/map and source reader contracts are in [contracts](contracts.md).
- Pins retain snapshot layers through completion/durable abort. Encrypted volumes are refused in
  this increment, since #50 and #51 admit no snapshot of one (round 1, R1-C6, R1-S1); the later
  plan that lifts #51's refusal brings this decision: pins also keep the key references, retained
  archive key references survive ordinary live-volume deletion, the worker reads stored
  ciphertext, not a DEK, and no encrypted plaintext recovery is claimed.
- A universal object-client request hook covers every actual attempt/control call/retry for #59;
  physical wire shaping is separately qualified. S3 endpoint profile preserves mTLS/TLS1.3 plus
  signed authentication; native providers lacking that boundary need an explicit exception ADR.

## Options

- **Use target Hello/token:** smaller change but can take the application's session/fence and
  grants far more than immutable read access; rejected as the proposed direction.
- **Short-lived offline reader tokens:** potentially better availability, but a trusted elapsed-
  clock/expiry/restart/revocation proof is absent. Do not claim the right ends at deletion.
- **Explicit registered-source closure barrier:** safe without timeout-derived authority, at
  the availability cost of blocked deletion while a holder is unreachable; proposed direction.
  Settled at approval, 2026-11-13: this proposed direction (see decisions.md, Settled at approval).
- **Delta-manifest chains:** less metadata copying, but later restore depends on other backup
  manifests and conflicts with the existing intent's standalone archive outcome.

## Consequences and open choices

Revocation/deletion may wait indefinitely for an unavailable registered reader source that is not
forgotten. Root publication uncertainty can retain key references conservatively, once encrypted
backups are admitted. A versioned/conditional/durable S3 profile is narrower than arbitrary
“S3-compatible” endpoints and needs real qualification.
SeaweedFS Apache-2.0 is a candidate lab store, not an accepted licence/deployment or certified
Object Lock backend. No store is installed or exercised by this plan.
Settled at approval, 2026-11-13: the proposed profile; SeaweedFS as the lab store (see decisions.md, Settled at approval).

Agree #51 snapshot-root/completeness byte ABI and #61 store lifetime evidence before using source
proofs. Diff candidates can include false positives, unlike the intent's identical-answer wording;
this cost/contract adjustment remains explicit. Agree #50 identity/integrity/key-lifetime policy;
XTS/CRC is not malicious integrity or replay prevention. Off-cluster KEK escrow/plaintext recovery
is #54's later seam; archive presence alone does not prove a restorable point or bounded RPO.
Settled at approval, 2026-11-13: diff false positives and #50's key seams as proposed (see decisions.md, Settled at approval).
Settled 2026-11-13 (user's instruction): #52 uses #51's completeness evidence and #61's store UUID as their plans settle them; its source proofs are built only once both are implemented, and a source without either is refused (see decisions.md).

No accepted ADR is superseded and no approval/accepted risk is recorded. Audit0008/9/11 remain
Proposed. The full issue remains open after the scoped increment.
Settled at approval, 2026-11-13: this ADR is accepted with the plan (see decisions.md, Settled at approval).

## Confirmation

Service/reader/model tests catch unauthorized sessions, post-revocation permits and premature
pin release. Repository fuzz/golden/failure tests catch corrupt/substituted versions and early
root publication. Later explicit lab tests measure source/application cost and prove qualified
store/version/restore behavior. No runtime test or measured result is asserted during drafting.
