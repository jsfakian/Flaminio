# Demo: Flaminio vs competitors

This is the honesty layer for scenario 8 of [demo.md](demo.md). Architecture positioning
started in the [README](../README.md); live numbers only come from **this** lab.

## Who we compare to

| Product | Role in the story | Live on demo cluster? |
|---|---|---|
| **Longhorn** (v1) | Direct peer: CSI block on small / edge k8s | Optional (`make demo-longhorn`) |
| **OpenEBS Mayastor** | NVMe-class data path peer | Architecture only for now |
| **Rook-Ceph RBD** | Scale / ops-heavy alternative | Architecture only — wrong weight for 2-node edge |

Pin Longhorn with `LONGHORN_VERSION` in `scripts/lab.conf` when you install it.

## Fair-comparison rules

1. Same VMs (fl-a / fl-b), same NICs / WireGuard path, same fio job files when you go beyond
   the demo checksum workload.
2. Same replica count (RF2) and comparable volume size.
3. Say out loud: Longhorn v1 stores replicas as **files on a host filesystem**; Flaminio uses
   **raw devices** and its own on-disk format.
4. Do not paste third-party blog numbers next to our lab results.
5. If Longhorn is not installed, show Layer A (matrix) + Flaminio-vs-**raw** only.

## Layer A — architecture (always)

| Dimension | Flaminio today | Longhorn | Mayastor | Rook-Ceph |
|---|---|---|---|---|
| Replica storage | Raw + own format (**shipped**) | Sparse files (v1) / SPDK (v2) | SPDK pools | Ceph OSD / RBD |
| Data path | Rust, io_uring, ublk (**shipped**) | iSCSI engine (v1) | NVMe-oF / SPDK | librbd |
| Replication | Sync RF2, ISR + epochs (**shipped**) | Sync replicas | Pool replication | CRUSH / PG |
| Fencing | Epochs at replicas (**shipped**) | Node / attach | Engine-dependent | Ceph watchers |
| Metadata | Dedicated etcd meta (**shipped**) | K8s CRDs | Control plane | Ceph monitors |
| Rebuild | Dirty-region (**partial**) | Full / incremental | Pool rebuild | Backfill |
| Encryption | Planned | LUKS / CSI | Optional | Optional |
| Ops surface | Thin CSI (**partial**) | Mature UI | Lighter than Ceph | Heavy |

## Layer B — same lab (optional)

```bash
make demo-longhorn
make demo-compare          # Ready-time + checksum verify both SCs
# or LONGHORN=1 make demo  # scenario 8
```

Results land in `.lab/demo/bench-compare-*.md`. Treat Ready-time as a demo metric, not a
substitute for [benchmarks](benchmarks/).

## Narrative close

- **Why not Longhorn?** Raw devices, fencing at the replica, metadata off the I/O path; we are
  early on UI and full rebuild.
- **Why not Ceph here?** Two-site edge demo; Ceph’s operational weight is the wrong pitch.
- **Mayastor:** respected NVMe peer; head-to-head is on the roadmap ([ARCHITECTURE §16.3](ARCHITECTURE.md)),
  not claimed today.
