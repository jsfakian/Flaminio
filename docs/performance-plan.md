# Performance plan: reads to 90% of raw, and a write benchmark that can be trusted

Two parts:
- **Reads:** close the gap on QD32 and 1M sequential reads, measured against the raw disk.
- **Writes:** measure them at all. Until now, write numbers have been unstable, so nothing was
  judged on them. First a method that compares like with like, then the work.

Both start with the same measurement rules (below), because a percentage of raw only means
something if both sides were measured the same way.

## 1. Where reads stand (fl-b, NVMe, 2026-10-09)

| Read job | Flaminio (pod or node) | Raw | Share |
|---|---:|---:|---:|
| 4K random, QD1 | 12 681 IOPS | 13 854 | 91% |
| 4K random, QD32 | 178 000–184 000 IOPS | 266 507 | **67–69%** |
| 1M sequential, QD8 | 3 180–3 820 MiB/s | 6 277 | **51–61%** |

**Profile** (`perf -e cpu-clock`, system-wide, during each job). In both jobs one thread does
all the work: the ublk queue thread, at 85–88% of a CPU. The VM has 4 vCPUs.

- **1M sequential.** Per byte, the queue thread spends:
  - **about 44% on CRC32C verification** of every 4 KiB read (`crc32c` with SSE4.2,
    3 streams: about 8.5 GB/s per core here);
  - **about 35% in the kernel, copying** from our buffer into the request's pages
    (`_copy_from_iter`, page pinning), because ublk is not zero-copy yet;
  - about 20% on everything else.

  Checksums and the copy alone limit one thread to about 4 GB/s. **6 GB/s needs at least two
  threads, or less work per byte.**
- **4K random, QD32.** About **4.8 µs of CPU per read** on the queue thread, spread thin. No
  symbol takes more than 1.5%:
  - the async executor: a task and a channel per ublk tag, wakeups, queues;
  - allocations (`malloc` and `free`) and B-tree maps in the engine (ops, sub-ops, reads);
  - the target state machine's pump for every read;
  - interrupt and virtio exits.

  The MVP's path, without the replication engine, did about 3.9 µs (219k IOPS, 86%). **90% of
  raw needs at most 3.5 µs on one thread, or several threads.**

## 2. Measurement rules (reads and writes)

Every benchmark run, Flaminio's or the raw disk's, follows the same steps:

1. **Same state of the device.**
   - For reads: the measured range was written once, completely, before the run. A
     never-written block reads as zeros without touching flash (the store skips it, and the
     drive returns zeros for unmapped blocks), which inflates numbers.
   - For writes: the empty state of section 4.
2. **Same span.** The raw jobs cover a range of the same size as the Flaminio volume (16 GiB),
   not the whole 100 GiB disk. A drive's mapping-table cache makes larger spans slower, and the
   two sides must pay the same.
3. **Caches cleared before every job**, in this order:
   1. `sync`;
   2. `echo 3 > /proc/sys/vm/drop_caches` in the guest;
   3. the same on the host (the data disks use `cache=none`, so this is a safeguard).

   Flaminio's in-memory chunk maps stay warm, as they would in production: they are metadata,
   not cached data. Flaminio keeps no data cache.
4. **Same CPU budget.** Both sides run in the same VM with the same vCPUs. The VM's vCPU count
   is recorded, and is the same for all runs compared.
5. **Interleaved repetitions:** at least 3, in the order raw, Flaminio, raw, Flaminio… The
   tables give the median and the spread. A job whose spread exceeds 10% is reported as
   unstable, not as a result.
6. **Recorded with every result:**
   - CPU per thread (`top -H`) and a `perf` profile of the Flaminio side;
   - the drive's temperature before and after (`smartctl`/`nvme smart-log` on the host). A run
     with thermal throttling is discarded.
7. **Results** are kept in `docs/benchmarks/data/`, as fio JSON, with the script that produced
   them.

**Step R0** (and W2 below) puts these rules into the lab's benchmark scripts. `make bench-pod`
and `make bench-raw` do not yet clear caches, match spans or repeat.

## 3. Reads: the plan

Goal: **4K QD32 and 1M sequential at 90% of raw or more** on fl-b, with QD1 kept at 90% or
more. Each step ends with the same benchmark, and the numbers go into this document.

| Step | What | Expected effect | Gate |
|---|---|---|---|
| R0 | **Harness:** the rules of section 2 in `bench-pod`/`bench-raw`; `perf` and `top -H` captured with every run | Trustworthy baselines | 3 interleaved runs with spread ≤ 10% |
| R1 | **Ceilings, by experiment** (debug switches, never shipped on): (a) verification off; (b) a local-read path that skips the engine, as in the MVP; (c) fio with 2 jobs on the raw disk | Shows how much each cost explains, before any rework | Numbers for each |
| R2 | **A lean per-I/O path on one queue:** one completion loop per queue instead of an async task per tag (no channels, no wakeups); pre-allocated slots instead of maps keyed by op id; no allocations per I/O; all completions handled, then one submission (fewer `io_uring_enter` calls and virtio exits) | QD32: from 4.8 µs toward 3 µs per read | 4K QD32 ≥ 85% of raw |
| R3 | **Local reads admitted without the state machine:** each chunk publishes, to the queue threads, its read lease's expiry, its phase, whether the local replica may serve, and its write locks. A read that passes these checks goes straight to the store. Anything else goes through the engine as today. This changes who decides, so it goes into the simulator first: the admission check modelled exactly, plus a mutation (admitting against a stale lease) that must be caught | Removes the target's pump from the hot path | Simulator passes and catches the mutation; then QD32 ≥ 90% |
| R4 | **Several ublk queues:** one queue thread per queue, each with its own ring and buffers, and reads admitted as in R3. The engine stays one thread, for writes and control. Item 5 of [follow-ups](followups.md) (several volumes per node) builds on this | 1M: checksums and copies spread over 2–3 cores | 1M ≥ 90% of raw; QD32 ≥ 90% |
| R5 | **Checksum cost:** keep verification on every read (it is how bit rot and misdirected writes are caught), but make it cheaper: verify the whole extent of an I/O in one call, with the widest CRC32C kernel the CPU has: `vpclmulqdq` folding, 256-bit with AVX2 on fl-b's Xeon E-2436 and 512-bit with AVX-512 on fl-a's Xeon 6527P, against today's 3-stream SSE4.2, chosen at run time | 1M: verification from about 44% of a core toward 20–25% | Measured with R1(a)'s switch |
| R6 | **Zero-copy reads** only if the integrity check stays. With ublk's auto buffer registration the data lands in the request's pages without passing through user space, so it cannot be verified there. Options: verify with a mapping of the request's pages (`UBLK_F_USER_COPY` reads them back: a copy again), or verify in the kernel (none exists). **This is a decision, not an optimisation**, made with R1's numbers: verification is part of Flaminio's integrity promise | Up to the 35% copy share, if a way keeps verification | Decision recorded in ARCHITECTURE |

**VM size.** fl-b has 4 vCPUs on a 12-core host. fio itself uses most of one at 266k IOPS.
Several queues need cores to run on, so R4 is measured both:
- at 4 vCPUs, as today;
- at 8 vCPUs, with the raw baseline remeasured at 8 too.

The 4-vCPU numbers stay the headline: a storage node should not need a large VM to read at
disk speed.

**Order:** R0 → R1 → R2 → R3 → R4 → R5. R6 waits for R1's numbers and a decision.

## 4. Writes: the plan

### Why write numbers have been unstable

An SSD's write speed depends on what happened to it before. Several layers matter in the lab,
and none of them is controlled today:

| Layer | Effect on writes |
|---|---|
| The drive's FTL | A drive with free, erased blocks (fresh, or just trimmed) writes fast. Once it must collect garbage to find free blocks, it slows down, sometimes by several times. Writes first land in an SLC cache that is fast until it fills. |
| The drive's DRAM and SLC caches | Short runs measure the caches, not the flash |
| The host's file layer | Each VM's data disk is a preallocated file on ext4. The first write to a preallocated range converts unwritten extents (journalled metadata), later writes do not. On fl-b's host the NVMe also holds the host's OS. fl-a, fl-c and fl-w share the SATA SSDs. |
| The guest | Page cache (bypassed with `O_DIRECT`), and the virtio disk's write cache (`cache=none` exposes a volatile write cache to the guest) |
| Flaminio's store | Its journal, checkpoints, and allocation state: a fresh store and an aged, fragmented one differ |
| Replication | The 1 GbE link (about 117 MB/s) bounds every RF 2 write in the lab, whatever the disks do |

### W0: a disk that can be reset (needs your decision)

To compare from an empty state, the reset must reach the flash. Through a file on ext4 it
does not. Options, best first:

1. **A dedicated device per data VM**, given to the VM whole (virtio-blk on the block device,
   or PCI passthrough of an NVMe). The guest's `blkdiscard` (or `nvme format`) then reaches the
   drive. It needs a spare disk, or space on one:
   - rhythm's volume group has no free space;
   - rhythm-backup's NVMe also holds its OS.
2. **A dedicated partition or LV** on the existing drives, if space can be freed on the hosts.
   That means shrinking a filesystem: offline, on the hosts' consoles.
3. **Keep file-backed disks, and reset every layer:**
   1. the guest discards the whole disk;
   2. QEMU (with `discard=unmap`) punches holes in the file;
   3. the host trims its filesystem;
   4. the file is preallocated again;
   5. then a fixed idle wait.

   Workable, but the host filesystem stays in the path, and the result is less clean. On fl-b's
   host the OS shares the drive.

Recommendation: **option 1 on rhythm-backup** (an NVMe added for the benchmark VM, or
passthrough of a spare one), and option 3 on rhythm until then. Every write result states which
option it was measured with.

### W1: the empty state, before every write run

The same procedure runs before every write job, for Flaminio and for the raw disk alike:

1. **Stop all I/O on the node:** stop its data plane, detach volumes, and check that nothing
   holds the disk.
2. **Discard the whole device**: `blkdiscard` (option 1 or 2), or the full reset of option 3.
   For NVMe, `nvme format --ses=0` where the drive supports it is equivalent.
3. **Idle for a fixed time** (5 minutes, to be confirmed by measurement): the drive finishes
   background garbage collection.
4. **Clear caches** (section 2, step 3) in the guest and on the host.
5. **Flaminio only:**
   - format the store (`flaminio-disk format`);
   - start the node;
   - create a fresh volume of the test size.

   For the raw disk there is nothing else to do.
6. **Check the state:**
   - the drive's temperature is in range;
   - for NVMe, `nvme smart-log` shows no ongoing operation;
   - its *Data Units Written* counter is recorded, to compute write amplification after.

### W2: what is measured

- **Two regimes, both reported:**
  - **Fresh** (right after W1). It shows the SLC cache and the best case. It is not a
    sustained number.
  - **Steady state** (SNIA PTS-style).
    1. Precondition the test span: sequential 128 KiB writes, twice over.
    2. Run the workload in 1-minute rounds until 5 rounds in a row stay within ±10% of their
       mean, with no trend.
    3. Report the steady-state window.

  Results are judged on steady state.
- **Same span, same free space:** 16 GiB on both sides, on a freshly discarded 100 GiB disk.
  The drive then has the same free space to collect garbage into, for Flaminio and for raw.
- **Same durability.** Flaminio acknowledges a write only once it is durable on every ISR
  member. The raw comparison is therefore `O_DIRECT` with `O_DSYNC` (the `-sync` jobs of
  `make bench-raw`). Plain `O_DIRECT` without sync is not durable behind the virtio write
  cache. It is reported only as a ceiling.
- **Jobs:**
  - 4K random write at QD1 (latency) and QD32;
  - 1M sequential write at QD8;
  - 70/30 random read/write at 4K QD32.

  Each runs long enough to write at least 3× the SLC cache: the size of that cache is measured
  first, as the point where a fresh sequential write slows down.
- **Write amplification**, two levels:
  - Flaminio's own: bytes the guest sent to the data disk (`/sys/block/vdb/stat`) per byte the
    client wrote. It covers journal, metadata and checksums.
  - The drive's: NAND or host writes from SMART, per byte the guest sent.
- **Repetitions:** 3, interleaved, each preceded by W1, as in section 2.

### W3: comparisons

| Comparison | What it isolates | Expected bound |
|---|---|---|
| **A. RF 1, local** (volume on the node's own disk, no replication) **against raw `O_DSYNC`** | The store's overhead: journal, group commit, checksums, metadata | ≥ 80% of raw sync at steady state, to be confirmed |
| **B. RF 2 against A, with the network measured** (`iperf3` and RTT in the same run) | Replication's overhead | Bandwidth: the link (about 117 MB/s in the lab). Latency: A plus one round trip |
| **C. RF 2 against raw `O_DSYNC` on fl-b with fl-a's disk** | Only as context | The lab's 1 GbE dominates: not a product number |

A and B are the numbers to judge. C needs a faster interconnect to mean anything (25 GbE or
more, phase 6).

### W4: write work, after the baseline

Only with trustworthy numbers from W1–W3, in order of the likely gain:
1. **Group commit:**
   - tune the batch size and wait;
   - measure the journal's share of device writes (W2's write amplification).
2. **Zero-copy writes** (performance item 1.3): the data goes from the request to the store
   and the network without a copy into a `Vec`.
3. **Several queues for writes** (R4 brings them for reads).
4. **Write amplification:** journal writes for large sequential writes. Large aligned writes
   could bypass the journal (written in place, then one metadata update).

## 5. Order, and what is needed

1. **R0 and W2's harness** together: the same scripts serve both.
2. **R1:** a few hours, and it settles R6's question with numbers.
3. **W0: your decision** on the device layer, then W1.
4. **R2 → R3** (simulator first) **→ R4 → R5.**
5. **W3 baselines**, then W4.

**Decision points for you:**
- W0's device option.
- R6: zero-copy against verifying every read.
- Whether R4 may be judged at 8 vCPUs, or only at 4.
