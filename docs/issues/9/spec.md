# Issue #9: 4K reads at queue depth 1 — specification

## Behaviour

**S1. The raw reference reads where the volume's data is.** `make bench-reads` traces the reads
from the node's disk while it reads the whole volume once more, after the warm-up. The volume's
range on the disk is:
- **from** the first 1 GiB region of the disk that the traced read took 64 MiB or more from;
- **to** the end of the last such region.

The threshold leaves out stray reads, such as the store's metadata.

Every raw job, in every repetition, reads a span of the volume's size (rule 2, unchanged),
starting at the range's start. If that span would pass the end of the disk, it ends there
instead. There is no rotation between regions.

**S2. No guessing.** The bench stops, before any measured job, if:
- the trace cannot run: bpftrace fails, or does not finish (no end marker), or the traced read
  fails;
- the traced read took less than 90% of the volume's size from the disk: the volume's data is
  not on the node's disk, or has not been written;
- it took more than 110%: something else read the disk meanwhile;
- the range starts at or after its end: no region reached 64 MiB;
- the range is more than 1.5 times the volume's size, plus the one region a volume not aligned
  to 1 GiB takes. Either the data is too spread out for a span of the volume's size to stand
  for it, or something else read the disk meanwhile. The message names both, and suggests a
  refill (`FILL=1`) or a new volume for the first.

**S3. Recorded.** These are in the run's log and its `summary.txt`:
- the volume (`VOL`);
- the range, and the bytes read from each region (in the run's directory, `regions.txt`);
- the bytes the traced read took from the disk;
- the raw span;
- the node's vCPUs, the ublk queues, and the disk's hardware queues.

**S4. The bench's queue count is unchanged.** `QUEUES` stays 1 by default. The results record,
with the bench, against the new reference, on fl-b with `VOL=volb`:
- all three jobs with `QUEUES=1` and with `QUEUES=4` (one per vCPU on fl-b);
- QD1 with `POLL_US=100`, with 1 and 4 queues.

**S5. The node's default is unchanged.** `--queues` stays 1 by default. Its usage text says:
- one queue per CPU lowers latency at low queue depth, as each CPU's requests stay on it;
- a single deep submitter then shares its CPU with its queue's thread, and loses throughput;
- with several queues the kernel gives the device no I/O scheduler;
- each queue has its own buffers.

**S6. The rest of the rules are unchanged** (performance plan, section 2):
- caches dropped before every job;
- interleaved repetitions, raw first;
- a spread above 10% reported as unstable;
- the end-to-end check of the whole volume after the run.

Rule 2 now says where its span starts, and why.

## Tests

- **shellcheck** covers the changed scripts (`make lint`).
- **The checks of S2, by arithmetic:** `scripts/tools/bench-reference-test.sh` runs the
  function that turns a range into the raw span (design, `bench_reference`) on vectors with
  their expected output. `make lint` runs it, and so CI. The vectors:
  - fl-b's numbers give raw at 0–16 GiB;
  - a range at the end of the disk gives a span moved back inside it;
  - each refusal of S2;
  - the empty map, and a wrong argument count.
- **The run, on the lab** (fl-b, `VOL=volb`, whose data is at 0–21 GiB). The range holds only
  while volb is not refilled: no `FILL=1`, and its fill marker present. A refill moves the data.
  - `VOL=volb make bench-reads` finds the range 0–21 GiB;
  - raw reads 0–16 GiB in every repetition, with a spread of 10% or less (it was 35.9%);
  - `summary.txt` holds what S3 lists, and the run's directory `regions.txt`;
  - the end-to-end check passes;
  - the same with `QUEUES=4`.
- **The trace's stops, on fl-b:** the guest step that traces (design, `range`), then the
  function, by hand:
  - **under 90%:** the trace pointed at a disk the volume is not on (the root disk);
  - **over 110%:** another fio reads 2 GiB at 2 GiB of the data disk, inside the volume's
    range, over and over (`--time_based`, `--direct=1`, 1 MiB at depth 8, so that it reads the
    disk, not the page cache), from before the step starts until after it ends.

  After each, no `bpftrace` is left running. Nor after a run interrupted during the trace, once
  the traced read's time has passed: an SSH drop signals nothing in the VM, and bpftrace ends
  with fio.
