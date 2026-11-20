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
- some chunk's ISR leaves out `NODE`, checked before the trace: that chunk would be read over
  the wire (a replica on `NODE` that is resyncing, or an ISR that a restore left elsewhere). The
  message names that cause;
- the trace cannot run: bpftrace fails, or does not finish (no end marker), or the traced read
  fails;
- the traced read took less than 90% of the volume's size from the disk: the reads went over
  the wire (a replica on `NODE` left the ISR after the check), or the volume's data is not on
  the node's disk, or has not been written. The message names these causes;
- it took more than 110%: something else read the disk meanwhile;
- the range starts at or after its end: no region reached 64 MiB;
- the range is more than 1.5 times the volume's size, plus the one region a volume not aligned
  to 1 GiB takes. Either the data is too spread out for a span of the volume's size to stand
  for it, or something else read the disk meanwhile. The message names both, and suggests a
  refill (`FILL=1`) or a new `bench-` volume for the first. Either touches only the bench's
  own volumes (S7).

**S3. Recorded.** These are in the run's log and its `summary.txt`:
- the volume (`VOL`);
- the range, and the bytes read from each region (in the run's directory, `regions.txt`);
- the bytes the traced read took from the disk;
- the raw span;
- the node's vCPUs, the ublk queues, and the disk's hardware queues;
- from the next step (R3-C): the mode and `POLL_US`, and each job's share with two decimals,
  truncated rather than rounded, so that a printed 90.00% means a ratio of 0.9 or more.

**S4. The bench's queue count is unchanged.** `QUEUES` stays 1 by default. The results record,
with the bench, against the new reference, on fl-b with `VOL=volb`:
- all three jobs with `QUEUES=1` and with `QUEUES=4` (one per vCPU on fl-b);
- QD1 with `POLL_US=100`, with 1 and 4 queues.

These were recorded on 2026-10-29 ([results](../../benchmarks/2026-10-29-reads-qd1-queues.md),
in 7afa35d), with `volb`, before S7. S7 now refuses `volb`, so later runs use a `bench-`
volume.

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

**S7. The bench writes only its own volume** (plan review rounds 1 to 3). A run detaches `VOL`
from every node, attaches it to `NODE` at a new epoch, and may write all of it. So, before anything
else (before it detaches, attaches or writes anything), the bench refuses a `VOL` that:
- is not a volume id (`domain.ValidID`); or
- is not named `bench-…`, the bench's own names. The CSI driver's volumes are named
  `pvc-<uid>`, and no other lab command makes a `bench-` name unless told to.

If `VOL` does not exist, the bench waits until `NODE` is alive, creates it with `NODE`
preferred, and fills it. It refuses the new volume unless every chunk has `NODE` in its ISR,
naming the cause (placed without `NODE`) and the fix (delete it). It fills an existing one when
its fill marker is missing on `NODE`, or with `FILL=1`. `make bench-reads` defaults `VOL` to
`bench-reads`.

When the run ends, however it ends, the bench detaches its volume from `NODE`. If another
volume is attached on `NODE`, the bench stops before it restarts the node or attaches anything,
naming it, and leaves it attached: a restart would drop it.

## Tests

- **shellcheck** covers the changed scripts (`make lint`).
- **The checks of S2, by arithmetic:** `scripts/tools/bench-reference-test.sh` runs the
  function that turns a range into the raw span (design, `bench_reference`) on vectors with
  their expected output. `make lint` runs it, and so CI. The vectors:
  - fl-b's numbers give raw at 0–16 GiB;
  - a range at the end of the disk gives a span moved back inside it;
  - each refusal of S2;
  - the empty map, and a wrong argument count.
- **The checks of the volume, S2 and S7,** in the same script test (design, `bench_volume` and
  `bench_isr`). The refusals' messages are checked as strings:
  - `bench-reads` passes;
  - `vol1` and `pvc-0d8f…` are refused as not the bench's;
  - `bench-x;id`, `bench x`, `-x` and an empty name are refused as not ids;
  - a record with no chunks is refused;
  - a record whose every chunk has `NODE` in its replicas and its ISR passes;
  - a record with some chunks, or none, on `NODE` is refused as placed without `NODE`, with
    `VOL=bench-x make vol-delete` in the message;
  - a chunk with a replica on `NODE` that its ISR leaves out, or with no ISR, is refused with
    the ISR cause;
  - what `NODE` serves (design, `bench_attached`), with node.sh's exit code:
    - nothing, the bench's own volume, or a stopped node (`not running` with exit 3) passes;
    - another volume stops the bench. For `vol1` on fl-b the message holds "Unmount it if it
      is mounted" and `VOL=vol1 scripts/lab.sh vol detach fl-b`. For a `pvc-…` volume it holds
      "Delete the pod that uses it", and no `vol detach`;
    - any other answer is refused, exit 3 with other output included;
    - exit 1 is refused as node.sh's failure, and exit 255 ends the bench as a lost
      connection.
- **`node.sh attached`,** in `scripts/tools/node-scripts-test.sh`, against a fake control
  socket:
  - `ok none` gives `none-attached`;
  - `ok V /dev/ublkbN` gives `volume V`, a volume named `none` included;
  - an error, an empty answer (an `answers` file holding one blank line), an id that is not
    one, or another device path dies;
  - a node that is not running gives `not running`.
- **S5:** a unit test of `parse` in `node.rs`: with no `--queues`, the count is 1. The usage
  text's four points are checked by reading it.
- **The run, on the lab** (fl-b, `VOL=volb`, whose data is at 0–21 GiB; recorded on
  2026-10-29). The range holds only while volb is not refilled: no `FILL=1`, and its fill
  marker present. A refill moves the data.
  - `VOL=volb make bench-reads` finds the range 0–21 GiB;
  - raw reads 0–16 GiB in every repetition, with a spread of 10% or less (it was 35.9%);
  - `summary.txt` holds what S3 lists, and the run's directory `regions.txt`;
  - the end-to-end check passes;
  - the same with `QUEUES=4`.
- **The run since S7, which refuses `volb`:** `VOL=bench-qd1 make bench-reads` on fl-b.
  - It finds the range its `regions.txt` shows;
  - raw reads a span of the volume's size from that range's start in every repetition, with a
    spread of 10% or less;
  - `summary.txt` holds what S3 lists;
  - the end-to-end check passes;
  - the same with `QUEUES=4`.
- **S7, on the lab:**
  - `VOL=vol1 make bench-reads` stops before anything else: no run directory appears under
    `.lab/bench/`, and `metactl get vol1` shows the same epoch before and after;
  - `VOL=bench-qd1 make bench-reads`, with no such volume, creates it with a replica of every
    chunk on fl-b, fills it, and passes the end-to-end check;
  - `make bench-reads`, then `make node-fault`: both pass, as the bench let go of its volume;
  - a `make bench-reads` interrupted (Ctrl-C) during its warm-up leaves nothing attached on
    fl-b (R3-B). The trap's detach is best effort: the node's detach waits up to 30 s for the
    device. So the check polls `node.sh attached` for about 40 s, and asserts that it comes to
    `none-attached` (R4-D);
  - with `vol1` attached on fl-b, `make bench-reads` stops, naming `vol1` and the command that
    detaches it, and `vol1` stays attached (`node.sh attached` says `volume vol1`). This holds
    whatever flags the node runs with. It is run twice: with `MODE=verify` against a node in
    that mode, and with the default zero copy against the same node, which the bench would
    otherwise restart (R3-A);
  - with fl-b's node stopped (`scripts/lab.sh node down b`), `make bench-reads` goes on, as
    `not running` means nothing is attached: it starts the node and runs (R5-B1);
  - `VOL=bench-qd1 make vol-delete`, the command the placement refusal names, deletes the
    volume the second check created (R3-D).
- **The trace's stops, on fl-b:** the guest step that traces (design, `range`), then the
  function, by hand:
  - **under 90%:** the trace pointed at a disk the volume is not on (the root disk);
  - **over 110%:** another fio reads 2 GiB at 2 GiB of the data disk, inside the volume's
    range, over and over (`--time_based`, `--direct=1`, 1 MiB at depth 8, so that it reads the
    disk, not the page cache), from before the step starts until after it ends;
  - **the traced read fails:** `range` with a `SPAN` twice the volume's size. fio reads less
    than `SPAN`, or fails, and the step stops with "the traced read of … failed", not with a
    range that the function would call under 90%;
  - **bpftrace fails:** a copy of the step whose tracepoint is misspelt (`block_rq_issuex`).
    The step prints the trace's last lines, bpftrace's error among them, and stops with "the
    trace of … failed".

  After each, no `bpftrace` is left running. Nor after a run interrupted during the trace, once
  the traced read's time has passed: an SSH drop signals nothing in the VM, and bpftrace ends
  with fio.
