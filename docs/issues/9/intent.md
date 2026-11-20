# Issue #9: 4K reads at queue depth 1 — intent

[Issue #9](https://github.com/jsfakian/Flaminio/issues/9), follow-ups §1, the
[performance plan](../../performance-plan.md).

## Why

Zero-copy reads reached 84% of raw at QD1 on fl-b, against the raw reference the bench used
before pull request #28 (the issue's title). The plan's goal is 90%. A profile of one read's
round trip (2026-10-29, [results](../../benchmarks/2026-10-29-reads-qd1-queues.md)) found three
things. The bench's figures below are those results' and their `data/*-summary.txt`'s, as
recorded in 7afa35d. Figures measured by hand, or in October, are labelled as such.

1. **The raw reference had stopped meaning anything.**
   - Before #28, `make bench-reads` read the raw disk in a different 16 GiB region in each
     repetition (at 0, 32 and 64 GiB), because the store's data was once spread over all of
     them.
   - By 2026-10-29 the regions at 32 and 64 GiB took 37% longer per read than the one at 0
     (85 µs against 62).
   - Two of the three repetitions landed there. On 2026-10-29 the bench reported raw's spread as
     35.9% and Flaminio at 115% of raw, flagged unstable: a result that cannot be used.
   - The data of the volume measured, volb, is at 0–21 GiB.
2. **One queue per CPU saves about 3 µs per read at QD1, and costs at QD32.** About 1–1.5 µs
   of it is the I/O scheduler: the kernel gives one queue `mq-deadline`, and several `none`.
   - With one ublk queue, a read is submitted on one CPU and served by a thread on another,
     which has to be woken. With one queue per CPU, the kernel sends each CPU's requests to its
     own queue, whose thread runs on that CPU.
   - In the bench, against raw where the volume's data is (the new reference, item 1), QD1 goes
     from 84.7% to 87.9% (13 097 and 13 602 IOPS, against 15 465 and 15 467).
   - But one fio job at QD32 then shares its CPU with its queue thread, and loses 11–12%
     (230 100 IOPS to 203 266: 98% of raw to 86%).
3. **What is left is the round trip, and the lab's disk.**
   - The queue thread sleeps and wakes twice per read: once for the request, once for the
     disk.
   - fl-b's virtual disk answers 3 µs slower when the next read does not follow the last
     completion at once. Raw fio spinning between reads shows it. A 2 µs gap, the shortest
     tested, already costs it. Every layer above the disk pays this; how short a gap avoids it
     was not measured.
   - Spinning instead of sleeping comes to about 90% of the new reference at most, and costs a
     core:
     - after each completion (`--poll-us 100`, the bench): 88.5% with one queue (13 677 against
       15 458), 90.2% with four (13 904 against 15 414);
     - while a read is in flight (a prototype, with 4 queues, by hand): about 89%, against the
       raw runs beside it (15.41–15.52k, over 0–21 GiB).

     History: October's `--poll-us 100` run, 13 799 IOPS, would have been 89.9% of the 15.35k
     raw measured by hand.

## What we want

1. A raw reference that reads the disk where the volume's data is, with the span the rules
   already ask for.
2. The QD1 result with one queue per vCPU, and what that setting costs, in the results and in
   the node's usage text.
3. The analysis recorded, with its numbers, so that the next step starts from them.
4. A bench that cannot move or overwrite a volume not named `bench-…`, and that stops when
   the node's replicas are not all in sync (plan review rounds 1 and 2; spec S2, S7).

## What closes #9

**#9 closes on one full `make bench-reads` run on fl-b, all three jobs, without spinning**
(`POLL_US=0`, the default, and no spinning while a read is in flight), in which:
- **QD1's share is 0.900 or more,** unrounded. Today `summary.txt` rounds the share to a whole
  percent, so 89.5% shows as 90% (R2-C2), and its medians are printed as integers. From the
  next step it prints the share truncated to two decimals, so a printed 90.00% decides it
  (R3-C; design, "What `summary.txt` says");
- **QD32's and 1M's shares are 0.900 or more too,** read the same way: the performance plan's
  goal for reads (section 3) is QD32 and 1M at 90% or more, with QD1 kept there;
- **no row is flagged `UNSTABLE`,** and the end-to-end check passes.

`summary.txt` also shows the mode and `POLL_US` from the next step, so the whole condition is
read from it.

The queue count is the run's own: the plan excludes spinning, not a setting. A count that
costs QD32 closes #9 only if QD32 stays at 90% or more in the same run. The recorded 4-queue
run did not: 86%.
- **The issue's literal "Done when"** ("≥ 90% of raw at QD1 on fl-b, by the plan's measurement
  rules (`make bench-reads`)") is already met, but only with polling: the recorded
  `QUEUES=4 POLL_US=100` run gave 90.2% (`zero-copy-q4-poll100-summary.txt`). Polling spins a
  core per busy queue, so that run does not close #9. This plan's goal is 90% without spinning.
- **No run without spinning reaches it:** 84.7% with one queue, 87.9% with four
  ([results](../../benchmarks/2026-10-29-reads-qd1-queues.md)).
- **The issue** gets this condition in its "Done when" (design, Documents).

Issue #9 stays open after this change.

## Out of scope

- **Spinning:** while a read is in flight, or after each completion (`--poll-us`). Both cost up
  to a core per busy queue under load, for 90.2% at most (the bench):
  - `--poll-us 100` on one queue gains about 3.8 points (84.7% to 88.5%);
  - over 4 queues, 2.3 points (87.9% to 90.2%).

  As decided, `--poll-us` stays a per-node setting, off by default. It applies to every queue of
  every device the node serves, so with several volumes on a node (#15) every busy queue spins
  (performance plan, Decided).
- **The default queue count,** of the node and of the bench: it stays 1.
  - One queue per CPU costs one fio job 11–12% at QD32.
  - Each queue has its own buffers: in copy mode, depth × the largest I/O, 64 MiB.
  - Choosing the count per machine is the node agent's job (issues #12–#14).
- **The ublk device's I/O scheduler.** With one queue the kernel gives the device
  `mq-deadline`. Setting `none` saved 1–1.5 µs per read at QD1. The node could set it. That is
  a separate change (results, next candidates).
- **The lab's virtual disks:** an iothread with polling might remove the 3 µs, for both sides.
  It changes the baseline of every benchmark so far.
- **Changes to the kernel side of ublk:** completing simple reads in the kernel (ublk with
  BPF). It is among the next candidates, and larger than this issue. Batched commands batch
  nothing at QD1, where one request is in flight.
- **The user-space work per read** (about 2.3 µs). The parts that could go add up to about
  1 µs:
  - allocations, about 0.2 µs;
  - the async executor, about 0.35 µs;
  - the engine's per-read bookkeeping.

  The store's map lookup is mostly cache misses, and stays. In the bench, 90% of raw (about
  15.45k) is 13.9k IOPS, 71.9 µs per read (results, "What 90% needs without spinning"):
  - **With one queue** (76.4 µs), 90% needs about 4.5 µs less per read, so 1 µs is not enough.
  - **With 4 queues** (73.5 µs), it needs 1.6 µs less. Trimming about 1 µs leaves about 0.6 µs:
    it does not reach 90% alone at either count. And 4 queues is the setting that costs QD32.

  (Against the 15.35k measured by hand, the 4-queue gap was 1.0–1.4 µs: history.) It is one of
  the next candidates, which are not ranked (design, "What is left").
