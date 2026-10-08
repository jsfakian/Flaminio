# 0006. 2-node edge sites keep writing on a single copy

- **Status:** Accepted
- **Date:** 2026-10-08

## Context

A 2-node edge site (plus a data-less witness) stores every chunk on both data nodes (RF=2). When
one node is down, only one copy is in sync. With `minInSyncReplicas: 2`, every write would block
until the node returns or an admin steps in. Edge sites are often unattended, and an outage of
the applications is usually worse than a short window of reduced redundancy.

## Decision

- The 2-node edge topology defaults to `replicas: 2`, `minInSyncReplicas: 1`. **Writes continue
  on the surviving node while the other is down.**
- The epoch, fencing and lease rules (ARCHITECTURE §8.4–§8.5) still apply in full, so continuing
  on one copy never causes split-brain. The witness is what lets the surviving node win the
  metadata majority and take over.
- When the failed node returns, it resyncs only the regions written while it was away (§11.4).

## Consequences

- **Risk accepted:** while degraded, acknowledged writes exist on one disk only. If that disk or
  node is lost before the resync finishes, those writes are lost. The data is recoverable only
  up to the last backup or async-replication point.
- To make that risk visible and bounded:
  - Volumes report a `Degraded` condition with a `SingleCopyWrites` reason, and a metric and
    alert fire as soon as a volume accepts its first single-copy write.
  - The degraded period triggers an extra snapshot and async-replication or backup cycle to the
    central site, when that site is reachable.
  - Resync gets top priority as soon as the second node returns.
- Sites that would rather block writes than accept the risk can set `minInSyncReplicas: 2` in
  their StorageClass.
- This default applies **only** to the 2-node edge topology. Bare-metal and 3-node defaults stay
  at `replicas: 3`, `minInSyncReplicas: 2`.
