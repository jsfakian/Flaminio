# Reads, QD1: the raw reference, one queue per vCPU, and the round trip (2026-10-29)

[Issue #9](https://github.com/jsfakian/Flaminio/issues/9), design in [docs/9](../9/design.md).
fl-b, 4 vCPUs, zero copy, volume volb (16 GiB, replicas fl-b+fl-a and fl-b+fl-c), one fio job.

**Verdict.**
- **The bench's raw reference had stopped meaning anything.** It now reads where the volume's
  data is.
- **Against that reference, the defaults give 85% at QD1.**
- **One queue per vCPU gives 88%.** It costs a single fio job 11–12% at QD32 (98% → 86%), so it
  stays a setting, not the default.
- **Only with 100 µs polling as well does QD1 reach 90% (90.2%),** for a core.
- **Without spinning, the goal is not met.** Issue #9 stays open.

## The raw reference

`bench-reads` used to read raw in a different 16 GiB region per repetition: at 0, 32 and
64 GiB. On 2026-10-29 the regions at 32 and 64 GiB took 37% longer per 4K read than the one at 0
(85 µs against 62). The last run with the old reference:

| Rep | Raw region | Raw | Flaminio |
|---:|---|---:|---:|
| 1 | 0 GiB | 15 388 | 13 011 |
| 2 | 32 GiB | 11 310 | 13 373 |
| 3 | 64 GiB | 11 363 | 13 055 |

Raw's median, 11 363, made Flaminio 115% of raw, flagged unstable (spread 35.9%).

volb's data is at 0–21 GiB. A traced read of all of it shows where it is (MiB per 1 GiB region):
- **0:** 158;
- **1–3:** 1 024 each;
- **4:** 865;
- **5–7:** none;
- **8:** 290;
- **9–19:** 1 024 each;
- **20:** 734.

The hole at 4.8–8.7 GiB holds other data of the store.

Now the bench reads raw over a span of the volume's size (rule 2) from the start of that range,
0–16 GiB. Over the whole range, 0–21 GiB, raw reads the same at QD1: 15.44k against 15.35k
IOPS.

## The bench

`VOL=volb make bench-reads`, 3 interleaved repetitions of 30 s, raw at GiB 0–16. Every run
passed fio's check of all 16 GiB.

| Setting | 4K random, QD1 | 4K random, QD32 | 1M sequential, QD8 | Queue thread CPU at QD1 |
|---|---:|---:|---:|---:|
| Defaults (1 queue) | 13 097 IOPS, **85%** | 230 100, **98%** | 6 636 MiB/s, **100%** | 9–11% |
| 4 queues | 13 602, **88%** | 203 266, **86%** | 6 632, **100%** | 5–9% |
| 1 queue, 100 µs polling | 13 677, **88%** | | | 97–100% |
| 4 queues, 100 µs polling | 13 904, **90%** | | | 97% |

Raw's medians were:
- **QD1:** 15 414–15 467 IOPS;
- **QD32:** 235 786 and 237 556;
- **1M:** 6 603–6 607 MiB/s.

Spreads were 1.2% or less.

## Where the time goes, at QD1

Hand measurements, with what produced them in the [data](data/2026-10-29-reads-qd1-queues/).

**Phases of one read,** from block tracepoints (`qd1-phases.bt`), the ublk device's scheduler
set to `none` by hand in both rows:

| Queues | Request to disk | Disk | Disk to completion |
|---:|---:|---:|---:|
| 1 | 4.2–5.1 µs | 65.5–65.7 µs | 3.3 µs |
| 4 | 3.2–3.3 µs | 65.7 µs | 3.4–3.5 µs |

**One queue per vCPU.**
- **What it changes:** the kernel maps each CPU to its own queue, and libublk pins each queue's
  thread to it. A read and both its completions then stay on the submitting CPU. fl-b's virtio
  disk has 4 queues, one per CPU, so its interrupt does too.
- **What it saves:** of the 3.1–3.5 µs per read, about 1.2 µs is the I/O scheduler, which the
  kernel gives a one-queue device (`mq-deadline`) and not a several-queue one. `none` set by
  hand on one queue gave about 1.5% more IOPS. The rest, about 2 µs, is the queue count.
- **What it costs:** one fio job's requests all go to its CPU's queue, whose thread shares that
  CPU with fio. At QD32 it loses 11–12%.

**The queue thread, per read** (`perf`, cpu-clock, DWARF call graphs, with 4 queues):
- **In the kernel, about 4.8 µs:** two sleeps and two wake-ups, two `io_uring_enter` calls (the
  minimum, one per sleep, counted with bpftrace), and the disk's virtio notification (about
  1.1 µs), which raw pays too.
- **In user space, about 2.3 µs:**
  - the store's map lookup, about 0.2 µs, mostly cache misses;
  - allocations, about 0.2 µs;
  - the async executor, about 0.35 µs;
  - a long tail.

  The CPU is otherwise idle 86% of the time.

**The disk answers Flaminio's reads 3 µs slower than raw's.**
- **The numbers:** 65 µs against 62 µs, in the same regions, both completing on the issuing
  CPU (`dev-cpu.bt`).
- **Raw with a gap:** raw fio spinning 2, 4, 6 or 8 µs between reads gets 65–66 µs too. Its
  IOPS (14.1k at 2 µs to 13.0k at 8 µs) confirm the gaps.
- **What follows:** the disk answers faster when the next read follows a completion at once.
  Every layer above it pays this.
- **The cause, untested:** fl-b's domain defines no iothread.

**Spinning.**
- **`--poll-us 100`:** takes QD1 to 88% with one queue and 90% with four, for a core.
- **Spinning only while a disk read is in flight** (a prototype, not kept:
  `prototype-poll-inflight.diff`): with 4 queues, 13.72–13.82k IOPS. The raw runs beside it
  read 15.41–15.52k (over 0–21 GiB), so that is about 89%.

## What 90% needs without spinning

In the bench, raw reads about 15.45k IOPS at QD1. 90% of that is 13.9k, 71.9 µs per read.
- **One queue** (76.4 µs) needs 4.5 µs less.
- **Four queues** (73.5 µs) need 1.6 µs less.

Next candidates:
- trimming the user-space work per read (about 1 µs: allocations, the executor, the engine's
  bookkeeping);
- the node setting `none` on a one-queue ublk device (about 1.2 µs);
- completing simple reads in the kernel (ublk with BPF), or batched commands;
- the lab disk's iothread, with both sides measured again.

## Data

[data/2026-10-29-reads-qd1-queues/](data/2026-10-29-reads-qd1-queues/):
- **The bench runs:** `zero-copy-*-runs.tsv` and `-summary.txt`, with each run's region map
  (`-regions.txt`).
- **The last run with the rotating reference:** `rotating-raw-q1-*`.
- **What produced the hand measurements:**
  - the bpftrace programs (`*.bt`);
  - their command lines (`hand-measurements.sh`);
  - the prototype's diff.
