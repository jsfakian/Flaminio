# 0008. Metadata recovery requires an authority barrier

- **Status:** Proposed
- **Date:** 2026-11-12

## Context

Architecture §5.4 uses etcd revision-based volume first epochs, while §11.5 describes rebuilding metadata from disks. A snapshot can predate deletion, recreation or ISR changes. Maximum fence/sequence values alone cannot reconstruct acknowledged history. The format's volume-descriptor page is reserved, so disk-only recovery is not implemented. This proposal refines recovery without replacing ADR 0002's dedicated etcd decision.

## Decision

- Stop provisioning and attach/ISR mutations during recovery. Establish and record exclusion of all previous writable authorities before exposing recovered volumes. Losing Kubernetes Node objects or observing liveness expiry is not proof of physical fencing.
- An in-place etcd restore must establish a revision above the old cluster's proven high-water bound, use compaction/watch reset, and validate persisted fences and volume first epochs before issuing tokens. A numeric bump such as 10^9 is an example, not a proof. If the bound cannot be established, refuse this recovery mode.
- Restoring to a new authority requires an explicit recovery identity/generation, persisted on admitted replicas and bound into new capabilities, with old credentials/sessions excluded. Its token/format compatibility and exclusion proof must be designed before implementation. This is not a claim that changing an ID alone fences unreachable machines.
- Inventory disk/cluster identity, volume incarnation, descriptors, tombstones/deletions, placement/ISR versions, fences, snapshot/clone generations and key references. Preserve evidence and quarantine missing, stale or ambiguous disks. An unavailable member must not be assumed current when it returns.
- Select sources from justified authoritative membership and run reconciliation. Highest sequence number, majority of arbitrary surviving disks, or matching checksums alone do not establish acknowledged durability.
- Distinguish no-data-loss recovery from explicit force recovery with possible data loss. Never automatically promote an out-of-ISR copy or resurrect a deleted volume. Force recovery records the chosen sources, lost guarantees and administrator acknowledgement.
- Do not wipe surviving data disks as a substitute for recovery. The current implementation has no supported metadata-reset-with-data-preserved path; preserve disks and refuse unsafe startup until the recovery machinery exists.

## Consequences

Recovery may remain unavailable when authority cannot be established; availability cannot take precedence over acknowledged-data safety. A recovery identity may require new token and on-disk versions. Backup catalogs and immutable deletion/key records must survive independently of the failed metadata service.

## Confirmation

Under #54, test stale snapshots, deleted/recreated names, divergent/out-of-ISR disks, old live targets, returning isolated nodes, missing keys and interrupted recovery. Every no-data-loss success must preserve the acknowledged history; ambiguous cases must refuse. The new identity/exclusion rules require model checks and caught mutations before shipping.
