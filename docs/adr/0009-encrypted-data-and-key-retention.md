# 0009. Encrypted data integrity and retained-key lifetimes are explicit

- **Status:** Proposed
- **Date:** 2026-11-12

## Context

Architecture §15.1 includes a compromised replica, §12 makes authenticated checksums optional, and §15.4 describes destroying a volume's DEK while §14 retains immutable backups. These guarantees need a single key and integrity model before #50–#54. AES-XTS alone protects confidentiality, not authentication (NIST SP 800-38E).

## Decision

- For the encrypted profile that claims protection from malicious replicas, require authenticated integrity. Bind ciphertext and tags to the immutable data/volume incarnation, logical address and the version context needed to prevent substitution. The exact format, key separation and compatibility flags must be specified before implementation.
- Linked clones and snapshots must retain the original authenticated data identity where extents are shared; a child's writable data uses its own defined identity. Decide this before claiming instant encrypted clones.
- Authentication does not by itself prevent replay of an older valid ciphertext/tag. Either maintain trusted freshness evidence and verify it on reads or state explicitly that malicious replay is outside the supported guarantee. Scrub-only detection is not read-time replay prevention.
- Retained snapshots, clones and backups pin the keys and metadata needed to restore them. Ordinary PVC deletion removes the live volume without silently destroying keys required by retained recovery points.
- Destroying every recoverable copy is a separate, authorized crypto-erasure operation. It checks retention/legal holds and the complete reference graph, records an audit event, and describes external escrow/KMS copies that it cannot revoke. Disk release does not destroy another disk's volume or backup keys.
- Separate live-volume, integrity and repository keys. Define KEK re-wrap, DEK rewrite/rotation, repository key rotation, KMS outage behavior and escrow recovery. GC may release a key only after all retained references and holds are gone.

## Consequences

Tags and freshness metadata cost space and CPU. Shared encrypted clones and immutable backup retention constrain key deletion. A destroyed or lost external KMS key can make otherwise intact backups unrestorable; restore exercises must include keys, not only ciphertext.

## Confirmation

#50/#51/#53/#54 must test tampering, address/volume substitution, documented replay behavior, parent/clone deletion orders, retention-held keys, KMS rotation/outage and restore after PVC deletion. No new encrypted-format bit is finalized before those tests and the format ADR are reviewed.
