# Issue #9 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.
- Plan review round 1 (2026-11-14) fixes, made after approval: see below.
- Plan review round 2 (2026-11-14) fixes, made after approval: see below.
- Plan review round 3 (2026-11-14) fixes, made after approval: see below.
- Plan review round 4 (2026-11-14) fixes, made after approval: see below.
- Plan review round 5 (2026-11-14) fixes, made after approval: see below.
- Plan review round 6 (2026-11-14) fixes, made after approval: see below.
- Plan review round 7 (2026-11-14) fix, made after approval: see below.

Approved with the recommended answer to each open question (see Settled at approval). The plan's
review loop continues after approval; later fixes are recorded as changes made after approval.
Work on this issue merged before this approval was recorded (pull request #28); the issue is
still open. Approved before its plan review cleared (status: pending).

## Settled at approval

None: `spec.md` and `design.md` have no Open questions section, no numbered question and no
choice labelled Proposed.

## Changes after approval: plan review round 1 (2026-11-14)

Made after the approval of 2026-11-13, in round 1 of this batch's plan review (reviewed commit
e0c91cd). The user asked for unattended work that takes each reviewer's recommended fix; where a
finding offers options, the simplest safe one is taken. Code and lab scripts are not edited
here: the design states the change, and the next step makes it. No result or measurement is
added; figures are quoted from the
[results](../../benchmarks/2026-10-29-reads-qd1-queues.md) and their `data/*-summary.txt`, as
recorded in 7afa35d.

- **R1-C1 (Medium), the figures:** intent and design now quote the bench's figures: 84.7% and
  87.9% at QD1 with 1 and 4 queues; 88.5% and 90.2% with `--poll-us 100`; about 89% for the
  in-flight prototype; 71.9 µs per read for 90%, so 4.5 µs less with one queue and 1.6 µs with
  4. The hand and October values stay, labelled as history. Re-derived from them:
  - "trimming would come close" goes. Trimming about 1 µs leaves about 0.6 µs at 4 queues, so
    it does not reach 90% alone at either count;
  - the out-of-scope list is unchanged: spinning gives 90.2% at most, for a core;
  - the next candidates keep their order, now ranked by what each leaves of the gap: trimming
    (every queue count, about 0.6 µs left at 4); `none` (one queue only, about 2.3 µs left
    with trimming); ublk with BPF or batched commands (the kernel's part, about 4.8 µs, is the
    only part near the one-queue gap's size); the lab's iothread (it changes raw too).
    Superseded in round 2 (R2-C1): the ranking is dropped.
- **R1-C2 (Low), what closes #9:** intent, "What closes #9": `make bench-reads` on fl-b at 90%
  of raw or more for 4K QD1 without spinning (`POLL_US=0`, no in-flight spinning), at the run's
  own queue count. The plan's own stated goal (90% "without spinning") is the one taken, not
  the reviewer's other option, "at the node's defaults", which would narrow it. The issue's
  literal "Done when" is already met by the recorded `QUEUES=4 POLL_US=100` run (90.2%), only
  with polling, so that run does not close #9. The design's Documents add the issue's "Done
  when" amendment for the next step.
- **R1-C3 (Low), untested rules:** spec Tests add two hand stops for S2's first bullet: `range`
  with a `SPAN` twice the volume's (stops with "the traced read … failed"), and a bpftrace
  failure. For the second, the reviewer's example, a disk with no `/sys/class/block/<name>/dev`,
  never reaches bpftrace: `block_device` refuses a path that is not a block device, and a device
  node outside `/sys/class/block` fails at the `read` of that file, under `set -e`, before the
  trace (`bench-reads.sh`, `range`). So the stop uses a copy of the step whose tracepoint is
  misspelt, which bpftrace refuses. S5 gets a `parse` unit test in `node.rs` (the default is 1)
  and a reading of the usage text.
- **R1-C4 (Low), followups:** the design's Documents add `docs/followups.md` item 1, with QD32
  98%, 1M 100% and QD1 85% against the new reference, linking the results (next step).
- **R1-C5 (Low), the build measured:** the design's Documents say the results name the commit
  their figures were measured on; the 2026-10-29 runs recorded none, and the results say so
  rather than guess. "What is left" says that each of #15 and #75, when it lands, re-measures
  QD1 with `QUEUES=1` and `4`, each with `POLL_US=0` and `100`, in its own lab check, naming
  its commit, and that #9's next step starts from those rows. That is a change those two plans
  need; it is not made here.
- **R1-S1 (Medium), the bench writes any volume:** spec S7, design "Next step: the volume and
  local reads". Of the reviewer's options (write `VOL` only with `FILL=1`, or refuse a volume
  the bench did not create), the second is taken, by a name prefix: the bench takes only
  `bench-…` volumes, creates one when it is missing (with `NODE` preferred; round 2 checks the
  placement, R2-B2) and fills it. A prefix needs no state: unlike a marker kept in the VM, it
  holds on a new `NODE` and after a VM is recreated. CSI volumes are named `pvc-<uid>`. The
  first option would still detach and depose any volume named. The script-test vector the
  reviewer asks for is the refusal of a name that is not the bench's (`vol1`, `pvc-…`), the
  prefix's counterpart of the no-marker refusal. The spec's tests on `volb` stay as the record
  of 2026-10-29. S7 now refuses `volb`, so later runs, R1-C5's re-measurements among them,
  use a `bench-` volume. This narrows what the commit of #28 said ("VOL can be any volume"), which
  was no spec rule; the finding shows it is unsafe.
- **R1-S2 (Low), VOL unvalidated and unquoted:** the guard's first line runs `valid_id`
  (moved to `lib.sh` for the script test) before the run's directory and `release_lab_volume`;
  every remote use of `VOL` is quoted with `printf %q` (`release_lab_volume`, `cmd_vol attach`,
  the fill marker). Script-test vectors for invalid ids are added.
- **R1-S3 (Low), replicas instead of ISR:** the bench counts chunks whose `.isr` holds `NODE`
  (`chunks_in_isr`, in `lib.sh`, tested on records), and its refusal names that cause. S2 names
  it, and the under-90% message names first the reads going over the wire. `chunks_on` itself
  keeps counting replicas: `drain-test` uses it to check that no replica is left on a node,
  which an ISR count would get wrong.
- **R1-N1:** the design says that the usage line lacks `VOL`'s default and `POLL_US`, and the
  next step adds them, with `FILL`.
- **R1-N2:** intent item 1 is in the past tense: "Before #28, `make bench-reads` read …".
- **R1-N3:** intent says that `--poll-us` is a per-node setting, which with several volumes on
  a node (#15) spins every busy queue.

## Changes after approval: plan review round 2 (2026-11-14)

Made after the approval of 2026-11-13, in round 2 of this batch's plan review (reviewed commit
a01f2fb), taking the reviewer's recommended fixes and, where a finding offers options, the
simpler one. No result or measurement is added.

- **R2-B1 (Medium), the bench's volume and the lab's test volume:** spec S7 and the design's
  "Letting go".
  - The bench detaches its volume when it ends, from an `EXIT` trap set just before its attach,
    so an interrupted run lets go too. The trap's command is built when it is set, because
    `node` is a local.
  - When another volume is attached on `NODE`, the bench stops at its attach, where the node's
    refusal names that volume. Of the two options (detach it, as `release_lab_volume` does, or
    stop with the reason), stopping is the simpler. With `VOL` naming the bench's volume,
    lab.sh no longer knows the test volume's name. Corrected in round 3 (R3-A, R3-N1): a node
    restart before the attach dropped the other volume first. So the stop moves before any
    restart, through a new `node.sh attached`, and is no longer free of code. Only with that
    does the bench keep S7's rule that it moves no volume not named `bench-…`.
  - For writes, of the two options (`write_start` deletes `bench-` volumes, or the docs say to
    delete them first), the docs are taken: `write_start` already stops, naming the volume, and
    is not changed.
  - Spec Tests add `make bench-reads` then `make node-fault`, both passing, and the stop with
    `vol1` attached.
- **R2-B2 (Low), placement:** before `create` the bench waits until `NODE` is alive
  (`write_volume`'s loop, shared). After it, `chunks_in_isr` on the create's output must count
  every chunk, since a new chunk's ISR is its replicas. Otherwise the bench stops, naming the
  cause (placed without `NODE`, which is draining or not alive) and the fix
  (`VOL=… make vol-delete`). There is no separate draining check: the check after `create`
  catches every cause that makes placement drop `NODE`. The ISR refusal of a later run names
  that cause too. A script-test vector covers the check after `create`. S7 no longer states
  the preference as a guarantee.
- **R2-C1 (Low), the ranking:** dropped, as it cannot be stated exactly: only `none`'s saving
  is measured. The candidates are listed with what the figures say about each:
  - `none` leaves 3.3 µs at one queue;
  - trimming leaves about 3.5 µs at one queue and about 0.6 µs at 4;
  - ublk with BPF is not measured, and up to about 3.7 µs is within its reach, more than the
    4-queue gap.

  "None closes the gap alone" goes. Batched commands leave the QD1 candidates: at QD1 one
  request is in flight, so there is nothing to batch (intent, Out of scope).
- **R2-C2 (Low), how the 90% is read:** intent, "What closes #9". One full run without
  spinning, in which:
  - the QD1 share is the medians' ratio, unrounded, at 0.900 or more;
  - the QD32 and 1M shares are at 0.900 or more too;
  - no row is flagged `UNSTABLE`, and the end-to-end check passes.

  For QD32, of the options named (QD32 at or above its recorded baseline, or the performance
  plan's rule), the plan's rule is taken: its goal for reads (section 3) is QD32 and 1M at 90%
  or more, with QD1 kept there. The recorded baseline, 98% at one queue, is within a run's
  noise of a one-queue run (raw's QD32 spread was 2.2%). So a closing run at the defaults could
  miss it on noise alone. This narrows round 1's condition ("the run's own queue count"). The
  finding shows that, as stated, it could close #9 on a run whose QD32 (86% at 4 queues) misses
  the performance plan's goal. The queue count stays the run's own. The issue's "Done when"
  amendment (design, Documents) carries the same condition.
- **R2-B3 (Low), #15's reads on `vol1`:** the design's "What is left" records what is pending
  in #15's plan:
  - its `bench-ab` reads move to a `bench-` volume, as S7 gives no bypass;
  - it calls `bench_volume` itself if it does not reuse `cmd_bench_reads`;
  - it deletes that volume before its write phase, since `write_start` stops on it.

  #15's plan is not edited here.
- **R2-N1:** the bench's runs differ by 2.8 µs (76.35 − 73.52), not 2.9.
- **R2-N2:** the provenance sentence goes: the 2026-10-29 runs recorded no commit, and the
  results say only that.
- **R2-N3:** spec Tests add the runnable form, `VOL=bench-qd1 make bench-reads`. It checks the
  range its `regions.txt` shows, raw from that range's start, and a spread of 10% or less.
- **R2-N4:** the usage line says that `make` supplies the default, bench-reads. `scripts/lab.sh
  bench-reads` alone takes `lab.conf`'s `vol1`, and refuses it.
- **R2-N5:** intent item 4 says "a volume not named `bench-…`".

## Changes after approval: plan review round 3 (2026-11-14)

Made after the approval of 2026-11-13, in round 3 of this batch's plan review (reviewed commit
22fca47), taking the reviewer's recommended fixes. No result or measurement is added.

- **R3-A (Medium), a restart dropped the other volume:** `cmd_bench_reads` restarts `NODE` when
  its flags differ from the run's, before the attach. So the bench now asks `NODE` which volume
  it serves, after `release_lab_volume` and before any restart. It uses a new `node.sh
  attached`, which reads the control socket's `status`. Any volume stops the bench, naming it,
  and the bench never detaches it: S7 holds. Of the reviewer's two options, this is the one
  that keeps S7 true; the other made the restart an exception. The lab check runs with
  `MODE=verify` (the node's flags match) and with zero copy (they differ), so it does not
  depend on the node's flags.
- **R3-B (Low), the trap's scope:**
  - on the normal path the bench detaches its volume after the end-to-end check, then clears
    the trap (`trap - EXIT`), as lab.sh's other traps are cleared. A caller in the same
    process, such as #15's `bench-ab`, gets `NODE` back free;
  - the trap stays for failures and interruptions. Its ssh is bounded by `VM_SSH_OPTS`
    (`ConnectTimeout=10`, `ConnectionAttempts=3`: about 30 s to a VM that does not answer, as
    after `lost`). No new time-out is added;
  - a lab check interrupts a run during its warm-up and finds nothing attached.
- **R3-C (Low), reading the condition:** of the two options, `summary.txt` is taken over
  computing from `runs.tsv`, so that the condition is read from one file. In the next step
  `bench_summary` prints the share with two decimals, truncated rather than rounded, from the
  medians it computed. The line after the table gives the mode and `POLL_US`. S3 lists both,
  and the intent's closing condition reads them.
- **R3-D (Low), the untested placement path:** `chunks_in_isr` (a count) becomes `bench_isr VOL
  NODE`, which stops with the cause. It gives one message for chunks with no replica on `NODE`
  (placed without it: delete it with `VOL=VOL make vol-delete`), and another for chunks whose
  ISR leaves `NODE` out. One function serves both checks, after `create` and before the trace.
  The script test adds a partial-placement vector and checks the messages as strings, the
  `vol-delete` command included. The command is valid: `lab.conf` sets `VOL` only when it is
  unset, and a lab check runs it. The design lists the files the next step touches, with the
  Makefile's `VOL` default, which the Makefile does not set today.
- **R3-N1:** the round-2 entry for R2-B1 is reworded (above). Its claim that the bench keeps S7
  holds only with R3-A's fix.

## Changes after approval: plan review round 4 (2026-11-14)

Made after the approval of 2026-11-13, in round 4 of this batch's plan review (reviewed commit
325e11c), taking the reviewer's recommended fixes. No result or measurement is added.

- **R4-A (Low), `node.sh attached`'s grammar:** it accepts exactly `^ok none$` and
  `^ok (ID) /dev/ublkb[0-9]+$`, where ID is `volume_id`'s regex, and dies on any other answer.
  It prints `none-attached` or `volume ID`, so a volume named `none` cannot pass for no volume,
  and the id only once it has matched. `node-scripts-test.sh`, which already runs node.sh
  against a fake control socket, gets vectors for each form, a volume named `none`, an error,
  an empty answer, a bad id and a bad path.
- **R4-B (Low), the stop's command:** the stop moves into `bench_attached NODE VOL`, in
  `lib.sh`, so that its message can be checked as a string. The message names
  `VOL=V scripts/lab.sh vol detach <NODE's letter>`. There is no make target for it, and
  `vol detach` takes `VOL` from the environment and the node as its argument.
- **R4-C (Low), `bench_isr`'s edges:** a record with no chunks is refused, so it can no longer
  pass vacuously. The placement message no longer blames one cause: the volume was placed
  without `NODE`, or a drain or a failover moved its replicas since. A vector covers zero
  chunks.
- **R4-D (Low), the interrupted-run check:** the trap's detach is best effort, as the node's
  detach waits up to 30 s for the device. The check therefore polls `node.sh attached` for
  about 40 s, and asserts that it comes to `none-attached`.

## Changes after approval: plan review round 5 (2026-11-14)

Made after the approval of 2026-11-13, in round 5 of this batch's plan review (reviewed commit
a3bb571), taking the reviewer's recommended fixes. No result or measurement is added.

- **R5-B1 (Low), a stopped node:** `bench_attached` accepts `not running`, node.sh's word for
  its exit 3, as nothing attached, which was the reviewer's preferred option. The caller passes
  the output on for exit 0 and 3, stops on 255 as a lost VM, and stops on any other exit. A
  vector covers `not running`, and a lab check stops fl-b's node, then runs `make bench-reads`,
  which starts it.
- **R5-B2 (Low), the empty-answer vector:** the design says that each `node.sh attached` vector
  writes its answer after `fresh`. The empty answer is an `answers` file holding one blank
  line, because an empty file makes the fake server answer `error: no answer`.
- **R5-B3 (Low), a CSI volume in the stop:** the message's advice depends on the volume's name.
  For `pvc-…` it says to delete the pod that uses it, and names no lab command. For any other
  volume it says to unmount it if it is mounted, then gives the `vol detach` command, as
  node.sh's `down` advises for a mounted device. This is the simpler of the reviewer's options:
  it uses the name the bench already has, and needs no mount query. Both kinds are checked as
  strings.
- **R5-N1, NODE's letter:** none is derived. The message passes `NODE`'s name, such as `fl-b`,
  which `vms_of` accepts as well as the letter (`lib.sh`). VM names come from `lab.conf`, so a
  derivation such as `${node#fl-}` would assume the default names.

## Changes after approval: plan review round 6 (2026-11-14)

Made after the approval of 2026-11-13, in round 6 of this batch's plan review (reviewed commit
beb2a64), taking the reviewer's recommended fixes. No result or measurement is added.

- **R6-A (Low), the exit mapping untested:** `bench_attached` takes node.sh's exit code as a
  third argument, `RC`, and maps it, so that the caller maps nothing and the script test covers
  every case:
  - 255 calls `lost`, which moves to `lib.sh` as `valid_id` did;
  - 3 passes only with `not running`, and any other output with it refuses as unexpected;
  - 0 reads the answer;
  - any other code refuses with "NODE's node.sh attached failed (exit RC); its message is
    above".

  Vectors cover RC 0, 3 (with `not running` and with other output), 1 and 255. Of the
  reviewer's options, folding the mapping into `bench_attached` is taken over a separate
  function: one function, one set of vectors.
- **R6-N1:** the vector checks `Unmount it if it is mounted`, as the message is written, and
  the checks are case-sensitive.

## Plan review round 7 (2026-11-14): changes made after approval

Round 7 of this batch's plan review (reviewed commit `f1145d4`; reviewer: plan-reviewer
(design-code and design-security, one agent)).

- **R7-A (Medium):** moving `valid_id` and `lost` to `lib.sh` would break
  `scripts/tools/lab-deletion-test.sh`, which extracts both from lab.sh by name (line 21). The
  Files touched list now includes it: both leave its `extract` list, as it already sources `lib.sh`
  (line 14).
