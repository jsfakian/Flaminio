# Issue #10: 1M sequential writes at about 70% of raw — intent

[Issue #10](https://github.com/jsfakian/Flaminio/issues/10), follow-ups §1, the
[performance plan](../performance-plan.md) (W4).

## Why

At RF 1, Flaminio writes 1M sequentially at QD8 (O_DSYNC) at about 70% of raw written across
the disk. The plan's bound is 80%. A profile on fl-b (2026-10-29) shows why. It used block
tracepoints on the data disk and the ublk device, zero copy and one queue, steady state past the
drive's fast cache. "Steady" is the last 30 s of a 60 s run, from the empty state.

**Raw flushes only once its data has quiesced.** The block layer holds a flush back while data
writes are in flight. Each flush then covers nearly every write in flight, and runs with the
drive quiet:

| | Per flush | Flush | Between flushes | Written during a flush | Steady MiB/s |
|---|---:|---:|---:|---:|---:|
| Raw, across GiB 16–100 | 7.6 MiB | 4.7 ms | 3.1 ms | 0.35 MiB | ~984 |
| Flaminio | 4.0 MiB | 5.5 ms | 0.3 ms | 3.9 MiB | ~686 |

**Flaminio starts a commit as soon as the last one lands.**
- **Half the writes go each time:** the next group was staged during that flush, and its data
  written then (W4.2). So the flush is slower, and covers half the writes in flight.
- **They stay half:** the writes it acknowledged come back just after the next commit has
  started.

**The byte floor (`--commit-target-mib 8`, W4.5) has the right idea.**
- **What it does:** after a landing, it holds the staged group, and lets it grow to 8 MiB, or
  for at most 2 ms.
- **Where it stops short:** three things keep it at ~701 MiB/s. The prototypes, added one at a
  time on the floor:

| Prototype | What it removes | Steady MiB/s |
|---|---|---:|
| Stage with nothing in flight | With all the writes in the commit that landed, nothing is staged, so the returning writes are not written until the commit starts | ~721 |
| Hand a write to the engine once its data is copied in | The queue waited until every write being copied in was done | ~746 |
| Hold the window while writes are still being copied in | Most commits started when the 2 ms ran out, with 13–14 of the 16 requests in flight | **~849, 86% of raw** |

With the last one, commits carry all 16 requests (8 MiB), and 49 of 8 684 started on a timeout.

**Copies.**
- **What they cost:** each write's data is copied twice. The kernel copies it into the staging
  slot (on io-wq workers). The queue thread then copies it into an aligned buffer, which is 40%
  of its time.
- **A shortcut:** one copy fewer, an unsound shortcut for the measurement only, gave ~926 MiB/s
  (94%).
- **Kept out:** a sound version (a buffer pool the kernel copies into) is a separate step.

## What we want

1. **1M at 80% of raw or more,** from the empty state:
   - against raw across the disk (~984 MiB/s on fl-b), as the plan has judged 1M since W4.1;
   - both in the profile's traced run and in `make bench-writes`'s steady rounds. The
     campaign's own raw 1M stays in the drive's cache, and is reported, not judged against.
2. **No other write job worse beyond run-to-run noise (10%):**
   - 4K QD1, 4K QD32 and the 70/30 mix;
   - their paths unchanged: the changes apply to writes of at least one whole cluster.
3. **Crash safety unchanged,** and the held group safe on every path that waits for the store.

## Out of scope

- **One copy of a write's data** (the buffer pool): measured above, the next step.
- **RF 2 at 1M:** bound by the 1 GbE link (106 MiB/s).
- **Several queues or fio jobs:** W4.4 measured them.
- **The checksum's cost** (19% of the queue thread): a faster CRC-32C kernel is a separate
  change.
- **Flush coalescing with the floor:** it does not combine with it today, and still does not.
  To use it, set `--commit-target-mib 0`.
