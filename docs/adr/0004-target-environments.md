# 0004. Bare metal first, then edge, then cloud

- **Status:** Accepted
- **Date:** 2026-10-08

## Context

All three environments are long-term goals, but they pull the design in different directions.
Bare metal wants raw performance. Edge wants tiny footprints and tolerance for disconnection.
Cloud wants cost-aware placement and has to cope with ephemeral disks.

## Decision

1. **Bare metal** (phases 0–3) is the reference environment. All performance targets are
   measured there.
2. **Small edge clusters** (phase 4): 1 node, 2 nodes + witness, or 3 nodes. Compact mode,
   TPM-sealed keys, WAN-tolerant backup and async replication.
3. **Cloud** (phase 5): cloud KMS, instance-store NVMe, zone-cost-aware placement, autoscaler
   integration.

## Consequences

- Phases 0–3 must not hard-code assumptions that block edge later: no fixed "≥ 3 nodes", a
  compact resource mode from the start, and every external dependency (KMS, object store, DR
  peer) treated as sometimes unreachable.
- Async cross-cluster replication moves forward to phase 4, because edge sites need it for DR to
  a central site.
- Erasure coding, RDMA, NVMe-oF export, RWX, instant restore and the UI move to phase 6.
