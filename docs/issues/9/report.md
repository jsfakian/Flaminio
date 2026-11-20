# Issue #9 — report

## Review rounds

### Plan review round 1 — e0c91cd

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code), plan-reviewer (design-security).

- **R1-C1 — Medium:** The plan's QD1 figures were the preliminary hand ones against 15.35k, and
  its reasoning rested on them. Evidence: docs/benchmarks/2026-10-29-reads-qd1-queues.md and its
  `data/*-summary.txt` (7afa35d) give 84.7%/87.9% at QD1 with 1/4 queues, 88.5%/90.2% with
  `--poll-us 100`, about 89% for the in-flight prototype, and a 4-queue gap of 1.6 µs (the plan
  said 1.0–1.4 µs, 89.9%, 89.4–90.0%, 88.3–88.7%).
- **R1-C2 — Low:** The plan did not say what closes #9, and the issue's literal "Done when" is
  already met by the `QUEUES=4 POLL_US=100` run (90.2%). Evidence:
  zero-copy-q4-poll100-summary.txt; intent.md:48-50.
- **R1-C3 — Low:** S2's first bullet (the trace cannot run) and S5 had no named test. Evidence:
  spec.md:18, 41-47, 72-76; node.rs:140 has no parse test of the default.
- **R1-C4 — Low:** docs/followups.md item 1 still shows QD32 95% and QD1 84%, and the design's
  Documents list left it out. Evidence: followups.md:13; performance-plan.md:22-23.
- **R1-C5 — Low:** #9's figures name no build, and #15 and #75 change the read path without
  re-measuring the 4-queue and polling rows. Evidence: #15 S12 (one queue only), #75 design
  (cost argued, not measured); performance-plan.md:95 (W4.3 names its commit).
- **R1-S1 — Medium:** The bench detaches, re-attaches at a new epoch, and may overwrite any
  volume named in `VOL`, a CSI volume included. Evidence: lab.sh:1511, 1519, 1535-1538; the
  per-node fill marker is missing on a new node or VM.
- **R1-S2 — Low:** `VOL` reaches remote shell commands unvalidated and unquoted, before any
  check. Evidence: lab.sh:1086, 713, 718, 1535, 1538; lib.sh:157; `valid_id` at lab.sh:1633.
- **R1-S3 — Low:** The local-read precondition counts replicas, not ISR members, so the
  under-90% refusal can blame the wrong cause and suggest an overwrite. Evidence:
  lab.sh:1257-1258, 1532; target.rs:670; lib.sh:237.
- **R1-N1 — Nit:** The usage line lacks `VOL`'s default and `POLL_US`. Evidence: lab.sh:2243.
- **R1-N2 — Nit:** intent.md:14 described the old rotation in the present tense. Evidence:
  lab.sh:1565.
- **R1-N3 — Nit:** `--poll-us` was called a setting for latency-sensitive volumes; it is
  per node. Evidence: node.rs:44-46; ublk.rs:1011.

Changes/resolution:
- R1-C1: intent and design quote the bench's figures, citing the results and summaries
  (7afa35d), with the hand and October values labelled as history. "Trimming would come close"
  goes (it leaves about 0.6 µs at 4 queues); the next candidates are re-derived, ranked by what
  each leaves of the gap, in the same order.
- R1-C2: intent gains "What closes #9": 90% of raw at QD1 by `make bench-reads` on fl-b without
  spinning (`POLL_US=0`), at the run's own queue count, as the plan's goal states. It records
  that the literal "Done when" is met only with polling (90.2%), which does not close #9; the
  design's Documents amend the issue's "Done when" in the next step.
- R1-C3: spec Tests add hand stops for a failed traced read (`SPAN` twice the volume's) and a
  bpftrace failure (a misspelt tracepoint: the reviewer's example stops before bpftrace runs,
  see decisions.md), and a `parse` unit test of S5's default with a reading of the usage text.
- R1-C4: the design's Documents add followups item 1 with QD32 98%, 1M 100%, QD1 85% and a link
  to the results (next step).
- R1-C5: the design's Documents have the results name their commit (the 2026-10-29 runs
  recorded none, and say so); "What is left" has each of #15 and #75 re-measure QD1 with
  `QUEUES=1/4` and `POLL_US=0/100` in its own lab check, naming its commit. Those plans need
  that change; it is not made here.
- R1-S1: new spec S7 and the design's next step: the bench takes only `bench-…` volumes,
  creates a missing one with `NODE` preferred and fills it; make defaults `VOL` to
  `bench-reads`; help, usage and lab.md say that the bench attaches and writes it. Script-test
  vectors refuse `vol1` and `pvc-…`; a lab check shows `VOL=vol1` stops before anything runs.
- R1-S2: `bench_volume` runs `valid_id` (moved to lib.sh) as the first line, before the run's
  directory and `release_lab_volume`; every remote use of `VOL` is quoted with `printf %q`;
  script-test vectors for invalid ids.
- R1-S3: the bench counts chunks whose `.isr` holds `NODE` (`chunks_in_isr`, with script-test
  vectors) and its refusal names that cause; S2 and the under-90% message name the reads going
  over the wire. `chunks_on` keeps counting replicas for `drain-test`.
- R1-N1: the design notes the usage line's gap; the next step adds `VOL`'s default,
  `POLL_US` and `FILL`.
- R1-N2: intent item 1 is in the past tense.
- R1-N3: intent calls `--poll-us` a per-node setting that, with several volumes on a node,
  spins every busy queue.

Status: needs_incremental_review. These changes were made after the approval of 2026-11-13 (decisions.md); no approval of them is inferred.

### Plan review round 2 — a01f2fb

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

- **R2-B1 — Medium:** The bench's own volume clashes with the lab's single test volume: a
  node serves one volume at a time, and the bench never detaches its volume, so later lab checks
  stop at their attach, and the bench stops when vol1 is attached; a write campaign stops on a
  `bench-` volume. Evidence: control.rs:94-95; lab.sh:1083-1088, 707-718, 1491-1590,
  2139-2154.
- **R2-B2 — Low:** Creating the volume trusted the preferred node: `domain.Place` drops a node
  that is not alive, draining or a witness, and the create follows a possible restart with no
  wait. Evidence: domain.go:366-368; service.go:398-400; lab.sh:781-786; write_volume at
  lab.sh:1878-1901.
- **R2-C1 — Low:** The re-derived ranking contradicted its numbers: ublk with BPF could save up
  to about 3.7 µs, more than the 4-queue gap, and trimming leaves the least only at 4 queues.
  Evidence: design.md:290-300; intent.md:109-111; decisions.md:35-38.
- **R2-C2 — Low:** "What closes #9" did not say how the 90% is read (the summary rounds to a
  whole percent), did not exclude an UNSTABLE run, and could close on a 4-queue run with QD32
  at 86%, against the performance plan's goal. Evidence: lab.sh:1619-1620;
  zero-copy-q4-summary.txt; performance-plan.md:31, 224.
- **R2-B3 — Low:** #15's measurement reads on vol1, which S7 refuses, and moving it to a
  `bench-` volume trips write_start; nothing records this where #15 will see it. Evidence:
  flaminio-15 design.md:950-951; design.md:308-309.
- **R2-N1 — Nit:** the bench's runs differ by 2.8 µs, not 2.9. Evidence: 1e6/13 097 − 1e6/13 602.
- **R2-N2 — Nit:** the provenance sentence guessed which bench produced the results. Evidence:
  7afa35d changed the bench in the commit that recorded them.
- **R2-N3 — Nit:** the spec's S1 lab check runs `VOL=volb make bench-reads`, which S7 refuses.
  Evidence: spec.md:94-102.
- **R2-N4 — Nit:** the usage line was to say `VOL (bench-reads)`, a default only the Makefile
  sets. Evidence: design.md:148, 162-163.
- **R2-N5 — Nit:** intent item 4 said "a volume it did not create"; S7 checks the name only.
  Evidence: intent.md:56; spec.md:67-69.

Changes/resolution:
- R2-B1: spec S7 and the design's "Letting go": the bench detaches its volume when it ends, from
  an EXIT trap built when set (`node` is a local). With another volume attached on NODE it stops
  at its attach, where the node's refusal names that volume, and does not detach it (simpler:
  existing behaviour, no code, and S7's rule). For writes, docs/lab.md says to delete the bench's
  volume before a campaign on NODE; write_start is unchanged. Spec Tests add `make bench-reads`
  then `make node-fault`, and the stop with vol1 attached.
- R2-B2: the bench waits until NODE is alive (write_volume's loop, shared) before `create`, then
  refuses unless `chunks_in_isr` counts every chunk of the create's output, naming the cause and
  `VOL=… make vol-delete`; the later ISR refusal names that cause too; a script-test vector;
  S7 no longer states the preference as a guarantee.
- R2-C1: the ranking is dropped, as only one saving is measured. The candidates are listed with
  what the figures say, BPF's unmeasured reach (about 3.7 µs) included. "None closes the gap
  alone" goes, and batched commands leave the QD1 candidates.
- R2-C2: one full run without spinning, with the QD1, QD32 and 1M shares at 0.900 or more as the
  medians' unrounded ratio, no UNSTABLE row, and the end-to-end check passed. QD32 follows the
  performance plan's rule (section 3), not the recorded 98% baseline, which a one-queue run
  could miss on noise. The issue's "Done when" amendment carries it.
- R2-B3: the design's "What is left" records the change pending in #15's plan: bench-ab's reads
  on a `bench-` volume through S7's guard, and its deletion before the writes. #15 is not
  edited.
- R2-N1: 2.8 µs. R2-N2: the provenance sentence goes; no commit was recorded. R2-N3: spec Tests
  add `VOL=bench-qd1 make bench-reads` with its own range. R2-N4: the usage line says `make`
  supplies the default. R2-N5: "a volume not named `bench-…`".

Status: needs_incremental_review. These changes were made after the approval of 2026-11-13 (decisions.md); no approval of them is inferred.

### Plan review round 3 — 22fca47

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

- **R3-A — Medium:** "The bench stops at its attach when another volume is attached" is false
  when the node restarts: `cmd_bench_reads` restarts NODE when its flags differ, before the
  attach, which drops the other volume's device; S7's rule breaks and the lab check depends on
  the node's flags. Evidence: lab.sh:1509-1515; node.sh:35, 44.
- **R3-B — Low:** The trap's scope and clearing were unspecified: lab.sh clears every other
  EXIT trap, a caller in the same process would keep the bench volume attached until exit, and
  the trap's ssh could wait on a dead VM. Evidence: lab.sh:843, 1125, 1184, 1468.
- **R3-C — Low:** The closing condition was not exactly computable from `summary.txt`: medians
  are printed as integers, and the mode and `POLL_US` are only in the directory's name.
  Evidence: bench_summary, lab.sh:1595-1625.
- **R3-D — Low:** The post-create check lacked a partial-placement vector and a check of the
  `VOL=… make vol-delete` command its message names; the Makefile's `VOL` default was not in a
  list of next-step changes. Evidence: design.md "Its tests"; Makefile:287-290; lab.conf:108.
- **R3-N1 — Nit:** decisions.md's R2-B1 entry claimed the bench keeps S7, true only after R3-A.
  Evidence: decisions.md, round 2.

Changes/resolution:
- R3-A: after `release_lab_volume` and before any restart, the bench asks NODE which volume it
  serves (a new `node.sh attached`, from the control socket's `status`). Any volume but `VOL`
  stops it, naming that volume, and the bench never detaches it, so S7 holds. Spec S7 says so,
  and the lab check runs with `MODE=verify` (flags match) and with zero copy (flags differ).
- R3-B: on the normal path the bench detaches its volume after the end-to-end check, then runs
  `trap - EXIT`. The trap stays for failures, and its ssh is bounded by `VM_SSH_OPTS`
  (ConnectTimeout=10, ConnectionAttempts=3, about 30 s). A lab check interrupts a run during
  its warm-up and finds nothing attached.
- R3-C: `summary.txt` is taken over `runs.tsv`. In the next step `bench_summary` prints the
  share truncated to two decimals, and the line after the table gives the mode and `POLL_US`.
  S3 lists both, and the intent's closing condition reads them.
- R3-D: `chunks_in_isr` becomes `bench_isr VOL NODE`, which stops with the placement cause and
  `VOL=VOL make vol-delete`, or the ISR cause. The script test adds a partial-placement vector
  and checks the messages as strings, and a lab check runs `VOL=bench-qd1 make vol-delete`. The
  design lists the files the next step touches, the Makefile's `VOL` default among them.
- R3-N1: the R2-B1 entry is reworded: the claim holds with R3-A's fix.

Status: needs_incremental_review. These changes were made after the approval of 2026-11-13 (decisions.md); no approval of them is inferred.

### Plan review round 4 — 325e11c

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

- **R4-A — Low:** `node.sh attached`'s grammar and ids were not specified: a volume may be named
  `none`, and an error or empty answer must not read as "nothing attached". Evidence:
  control.rs:104-107; common.sh:23 (CONTROL_SOCKET for script tests).
- **R4-B — Low:** The stop message's "Detach V first" named no runnable command: there is no make
  target, and `lab.sh vol detach` takes `VOL` from the environment and a node argument that
  defaults to `a`. Evidence: lab.sh:720-721; Makefile.
- **R4-C — Low:** `bench_isr` passed a record with zero chunks vacuously, and its placement
  message blamed one cause where a drain or failover may have moved replicas. Evidence: design
  step 7 (`get` path).
- **R4-D — Low:** The interrupted-run check could flake: an interrupted fio may hold the device,
  the node's detach waits up to 30 s, and the trap ignores failures. Evidence:
  control.rs:205-211.

Changes/resolution:
- R4-A: `node.sh attached` accepts exactly `^ok none$` and `^ok (ID) /dev/ublkb[0-9]+$` (ID by
  `volume_id`'s regex), prints `none-attached` or `volume ID`, and dies on anything else.
  node-scripts-test.sh gets vectors for each form, a volume named `none`, an error, an empty
  answer, a bad id and a bad path.
- R4-B: `bench_attached NODE VOL` (lib.sh) makes the stop, naming
  `VOL=V scripts/lab.sh vol detach <letter>`. The script test checks it as a string.
- R4-C: `bench_isr` refuses a record with no chunks (vector added). Its placement message names
  placement, a drain or a failover without blaming one.
- R4-D: the design calls the trap's detach best effort. The lab check polls `node.sh attached`
  for about 40 s and asserts `none-attached`.

Status: needs_incremental_review. These changes were made after the approval of 2026-11-13 (decisions.md); no approval of them is inferred.

### Plan review round 5 — a3bb571

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

- **R5-B1 — Low:** A stopped node unit made `bench_attached` refuse: `node.sh attached` prints
  `not running` (exit 3), which nothing mapped to "nothing attached". Evidence: node.sh
  `running()`, lines 95-99; design step 3.
- **R5-B2 — Low:** The empty-answer vector would pass for the wrong reason: the fake server
  answers `error: no answer` when `answers` is empty. Evidence: node-scripts-test.sh, the python
  server (lines 74-79) and `fresh` (line 112).
- **R5-B3 — Low:** The stop message advised `vol detach` for any volume, CSI `pvc-…` volumes
  included; node.sh `detach` has no mount check, so on a mounted device it would fail after
  30 s with the device half deleted. Evidence: node.sh:24-27 (the check is on `down`);
  control.rs:205-211.
- **R5-N1 — Nit:** How the message gets NODE's letter from `fl-b` was unspecified. Evidence:
  design step 3.

Changes/resolution:
- R5-B1: `bench_attached` accepts `not running` as nothing attached. The caller passes the output
  on for exit 0 and 3, stops on 255 as a lost VM, and stops on any other exit. A vector covers
  it, and a lab check runs `make bench-reads` with fl-b's node stopped.
- R5-B2: each `node.sh attached` vector writes its answer after `fresh`. The empty answer is an
  `answers` file holding one blank line.
- R5-B3: for `pvc-…` the message says to delete the pod that uses it and names no lab command.
  For other volumes it says to unmount first if mounted, then gives
  `VOL=V scripts/lab.sh vol detach fl-b`. Both are checked as strings.
- R5-N1: no letter is derived. The message passes NODE's name, which `vms_of` accepts, since VM
  names come from lab.conf.

Status: needs_incremental_review. These changes were made after the approval of 2026-11-13 (decisions.md); no approval of them is inferred.

### Plan review round 6 — beb2a64

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

- **R6-A — Low:** The caller's mapping of node.sh's exit code (0, 3, 255, other) sat in
  `cmd_bench_reads`, outside every script test. Only the lab check covered exit 3, no message
  was given for another exit, and exit 3 with other output was unsaid. Evidence: design step 3,
  "The call".
- **R6-N1 — Nit:** The vector checked `unmount it if it is mounted` while the message begins
  "Unmount it if it is mounted". Evidence: design.md, spec.md vectors.

Changes/resolution:
- R6-A: `bench_attached NODE VOL RC` maps the exit code itself, so the caller maps nothing:
  - 255 calls `lost`, which moves to lib.sh;
  - 3 passes only with `not running`, and other output with it refuses as unexpected;
  - 0 reads the answer;
  - any other code refuses with "NODE's node.sh attached failed (exit RC); its message is
    above".

  Vectors cover RC 0, 3 (both outputs), 1 and 255.
- R6-N1: the vector checks `Unmount it if it is mounted`, case-sensitively, as the message is
  written.

Status: needs_incremental_review. These changes were made after the approval of 2026-11-13 (decisions.md); no approval of them is inferred.

### Plan review round 7 — f1145d4

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

- R6-A and R6-N1 fixed.
- **R7-A — Medium:** moving `valid_id` and `lost` out of lab.sh breaks
  `scripts/tools/lab-deletion-test.sh`, which extracts both from lab.sh by name (line 21) and fails
  "not found"; the plan did not list that file.

Changes/resolution: the design's Files touched list adds `lab-deletion-test.sh`: both functions
leave its `extract` list, since it already sources `lib.sh` (line 14). Recorded in decisions.md,
round 7, after approval.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 8 — 0b109f302eba87281ee7d61d454ae5db4d25cc4a

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

No comments from the requested review scopes, including Nits.

Status: clear. Approval remains separate; no approval is inferred.
