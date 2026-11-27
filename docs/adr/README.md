# Architecture Decision Records

Each significant design decision is recorded here as a short ADR: the context, the decision and
its consequences. An ADR's status is one of **Proposed**, **Accepted**, **Superseded by NNNN** or
**Rejected**. Accepted ADRs are not edited. To change one, write a new ADR that supersedes it.

| ADR | Title | Status |
|---|---|---|
| [0001](0001-languages.md) | Go for the control plane, Rust for the data plane | Accepted |
| [0002](0002-metadata-backend-etcd.md) | Metadata service backed by a dedicated etcd cluster | Accepted |
| [0003](0003-kernel-baseline.md) | Linux 6.6 minimum kernel | Accepted |
| [0004](0004-target-environments.md) | Bare metal first, then edge, then cloud | Accepted |
| [0005](0005-replica-confirmed-leases.md) | Replica-confirmed I/O leases | Accepted |
| [0006](0006-edge-two-node-degraded-writes.md) | 2-node edge sites keep writing on a single copy | Accepted |
| [0015](0015-node-wide-shutdown-requires-held-fence.md) | Node-wide shutdown recovery requires a held shutdown fence | Accepted |
| [0016](0016-create-retains-immutable-admitted-plan.md) | Creation retains an immutable admitted plan and cooperative progress | Accepted |
| [0017](0017-bound-uncertainty-with-per-chunk-covers.md) | Bound uncertainty with per-chunk maximum-Version covers | Accepted |

The 2026-11-12 review found that ADR 0005's immediate-read rule predates the stricter,
implemented takeover protocol. Proposed ADR 0007 records the correction; do not use the old
wording to relax the current protocol. Accepted ADR files remain unchanged pending acceptance
of a superseding decision.

| Proposed ADR | Title | Status |
|---|---|---|
| [0007](0007-takeover-readiness-and-clock-contract.md) | Takeover readiness and clock contract (supersedes 0005 upon acceptance) | Proposed |
| [0008](0008-metadata-recovery-authority.md) | Metadata recovery authority barrier | Proposed |
| [0009](0009-encrypted-data-and-key-retention.md) | Encrypted integrity and retained-key lifetimes | Proposed |
| [0010](0010-block-device-durability-contract.md) | Block-device durability contract | Proposed |
| [0011](0011-snapshot-and-backup-commit-boundaries.md) | Snapshot and backup commit boundaries | Proposed |
| [0012](0012-privileged-data-plane-pod.md) | A privileged data-plane pod, until privilege separation | Proposed |
| [0013](0013-certificates-rotate-by-file-replacement.md) | A certificate is rotated by replacing its files; every process watches its own | Proposed |
| [0014](0014-maintenance-attach-only-while-not-held.md) | A target that no client asked for is admitted only while no client holds the volume | Proposed |
