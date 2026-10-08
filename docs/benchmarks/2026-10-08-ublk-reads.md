# ublk frontend: read baseline (2026-10-08, MVP step 4)

fio on `/dev/ublkb0`: the full data plane (ublk → `flaminio-io` → volume → replica store →
`io_uring`/`O_DIRECT` → `/dev/vdb`) against fio on the raw `/dev/vdb`. Reads only (see
[the reads-only rule](2026-10-08-store-baseline.md)). VMs and hardware as in
[the raw baseline](2026-10-08-raw-baseline.md).

## Method

- `flaminio-io serve` exposes a 64 GiB volume (64 × 1 GiB chunks), one ublk queue of depth 128.
- Before **every** run, the first 32 GiB of the volume is filled with 1 MiB writes (setup, not
  measured). Reads stay inside those 32 GiB. Filling every time matters: `mkfs.ext4` discards
  the whole device, and reads of discarded ranges return zeros without touching the disk. One
  early run measured exactly that: 47k IOPS on a SATA disk that does 6.4k.
- 3 repetitions, interleaved: ublk a, raw a, ublk b, raw b. 30 s per job after 3 s of ramp-up,
  fio `io_uring`, `direct=1`, continuous queue (`iodepth`). The JSON for every run and the
  `summarize.py` output are in [`data/2026-10-08-ublk/`](data/2026-10-08-ublk/).

## Results

| Read job | fl-a ublk | fl-a raw | ublk/raw | fl-b ublk | fl-b raw | ublk/raw |
|---|---:|---:|---:|---:|---:|---:|
| 4K random, QD1 | 6 181 IOPS ±4%, p50 155 µs | 6 361 ±0% | **97%** | 12 566 ±3%, p50 74 µs | 13 455 ±3% | **93%** |
| 4K random, QD32 | 113 861 ±0%, p99 570 µs | 122 687 ±0% | **93%** | 219 153 ±4%, p99 222 µs | 254 524 ±2% | **86%** |
| 1M sequential, QD8 | 874 MiB/s ±0% | 892 ±1% | **98%** | 3 418 ±0% | 5 509 ±13% | **62%** |

## What it shows

- **At QD1 the whole stack costs about 5–10 µs per read:** ublk round trip, plan, checksum
  verification. That is 97% of raw on SATA and 93% on NVMe.
- **The queue stays full.** In the first frontend version, reads went through the batching
  driver and the event loop waited for each batch: 40% of raw at QD32 and 55% at 1M on fl-a.
  With reads planned by the store and queued straight on the ublk `io_uring`, the same jobs
  reach 93% and 98%.
- **fl-b's sequential reads cap at about 3.4 GB/s** (62% of the NVMe's 5.5 GB/s): the single
  ublk queue thread is near its CPU limit. Measured on 2026-10-09 with `top -H` during 1M QD8
  reads at 3.2 GB/s: the queue thread ran at 80–87% of one CPU, so it is close to saturated but
  still partly waiting on I/O. It does all the checksum verification, and the kernel copies
  every byte between the ublk buffers and the requests (no zero-copy yet). Next: zero-copy
  (`UBLK_F_AUTO_BUF_REG`) and multiple queues (one store shard per queue).
