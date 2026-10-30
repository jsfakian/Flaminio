# Issue #9: 4K reads at queue depth 1 — intent

[Issue #9](https://github.com/jsfakian/Flaminio/issues/9), follow-ups §1, the
[performance plan](../../performance-plan.md).

## Why

Zero-copy reads reach 84% of raw at QD1 on fl-b, against the raw reference the bench has used
so far. The plan's goal is 90%. A profile of one read's
round trip (2026-10-29, [results](../../benchmarks/2026-10-29-reads-qd1-queues.md)) found three
things.

1. **The raw reference no longer means anything.**
   - `make bench-reads` reads the raw disk in a different 16 GiB region in each repetition (at
     0, 32 and 64 GiB), because the store's data was once spread over all of them.
   - Now the regions at 32 and 64 GiB take 37% longer per read than the one at 0 (85 µs against
     62).
   - Two of the three repetitions land there. On 2026-10-29 the bench reported raw's spread as
     35.9% and Flaminio at 115% of raw, flagged unstable: a result that cannot be used.
   - The data of the volume measured, volb, is at 0–21 GiB.
2. **One queue per CPU saves about 3 µs per read at QD1, and costs at QD32.** About 1–1.5 µs
   of it is the I/O scheduler: the kernel gives one queue `mq-deadline`, and several `none`.
   - With one ublk queue, a read is submitted on one CPU and served by a thread on another,
     which has to be woken. With one queue per CPU, the kernel sends each CPU's requests to its
     own queue, whose thread runs on that CPU.
   - Against raw where the volume's data is (the new reference, item 1), QD1 goes from 85% to
     88%.
   - But one fio job at QD32 then shares its CPU with its queue thread, and loses 11–12%.
3. **What is left is the round trip, and the lab's disk.**
   - The queue thread sleeps and wakes twice per read: once for the request, once for the
     disk.
   - fl-b's virtual disk answers 3 µs slower when the next read does not follow the last
     completion at once. Raw fio spinning between reads shows it. A 2 µs gap, the shortest
     tested, already costs it. Every layer above the disk pays this; how short a gap avoids it
     was not measured.
   - Spinning instead of sleeping comes to about 90% of the new reference, and costs a core:
     while a read is in flight (a prototype, with 4 queues), 89.4–90.0%; after each completion
     (`--poll-us 100`, measured in October), 89.9%.

## What we want

1. A raw reference that reads the disk where the volume's data is, with the span the rules
   already ask for.
2. The QD1 result with one queue per vCPU, and what that setting costs, in the results and in
   the node's usage text.
3. The analysis recorded, with its numbers, so that the next step starts from them.

The goal is not met here without spinning. The bench runs found 90% only with 4 queues and
100 µs polling, which cost a core ([results](../../benchmarks/2026-10-29-reads-qd1-queues.md)).
Issue #9 stays open after this change.

## Out of scope

- **Spinning:** while a read is in flight, or after each completion (`--poll-us`). Both cost up
  to a core per busy queue under load, for about 90% at most:
  - over 4 queues, 1–2 points;
  - `--poll-us 100` on one queue gains about 5 (84.7% to 89.9%).

  As decided, `--poll-us` stays a setting for latency-sensitive volumes. The bench runs measure
  it against the new reference.
- **The default queue count,** of the node and of the bench: it stays 1.
  - One queue per CPU costs one fio job 11–12% at QD32.
  - Each queue has its own buffers: in copy mode, depth × the largest I/O, 64 MiB.
  - Choosing the count per machine is the node agent's job (issues #12–#14).
- **The ublk device's I/O scheduler.** With one queue the kernel gives the device
  `mq-deadline`. Setting `none` saved 1–1.5 µs per read at QD1. The node could set it. That is
  a separate change (results, next candidates).
- **The lab's virtual disks:** an iothread with polling might remove the 3 µs, for both sides.
  It changes the baseline of every benchmark so far.
- **Changes to the kernel side of ublk:** completing simple reads in the kernel (ublk with BPF),
  or batched commands. They are the next candidates, and larger than this issue.
- **The user-space work per read** (about 2.3 µs). The parts that could go add up to about
  1 µs:
  - allocations, about 0.2 µs;
  - the async executor, about 0.35 µs;
  - the engine's per-read bookkeeping.

  The store's map lookup is mostly cache misses, and stays.
  - **With one queue,** 90% needs about 4.5 µs less per read, so 1 µs is not enough.
  - **With 4 queues,** it needs 1.0–1.4 µs less, so trimming would come close. But that is the
    setting that costs QD32.

  It is the first of the next candidates.
