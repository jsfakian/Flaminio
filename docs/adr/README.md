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
| [0007](0007-privileged-data-plane-pod.md) | A privileged data-plane pod, until privilege separation | Proposed |
