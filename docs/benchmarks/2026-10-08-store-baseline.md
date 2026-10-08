# Replica-store baseline (2026-10-08, MVP step 2)

The replica store driven directly by `flaminio-bench` on each VM's data disk (`io_uring`,
`O_DIRECT`), with no frontend and no network, compared with the raw disk. Hardware and VM
setup are as in [the raw-disk baseline](2026-10-08-raw-baseline.md).

**This comparison is about reads.** Write performance on SSDs and NVMe is not stable enough to
draw conclusions from. It depends on what the drive did before (internal garbage collection
after large writes), and our fl-b write results moved by about 40% between runs minutes apart.
Write numbers are kept in the appendix as indicative only.

## Method

- Each disk holds a store with a 32 GiB working set (32 chunks × 1 GiB), filled once
  sequentially. `make bench-store FORMAT=0` then runs the read jobs on it, and `make bench-raw`
  runs fio on the raw disk. Both ran **3 times, interleaved** (store a, raw a, store b, raw b).
  Each job runs 30 s after 3 s of ramp-up.
- **Batches.** `flaminio-bench` submits `batch` requests together and waits for all of them.
  fio's `iodepth` keeps that many in flight continuously. For a like-for-like comparison, fio
  also ran in the same closed-batch mode (`iodepth_batch_submit` = `iodepth_batch_complete_min`
  = depth). Both comparisons are shown.
- The JSON for every run, and the output of `scripts/tools/summarize.py`, are in
  [`data/2026-10-08-reads/`](data/2026-10-08-reads/). "±" is half the min–max range across the
  three runs, as a percentage of the mean.

## Results

| Read job | fl-a store | fl-a raw | store/raw | fl-b store | fl-b raw | store/raw |
|---|---:|---:|---:|---:|---:|---:|
| 4K random, depth 1 | 6 392 IOPS ±2%, p50 155 µs | 6 411 ±2% | **100%** | 10 856 ±1%, p50 88 µs | 12 820 ±0% | **85%** |
| 4K random, 32 closed batches | 67 653 ±0% | 45 426 ±1% | **149%** | 117 965 ±0% | 115 224 ±0% | **102%** |
| 1M sequential, 8 closed batches | 452 MiB/s ±0% | 469 ±0% | **96%** | 3 326 ±2% | 3 602 ±2% | **92%** |
| 4K random, 32 (raw: fio continuous) | 67 653 | 114 844 ±1% | 59% | 117 965 | 245 308 ±0% | 48% |
| 1M sequential, 8 (raw: fio continuous) | 452 | 853 ±1% | 53% | 3 326 | 4 983 ±0% | 67% |

## What the numbers say

1. **The store costs little per read.**
   - Run the same way as fio, it reaches 92–149% of the raw disk.
   - At depth 1 it adds roughly 5–10 µs: lookup, checksum and submission. That is invisible
     on fl-a's SATA mirror (155 µs either way) and about 15% on fl-b's NVMe (88 µs against
     about 77 µs).
   - In closed batches it beats fio's batch mode on fl-a. One plausible reason is that its
     reads are submitted with a single `io_uring_enter`.
2. **The remaining gap is the closed batches,** not the store. Against fio's continuous queue
   the store reaches 48–67%, because every batch waits for its slowest read and the queue
   drains between batches. A frontend that keeps the queue full is what closes it. The ublk
   frontend (step 4) does that, and it is the next measurement.
3. **Compare within one campaign only.** An earlier single run on fl-b put the store at 107% of
   raw at depth 1. In this campaign the raw disk itself was 6% slower and the store 25% slower.
   The difference between campaigns is device state after heavy writes in between. Within the
   campaign, the three runs agree to within 2%.

CRC-32C runs at about 10.4 GB/s per 4 KiB block on fl-a (`cargo run --release -p
flaminio-format --example crc_speed`), about a third of the CPU per 1 MiB read. The rest is
mostly kernel time for direct I/O. Checksums are not the bottleneck.

## What step 2 changed

- **Group commit:** many writes share one journal batch and one flush. A unit test checks
  that 32 writes cost 1 flush, not 32.
- **One flush per group instead of two:** data and journal are written together, and recovery
  verifies the last batch's data against the checksums it carries. This is backed by the crash
  tests and two targeted mutation checks.
- **Contiguous reads merged into one device read:** no measurable gain on these disks.
- **Checkpoint pages written as one batch.**

## Appendix: write numbers (indicative only, not for conclusions)

Single runs, from before the read campaign. They are listed for completeness, not as evidence.

| Job | fl-a store | fl-a raw durable (`O_SYNC`) | fl-b store | fl-b raw durable (`O_SYNC`) |
|---|---:|---:|---:|---:|
| 4K random write, 1 | 3 638 IOPS | 6 046 | 457 | 313 |
| 4K random write, 32 | 19 475 | 43 129 | 12 241 | 12 156 |
| 1M sequential write, 8 | 399 MiB/s | 451 | 864 | 1 840 |

Raw JSON: [`data/2026-10-08-store/`](data/2026-10-08-store/) and
[`data/2026-10-08-raw-sync/`](data/2026-10-08-raw-sync/). A comparison of journal sizes was
started and then stopped: it can only be judged on write numbers.
