# Raw data-disk baseline (2026-10-08)

This is the reference that every Flaminio number in the MVP is compared against: fio straight on
each VM's data disk (`/dev/vdb`), with no Flaminio in the path. Measured with
`make bench-raw RUNTIME=30`. The raw fio JSON is in [`data/2026-10-08-raw/`](data/2026-10-08-raw/).

## Setup

| | fl-a (rhythm) | fl-b (rhythm-backup) |
|---|---|---|
| Host CPU | Xeon 6527P, 48 threads | Xeon E-2436, 12 threads |
| Host storage under `/dev/vdb` | ext4 → LVM → md RAID1 of 2× Micron 5400 SATA SSD | ext4 → Samsung PM9A1 NVMe (consumer, no power-loss protection) |
| VM | 8 vCPU, 16 GiB | 4 vCPU, 8 GiB |
| Guest | Ubuntu 26.04, kernel 7.0.0-34 | same |
| Data disk | 100 GiB raw file, preallocated, virtio-blk, `cache=none`, `io=native` | same |

fio: `ioengine=io_uring`, `direct=1`, 30 s per job after 3 s of ramp-up, 1 job. Each disk was
**preconditioned** with one full sequential write first (fl-a: 468 MiB/s, fl-b: 2822 MiB/s).
Without that, never-written blocks of the preallocated file read as zeros without touching the
device, which inflates read results.

## Results

| Job | fl-a IOPS | fl-a MiB/s | fl-a p50 / p99 µs | fl-b IOPS | fl-b MiB/s | fl-b p50 / p99 µs |
|---|---:|---:|---:|---:|---:|---:|
| 4K random read, QD1 | 6 673 | 26 | 148 / 171 | 13 636 | 53 | 63 / 97 |
| 4K random write, QD1 | 13 131 | 51 | 70 / 93 | 32 430 | 127 | 28 / 34 |
| 4K random read, QD32 | 116 236 | 454 | 242 / 749 | 261 343 | 1 021 | 115 / 232 |
| 4K random write, QD32 | 51 758 | 202 | 602 / 1 122 | 275 947 | 1 078 | 113 / 173 |
| 1M sequential read, QD8 | 884 | 885 | 9 109 / 14 876 | 3 323 | 3 324 | 2 375 / 2 998 |
| 1M sequential write, QD8 | 476 | 477 | 16 187 / 20 840 | 1 875 | 1 875 | 4 014 / 6 455 |

## Reading these numbers

- **fl-b is 2–5× faster than fl-a everywhere**, as expected from NVMe vs. SATA. In an RF=2
  volume, each write waits for both replicas, so **fl-a's disk will bound replicated write
  performance**, together with the network.
- **fl-a reads sequentially at about twice its write speed**: RAID1 serves reads from both
  mirrors, but every write goes to both disks.
- **Writes at QD1 are faster than reads on both machines.** Writes complete in the drive's write
  cache, while reads must reach the flash. That flatters write latency:
  - The Micron 5400 has power-loss protection, so its cache is effectively durable.
  - The PM9A1 does not. Its 28 µs writes are not durable until a cache flush.
  - Flaminio acknowledges a write only when it is durable (it issues flushes, see the format
    spec §6.2). So this baseline **overstates what a durable write costs on fl-b**.
  - The durable-write (`O_SYNC`) baseline was added in step 2. See
    [2026-10-08-store-baseline.md](2026-10-08-store-baseline.md). On fl-b a durable 4K write
    costs about 3 ms.
- **The network is not measured here.** The servers share a 1 GbE link (about 117 MB/s and
  0.44 ms round trip between hosts). Once replication arrives, that is well below either disk's
  sequential write speed, and its round trip exceeds both disks' QD1 write latency. An `iperf3`
  baseline between the VMs belongs in step 5.
