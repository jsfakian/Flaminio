# MVP microbenchmark (2026-10-09, MVP step 6)

The MVP's end-to-end result: the Flaminio data plane, serving a **replicated (RF=2) volume**
across the two test servers, measured with fio on the block device and compared with the raw
disk. Reads only (see the [reads-only rule](2026-10-08-store-baseline.md)). Replication is
judged on correctness ([replication-mvp.md](../replication-mvp.md)).

```
 fio ─▶ /dev/ublkb0 ─▶ flaminio-io (fl-a) ─┬─▶ store ─▶ /dev/vdb (fl-a, SATA SSD mirror)
                                          └─▶ TCP over 1 GbE ─▶ flaminio-io replica (fl-b) ─▶ /dev/vdb (NVMe)
```

## Reads on fl-a, every layer compared with the raw disk

Each row is 3 repetitions, 30 s per job, fio `io_uring` + `direct=1` with a continuous queue,
interleaved with raw-disk runs in the same session. "±" is half the min–max range.

| Read job | Raw `/dev/vdb` | Store only (step 2, closed batches) | ublk, RF=1 (step 4) | **ublk, RF=2 (this run)** |
|---|---:|---:|---:|---:|
| 4K random, QD1 | 6 424 IOPS ±2%, p50 ~150 µs | 100% | 97% | **6 237 ±4% (97%)**, p50 153 µs |
| 4K random, QD32 | 122 804 ±0% | 59% (closed batches) | 93% | **114 482 ±1% (93%)**, p99 435 µs |
| 1M sequential, QD8 | 909 MiB/s ±0% | 53% (closed batches) | 98% | **878 ±1% (97%)** |

Each ratio is against the raw disk measured in the same session. The raw column shows this
session's raw runs.

**Reading the table:**
- **Replication costs reads nothing.** Reads are served by the local replica, so RF=2 and RF=1
  are the same within noise.
- **The full stack costs 3–7%:** ublk, the volume, the store with checksums, `io_uring`.
- **The step-2 store numbers are lower only because of closed batches.** That is a property of
  the benchmark, not of the store, as the step-2 write-up explains.

On fl-b (NVMe, measured RF=1 in step 4) the stack reaches 93% at QD1 and 86% at QD32. 1M
sequential reads cap at about 3.4 GB/s (62%), most likely the single ublk queue thread (not yet
measured).

## Replication (correctness)

| Test | Result |
|---|---|
| Replica stopped for 15 s during 4 GiB of fio-verified writes | No write failed, all data verified, resynced (`InSync` 291 s after the load) |
| Disk comparison afterwards | Identical per-chunk digests on fl-a and fl-b |
| Restart of the target, then comparison | Nothing to copy; `InSync` after the 64 GiB comparison (284 s) |
| Replicated write throughput (fill, indicative only) | 83 MiB/s, bounded by the 1 GbE link (about 110 MiB/s) |

Raw JSON: [`data/2026-10-09-rf2/`](data/2026-10-09-rf2/). Earlier layers:
[raw](2026-10-08-raw-baseline.md), [store](2026-10-08-store-baseline.md),
[ublk RF=1](2026-10-08-ublk-reads.md).
