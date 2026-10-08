# MVP plan: replicated data plane on two servers

**Goal:** the data plane alone, with no control plane, no Kubernetes and no metadata service. One
VM on each test server, each with one virtual disk. A replicated Flaminio volume spans both VMs,
and a microbenchmark runs against it.

```
      rhythm (147.52.71.227)                         rhythm-backup (147.52.71.226)
 ┌──────────────────────────────────┐            ┌──────────────────────────────────┐
 │ VM fl-a                          │            │ VM fl-b                          │
 │  fio ─▶ /dev/ublkb0              │            │                                  │
 │          │                       │   1 GbE    │                                  │
 │   flaminio-io: volume target ────┼── TCP ─────┼─▶ flaminio-io: replica           │
 │          │ local replica         │  ~0.4 ms   │          │                       │
 │   replica store ▶ /dev/vdb       │            │   replica store ▶ /dev/vdb       │
 └──────────────────────────────────┘            └──────────────────────────────────┘
```

**Frontend decision:** a userspace process using **ublk** + `io_uring`, not a kernel module. It is
the architecture's choice (§5.1), and on the test servers' kernel 7.0 it gives near-kernel
efficiency through batching. It also keeps a data-plane crash from becoming a kernel panic, and
lets the process restart or upgrade without detaching the disk (`UBLK_F_USER_RECOVERY`).

Because there is no control plane, placement, epochs and membership come from a **static config
file** for the MVP. Every piece built here is reused later. Only the config file is replaced.

## Steps

| # | Step | Runs on | Done when |
|---|---|---|---|
| 1 ✅ | **Replica store**: crate `flaminio-store`. Opens and recovers a formatted disk, creates and deletes chunks, writes (redirect-on-write and deferred), reads with checksum verification, snapshots, discard, fencing, checkpoints | Any machine | Crash-consistency property tests pass: random workloads, power loss at every point (including mid-checkpoint and mid-recovery), every acknowledged write survives |
| 2 | **Linux I/O backend**: `io_uring` with `O_DIRECT`, aligned buffers, group commit (many writes per journal batch). Bench tool driving the store directly | Linux VM | Store benchmark on a VM virtual disk, compared against raw-disk fio |
| 3 ✅ | **Test VMs**: one libvirt/KVM VM per server (Ubuntu 26.04 guest, kernel 7.0), a second virtual disk `/dev/vdb` for Flaminio, and VM-to-VM networking across the two servers. See [lab.md](lab.md) | Both servers | VMs reach each other; fio baseline numbers for the raw `/dev/vdb` |
| 4 | **ublk frontend, single node**: `flaminio-io` exposes `/dev/ublkb0`, backed by a chunked volume on the local replica store (RF=1) | VM fl-a | `mkfs.ext4` + mount + fio work; data survives a `flaminio-io` restart |
| 5 | **Replication**: volume target with star fan-out, a replication protocol over TCP, ISR and epoch fencing from static config, replica service on fl-b | Both VMs | RF=2 volume serves fio; killing the remote replica degrades the volume without errors; restarting it resyncs |
| 6 | **Microbenchmark** (raw-disk baseline already recorded: [benchmarks/2026-10-08-raw-baseline.md](benchmarks/2026-10-08-raw-baseline.md)): a scripted fio suite (4K random read/write at QD1 and QD32, 1M sequential) for raw disk, RF=1 and RF=2, with results recorded in `docs/benchmarks/` | Both VMs | Results committed with hardware and configuration details |

## Constraints of the test environment

- **The 1 GbE link** between the servers caps replicated write throughput at about 110 MB/s.
  Latency (about 0.44 ms round trip) is what dominates small-write numbers. Results must be read
  with that in mind.
- **Disk speed:** rhythm's VM disk will sit on SATA SSD (Micron 5400) or HDD. rhythm-backup has
  NVMe. The replicated write path is bounded by the slower side.
- **VM tooling:** decided in step 3. Both servers use libvirt/KVM (installed on rhythm by
  `make host-prep`), with identical VM definitions, so results differ only by hardware.
- **Order:** step 3 was done before step 2, because steps 2, 4 and 5 all need Linux.
