# Writes, RF 1: 1M at 90% of raw, holding staged groups (#10, 2026-10-29 – 11-04)

[Issue #10](https://github.com/jsfakian/Flaminio/issues/10), design in
[docs/issues/10](../issues/10/design.md). Comparison A of the
[performance plan](../performance-plan.md), after W4.6:
- fl-b (NVMe, 4 vCPUs), RF 1, zero copy, one ublk queue;
- O_DSYNC on both sides;
- every run from the empty state.

Build: PR #32's branch, commit `98586cf`.

**Verdict: pass.**
- **1M sequential writes reach 89–93% of raw written across the disk** (spec: 80% or more).
  The campaign gives 935 MiB/s, up 33% from W4.2's 701.
- **The small-write jobs stay within 10% of W4.2:** 4K QD1 +5%, 4K QD32 +5%, the 70/30 mix
  −1.3%.
- **Every integrity check is clean:** 12 Flaminio runs, 36 checks, 0 errors, 0 Flaminio checksum
  failures.
- **Large writes at low depth are no worse:** 1M QD1 at 117 MiB/s against 107, 64K QD1 at 9.0
  against 8.8.
- **Holding is on by default,** with an 8 MiB floor. `--commit-target-mib 0` turns it off.

## Why 1M stopped at about 70%: the profile

1M QD8 O_DSYNC on fl-b, 2026-10-29, 60 s traced, from the empty state. Steady state is the last
30 s, after the drive's fast cache (about 30 GiB written). Block tracepoints on the data disk
and the ublk device.

| Arm | Steady MiB/s | MiB per flush | Flush, ms | Gap between flushes, ms |
|---|---:|---:|---:|---|
| Raw, GiB 16–100 | ~984 | 7.6 | 4.73 | 3.06 (0.35 MiB written during flushes) |
| Flaminio, defaults before #10 | ~686 | 4.0 | 5.5 | 0.3 (3.9 MiB of early writes during each flush) |
| `--commit-target-mib 8` | ~701 | 6.6 | 4.8 | 4.6 |
| `--flush-coalesce 2` | lower (746 over the whole run) | 2.8 | 5.2 | 0.3 |

**What it showed:**
- **Raw flushes once its data has quiesced.** The block layer defers a flush while data writes
  are in flight, so each flush covers about all 8 writes and runs with the drive quiet.
- **Flaminio flushed as soon as a commit's own writes were done,** with the next group's early
  writes landing during it: a slower flush, 4 MiB each.
- **The byte floor (W4.5) stopped short for three reasons:**
  - the returning writes waited unwritten: with nothing in flight, nothing was staged;
  - the queue waited for every copy;
  - the window ran out with 13–14 of 16 requests.
- **One cycle with the floor at 8 MiB,** in µs after the previous flush completes:
  - fio's writes complete at 160–320;
  - fio resubmits at 394–1 065 (ublk splits each 1M write into two 512 KiB requests);
  - the first disk write goes out at 2 402.
- **The queue thread,** with the first two prototypes below: memcpy about 40% (staging slot
  to aligned buffer), CRC-32C about 19%, page pinning 5–8%.

## The prototypes, step by step

The same traced run, the same day. Shares are of the raw run's ~984 MiB/s.

| Step | Steady MiB/s | MiB per flush | What it showed |
|---|---:|---:|---|
| Floor 8 + stage with nothing in flight | ~721 | 6.8 | first disk write at 2 070 µs (from 2 402) |
| + the queue does not wait for staging copies | ~746 | 6.05 | first disk write at 819 µs |
| + no second copy (unsound, RF 1 only) | ~797 (81%) | 6.37 | queue thread at 30% (from 40%) |
| `--io-buf 1M` with all of the above | ~779 | | no gain |
| + arrivals hold, with the no-copy shortcut | ~926 (94%) | 7.99 | 16 requests in 3 451 of 3 478 cycles |
| + arrivals hold, sound (no shortcut) | ~849 (86%) | 8.00 | 16.0 writes per commit; 49 of 8 684 started on timeout |

- **Before the arrivals hold,** commit sizes varied widely: 15–16 requests per flush often, but
  many 8–12. Full 8 MiB commits every cycle would give about raw's speed.
- **The arrivals hold:** the floor's window does not end while writes are being copied in, up to
  8 ms more.
- **The design started from the sound arm** (849 MiB/s,
  [`prototype.diff`](data/2026-10-29-writes-1m/prototype.diff)). The no-copy shortcut did not
  ship.

## What shipped

- **S1. Holding is on by default, with an 8 MiB floor.** After a landing, the held group's
  commit starts at 8 MiB, at as many writes as were outstanding at the landing, when its window
  passes (2 ms for large writes, 300 µs otherwise), or when a request it cannot take queues
  behind it.
- **S2. A stageable write is staged with nothing in flight,** its data written at once.
- **S3. A large zero-copy write reaches the engine once its own data is copied in,** without
  waiting for the other writes being copied in.
- **S4. A large-write wait does not end while large writes are being copied in,** up to 8 ms
  more. The halves of a 1 MiB write commit together.
- **S5. A held group is never stranded.** Paths that need the store settled start its commit
  first: both Hellos, inspect, delete, detach, and turning asynchronous commits off.
- `--commit-target-mib 0` turns holding off. `--flush-coalesce 2` requires it.

## The traced profile, after

fl-b, RF 1, zero copy, one queue, each run from the empty state. The drive was slow that day,
so read the ratios, not the absolute numbers.

| Run | Holding (#10) | Holding off (`--commit-target-mib 0`) | Raw |
|---|---:|---:|---:|
| 1M QD8, steady | ~621 MiB/s, 7.9 MiB and 15.9 writes per flush | ~590 MiB/s, 4.0 MiB and 8 writes per flush | 606 / 547 MiB/s (two runs) |
| 1M QD1 | 117 MiB/s, 2.0 writes per commit | 107 MiB/s | |
| 64K QD1 | 9.0 MiB/s | 8.8 MiB/s | |

- **1M with holding is 102–114% of raw** (spec: 80% or more).
- **Commits carry 7.9 MiB on average** (spec: 7 MiB or more).
- **1M QD1 commits both halves together:** 2.0 writes per commit (spec: 2).
- **Large writes at low depth are no worse.**

## The campaign

`make bench-writes`, RF 1, zero copy (`ZERO_COPY=1`), one queue, `VOL=wbench`, from 2026-11-03
16:54 to about 01:00.
- **Every run from the empty state** (the plan's W1), then the whole disk written once, the span
  preconditioned and left idle (W2).
- **3 interleaved repetitions,** raw then Flaminio.
- **Steady state:** 60 s rounds until the last 5 are within ±10% of their mean, and the best-fit
  line's change across them is within 10% of the mean. At most 12 rounds.

Steady state, medians of 3:

| Job | Raw | Flaminio | Share of raw | W4.2 | Change vs W4.2 |
|---|---:|---:|---:|---:|---:|
| 4K random, QD1 | 467 IOPS | 416 | **89%** | 396 | +5% |
| 4K random, QD32 | unsteady (2 of 3 reps) | 10 357 | – | 9 847 | +5% |
| 70/30 random, 4K QD32 (reads + writes) | 23 834 IOPS | 25 963 | **108%** | 26 317 | −1.3% |
| 1M sequential, QD8 | 1 748 MiB/s (cached span) | **935** | **89%** across the disk (below) | 701 | **+33%** |

- **1M, Flaminio's repetitions:** 877, 964 and 935 MiB/s.
- **Raw 4K QD32 did not reach steady state** in reps 2 and 3, in 12 rounds: that is the drive.
  So no share is given. QD32 is judged against W4.2's Flaminio figure, as the spec says.
- **Raw 1M rewrites one 16 GiB span,** which the drive keeps in its fast cache. It is reported,
  as in W4.1 and W4.2, not judged against.

**Write amplification** is unchanged. Bytes to the disk per client byte: 3.36 for 4K QD1, 2.22
for QD32, 2.25 for the mix, 1.00 for 1M. The drive's is about 1.0 on every job.

## 1M against raw across the disk

The spec judges 1M against raw written across the disk. Measured 2026-11-04 on fl-b, after the
campaign ([`ab-1m.sh`](data/2026-10-29-writes-1m/ab-1m.sh)):
- 1M QD8 O_DSYNC, 60 s each, Flaminio and raw interleaved, so both see the same drive state;
- Flaminio: RF 1, zero copy, one queue, a fresh store and a new 16 GiB volume for each run;
- raw: `/dev/vdb`, GiB 16–100, the profile's raw run.

| Pair | Flaminio | Raw | Share |
|---:|---:|---:|---:|
| 1 | 946 MiB/s | 1 043 MiB/s | **91%** |
| 2 | 981 MiB/s | 1 054 MiB/s | **93%** |

- **Median latency:** 8.3–8.6 ms against raw's 7.1–7.2 ms.
- **Raw alone, after an idle night,** gave 1 340 and 1 268 MiB/s just before. The drive is
  faster rested, so only the interleaved pairs are compared.
- **The campaign's 935 MiB/s is 89%** of these raw runs. The 2026-10-29 profile had raw at
  about 984.

**Verdict: pass** (80% or more), both in the traced profile (102–114%) and in the campaign
(89–93%).

## Integrity

After every Flaminio run:
1. 16 GiB written sequentially, with a checksum in every block, and read back;
2. 2 GiB of random writes, read back;
3. the random writes read back again after a restart.

**12 runs, 36 checks: 0 errors, 0 Flaminio checksum failures.**

## Other load in the lab

From 19:00 the other lab host, rhythm, carried CPU load (an Ollama server at 40–46% CPU). The
campaign ran on fl-b, on rhythm-backup, at RF 1, so the measured path never touches rhythm. Runs
before and after 19:00 agree: 4K QD1 before; QD32, 1M and the mix after.

Data: [`data/2026-10-29-writes-1m/`](data/2026-10-29-writes-1m/):
- the profile: [`notes.md`](data/2026-10-29-writes-1m/notes.md),
  [`trace-disk.sh`](data/2026-10-29-writes-1m/trace-disk.sh),
  [`trace-ublk-and-disk.sh`](data/2026-10-29-writes-1m/trace-ublk-and-disk.sh),
  [`cycle.awk`](data/2026-10-29-writes-1m/cycle.awk),
  [`phases.awk`](data/2026-10-29-writes-1m/phases.awk);
- the campaign: [`runs.tsv`](data/2026-10-29-writes-1m/campaign/runs.tsv),
  [`rounds.tsv`](data/2026-10-29-writes-1m/campaign/rounds.tsv),
  [`summary.txt`](data/2026-10-29-writes-1m/campaign/summary.txt),
  [`integrity.txt`](data/2026-10-29-writes-1m/campaign/integrity.txt);
- across the disk: [`ab-1m.txt`](data/2026-10-29-writes-1m/ab-1m.txt),
  [`ab-1m.sh`](data/2026-10-29-writes-1m/ab-1m.sh).
