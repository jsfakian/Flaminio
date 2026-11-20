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
   From the next step, the message names first the reads going over the wire (below, local
   reads from the ISR).
3. Refuses `TOTAL × 10 > SB × 11`: something else read the disk during the trace.
4. Refuses `FIRST ≥ END`.
5. Refuses `(END − FIRST − 1) × 2 > S × 3`: a range over 1.5 times the volume, plus the one
   region a volume not aligned to 1 GiB takes (a 1 GiB volume at 3.5–4.5 GiB spans 2). The
   message names both causes: data spread over the disk (a refill, `FILL=1`, or a new
   `bench-` volume), or other reads during the trace.
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
  - `END − FIRST` of 25 passes and 26 refuses; with 1G, 2 passes and 3 refuses;
  - `TOTAL` 15461882266 passes and 15461882265 refuses (90% of 16 GiB is 15461882265.6);
  - `TOTAL` 18897856102 passes and 18897856103 refuses (110% is 18897856102.4).
- **The rest:**
  - each other refusal;
  - leading zeros;
  - the empty map's `0 0 0 <disk>`;
  - a wrong argument count.
- **The volume (next step: S2, S7):**
  - `bench_volume`: `bench-reads` passes; `vol1` and `pvc-0d8f…` are refused; so are
    `bench-x;id`, `bench x`, `-x` and an empty name, which `valid_id` refuses;
  - `bench_isr bench-x fl-b` on records written in the test:
    - a record with no chunks refuses (R4-C);
    - a record whose every chunk has fl-b in its `replicas` and its `isr` passes;
    - **partial placement** (R3-D): some chunks with no replica on fl-b refuse with the
      placement cause, and the message holds `VOL=bench-x make vol-delete`;
    - no chunk on fl-b refuses in the same way;
    - a chunk whose `replicas` hold fl-b but whose `isr` does not, or which has no `isr`,
      refuses with the ISR cause. A filter on `.replicas` would pass it, so the vector fails
      without the change.

    The messages are checked as strings.
  - `bench_attached fl-b bench-x RC` (R4-B, R5-B1, R5-B3, R6-A). The messages are checked as
    strings, case included:
    - RC 0: `none-attached` and `volume bench-x` pass;
    - RC 0: `volume vol1` refuses, and the message holds `Unmount it if it is mounted` and
      `VOL=vol1 scripts/lab.sh vol detach fl-b`;
    - RC 0: `volume pvc-0d8f…` refuses, and the message holds `Delete the pod that uses it`,
      and not `vol detach`;
    - RC 0: `volume` with an id that `valid_id` refuses, an empty line, `ok none` (the socket's
      own words) and `not running` refuse as unexpected;
    - RC 3: `not running` passes; `none-attached` refuses as unexpected;
    - RC 1 refuses with `node.sh attached failed (exit 1)`;
    - RC 255 exits 255, through `lost`. The test sets `VM_C` and `LOCAL_STATE_DIR`, which
      `lost` reads.
- **`node.sh attached`, in `scripts/tools/node-scripts-test.sh`** (R4-A), which runs node.sh
  against a fake control socket (`CONTROL_SOCKET`, under `FLAMINIO_SCRIPT_TEST=1`). Each vector
  writes its answer to the case's `answers` file after `fresh`, which leaves
  `ok 2 replicas deleted` there (R5-B2):
  - `ok none` prints `none-attached`;
  - `ok bench-x /dev/ublkb1` prints `volume bench-x`;
  - `ok none /dev/ublkb1`, a volume named `none`, prints `volume none`;
  - `error: …`, `ok bench x /dev/ublkb1` and `ok bench-x /dev/sda` die;
  - **an empty answer** dies. Its `answers` file holds one blank line (`echo >"$s/answers"`).
    An empty file would not do: the fake server then answers `error: no answer`, and the
    vector would pass for the wrong reason;
  - a unit that is not running gives `not running`, exit 3.

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
raw disk where the volume's data is", instead of "the same 16 GiB" or offsets. The Makefile's
help and `docs/lab.md` also name `VOL` (vol1). The usage line (`lab.sh`, `usage`) does not, and
leaves out `POLL_US`, which the help lists (R1-N1). The next step fixes that (below).

**Next step: the volume and local reads** (S2, S3, S7; settled after approval, in plan review
rounds 1 to 3: R1-S1 to R1-S3, R2-B1, R2-B2, R3-A to R3-D). Pull request #28 took any `VOL`,
and checked replicas, not the ISR. The lab scripts are code, so this plan states the change,
and the next step makes it. `cmd_bench_reads`, in this order:
1. **The guard:** the first command after its `local` lines, before it creates the run's
   directory or calls `release_lab_volume`, is `bench_volume "$VOL"`. This new function in
   `scripts/lib.sh` stops unless `VOL` passes `valid_id` and starts with `bench-`. Its message
   says that the bench detaches, attaches and writes its volume, so it takes only its own.
   `valid_id` moves from `lab.sh` to `lib.sh`, which `lab.sh` sources, so that the script test
   can load it.
2. **`release_lab_volume`,** which detaches `VOL` everywhere, as now.
3. **Nothing else attached on `NODE`** (R3-A), before the node may restart. A node serves one
   attached volume at a time (`control.rs`: "X is attached here; one volume at a time"). The
   restart of step 4 stops the unit, and with it the device of any volume attached there.
   - **The question:** a new `node.sh attached` reads the control socket's `status` answer
     (`control.rs`) and accepts exactly two forms (R4-A):
     - `^ok none$`, for which it prints `none-attached`;
     - `^ok (ID) /dev/ublkb[0-9]+$`, where ID is `volume_id`'s regex
       (`^[a-zA-Z0-9][a-zA-Z0-9._-]{0,127}$`), for which it prints `volume ID`.

     Any other answer, an error or an empty line included, dies. The tokens differ, so a volume
     named `none` cannot pass for no volume, and the id is printed only once it has matched.
     It says `not running` (exit 3) when the unit is down, which means that nothing is
     attached.
   - **The call:** `rc=0; out=$(run_on_vm "$node" …/node.sh attached) || rc=$?`, then
     `bench_attached "$node" "$VOL" "$rc" <<<"$out"`. The caller maps nothing itself (R6-A).
   - **The answer's use:** `bench_attached NODE VOL RC`, a new function in `lib.sh`, reads the
     output on stdin. The exit code comes first:
     - **RC 255:** a lost connection, so it calls `lost NODE`, which exits 255, as elsewhere.
       `lost` moves from `lab.sh` to `lib.sh`, as `valid_id` does, so that the script test
       reaches it;
     - **RC 3:** passes only with the output `not running` (R5-B1), node.sh's word for a stopped
       unit. Nothing is attached, and step 4 starts the node. Any other output with exit 3
       refuses as unexpected;
     - **RC 0:** the output is read below;
     - **any other RC,** such as a `die` in node.sh (exit 1, its text already on stderr):
       "NODE's node.sh attached failed (exit RC); its message is above". An RC that is not a
       number refuses too.

     With RC 0 it passes two answers:
     - `none-attached`;
     - `volume VOL`: `release_lab_volume` has just let `VOL` go, so that is a leftover.

     It stops on anything else. The first part of the message is "NODE serves V: the bench
     would restart the node or attach its own volume, and V would lose its device". What
     follows depends on V (R5-B3), which must match `valid_id`:
     - **V named `pvc-…`,** a CSI volume, which a pod may have mounted: "Delete the pod that
       uses it, and run again". It names no lab command. node.sh's `detach` has no mount check:
       on a mounted device it would fail after 30 s, with the device half deleted under a live
       filesystem (`control.rs`).
     - **Any other V:** "Unmount it if it is mounted, then run VOL=V scripts/lab.sh vol detach
       NODE, and run again" (R4-B). There is no make target for it. `vol detach` takes `VOL`
       from the environment and the node as its argument (default `a`). NODE is passed as
       `fl-b` itself, with no letter to derive (R5-N1): `vms_of` accepts a VM's name as well as
       its letter (`b | "$VM_B"`, `lib.sh`), and VM names come from `lab.conf`.

     On any other output, `not running` with exit 0 included, it stops with "unexpected answer
     from NODE's node.sh attached (exit RC)". It does not echo the output.
   - **It never detaches V.** So it moves no volume not named `bench-…` (S7). And with `VOL`
     naming the bench's own volume, lab.sh no longer knows the lab's test volume's name for
     sure.
4. **The node's mode:** restarted when its flags differ from the run's, then `ensure_nodes`, as
   now.
5. **Its own volume:** `meta_retry get "$VOL"`.
   - On `code = NotFound`, the bench first waits until `NODE` is alive in the metadata service
     (R2-B2). The node may just have restarted, and `domain.Place` drops a preferred node that
     is not a candidate: not alive, draining or a witness (`service.go`). The wait is
     `write_volume`'s loop (60 s), moved into a function that both call.
   - Then it runs `metactl create "$VOL" "$VOL_SIZE" 2 1 "$VOL_CHUNK" "$node"`, with `NODE` as
     the preferred node, which `placeChunk` puts first in every chunk.
   - **The placement is checked** (R2-B2): `bench_isr "$VOL" "$node"` on the create's output.
     A new chunk's ISR is its replicas (`domain.go`, `NewVolume`).
   - A volume just created is filled, as with `FILL=1`, whatever marker an older volume of that
     name left.
   - Any other error stops (`meta_failed`).
6. **The trap** (R2-B1, R3-B), just before `cmd_vol attach`: `trap '…' EXIT`, as `partition`
   and `chaos` do, so that a run that fails or is interrupted lets go of its volume.
   - Its command is built when it is set, with `NODE` and `VOL` expanded and quoted by
     `printf %q`: `node` is a local, gone by the time lab.sh exits.
   - It runs `node.sh detach`, and ignores a failure. It is best effort (R4-D): the node's
     detach waits up to 30 s for the device to stop (`control.rs`), and an interrupted fio may
     still hold the device. The lab check polls for the outcome (spec Tests).
   - Over ssh it is bounded by `VM_SSH_OPTS`'s time-outs (`ConnectTimeout=10`,
     `ConnectionAttempts=3`): about 30 s to a VM that does not answer, as after `lost`.
7. **The attach, then local reads from the ISR:** `bench_isr "$VOL" "$node"` on the `metactl
   get` record, in place of the replica count of `chunks_on`. The target reads only from an ISR
   source (`target.rs`, `source`).
8. **The run,** as now.
9. **Letting go** (R3-B): after the end-to-end check, the bench detaches its volume
   (`node.sh detach`, which must answer `detached`), then clears the trap (`trap - EXIT`), as
   lab.sh clears its other traps. A caller that runs `cmd_bench_reads` in its own lab.sh
   process, such as #15's `bench-ab`, gets `NODE` back free. It sets any EXIT trap of its own
   after the bench returns.

**`bench_isr VOL NODE`,** a new function in `lib.sh`. It reads a volume's record on stdin (a
`metactl get` or `create` output) and stops:
- **when the record has no chunks** (R4-C), an empty or unparsed output: "VOL's record has no
  chunks";
- **when some chunks have no replica on `NODE`:** "VOL has K/N chunks with no replica on NODE:
  it was placed without NODE (draining or not alive then), or a drain or a failover moved them
  since. Delete it (VOL=VOL make vol-delete) and run again". The message names no single cause
  (R4-C): on the `get` path the volume may be old. Deleting it is safe, as its name is
  `bench-`. `vol delete` takes `VOL` from the environment: `lab.conf` sets `vol1` only when
  `VOL` is unset (`: "${VOL:=vol1}"`), and make passes the environment on;
- **otherwise, when some chunks leave `NODE` out of their ISR:** "only K/N chunks of VOL have
  NODE in their ISR: the others would be read over the wire (a replica on NODE resyncing, or an
  ISR a restore left elsewhere)".

It reads `.replicas // []` and `.isr // []`: protojson leaves out an empty list. `chunks_on`
stays as it is: `drain-test` counts replicas with it, and must.

**The under-90% refusal** of `bench_reference` names the reads going over the wire first: "…
the reads went over the wire (a replica on this node out of the ISR), or its data is not on this
disk, or was never written (FILL=1 writes it)". With S7, `FILL=1` rewrites only the bench's own
volume.

**What `summary.txt` says** (S3; R3-C), so that the closing condition (intent) can be read from
it alone:
- **The share:** `bench_summary` prints it with two decimals, truncated rather than rounded:
  `int(10000 × F / R) / 100`, where F and R are the medians it computed, before it prints them
  as integers. A printed 90.00% then means a ratio of 0.9 or more.
- **The settings:** the line after the table gives the mode and the polling with the queues:
  "MODE, POLL_US µs polling, N vCPUs, Q ublk queue(s), DISK with H hardware queue(s)".

**Writes:** a write campaign on `NODE` stops on a `bench-` volume with replicas there
(`write_start`: "NODE holds replicas of …, and the campaign formats its disk"). The bench's row
in `docs/lab.md` says to delete it first (`VOL=bench-reads make vol-delete`). `write_start` is
not changed.

**Quoted:** every remote use of `VOL` is quoted with `printf %q`:
- `ENV_VARS="VOLUME=$(printf %q "$VOL")"` in `release_lab_volume` and in `cmd_vol attach`;
- the fill marker's `test -f` and `touch`.

`metactl` and `run_on_vm` quote their arguments already.

**The default:** the Makefile's `bench-reads` passes `VOL=$(or $(VOL),bench-reads)`; today it
passes no `VOL`. `lab.conf`'s default, `vol1`, is refused.

**The words:** the usage line, the Makefile's help and `docs/lab.md` say:
- `VOL`: a `bench-` name, created on `NODE` when missing. The Makefile's help and `docs/lab.md`
  give make's default, bench-reads. The usage line says that `make` supplies it (R2-N4):
  `scripts/lab.sh bench-reads` alone takes `lab.conf`'s `vol1`, and refuses it;
- that the bench attaches it to `NODE`, writes it when it creates it or with `FILL=1`, and
  detaches it when it ends;
- that another volume attached on `NODE` stops it, and that a write campaign on `NODE` needs the
  bench's volume deleted first (`docs/lab.md`);
- `POLL_US` and `FILL`, with the other settings.

**The files the next step touches** (R3-D):
- `scripts/lab.sh`: `cmd_bench_reads` (above); the wait shared with `write_volume`; the quoting
  in `release_lab_volume` and `cmd_vol attach`; `bench_summary`; the usage line. `valid_id`
  and `lost` move out;
- `scripts/lib.sh`: `valid_id` and `lost` (moved), `bench_volume`, `bench_attached`,
  `bench_isr`, and `bench_reference`'s under-90% message;
- `scripts/guest/node.sh`: `attached`, and `scripts/tools/node-scripts-test.sh`, its vectors;
- `Makefile`: `bench-reads` passes the `VOL` default, and its help says what "The words" list;
- `scripts/tools/bench-reference-test.sh`: the vectors above;
- `scripts/tools/lab-deletion-test.sh`: drop `valid_id` and `lost` from its `extract` list
  (line 21), since they move to `lib.sh`, which it already sources (line 14) (R7-A);
- `dataplane/crates/flaminio-io/src/node.rs`: S5's test (Queues);
- the documents marked *next step* (Documents).

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
- **Spread-out data:** the limit of 1.5× plus one region keeps the start of the range
  meaningful. A volume whose data is spread over the disk has no region that stands for it.
  That happens after many rewrites, when the store's cursor wraps.
- **A stray read near the data:** 64 MiB or more just before it moves `FIRST` earlier, with no
  refusal, as long as the range stays within the limit. An example is a 256 MiB chunk being
  resynced. `regions.txt` shows it.

**Rule 1 applies to raw's span too:** it must have been written. A never-written or discarded
range reads as zeros from the host's file, without touching the drive.
- The hole at 4.8–8.7 GiB holds other data of the store.
- The write campaigns' whole-disk write covered the rest.
- On fl-b, 0–16 and 0–21 GiB read alike, which agrees.

## Queues (S4, S5)

The bench's `QUEUES` stays 1, and so does the node's `--queues`. The node's usage text in
`flaminio-io/src/node.rs` gets the trade-off (S5). It also says that with several queues the
kernel gives the device no I/O scheduler, as it gives the raw disk.

**S5's test (next step, R1-C3):** a unit test in `node.rs`'s `tests`: `parse` with no
`--queues` gives `queues == 1`. It fails if the default changes. The usage text is checked by
reading it.

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

The bench, on fl-b, zero copy, one fio job, medians of 3 repetitions
([results](../../benchmarks/2026-10-29-reads-qd1-queues.md) and the `zero-copy-q1` and
`zero-copy-q4` summaries, recorded in 7afa35d):

| Job | 1 queue | 4 queues | Raw (1 queue's run, 4 queues' run) |
|---|---:|---:|---:|
| 4K random, QD32 | 230 100 IOPS, 98% | 203 266 IOPS, 86% | 235 786, 237 556 IOPS |
| 1M sequential, QD8 | 6 636 MiB/s, 100% | 6 632 MiB/s, 100% | 6 607, 6 603 MiB/s |

History: 2 runs each by hand, before the bench, gave 230–233k, 204–205k and 242–244k IOPS at
QD32, and 6 605–6 647 MiB/s at 1M on every side.

**From the bench's defaults to 4 queues,** at QD1, by IOPS (the bench).
- **The defaults** (one queue, `mq-deadline`): 13 097 IOPS, 76.4 µs per read.
- **4 queues** (the kernel picks `none` itself): 13 602 IOPS, 73.5 µs per read.
- **Against raw** (15 465 and 15 467 in the same runs): 84.7% to 87.9%.
- **The split, by hand:** the hand runs saved 3.1–3.5 µs per read (13.0k at 76.9 µs, and
  13.55–13.62k at 73.4–73.8 µs; the bench's runs differ by 2.8 µs). Of that:
  - **the scheduler,** about 1.2 µs: `none` set by hand on one queue gave about 1.5% more IOPS;
  - **the queue count,** about 2 µs.

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

**What 90% needs** (results, "What 90% needs without spinning"). In the bench, raw reads about
15.45k IOPS at QD1 (15 414–15 467). 90% of that is 13.9k, 71.9 µs per read.
- **With one queue** (76.4 µs): 4.5 µs less.
- **With 4 queues** (73.5 µs): 1.6 µs less. Trimming the user-space work (about 1 µs, intent)
  leaves about 0.6 µs, so it does not reach 90% alone.

History: against the 15.35k measured by hand, 90% was 72.4 µs per read, and the 4-queue gap
1.0–1.4 µs.

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
- **Spinning.** Both ways cost a core, which is why they are out of scope (intent).
  - **`--poll-us 100`, the bench:** 13 677 IOPS with one queue, 88.5% of 15 458; 13 904 with
    four, 90.2% of 15 414 (the `zero-copy-q1-poll100` and `zero-copy-q4-poll100` summaries).
    History: R2 (2026-10-10) measured 13 799 IOPS, which would have been 89.9% of the 15.35k
    measured by hand.
  - **While a disk read is in flight** (a prototype, not kept), by hand: 4 queues, scheduler
    `none`, 13.72–13.82k IOPS. That is 72.4–72.9 µs per read, 0.5–1.4 µs less than 4 queues
    alone (73.4–73.8 µs, another run). The raw runs beside it read 15.41–15.52k (over
    0–21 GiB), so that is about 89% (results).
- **Next candidates,** with what the figures say about each, against the gap above (4.5 µs
  with one queue, 1.6 µs with 4). They are not ranked: only one saving is measured, so no order
  by saving can be stated exactly (R2-C1).
  - **The node setting `none` on a one-queue ublk device:** about 1.2 µs, measured by hand. It
    applies to one queue only, and leaves 3.3 µs of 4.5.
  - **Trimming the user-space work per read:** about 1 µs, estimated from the profile
    (allocations, the executor, the engine's bookkeeping). It applies at every queue count, and
    leaves about 3.5 µs with one queue (about 2.3 µs with `none` too) and about 0.6 µs with 4.
  - **ublk with BPF** (completing simple reads in the kernel): not measured. It would skip both
    wake-ups and the user-space work. Of the kernel's part of a read, about 4.8 µs, raw pays only
    the disk's notification, about 1.1 µs. So up to about 3.7 µs of it is within reach, more
    than the 4-queue gap. It is larger than this issue.
  - **The lab disk's iothread,** with both sides measured again: it changes raw as well.

  Batched commands are not a QD1 candidate: at QD1 one request is in flight, so there is
  nothing to batch.
- **Before the next step measures anything** (R1-C5). The 2026-10-29 figures hold for the build
  they were measured on. Two plans change the read path:
  - #15: an engine thread that shares the engine lock with the queue threads, a generation check
    in the fast-read path, and no per-queue timer tasks. Its S12 measures the one-queue
    default only.
  - #75: a QoS check in `try_fast_read`, whose cost it argues but does not measure.

  So each of #15 and #75, when it lands, re-measures in its own lab check QD1 with `QUEUES=1`
  and `4`, each with `POLL_US=0` and `100`, on a `bench-` volume (S7), and names its commit.
  The next step of #9 starts from the latest of those rows. #74 does not move them: zero copy
  stays in direct mode.
- **Pending in #15's plan** (R2-B3). Its `make bench-ab` reads "first, on `vol1`". S7 gives no
  bypass, so those reads move to a `bench-` volume. If `bench-ab` reuses `cmd_bench_reads`,
  the guard applies; if it does not, it calls `bench_volume` itself. Its writes then run on
  `wbench`, and `write_start` stops on a `bench-` volume with replicas on fl-b. So `bench-ab`
  deletes its `bench-` volume (`delete_volume`) before its writes. #15's plan does not say
  this yet; this plan does not edit it.

## Documents

Pull request #28 made the changes below, from the results to the issue's comment. Those marked
*next step* were settled after approval, in plan review round 1 (decisions).

- **Results:** `docs/benchmarks/2026-10-29-reads-qd1-queues.md`. It holds:
  - the measurements above, and the bench runs;
  - the data in `docs/benchmarks/data/2026-10-29-reads-qd1-queues/`;
  - with the data, what produced it (rule 7): the bpftrace programs, the fio command lines of
    the hand measurements, and the prototype's diff;
  - *next step* (R1-C5): the commit its figures were measured on, as the performance plan's
    W4.3 names `a64ef64`. The 2026-10-29 runs recorded none, and the results say only that
    (R2-N2). Every later bench row names its commit.
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
  bpftrace. *Next step* (R1-S1, R1-N1): with the Makefile's help and the usage line, it says
  what "The words" above list.
- *Next step* (R1-C4): **`docs/followups.md`, item 1.** "Reads meet 90% of raw at QD32 (95%)
  and 1M (≈ 100%) … only 4K QD1 reads (84%, #9) remain" becomes QD32 98%, 1M 100% and QD1 85%,
  against raw where the volume's data is, with a link to the results. These are the
  performance plan's Status cells.
- **The issue** stays open, with a comment linking the results. *Next step* (R1-C2, R2-C2):
  its "Done when" becomes the intent's closing condition: one full run without spinning, with
  the QD1, QD32 and 1M shares at 0.900 or more, unrounded, no row flagged unstable, and the
  end-to-end check passed.
