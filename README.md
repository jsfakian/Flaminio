# Flaminio

**Fast, secure, self-healing block storage for Kubernetes.**

Flaminio turns the local disks of your Kubernetes nodes into a replicated, highly available
block storage pool. You add a disk and it joins the pool. You pull a disk and the data moves
somewhere else. You lose a node and your volumes keep serving I/O. You lose a whole cluster
and you restore from an encrypted, immutable backup.

> **Status: MVP data plane, in the lab.** A Rust data plane serves volumes as Linux block
> devices (`/dev/ublkbN`) from its own [on-disk format](docs/format/on-disk-format-v1.md), with
> synchronous replication to a second server. Reads through the full stack reach 86–98% of the
> raw disk, and replication survives a replica outage with no failed writes. There is no control
> plane yet (no Kubernetes integration, metadata service or TLS), so nothing here is ready for
> real data. See the [MVP plan](docs/mvp-plan.md), [lab](docs/lab.md) and
> [benchmarks](docs/benchmarks/).

---

## Why another storage system?

Cloud-native block storage usually forces a choice between:

- **Simple but slow.** Easy to install, but each I/O goes through several user/kernel copies and
  protocol hops, and rebuilds copy a whole replica.
- **Fast but heavy.** Excellent performance, but you need dedicated storage experts to run it,
  dedicated hugepages and CPU cores, and a separate operational model.

Flaminio aims to give you **close to raw-device performance**, with the **operational simplicity
of a Kubernetes-native app** and **security turned on by default**.

## Design goals

| Goal | What it means in practice |
|---|---|
| **Performance** | A thread-per-core, shared-nothing data plane in Rust on `io_uring` with direct I/O to raw block devices (no filesystem under the replicas). `ublk` for a low-overhead local block device. Reads come from the local replica when it is in sync. |
| **Security by default** | mTLS on every control and replication link. Per-volume encryption at rest, done on the node that runs the workload, so **replica nodes only ever see ciphertext**. KMS integration, signed releases, a minimal-privilege runtime. |
| **Effortless device lifecycle** | Disks are discovered automatically and claimed by policy. Removing a disk safely drains it first. A disk that fails suddenly is handled the same way as a planned removal, only faster. |
| **Configurable replication** | Replication factor 1–5 per StorageClass, with failure-domain-aware placement (disk, node, rack, zone). Erasure coding is on the roadmap. |
| **Fast, safe failover** | Epoch-based fencing and replica-confirmed leases, so split-brain is impossible by construction. Volumes fail over in seconds once the old node is confirmed fenced (otherwise after a bounded lease timeout), without STONITH hardware. |
| **Fast rebuilds** | Volumes are split into chunks that are spread across the cluster. Rebuilds run many-to-many, and a replica that comes back after a short outage only resyncs the regions it missed. |
| **Disaster recovery** | Crash- and application-consistent snapshots, incremental deduplicated backups to S3-compatible storage (client-side encrypted and optionally immutable), and asynchronous replication to a second cluster. |
| **Data integrity** | End-to-end checksums, background scrubbing, and automatic repair from a healthy replica. |

### Non-goals (for now)

- A shared/distributed filesystem (CephFS-style). RWX comes later through an NFS gateway.
- Object storage.
- Running outside Kubernetes.

## How it works (30-second version)

```
   Pod ──▶ /dev/ublkbN ──▶ Volume Target (on the pod's node)
                             │   encrypts, checksums, fans out writes
              ┌──────────────┼──────────────┐
              ▼              ▼              ▼
          Chunk replica   Chunk replica   Chunk replica      (on different failure domains)
          node-a/nvme0    node-c/nvme1    node-f/nvme0
```

- A **volume** is split into **chunks** (256 MiB by default). Each chunk has *N* replicas placed in
  different failure domains.
- A **Volume Target** runs on the node that uses the volume. It exposes a block device to the pod
  and sends every write to all in-sync replicas of the affected chunk in parallel.
- A small **replicated metadata service**, backed by a dedicated etcd cluster (not the Kubernetes
  one), stores placement, epochs and in-sync sets. It is not consulted on normal reads and
  writes, so I/O keeps running even if it loses quorum. Kubernetes CRDs hold only *intent*
  (what you asked for), not high-churn internal state.
- **Node agents** discover and manage disks, run the replica store, and report health.

The full design is in **[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)**.

## What using it looks like (target UX)

Let Flaminio claim every empty NVMe disk on storage nodes:

```yaml
apiVersion: storage.flaminio.io/v1alpha1
kind: DevicePolicy
metadata:
  name: nvme-all
spec:
  nodeSelector:
    matchLabels:
      flaminio.io/storage-node: "true"
  deviceSelector:
    type: nvme
    minSize: 100Gi
    mustBeEmpty: true          # never claims a disk that has partitions or a filesystem signature
  deviceClass: fast
```

Define a StorageClass:

```yaml
apiVersion: storage.k8s.io/v1
kind: StorageClass
metadata:
  name: flaminio-fast-r3
provisioner: csi.flaminio.io
allowVolumeExpansion: true
volumeBindingMode: WaitForFirstConsumer
parameters:
  deviceClass: fast
  replicas: "3"
  minInSyncReplicas: "2"       # writes block (they are not silently degraded) below this
  failureDomain: zone          # spread replicas across zones (falls back per policy)
  encryption: "true"
  kmsProvider: vault-prod
  dataLocality: preferred      # try to keep one replica on the consumer's node
  backupPolicy: hourly-s3
```

Remove a disk safely:

```bash
kubectl flaminio disk drain node-c/nvme1      # moves chunks off the disk, then marks it Drained
kubectl flaminio disk release node-c/nvme1    # wipes it (crypto-erase) and gives it back
```

## Comparison (initial positioning)

This is our own early positioning. We need to check it against current releases before we
publish it anywhere.

| | Flaminio (planned) | Longhorn (v1 engine) |
|---|---|---|
| Replica storage | Raw block device, own on-disk format | Sparse files on a host filesystem |
| Data path | Rust, `io_uring`, `ublk` / NVMe-oF | Per-volume engine, iSCSI frontend |
| Replication unit | 256 MiB chunks, declustered | Whole volume replica |
| Rebuild | Many-to-many, dirty-region resync | Mostly full replica rebuild |
| Internal state | Dedicated etcd-backed metadata service, off the I/O path | Kubernetes CRDs (etcd) |
| Fencing | Epochs enforced by replicas | Relies on Kubernetes node/attach state |
| Encryption at rest | Built in, replicas hold ciphertext only | Via LUKS / CSI options |
| Disk lifecycle | Policy-driven discovery, drain, crypto-erase | Manual disk config per node |

## Requirements (target)

- Kubernetes ≥ 1.29
- Linux kernel ≥ 6.6 with `ublk_drv` ([ADR 0003](docs/adr/0003-kernel-baseline.md))
  - Meets the baseline: Ubuntu 24.04 (6.8), Ubuntu 22.04 with the HWE kernel, RHEL / Rocky /
    Alma 10 (6.12), Debian 13 (6.12)
  - **Does not meet it:** RHEL 9 family (5.14), Debian 12 (6.1)
- x86_64 or arm64
- **Bare metal (first target):** at least 3 nodes for replicated volumes. 3 or 5 nodes run the
  metadata service. A 10 GbE network at minimum. 25 GbE+ or RDMA recommended for NVMe-class
  performance.
- **Edge (phase 4):** 1 node, 2 nodes + a small witness, or 3 nodes
- **Cloud (phase 5)**

## Roadmap

| Phase | Scope |
|---|---|
| **0. Foundations** | On-disk format, replica store, single-node volumes, `ublk` frontend, benchmark harness |
| **1. Replicated MVP** | Metadata service (etcd-backed), chunk placement, synchronous replication, ISR + epoch fencing + replica-confirmed leases, CSI driver (RWO, block + fs), device discovery and drain |
| **2. Resilience** | Dirty-region resync, many-to-many rebuild, scrubbing and repair, gray-failure detection, live data-plane upgrade, chaos test suite |
| **3. Security & DR** | Encryption at rest + KMS (Vault, KMIP), snapshots and clones, incremental S3 backups, immutable backups, restore to another cluster |
| **4. Edge** | Compact mode, 1-node / 2-node + witness / 3-node topologies, TPM-sealed keys, async cross-cluster replication (edge → central), WAN-tolerant backups |
| **5. Cloud** | Cloud KMS providers, instance-store NVMe, zone-cost-aware placement, autoscaler integration |
| **6. Scale & reach** | Erasure coding, RDMA transport, NVMe-oF export and vhost-user-blk for VMs, RWX via NFS gateway, instant restore, UI |

Phases 0–3 target **bare metal**. Edge and cloud follow in that order
([ADR 0004](docs/adr/0004-target-environments.md)).

## Repository layout (planned)

```
flaminio/
├── dataplane/          # Rust: I/O engine, replica store, volume target, frontends, node agent, privileged helper
├── control/            # Go: operator, CSI driver, controllers, admission webhooks
├── meta/               # Go: metadata service (dedicated etcd backend initially)
├── api/                # CRDs (Go types) and gRPC/protobuf definitions
├── cli/                # kubectl-flaminio plugin
├── deploy/             # Helm chart, manifests
├── test/               # e2e, chaos (fault injection), performance benchmarks
├── scripts/            # test-lab automation (VMs on the test servers), driven by the Makefile
├── Makefile            # `make help`: build, test, lint, lab VMs, benchmarks
└── docs/               # architecture, ADRs, format spec, lab guide, benchmarks
```

## Contributing

The project is at the design stage. The most useful thing you can do right now is review
[docs/ARCHITECTURE.md](docs/ARCHITECTURE.md), especially its **Open questions** section.

## License

Apache License 2.0 (proposed).
