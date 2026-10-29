# Issue #9: 4K reads at queue depth 1 — design

## Finding the volume's range (S1–S3)

**The trace: a new step, `range DEV SPAN DISK`,** in `scripts/guest/bench-reads.sh`. `DISK` is
the data disk's path (`/dev/vdb`), as the step `disk` prints it. Its only output on stdout is
one line, `FIRST END TOTAL DISK_BYTES`: the range in GiB, the bytes read from the disk, and the
disk's size.
1. **Install.** `bpftrace` is added to `provision.sh`'s packages. The step also installs it if
   missing (`>/dev/null 2>&1`), as the network benchmark installs `iperf3`, and stops with a
   message if that fails.
2. **Trace one read.** `sudo timeout 300 bpftrace -B none -c '<fio …>' -e '<program>'`, with
   all of its output, fio's stderr included, in a file in the VM.
   - **The fio command:** one sequential read of all of `DEV` (`SPAN`, 1 MiB, depth 8, direct),
     its report in a JSON file (`--output=`).
   - **The program:** on the tracepoint `block:block_rq_issue`, for the requests of `DISK`
     whose `rwbs` starts with `R`, it sums `args.bytes` per 1 GiB region:
     `@r[args.sector >> 21]`. `args.sector` counts 512-byte sectors of the whole disk. An
     `END` action prints `trace-end`.
   - **Matching the disk:** by its kernel `dev_t`, `MAJOR << 20 | MINOR`, from
     `/sys/class/block/<name>/dev`. Bash puts the number into the program. The program uses
     no bpftrace `$` variables, so that bash leaves it alone.
   - **Why `-c`:** bpftrace attaches its probe first, then starts fio, and exits, printing the
     map, when fio does. Nothing has to wait for the attach, or signal a process that runs as
     root.
   - **`timeout`** runs inside `sudo`, so it can stop bpftrace if fio hangs. bpftrace then
     sends SIGTERM to fio.
   - **The `-c` string** is split on spaces by bpftrace, with no shell: no quotes,
     redirections, globs or `$`. There is no `sudo` in it either: fio runs as root already.

   Checked on fl-b, with bpftrace 0.25: a read at 5 000 MiB lands in region 4. bpftrace exits
   by itself in 0.8 s, and none is left running.
3. **Check.** Before the trace, the step removes the previous run's trace file and fio report
   with `sudo rm -f`: the report belongs to root, in the sticky `/tmp`. A fio that fails before
   writing its report then cannot pass on an old one.
   - **bpftrace:** if it fails, or the file has no `trace-end`, the step prints the file's
     last 20 lines to stderr and stops.
   - **fio:** if its report shows an error, or less than `SPAN` read, the step stops with a
     message.

   Without these checks, a failed read would come out as "under 90%", the wrong reason.
4. **Order.** This runs after the warm-up step, whose read waits for reconciliation after a
   restart. Reconciliation may still read other volumes' replicas from the same disk. The
   checks of `bench_reference` catch reads far from the data. A large read just before the
   data is not caught (below, the limits).
5. **Print.**
   - **Parsing:** awk reads only the lines `@r[N]: BYTES`. They go, through `sort -n` (bpftrace
     sorts a map by value, and Ubuntu's awk, mawk, cannot sort), to
     `/tmp/bench-range-regions.txt` in the VM. awk exits 0 when nothing matches. Its numbers
     are printed with `%.0f`: mawk prints large integers in exponent form otherwise.
   - `FIRST`: the first region with at least 64 MiB read.
   - `END`: one past the last such region.
   - `TOTAL`: the bytes read from all regions, the small ones included.
   - **An empty map, or no region reaching 64 MiB,** gives `0 0 TOTAL DISK_BYTES`, which
     `bench_reference` refuses. A disk the read never touched gives `0 0 0 DISK_BYTES`: under
     90%.

   The threshold leaves out under 64 MiB of the volume's data at each end of the range. For a
   16 GiB volume, that is under 0.4% per end. volb's edge regions hold 0.15 and 0.7 GiB.

**The arithmetic: `bench_reference SPAN FIRST END TOTAL DISK_BYTES`,** a function in
`scripts/lib.sh`, beside the lab's other helpers. It runs on the workstation and uses integers
only. It prints `OFFSET LENGTH` in GiB for the raw jobs, or stops with a message (S2). `SPAN` is
the volume's size as `<n>G`. The harness takes it from the volume's record (`sizeBytes`, from
`metactl get`, which it reads already), and stops if it is not a whole number of GiB.
1. **The arguments:**
   - there are 5;
   - `SPAN` matches `^([1-9][0-9]*)G$`, giving `S` GiB and `SB = S << 30` bytes;
   - the other four match `^(0|[1-9][0-9]*)$`. Leading zeros would be octal to bash.
2. Refuses `TOTAL × 10 < SB × 9`: the volume's data is not on this disk, or was not written.
3. Refuses `TOTAL × 10 > SB × 11`: something else read the disk during the trace.
4. Refuses `FIRST ≥ END`.
5. Refuses `(END − FIRST) × 2 > S × 3`. The message names both causes: data spread over the
   disk (a refill, `FILL=1`, or a new volume), or other reads during the trace.
6. **The span:**
   - `D = DISK_BYTES >> 30`;
   - `OFFSET = FIRST`, or `D − S` if `FIRST + S > D`;
   - it refuses `OFFSET < 0`;
   - `LENGTH = S`.

**Its tests: `scripts/tools/bench-reference-test.sh`.** It sources `lib.sh`, and checks
vectors with the output each must give:
- **fl-b:** `16G 0 21 <16 GiB> 107374182400` gives `0 16`.
- **The end of the disk:** `16G 85 100 <15 GiB> 107374182400` gives `84 16`.
- **The boundaries, with 16G:**
  - `END − FIRST` of 24 passes and 25 refuses;
  - `TOTAL` 15461882266 passes and 15461882265 refuses (90% of 16 GiB is 15461882265.6);
  - `TOTAL` 18897856102 passes and 18897856103 refuses (110% is 18897856102.4).
- **The rest:**
  - each other refusal;
  - leading zeros;
  - the empty map's `0 0 0 <disk>`;
  - a wrong argument count.

`make lint` runs it, and so CI. Like shellcheck there, it is skipped, with a message, when the
`bash` found first is older than 4.4, which `lib.sh` needs. That is macOS's own bash. It has
`# shellcheck source=../lib.sh`.

**The harness** (`cmd_bench_reads`, `scripts/lab.sh`):
- **Calls the step and the function:** `range` after the warm-up, with the volume's size. Then
  it copies the region map from the VM (`vssh … cat`) to the run's `regions.txt`, as it copies
  the fio reports, before `bench_reference`. So the map is kept when the function refuses.
  Each result is assigned with a plain `x=$(...)`, not `local x=$(...)`, so that a failure
  stops the bench.
- **Uses the result:** every raw job gets `off=OFFSET G` and `span=LENGTH G`. The rotation and
  its comment go.
- **Logs** the volume (`VOL`), the range, `TOTAL`, the raw span, the vCPUs, the queues and the
  disk's hardware queues (`ls /sys/block/<disk>/mq | wc -l`).
- **Appends** the same to `summary.txt` after the table, which `tee` writes first.

The header comment, the usage line, the Makefile's help text and `docs/lab.md`'s row say "the
raw disk where the volume's data is", instead of "the same 16 GiB" or offsets. They also say
`VOL` (default vol1).

**Why a span of the volume's size.** Rule 2 asks for it: a drive's mapping cache makes larger
spans slower, and both sides must pay the same.
- **When the data spans more:** the volume's data spans more than its size when the store
  placed other data among it. volb's data on fl-b spans 0.85–20.7 GiB, with a hole at
  4.8–8.7 GiB.
- **The two effects:** by rule 2's argument, raw then reading the narrower span favours raw.
  But regions also differ in speed, which can go either way.
- **Measured on fl-b at QD1:** 0–16 GiB read 0.6% slower than 0–21 GiB (15.35k and 15.44k
  IOPS). That is within noise, and slightly in Flaminio's favour. It was not measured at QD32
  or 1M.

**The limits of the checks.**
- **Spread-out data:** the 1.5× limit keeps the start of the range meaningful. A volume whose
  data is spread over the disk has no region that stands for it. That happens after many
  rewrites, when the store's cursor wraps.
- **A stray read near the data:** 64 MiB or more just before it moves `FIRST` earlier, with no
  refusal, as long as the range stays within 1.5×. An example is a 256 MiB chunk being resynced.
  `regions.txt` shows it.

**Rule 1 applies to raw's span too:** it must have been written. A never-written or discarded
range reads as zeros from the host's file, without touching the drive.
- The hole at 4.8–8.7 GiB holds other data of the store.
- The write campaigns' whole-disk write covered the rest.
- On fl-b, 0–16 and 0–21 GiB read alike, which agrees.

## Queues (S4, S5)

The bench's `QUEUES` stays 1, and so does the node's `--queues`. The node's usage text in
`flaminio-io/src/node.rs` gets the trade-off (S5). It also says that with several queues the
kernel gives the device no I/O scheduler, as it gives the raw disk.

**Why one queue per CPU helps at QD1.**
- **The mapping:** the kernel maps each CPU to one hardware queue. libublk pins each queue's
  thread to the CPUs of its queue (`set_thread_affinity`, from the kernel's queue affinity).
  With 4 queues on fl-b's 4 vCPUs, each thread has one CPU.
- **The path:** a read submitted on CPU 2 is served by the thread on CPU 2. The store disk's
  completion interrupt is on CPU 2 too: fl-b's virtio disk has 4 hardware queues, one per CPU.
  The thread's completion of the ublk request wakes fio on CPU 2.
- **The difference:** no wake-up crosses CPUs. With one queue, the thread is woken on another
  CPU. In a VM, waking a halted vCPU costs microseconds.

**Why it costs at QD32.** One fio job's requests all go to the queue of fio's CPU. That
queue's thread is pinned there, so fio and the thread share one CPU. With one queue, the thread
runs on another CPU, beside fio.

Measured on fl-b, zero copy, one fio job, 2 runs each. This is preliminary: the bench runs of
S4 give the figures with 3 repetitions.

| Job | 1 queue | 4 queues | Raw |
|---|---:|---:|---:|
| 4K random, QD32 | 230–233k IOPS | 204–205k IOPS | 242–244k IOPS |
| 1M sequential, QD8 | 6 608–6 640 MiB/s | 6 639–6 647 MiB/s | 6 605–6 627 MiB/s |

**From the bench's defaults to 4 queues,** at QD1, by IOPS.
- **The bench's defaults** (one queue, `mq-deadline`): 13.0k IOPS, 76.9 µs per read.
- **4 queues** (the kernel picks `none` itself): 13.55–13.62k, 73.4–73.8 µs per read.
- **The split of the 3.1–3.5 µs saved:**
  - **the scheduler,** about 1.2 µs: `none` set by hand on one queue gave about 1.5% more IOPS;
  - **the queue count,** about 2 µs.
- **Against raw:** 0–16 GiB, the bench's new span, reads 15.35k. The share goes from 84.7% to
  88.3–88.7%. The results quote the bench's own runs.

**Where the queue count's gain shows.** QD1 phases, from block tracepoints, with the ublk
device's scheduler set to `none` by hand in both rows:
- **request to disk:** the read's `block_rq_issue` on the ublk device to the first
  `block_rq_issue` on the store disk;
- **disk:** from there to that disk's `block_rq_complete`;
- **disk to completion:** from there to `block_rq_complete` on the ublk device.

| Queues | Request to disk | Disk | Disk to completion |
|---:|---:|---:|---:|
| 1 | 4.2–5.1 µs | 65.5–65.7 µs | 3.3 µs |
| 4 | 3.2–3.3 µs | 65.7 µs | 3.4–3.5 µs |

The phases differ by 0.5–1.8 µs, mostly in the request's way to the queue thread. The rest of
the gain is outside them, in fio's submission or its wake-up. That both cross CPUs with one
queue is a hypothesis: with one hardware queue for all CPUs, blk-mq normally dispatches on the
submitting CPU.

**What 90% needs.** 90% of 15.35k is 13.82k: 72.4 µs per read.
- **With 4 queues:** 1.0–1.4 µs less. Trimming the user-space work (about 1 µs, intent) would
  come close.
- **With one queue:** about 4.5 µs less.

## What is left: the numbers

For the results document, and the next step.
- **Per read, Flaminio's queue thread** spends about 4.8 µs in the kernel:
  - two sleeps and two wake-ups;
  - two `io_uring_enter` calls, which bpftrace counted: one per sleep, the minimum;
  - the disk's virtio notification, which raw pays too.

  It spends about 2.3 µs in user space (`perf`, cpu-clock, DWARF call graphs).
- **The disk** takes 65 µs for Flaminio's reads, against 62 µs for raw's, in the same regions.
  - **The test:** raw fio, spinning between reads (`thinktime_spin`), takes 65–66 µs at the disk
    too.
  - **The gaps it covered:** 2, 4, 6 and 8 µs. The IOPS confirm the gaps, from 14.1k at 2 µs
    to 13.0k at 8 µs.
  - **What can be said:** the disk answers faster when the next read follows a completion at
    once.
  - **The likely cause, untested:** fl-b's domain defines no iothread, so QEMU serves the disk
    from its main loop without polling.
- **Spinning.** Both ways cost a core, which is why they are out of scope (intent). The bench
  runs of S4 measure `POLL_US=100` against the new reference, with 1 and 4 queues.
  - **While a disk read is in flight** (a prototype, not kept): 4 queues, scheduler `none`,
    13.72–13.82k IOPS. That is 72.4–72.9 µs per read, 0.5–1.4 µs less than 4 queues alone
    (73.4–73.8 µs, another run), and 89.4–90.0% of 15.35k.
  - **`--poll-us 100`** (R2, 2026-10-10): 13 799 IOPS, which would be 89.9% of 15.35k.
- **Next candidates:**
  - trimming the user-space work per read (about 1 µs: allocations, the executor, the
    engine's bookkeeping);
  - the node setting `none` on a one-queue ublk device (about 1.2 µs);
  - ublk with BPF;
  - batched commands;
  - the lab disk's iothread, with both sides measured again.

## Documents

- **Results:** `docs/benchmarks/2026-10-29-reads-qd1-queues.md`. It holds:
  - the measurements above, and the bench runs;
  - the data in `docs/benchmarks/data/2026-10-29-reads-qd1-queues/`;
  - with the data, what produced it (rule 7): the bpftrace programs, the fio command lines of
    the hand measurements, and the prototype's diff.
- **Performance plan, section 2, rule 2:** for reads, the span starts where the volume's data
  is, found by the trace, and keeps the volume's size. The write rules keep their own span.
- **Performance plan, Status:**
  - **The date and the reference:** "against raw on 16 GiB spans" becomes "against raw on a
    span of the volume's size, where its data is (since 2026-10-29)".
  - **"Zero copy now":** all three cells come from the `QUEUES=1` bench run.
  - **"With 100 µs polling":** its QD1 cell comes from the `POLL_US=100`, `QUEUES=1` run.
  - **4 queues:** a line under the table gives QD1 with 4 queues, which is not the default,
    without and with polling.
  - **Older cells,** those not measured again, are marked as measured against the rotating raw
    reference.
  - **The Goal line** ("84% by default, 88% with polling") gets the new figures, and its "QD32
    and 1M meet it" is checked against the `QUEUES=1` run.
  - **"Decided"** adds: one queue per vCPU lowers QD1 latency and costs a single submitter at
    QD32; the count stays a per-node setting.
- **R1–R6 results, R4:** "the single-job numbers do not change" gets a note. It was measured
  with one queue only (`q1-jobs1`); there is no 2-queue run with one job. With one queue per
  vCPU, QD1 gains and QD32 loses.
- **`docs/lab.md`:** the `bench-reads` row says where raw reads, names `VOL`, and notes
  bpftrace.
- **The issue** stays open, with a comment linking the results.
