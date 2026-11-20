# Issue #49 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.

Approved with the recommended answer to each open question (see Settled at approval). The plan's
review loop continues after approval; later fixes are recorded as changes made after approval.
Nothing is implemented. Approved before its plan review cleared (status: pending).

- Plan review round 1 (2026-11-18) fixes, made after approval: see below.
- Plan review round 2 (2026-11-18) fixes, made after approval: see below.
- Plan review round 3 (2026-11-18) fixes, made after approval: see below.
- Plan review round 4 (2026-11-18) fixes, made after approval: see below.
- Plan review round 5 (2026-11-18) fixes, made after approval: see below.
- Plan review round 6 (2026-11-18) fixes, made after approval: see below.
- Plan review round 7 (2026-11-18) fixes, made after approval: see below.
- Plan review round 8 (2026-11-18) fixes, made after approval: see below.
- Plan review round 9 (2026-11-18) fixes, made after approval: see below.
- Plan review round 10 (2026-11-18) fixes, made after approval: see below.
- Plan review round 11 (2026-11-18) fixes, made after approval: see below.
- Plan review round 12 (2026-11-18) fixes, made after approval: see below.
- Plan review round 13 (2026-11-18) fixes, made after approval: see below.

## Settled at approval

- **Q1. What stands for "a build with an injected bug" in a lab check:** (a) now, a device that
  lies under the workload, in CI (S16); (b), the engine in process with a `flaminio-repl`
  switch on, not (c), with steps 1 and 2. [spec.md](spec.md), Open questions.
- **Q2. Our checker, or Jepsen's:** ours (S5). [spec.md](spec.md), Open questions.
- **Q3. Keep the fio load, or replace it:** keep both (S9). [spec.md](spec.md), Open questions.
- **Q4. An operation that fails while its target is alive:** a failed run (S10).
  [spec.md](spec.md), Open questions.
- **Q5. The README's "a chaos run … loses no byte":** reworded now (S19), and again once a run
  with the history check is recorded. [spec.md](spec.md), Open questions; also
  [design.md](design.md), Lab, docs, risks and ADRs.

## Changes after approval — plan review round 1 (2026-11-18)

Made after approval, in this round of the batch's review (reviewed commit `dde1d79`; one plan
reviewer, design-code and design-security focuses together). Nothing here is approved anew, no
risk is accepted, and no test or lab result exists. The points settled at approval (Q1–Q5) are
unchanged.

**Choices, per finding**

- **R1-C1 (High): a later segment opens with reads.** Only the run's first segment initializes
  each block with an acknowledged write. Every later segment first reads every block, each read
  answered with success before that segment writes the block; its answer is the deadline of the
  previous segment's unanswered writes. A later segment without those reads is undecided (S7),
  and the contract says how the checker recognizes them (Opening reads). S2's "each block's first
  operation is a write" is read per run, as the intent's What we want 1 has it: a clarification,
  not a narrowing. New tests: an acknowledged write in segment k with an older value read first
  in segment k+1 (a lost write), and S16's lost write at a segment change.
- **R1-C2 (High), R1-B2 (Medium): the kill under load, without a block trace.** Of the
  reviewer's two mechanisms, both are taken in their simplest joint form, since either alone
  leaves a gap: (b) alone puts an SSH round trip between "ready" and the kill; (a) alone leaves
  the paced client idle between bursts, so the burst it signalled can drain before the kill
  lands. The kill helper runs on the target's VM:
  SIGUSR1 to the run's client, which runs its bursts back to back (at most 1,000 bursts) and
  creates `kill-ready` once such a burst is issued; then the helper's kill mark, then SIGKILL.
  The evidence is the history's (`in_flight_at_kill` ≥ 1 per killed segment, else the run
  fails), which R1-B2 names as its first option: the tracefs capture is dropped, with its root
  prerequisite, so there is no trace instance to clean up or to clash with #84's.
- **R1-B1 (Medium): the heal ledger.** Named apart from `FLRUN001`, with its format and
  validation in contracts.md. Its only copy is on the workstation: "remote" is dropped (the
  reviewer's second option), since a workstation that lost its connection still has its ledger.
  The delay's netem gets a fixed handle, and `chaos.sh slow off` deletes only that qdisc.
- **R1-S1 (Medium): only its own volume.** New S9 bullet: `make chaos` refuses a `VOL` that is
  not a valid id or is named `pvc-…`, `bench-…` (#9) or `bc-…` (#84), before anything else; and
  #9's `node.sh attached` must name `VOL` on the VM before each client and fio job. The refusal
  list is the reviewer's, plus #84's `bc-` names, which #84's check creates. `node.sh attached` is
  #9's, as #9 defines it; whichever plan lands first adds it.
- **R1-C3 (Medium): ids carry their segment.** High 32 bits of `op_id` and of a write's value ID
  are the segment ID; the checker rejects another segment's. Test: two segments' clients on one
  file.
- **R1-C4 (Medium): one writer of End.** The client is stopped first; `flaminio-history end`
  takes the file's `flock` (refused while held), truncates a torn tail, appends End and fsyncs;
  `flaminio-history seal` writes `FLRUN001`. Late Returns are not kept (the reviewer's "or
  dropped"): with the client gone there are none. The spec's S4 is unchanged.
- **R1-C5 (Medium): kinds and faults.** The four kinds are defined as properties of the history
  (design, Kinds); any other violation is reported as a stale read, since every violation holds a
  read the search cannot place, so S8's four kinds stay four. `kind` is in the verdict JSON. The
  faults around a violation come from the harness's marks on the target's clock, context only.
- **R1-C6 (Medium): S10's mechanisms.** The kill mark, on the target's clock, is stored in End;
  an error Return earlier than it (or any, in a segment without a kill) fails the run. The kill
  mark is the kill's time on the operations' own clock: it decides S10's failure, not the verdict,
  so S11's "no verdict depends on them" holds. Progress since the last heal: two reads of the
  target's successful-answer count, ordered by the harness, the second larger.
- **R1-C7 (Medium): where S16 runs.** `scripts/dev/history-lies.sh` under `make contracts` (the
  reviewer's first option, beside `meta-interop.sh`), with the Go lie test behind a build tag that
  `make race` does not set; it fails when the checker is absent.
- **R1-C8 (Medium): the lab and #83.** As #84 settled for its check: on the lab VMs before #83
  lands, afterwards only on a host #83 admits, with no bypass.
- **R1-C9 (Low): `std` only.** The decoder, slicer and classification are a library module
  (`sim/history.rs`) with its tests; the example is a thin `main`. CRC32C is hand-written with
  RFC 3720's vectors. Neither of the reviewer's options for SHA-256 is taken: the manifest's
  sealed SHA-256 is replaced by the sealed length (cut before adding). The frames' and header's
  CRC32C, the length, and End being the last frame protect against accidental changes, which is
  all the hash claimed; nothing else in the checker needed SHA-256.
- **R1-C10 (Low): the Go tool's place.** `control/cmd/flaminio-history` and `pkg/blockhistory`,
  inside the gofmt roots; `go.mod` makes `golang.org/x/sys` direct; `history_build` and a copy as
  `meta_build`/`cmd_meta up`.
- **R1-C11 (Low): the header.** `volume_epoch` is dropped (no rule reads it); `--node` is a flag,
  context only; `--run-nonce` takes 64 lowercase hex digits.
- **R1-C12 (Low): the operation cap.** The per-run 100,000-operation cap is replaced by the
  artifact-size cap alone (the reviewer's third option, the one that adds nothing).
- **R1-C13 (Low): the faults reach the checked chunks.** New S9 bullet: at each round's start,
  every history block's chunk has its replicas on the target and the faulted node.
- **R1-C14 (Low): no missing segment.** Segment IDs are 1..n in order in `FLRUN001`, and the
  harness checks n against ROUNDS (+1 with the move back).
- **R1-C15 (Low): the test files.** Named per rule in the design's table, with `write-load.sh`'s
  default case in `scripts/tools/node-scripts-test.sh`.
- **R1-C16 (Low): the audit follow-up.** #76–#84 listed in the intent's Out of scope as later
  steps of #49, with #82's live-partition against power-off test and #83's qualified host.
- **R1-S2 (Low): no secret kept.** S14 says no kept or uploaded file holds a capability token, a
  key or a journal excerpt; the script test greps the kept files for the fixture's token.
- **Nits:** "Q1–Q5 remain proposed" now says they were settled; spec S3 names `CLOCK_BOOTTIME`;
  the design's frame bound is the contract's 136 bytes.

**Rules narrowed:** none.

**Risks named, not accepted**

- A kill can still land with nothing in flight (`in_flight_at_kill` zero): the run then fails, it
  does not pass; such a run proves nothing about reconciliation (R1-C2).
- A client that cannot be stopped after the kill (for example blocked in the kernel on the dead
  device) leaves its segment without End: the run fails, undecided (R1-C4).
- A stale read whose newer value is overwritten before any read is named a lost write: the kinds
  name what the history shows (R1-C5).
- The 128 MiB artifact cap fails a run with more than about 6 h of workload (R1-C12).
- Without a hash, evidence integrity rests on per-frame CRC32C and lengths: accidental changes
  only, as before (R1-C9).
- `make chaos`'s device check needs #9's `node.sh attached`; until either plan lands it does not
  exist (R1-S1).
- The issue's comment of 2026-11-12 could not be read here (the GitHub API lists no comment on
  #49); R1-C16's list follows the review and the clear plans #82 and #83 that name #49.

## Changes after approval — plan review round 2 (2026-11-18)

Made after approval, in this round of the batch's review (reviewed commit `b026fe8`; one plan
reviewer, design-code and design-security focuses together). Nothing here is approved anew, no
risk is accepted, and no test or lab result exists. Q1–Q5 and the spec are unchanged.

**Choices, per finding**

- **R2-C1 (Medium): signal the client, not its unit.** The reviewer's fix: once it holds its
  history file's `flock` the client writes `client-pid` in its segment directory; the kill helper
  signals that PID alone, after checking that it is live, that its `/proc/<pid>/exe` is the copied
  `flaminio-history` and that `/proc/locks` shows it holding the history file's lock. The helper
  no longer takes the unit. Given a unit name, `run_detached`'s poll also ends when the unit is
  inactive with no `.rc`, returning 1, a failure unless the harness stopped that unit (as at a
  kill). The SIGKILL at a kill and the SIGTERM before the move back still go to the unit: the
  first must reach every process, and the second ends with the unit inactive, which the new poll
  rule covers. Script tests: the fake wrapper survives SIGUSR1, only the client's PID is
  signalled, a PID that fails the checks is refused, a SIGKILLed unit without `.rc` ends the wait.
- **R2-C2 (Medium): what `in_flight_at_kill` counts.** The reviewer's definition: operations
  invoked before the kill mark with no successful Return (none, or an error answered after the
  mark); one answered with success after the mark is not counted. The Kill phase names the two
  windows per burst with nothing outstanding (each an Invoke batch's fdatasync: drain to
  checkpoint issue, checkpoint answer to next issue); a mark in one fails the run. New
  `sim/history.rs` cases (success after the mark: zero; error after it: one).
- **R2-C3 (Low): `--node`.** Validated with `valid_id`'s grammar (at most 128 bytes, the header's
  bound) instead of `^fl-[a-z]+$`, so a `lab.conf` override and #56's nodes are accepted;
  `main_test.go` accepts a name that is not `fl-…`.
- **R2-C4 (Low): enforcing the 128 MiB cap.** The reviewer's fix: the harness passes each client
  `--max-bytes` (128 MiB less the earlier segments' sealed lengths and the other kept files); the
  client reserves a batch's Invokes, their Returns, a Checkpoint and End before issuing it; the
  checker refuses, as undecided, a manifest whose sealed lengths total more than 128 MiB
  (contracts, Run manifest). A test for each: the workload test, the script test, `sim/history.rs`.
- **R2-C5 (Low): `make lint` runs the new script test.** The Touched list says `lint` runs
  `scripts/tools/chaos-history-test.sh`, as spec.md's Tests (S9–S15, `make lint`) has it.
- **R2-S1 (Low): `slow on` deletes no unrelated qdisc.** It adds (or replaces) the `49:` netem
  only when wg0's root qdisc is the kernel's default (handle `0:`) or already that netem; else it
  refuses, changing nothing, and the run fails as the fault is not confirmed. Script test case.
- **Nits (all three easy, fixed):** the first matching kind bullet names the kind (S8); a value
  whose write was invoked after the read was answered is reported as foreign content, an addition
  to S6; a ledger `intend` is healed by the next `healed` line of the same kind, VM and unit.

**Rules narrowed:** none.

**Risks named, not accepted**

- A kill mark in either of a burst's two fdatasync windows, or with every outstanding operation
  answered with success after the mark, gives `in_flight_at_kill` zero and fails the run; such
  runs may be more frequent than the round-1 text ("an instant") suggested (R2-C2).
- The kill helper's PID checks and its signal are separate steps: the checks narrow, not close,
  the window in which the PID could change (R2-C1).
- A run whose wg0 already carries a root qdisc other than the default or the `49:` netem cannot
  inject the delay and fails (R2-S1).

## Changes after approval — plan review round 3 (2026-11-18)

Made after approval, in this round of the batch's review (reviewed commit `ec75178`; one plan
reviewer, design-code and design-security focuses together). Nothing here is approved anew, no
risk is accepted, and no test or lab result exists. Q1–Q5 and the spec are unchanged.

**Choices, per finding**

- **R3-C1 (Low): SIGTERM to the client's PID, not its unit.** The reviewer's fix: before the move
  back, `chaos.sh stop-client`, given the segment directory, makes the kill helper's checks and
  sends SIGTERM to the `client-pid` PID alone, so the `run_detached` wrapper survives and writes
  `.rc`. The client exits non-zero when its recorder failed or an answer is still outstanding at
  the end of its 30 s drain; unless SIGKILLed, a client's `.rc` must be 0, or the run fails. The
  unit-level signal, and the poll's exit on a unit that wrote no `.rc`, are kept for the SIGKILL
  at a kill alone. This replaces round 2's "the SIGTERM before the move back still go[es] to the
  unit". Script test: `stop-client` signals only the PID; a SIGTERMed client exiting non-zero
  fails the run.
- **R3-C2 (Low): a SIGKILLed unit is `failed`.** The reviewer's fix: the wait at a kill, and
  `run_detached`'s poll, end on a unit that is not active (`inactive` or `failed`); the harness
  then runs `systemctl reset-failed` on the client's unit, as `chaos.sh kill-node` does for
  `flaminio-node`. The heal ledger's `client`/`fio` row is healed by the unit stopped and not
  active (`inactive` or `failed`), the same root cause (a heal after an interrupted kill would
  otherwise never confirm). The script test's fake reports `failed` after a SIGKILL.
- **R3-C3 (Low): what in flight means at a kill.** Of the reviewer's two options, the first, as
  the simpler (no new record, no issue time; cut before adding): an Invoke's `local ns` is
  stamped when its batch is formed, before its fdatasync and issue, and S9's in flight means
  invoked in the history. An operation whose Invoke was stamped before the mark, issued or not,
  with no successful Return counts in `in_flight_at_kill`. The window sentence is corrected:
  nothing is invoked and unanswered only from a drain's last Return to the checkpoint read's
  Invoke stamp and from that read's answer to the next burst's stamps. `sim/history.rs` cases:
  an unanswered operation stamped before the mark and issued after it gives one, one stamped
  after the mark gives zero. *Withdrawn in round 4 (R4-B1, R4-C2):* this meaning narrowed S9, and
  in flight now means issued (see round 4); those cases are replaced.
- **R3-C4 (Low): the 128 MiB cap.** Of the reviewer's two options, the first, by cutting the
  term whose value cannot be known: `--max-bytes` is 128 MiB less the earlier segments' sealed
  lengths, with no share for the other kept files, and 128 MiB is the segment files' cap, the
  contract's rule (its wording now says so). The other kept files (the heal ledger, the fault
  log, the seed, the verdict, the report, `FLRUN001`, fio's summary lines) get one stated bound
  together: 8 MiB, which the harness checks after its last write, failing the run and keeping
  the files when it is passed. Tests: the script test's `--max-bytes` case without the cut term,
  and a case for the 8 MiB bound; the checker's 128 MiB refusal is unchanged.

**Rules narrowed:** none in the spec's text. R3-C3 stated S9's in flight as invoked in the
history, which round 2's definition already counted; the kill's evidence is no stronger than
that (risk below). *Corrected in round 4 (R4-B1):* that meaning did narrow S9, whose in flight is
issued and unanswered; it is withdrawn and does not stand.

**Risks named, not accepted**

- Round 2's risk said a mark in either fdatasync window gives `in_flight_at_kill` zero; that was
  wrong. A mark during a batch's fdatasync counts that batch's operations as in flight though
  none was yet issued: `in_flight_at_kill` of at least one shows that an operation was invoked
  and not answered with success at the mark, not that the SIGKILL cut one issued at the device
  (R3-C3). Zero comes only from a mark between a drain's last Return and the next Invoke stamps.
  *Withdrawn in round 4 (R4-B1, R4-C2):* the risk followed from the withdrawn meaning, and its
  last sentence was false; round 4's risks replace it.
- A run whose kept files other than the segment files pass 8 MiB fails, though its history may
  be sound (R3-C4).

## Changes after approval — plan review round 4 (2026-11-18)

Made after approval, in this round of the batch's review (reviewed commit `32b9784`; one plan
reviewer, design-code and design-security focuses together). Nothing here is approved anew, no
risk is accepted, and no test or lab result exists. Q1–Q5 and the spec are unchanged.

**Choices, per finding**

- **R4-B1 (Medium): in flight means issued.** The reviewer's first option, which keeps S9 as
  written (the second, a spec change narrowing S9 and gap 4, would narrow an approved rule that
  can hold). After submitting a batch, and before reaping any of its completions, the client
  appends one Issued frame per operation (type 5, the issue time sampled after the submission
  returned; no fsync of its own). `in_flight_at_kill` counts writes issued before the kill with
  no successful Return; reads, the checkpoint reads included, and unissued operations do not
  count. Round 3's "in flight means invoked in the history" and its risk are withdrawn (marked so
  in round 3's section). contracts.md defines the frame (136 bytes, CRC32C as every frame) and
  the decoder's checks: an Issued for an unknown op, of another kind or block than its Invoke, a
  second Issued, an Issued after its Return, or a Return with no Issued is malformed. The byte
  budget reserves the Issued frames; 128 MiB is now about 300,000 operations, some 4 h. Tests:
  `sim/history.rs`, only unissued operations or reads unanswered at the kill give zero, and the
  four Issued malformations; the workload and recorder tests check the Issued frames' order.
  *Corrected in round 5 (R5-C2):* the client's I/O is blocking, with no submission to sample
  after; Issued is appended by each operation's goroutine just before its syscall (see round 5).
- **R4-C2 (Medium): the kill and its samples in one process.** The reviewer's fix: `chaos.sh
  kill-node` reads `flaminio-node`'s MainPID (`systemctl show`, a nonzero decimal) and runs
  `sudo flaminio-history kill-at --pid PID --exe … --unit flaminio-node.service`, root because
  `node.sh` starts the node as root. `kill-at` makes the checks round 2 makes of `client-pid`, in
  its own process before signalling: the PID is live, its `/proc/<pid>/exe` is `node.sh`'s
  `flaminio-io` and its `/proc/<pid>/cgroup` places it in the unit (`flaminio-io` alone also runs
  `del`); otherwise it sends nothing. Then it reads `CLOCK_BOOTTIME`, sends SIGKILL with
  `kill(2)` and reads the clock again. End holds both samples (offsets 40 and 80).
  `in_flight_at_kill` counts writes issued before the first sample with no Return, or an error
  Return after the second; a success after the second is not counted. S10's live-target errors
  are those earlier than the second sample, so an error between the samples is not left
  unclassified: it fails the run (conservative). The round-3 risk text is withdrawn. Tests:
  `sim/history.rs` with the samples further apart than every operation's latency;
  `main_test.go`, `kill-at`'s one-process order (clock, `kill(2)`, clock) and its refusals; the
  script test, the helper's order and a refused MainPID. *Changed in round 5 (R5-C1, Nit 3):*
  `kill-at` opens a pidfd first and signals with `pidfd_send_signal`, and gates the kill on an
  outstanding write in the history (see round 5).
- **R4-C3 (Low): a bound after `stop-client`.** The reviewer's fix: the harness waits up to 45 s
  (the client's 30 s drain and its exit) for `.rc`; past it, it SIGKILLs the client's unit as at
  a kill, names the client and fails the run (S13). Script test: a fake client that ignores
  SIGTERM.
- **Nits (all three easy, fixed):** the `sim/history.rs` case that a history cannot express
  (stamped before the mark, issued after) is replaced by R4-B1's; the poll ends on a unit with no
  `.rc` in every case, and only after the SIGKILL at a kill is that not a failure; a run the 8 MiB
  check fails gets that reason appended to its report.

**Rules narrowed:** none. R4-B1 restores S9's in flight (issued and unanswered), which round 3's
meaning had narrowed.

**Risks named, not accepted**

- A kill while no write is issued and unanswered (during a burst's Invoke fdatasync, or the
  checkpoint read's fdatasync or I/O) gives `in_flight_at_kill` zero and fails a run on a sound
  build. Three of each back-to-back cycle's four phases are such windows; their share of the
  cycle, and so how often runs fail this way, is unknown until a lab run records it per segment
  (R4-B1). Shortening them (for example overlapping the next burst's fdatasync with this burst's
  writes) is not designed here. *Replaced in round 5 (R5-C1):* the kill is gated on an observed
  outstanding write, so these windows no longer give zero; round 5's risks name what remains.
- `kill-at`'s checks and its `kill(2)` are separate steps in one process: the PID could still be
  reused between them, in a window narrower than round 2's, not closed (R4-C2). *Withdrawn in
  round 5 (Nit 3):* `kill-at` opens a pidfd before its checks and signals through it, so a reused
  PID is never signalled.
- An error answered between the two kill samples fails the run as a live-target error, though the
  kill may have caused it; the window is the length of one `kill(2)` (R4-C2; now one
  `pidfd_send_signal`).
- `kill-at` runs as root, from the binary the lab user copied to the VM: no wider than that
  user's `sudo` today (R4-C2).
- The Issued frames take a third of each operation's bytes: the 128 MiB cap now fails a run with
  more than about 4 h of paced workload, not 6 h (R4-B1; R1-C12's figure).
- A SIGTERMed client that needs more than 45 s to exit is SIGKILLed and fails the run (R4-C3).

## Changes after approval — plan review round 5 (2026-11-18)

Made after approval, in this round of the batch's review (reviewed commit `659b0e0`; one plan
reviewer, design-code and design-security focuses together). Nothing here is approved anew, no
risk is accepted, and no test or lab result exists. Q1–Q5 and the spec are unchanged.

**Choices, per finding**

- **R5-C1 (High): the kill is gated on an outstanding write.** The reviewer's fix, not its
  alternative (overlapping fdatasyncs, which still leaves gaps). `kill-at` takes `--history`,
  the segment's history file, validated as the helper validates the segment directory (under the
  lab user's `~/.flaminio-lab/chaos-<h>/`, no dot, dotdot or empty component), opened read-only
  with `O_NOFOLLOW` and refused unless it is a regular file owned by `SUDO_UID`, since `kill-at`
  runs as root. It reads the header to find the frames, then polls the last 64 KiB of whole,
  CRC-good frames until some Write has an Issued frame and no Return, at most 10 s; past that it
  sends nothing and the run fails. Then it samples, signals and samples as before. Round 4's
  risk of the three windows giving zero is marked replaced. Tests: `main_test.go`, a fixture
  history with no outstanding write waits and refuses at the bound, sending nothing, one with an
  outstanding write kills, a torn or CRC-bad last frame is not read as an Issued, and the
  `--history` refusals; the script test's helper order includes the gate. *Changed in round 6
  (R6-B1, R6-S1, Nits 1 and 2):* the file must be owned by uid 0, as the client runs as root; it
  is opened with `openat2` and `RESOLVE_NO_SYMLINKS`, non-blocking; the run tree's home comes from
  `SUDO_UID`'s passwd entry; the poll reads the last 481 whole frames, not 64 KiB.
- **R5-C2 (Medium): Issued for blocking I/O.** The reviewer's fix (not `io_uring`, which
  `x/sys/unix` does not wrap): the client's I/O is blocking O_DIRECT `Pread`/`Pwrite`, one
  goroutine per operation; after its batch's fdatasync each goroutine samples the clock, appends
  its Issued frame through the recorder and enters the syscall directly. The stamp is the last
  point before the syscall, not after the issue; the contract says an Issued operation may not
  yet have reached the device for that gap. "Before any Return of its batch" is dropped (another
  goroutine's Return may come first); the decoder's rules, which name only the operation's own
  Return, are unchanged. The workload and recorder tests now check: Issued before its operation's
  syscall, after its batch's fdatasync, and before its Return. `kill-ready` is created once every
  operation of the first back-to-back burst has its Issued frame. Round 4's R4-B1 text is marked
  corrected.
- **R5-C3 (Low): the decoder rules' tests.** The `sim/history.rs` list adds an Issued whose kind
  or block differs from its Invoke's, an Issued with a nonzero field it must leave zero, and an
  End with one kill sample 0 and not the other or the second earlier than the first, each
  undecided.
- **Nits (all three easy, fixed):** (1) the tool's copy is `$HOME/flaminio-history`, and every
  remote call, `sudo` included, names it by that absolute path; (2) the contract's table says
  offsets 40, 48 and 80 are zero for Issued, and the Issued paragraph lists the fields it leaves
  zero; (3) `kill-at` refuses, before any `/proc` read, a `--pid` that is not a strict positive
  decimal greater than 1 (at most `PID_MAX_LIMIT`), matches the unit's whole cgroup line
  (`0::/system.slice/<UNIT>`), not a substring, and uses a pidfd: `pidfd_open` first, then the
  `/proc` checks, then `pidfd_send_signal`. A process that exited after `pidfd_open` is not
  signalled, and `kill-at` refuses. Round 4's PID-reuse risk is marked withdrawn.
  `main_test.go` refuses `--pid` 0, -1, 1, `+5`, `05` and a non-number, and a cgroup line holding
  the unit only as a substring. *Corrected in round 6 (Nit 3):* at most 4,194,303
  (`PID_MAX_LIMIT` - 1).

**Rules narrowed:** none. S9's in flight stays issued and unanswered; the gate only chooses when
the kill is sent.

**Risks named, not accepted**

- A write the gate saw as outstanding can still be answered with success before the kill takes
  effect: its Return may not yet be appended when the gate reads, or the target may answer it
  between the gate's last read and the SIGKILL. If it was the segment's only outstanding write,
  `in_flight_at_kill` is zero and a sound run fails. The window is the recorder's append latency
  plus microseconds, against a replicated write's milliseconds; how often it happens is unknown
  until a lab run records it per segment (R5-C1). *Narrowed in round 6 (R6-C2):* the append
  latency is now one unbuffered `write(2)`, made right after the syscall returns.
- A kill phase in which `kill-at` sees no outstanding write within 10 s fails the run, sending
  nothing (R5-C1).
- Issued marks the last point before the syscall: a write counted in `in_flight_at_kill` may not
  yet have reached the device when the kill came, for the gap between its goroutine's append and
  its syscall's entry (scheduling, at worst a goroutine preempted there). Such a write shows that
  the client was about to issue it, not that the SIGKILL cut it at the target (R5-C2).
- `kill-at` reads, as root, a file the lab user controls: it opens it with `O_NOFOLLOW`, accepts
  only a regular file owned by that user, reads a bounded header and tail, and acts on no byte
  whose frame's CRC fails; a hostile file can at most make it send the kill or refuse (R5-C1).
  *Corrected in round 6 (R6-B1, R6-S1):* the history is root's, so `kill-at` accepts only a
  regular file owned by uid 0, and its open blocked on a FIFO; round 6's risk replaces this one.

## Changes after approval — plan review round 6 (2026-11-18)

Made after approval, in this round of the batch's review (reviewed commit `a08bad2`; one plan
reviewer, design-code and design-security focuses together). Nothing here is approved anew, no
risk is accepted, and no test or lab result exists. Q1–Q5, the spec and the contract are
unchanged.

**Choices, per finding**

- **R6-B1 (High): the client's uid.** The client runs as root through `sudo`, as `write-load.sh`
  runs `sudo fio` (`/dev/ublkb1` is `root:disk`): its unit runs `sudo "$HOME/flaminio-history"
  run …`, so its history, `client-pid` and `kill-ready` are root's (uid 0, mode 0600) in the
  lab user's segment directory. `kill-at`'s gate accepts only a regular file owned by uid 0, the
  simpler of the reviewer's two owner rules (the other, the uid of the PID holding the lock,
  needs `kill-at` to parse `/proc/locks` too). It still refuses a file the lab user planted: that
  file is the user's. A root-owned file in the run tree is one root made in the lab user's
  directories or one that user can already read and write: no component is a symlink (R6-S1),
  `fs.protected_hardlinks = 1` (systemd's default) stops a hard link to a root file the user
  could not read and write, the user cannot make a device node, and `kill-at` prints nothing it
  reads from the file. Every command that reads or ends the history runs with `sudo`: `end`,
  `status`, `kill-at`, the kill helper's `client-pid` read, checks and SIGUSR1, `stop-client`'s
  checks and SIGTERM, and the collection (`sudo cat`). Tests: `main_test.go`, a regular file
  owned by uid 0 accepted and one owned by `SUDO_UID` or another uid refused (the owner
  injected, as the test runs unprivileged); the script test, the client's unit runs `sudo
  "$HOME/flaminio-history" run` and the other commands named run with `sudo`.
- **R6-S1 (Low): `kill-at`'s open.** The reviewer's fix, its `openat2` option: flags
  `O_RDONLY|O_NONBLOCK|O_NOCTTY|O_CLOEXEC` with `RESOLVE_NO_SYMLINKS`, then `fstat`, so a FIFO
  does not block the open and no component is a symlink (`O_NOFOLLOW` guarded only the last).
  One call covers every component, where `O_NOFOLLOW` would need a walk. `main_test.go`: a FIFO
  and a symlinked directory component, each refused at once. Round 5's "a hostile file can at
  most…" risk is marked corrected and replaced below.
- **R6-C1 (Low): the copy is executable.** The reviewer's fix: `cat > ~/flaminio-history.tmp &&
  chmod 0755 ~/flaminio-history.tmp && mv ~/flaminio-history.tmp ~/flaminio-history`; the script
  test's fake `vssh` asserts the `chmod 0755`.
- **R6-C2 (Low): Returns are appended at once.** The reviewer's fix: when `Pread`/`Pwrite`
  returns, its goroutine samples the clock and appends its Return before anything else; the
  recorder buffers nothing, each Issued and Return being one `write(2)` of its own.
  `recorder_test.go`: a Return is in the file, read through another fd, before the next batch's
  Invoke fdatasync and with no later append. Round 5's append-latency risk is marked narrowed.
- **R6-C3 (Low): `kill-ready`'s test.** The reviewer's wording: in the kill phase, `kill-ready`
  appears only after SIGUSR1, once every Issued frame of the first back-to-back burst is in the
  file.
- **Nits (all three easy, fixed):** (1) the gate polls the last 481 whole frames (65,416 bytes)
  at offsets `header_end + 136k` up to the file's size, and refuses a header with a bad magic or
  CRC or a payload length over 4,096; (2) the run tree's home is the passwd entry's of
  `SUDO_UID`, not `~` or `$HOME` (root's under `sudo`), and a `SUDO_UID` missing, not a number,
  0 or with no entry is refused; (3) `--pid` is at most 4,194,303 (`PID_MAX_LIMIT` - 1).

**Rules narrowed:** none.

**Risks named, not accepted**

- The client, `end` and `status` now run as root too, from the binary the lab user copied to the
  VM, as `kill-at` does: no wider than that user's `sudo` today (R6-B1).
- `kill-at` reads, as root, a root-owned file in the lab user's directories: the lab user can
  still move one root-owned file of the run tree (another segment's history, say) to the path
  the helper checked, between the helper's `/proc/locks` check and `kill-at`'s open. It acts on
  no byte whose frame's CRC fails and prints nothing it reads, so such a file can at most make it
  send the kill or refuse; a FIFO or socket there is refused without blocking (R6-B1, R6-S1).
- `fs.protected_hardlinks` is systemd's default, not checked by `kill-at`; with it 0, the lab
  user could hard-link a root file on the same filesystem into the run tree, which `kill-at`
  would read with the same bound: send the kill or refuse (R6-B1).

## Changes after approval — plan review round 7 (2026-11-18)

Fixed by the orchestrator, taking the reviewer's fixes. No rule narrowed; no risk named.

- **R7-C1 (Low): who creates the segment directory.** The harness creates it as the lab user,
  without `sudo` (`mkdir -m 0700`), before the client's unit starts; `run` creates only its own
  files (exclusive creation), never a directory, and refuses a missing parent. So the helper's
  `kill-mark` write, run as the lab user, stays possible. Tests: the script test checks the
  directory is made without `sudo` before the unit; `main_test.go` refuses `run` with a missing
  parent.
- **R7-S1 (Low): the `SUDO_UID` refusals' tests.** `main_test.go` now also refuses a `SUDO_UID`
  that is empty or not a strict decimal (`1000x`, `-1`) and one with no passwd entry (the lookup
  injected), each before the open.
- **Nit (easy, fixed):** the blank line that ended the test table after the S9/S10 row is
  removed.

## Changes after approval — plan review round 8 (2026-11-18)

Fixed by the orchestrator, taking the reviewer's fixes. No rule narrowed; no risk named.

- **R8-B1 (Low): `run`'s exclusive creation.** `run`, as root in a directory the lab user owns,
  creates `history`, `client-pid` and `kill-ready` each with `O_CREAT|O_EXCL`, so an existing
  name or a planted symlink is refused, never followed. `recorder_test.go`: an existing file, or
  a symlink, at each of those names refused, the symlink's target unchanged.
- **Nit (easy, fixed):** the harness also creates the run directory `~/.flaminio-lab/chaos-<h>/`
  as the lab user without `sudo` (`mkdir -p`) before the first segment; the script test checks
  it. (Changed in round 9, R9-B1: before each segment, on that segment's VM.)

## Changes after approval — plan review round 9 (2026-11-18)

Fixed by the orchestrator, taking the reviewer's fixes. No rule narrowed; no risk named.

- **R9-B1 (Low): the run directory on each VM.** A failover moves the next segment to another
  VM, so the harness runs `mkdir -p` for the run directory before each segment, on that
  segment's VM, then `mkdir -m 0700` for the segment directory, both as the lab user without
  `sudo`. The script test fakes two VMs.
- **R9-B2 (Low): a refused `kill-ready`.** `kill-ready` is created in the kill phase, not at
  start; if that creation fails (`EEXIST` included) the client stops issuing and exits non-zero,
  and the run fails. `recorder_test.go`: `run` refuses an existing `history` or `client-pid` (or
  a symlink) at start; SIGUSR1 with `kill-ready` already present, a file and then a symlink,
  makes the client exit non-zero, the symlink's target unchanged.

## Changes after approval — plan review round 10 (2026-11-18)

Fixed by the orchestrator, taking the reviewer's fix. No rule narrowed; no risk named.

- **R10-B1 (Low): a refused `kill-ready` now fails the run.** The kill helper refuses if
  `kill-ready` exists before it sends SIGUSR1, and after its wait requires a regular file owned
  by uid 0; either failure sends nothing more and fails the run. At a kill, a `.rc` that is
  present and non-zero also fails the run, so a client that saw `EEXIST` is not excused. Script
  test: a `kill-ready` planted before SIGUSR1 is refused with nothing killed, one owned by the
  lab user after the wait is refused, and a client exiting non-zero before a kill fails the run.

## Changes after approval — plan review round 11 (2026-11-18)

Fixed by the orchestrator, taking the reviewer's fixes. No rule narrowed; no risk named.

- **R11-B1 (Medium): a correct kill must not fail on the client's exit.** Once it has created
  `kill-ready`, the client never exits on its own: at its first failed operation (the kill's
  errors) or after 1,000 bursts with no kill, it stops issuing, keeps its answers and the
  `flock`, and waits for the SIGKILL or SIGTERM. Round 10's rule becomes: at a kill, a `.rc` that
  is present (any value) means the client quit early and fails the run. Tests: `recorder_test.go`,
  after `kill-ready` a failed operation stops issuance and the client keeps running; the script
  test, a fake client exiting after SIGUSR1 and before the unit's SIGKILL fails the run (made
  exact in round 12, R12-B2), and one
  that gets errors after `kill-at` and keeps running passes.
- **R11-B2 (Low): the `kill-ready` checks do not follow symlinks.** Before SIGUSR1 the helper
  refuses on `sudo test -e PATH || sudo test -L PATH`; after the wait it requires
  `sudo stat -c '%F:%u' -- PATH` (no `-L`) to print `regular empty file:0` or `regular file:0`.
  Script test: a dangling symlink before SIGUSR1 and a symlink to a root-owned file after the
  wait, each refused.
- **Nit (easy, fixed):** the wait's failure conditions are named, and the pre-signal refusal
  "sends nothing".

## Changes after approval — plan review round 12 (2026-11-18)

Fixed by the orchestrator, taking the reviewer's fixes. No rule narrowed; no risk named.

- **R12-B1 (Medium): a recorder failure after `kill-ready`.** The one exception to round 11's
  "never exits on its own": a recorder failure makes the client exit non-zero at once, so its
  `.rc`, present at the kill, fails the run, and a write whose Return could not be recorded never
  passes as S9's evidence. Tests: `recorder_test.go`, a recorder failure after `kill-ready` exits
  non-zero; the script test, a fake client hitting a recorder failure after `kill-at` fails the
  run.
- **R12-B2 (Low): the "any value" test.** The script-test case is now a fake client that creates
  `kill-ready` and exits 0 after `kill-at`'s SIGKILL and before its unit's SIGKILL: it fails the
  run, which round 10's rule would not have done.

## Changes after approval — plan review round 13 (2026-11-18)

Fixed by the orchestrator, taking the reviewer's fix. No rule narrowed; no risk named.

- **R13-B1 (Low): the recorder-failure test covers the waiting state.** `recorder_test.go` has
  two cases, each exiting non-zero at once without waiting for a signal: a recorder failure while
  issuing after `kill-ready`, and one on a Return appended after the first failed operation, once
  issuance has stopped.
- **Nit (easy, fixed):** the rule now reads "never exits on its own except on a recorder
  failure", then the waiting behaviour.
