# Reads with zero copy, against a 4-vCPU baseline (2026-10-09)

`make bench-reads` on fl-b (NVMe, 4 vCPUs), by the rules of the
[performance plan](../performance-plan.md):
- `vol1` (16 GiB, RF 2, a replica of every chunk on fl-b), written once in full by fio with a
  checksum in every 1 MiB block;
- raw jobs on 16 GiB of the same disk;
- caches dropped (guest and host) before every job;
- 3 interleaved repetitions (raw, Flaminio, raw…), 30 s per job after 3 s of ramp-up, fio
  `io_uring`, `direct=1`.

Two runs of the same jobs. Flaminio's node runs in one mode or the other:
- **Verify:** today's path. The disk is read into a buffer of ours, every 4 KiB is checked
  against its CRC32C, and the kernel copies the buffer into the request.
- **Zero copy** (`--zero-copy`): the disk is read straight into the request's pages, which the
  kernel registered with the ring (`UBLK_F_AUTO_BUF_REG`). No copy, and so no checksum check on
  that path.

## Results

Medians of 3. Every job's spread is at most 4.2%.

| Read job | Verify | Zero copy | Raw | Verify / raw | **Zero copy / raw** |
|---|---:|---:|---:|---:|---:|
| 4K random, QD1 | 12 685 IOPS, p99 87 µs | 12 843 IOPS, p99 86 µs | 15 419–15 468 | 82% | **83%** |
| 4K random, QD32 | 177 671 IOPS, p99 272 µs | 213 047 IOPS, p99 230 µs | 239 277–242 011 | 74% | **88%** |
| 1M sequential, QD8 | 3 276 MiB/s | 6 623 MiB/s | 6 336–6 397 (first 16 GiB) | 51% | **≈ 100%**, see below |

**The ublk queue thread's CPU:**

| Read job | Verify | Zero copy |
|---|---:|---:|
| 4K random, QD1 | 13% | 12% |
| 4K random, QD32 | 86% | 85% |
| 1M sequential, QD8 | 87% | **15%** |

**Integrity.** After each run, fio read the whole volume and checked the checksum it had
written into every block:
- verify mode: 16 GiB, 0 errors, and **0 Flaminio checksum failures** in the node's log;
- zero copy: 16 GiB, **0 errors**.

**1M against raw.** Zero copy's 6 623 MiB/s is above raw's first 16 GiB (6 336–6 397), so the
raw side was measured again, 1M at QD16 in 16 GiB spans at different offsets:

| Offset | 0 | 16 GiB | 32 GiB | 48 GiB | 64 GiB | 80 GiB |
|---|---:|---:|---:|---:|---:|---:|
| MiB/s | 6 435 | 6 596 | 6 665 | 6 666 | 6 670 | 6 570 |

- The disk reads its first 16 GiB more slowly than the rest. The store's data is spread over
  the whole data area, so Flaminio reads at the disk's speed: about 100%.
- A deeper raw queue does not change it: at offset 0, QD8 gives 6 366, QD16 6 446, QD32 6 448.
- From now on, `bench-reads` reads raw at offsets 0, 32 and 64 GiB in turn, one per
  repetition.

## What it shows

- **The copy was the bottleneck for large reads.** With it and the checksums gone, 1M reaches
  the disk's speed using 15% of a core, instead of half the disk's speed at 87%.
- **QD32 gains 20%** (74% → 88%), but the queue thread is still at 85%: the per-I/O
  bookkeeping is now the limit, and the next step is R2 (the lean per-I/O path).
- **QD1 is unchanged** (83%). At 75 µs against raw's 61 µs, about 14 µs per read go to
  the ublk round trip and the engine: also R2.
- **Integrity is unharmed.** fio's end-to-end check finds no error on either path. Zero copy
  gives up only Flaminio's own checksum on reads, which is measured in the separate verify run.
