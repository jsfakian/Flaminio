# Issue #48 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.

Approved with the recommended answer to each open question (see Settled at approval). The plan's
review loop continues after approval; later fixes are recorded as changes made after approval.
Nothing is implemented. Approved before its plan review cleared (status: pending).

- Plan review round 1 (2026-11-17) fixes, made after approval: see below.
- Plan review round 2 (2026-11-17) fixes, made after approval: see below.
- Plan review round 3 (2026-11-17) fixes, made after approval: see below.
- Plan review round 4 (2026-11-17) fixes, made after approval: see below.

## Settled at approval

- **Q1. Who takes the new epoch:** (a), the agent identity's holder, today the CSI node plugin;
  the authorization rule stays and the plugin gains a recovery loop. [spec.md](spec.md), Open
  questions (S7); also [intent.md](intent.md), Decisions the spec will ask for.
- **Q2. The bound, and who ends a device past it:** 60 s from when its data plane was last
  alive, ended by the successor, or by the agent when no data plane runs. [spec.md](spec.md),
  Open questions (S8); [design.md](design.md), Agent, deadline and races (The bound).
- **Q3. Where the 2 s is promised:** as S9: a planned restart of the process, every ISR member
  reachable, on the lab's 16 GiB volume; the pod's replacement time is reported, not promised.
  [spec.md](spec.md), Open questions (S9).
- **Q4. Crashes too:** a crash is resumed like an upgrade, and the bound ends a crash loop.
  [spec.md](spec.md), Open questions (S2).
- **Q5. A kernel without the reissue:** (a), devices are added as today, and the log and status
  say so. [spec.md](spec.md), Open questions (S1); [design.md](design.md), Kernel capability
  and ownership.
- **Q6. A deletion of the data-plane pod while attached:** the data plane exits at once.
  [spec.md](spec.md), Open questions (S4).
- **A record whose device identity conflicts** (the S5 clarification): as the design proposes, a
  mismatching or incomplete record cannot prove ownership, so its device is quarantined and
  logged for operator cleanup, never deleted. [design.md](design.md), Root records and crash
  points, and Lab, documentation, risks and ADRs; [adr-proposal.md](adr-proposal.md), Decision.
- **The bound with no enforcer running** (the S8/S11 clarification): as the design proposes, no
  deadline is enforced while neither a data plane nor the agent runs; agent availability is a
  stated operational dependency, and an older agent is a rollout limitation (the plugin is
  upgraded first). [design.md](design.md), Agent, deadline and races.
- **Ending a mounted device** (the delete-bound clarification): as the design proposes, S8's
  bound covers ending service and the waiting I/O, not the kernel id's disappearance, which
  mounted openers may delay. [design.md](design.md), Agent, deadline and races.

## Changes after approval — plan review round 1 (2026-11-17)

Made after approval, in this round of the batch's review (reviewed commit `f7db824`; reports
of the design-code and design-security reviewers). Nothing here is approved anew, no risk is
accepted, and no test or lab result exists. The design's wording of the points settled at
approval changed with this round; their substance did not.

**Choices, per finding**

- **R1-C1, R1-S2 (High): #74's helper.** #48 designs the helper's part (the code report's
  option (a), the security report's (b)); the plan is not narrowed to direct mode. The helper
  probes the features and sets the two flags at `add`; it writes root's record and holds #15's
  `flock`; at its child's exit it keeps a recoverable running device; it gains `kept`,
  `recover` (START_USER_RECOVERY, then the new pin and its descriptor) and `end-recovery` (with
  the child's pid); a kept device is ended by `stop` and `delete`, with no root `end-kept`.
  - **The helper does not outlive its child.** The reports and the batch ledger name "a helper
    that outlives the I/O process". #74 itself asks only that recovery go through the helper
    and that the pin be closed before the recovery and taken again after. A helper that
    started a second child would have to keep `CAP_SETUID` and `CAP_SETGID` (against #74's S1)
    and to stop exiting with its child (S5), and a unit's stop or a pod's deletion ends the
    helper anyway. So the device and root's record outlive both processes, and the next helper
    takes the device over by the record. That one path serves every exit, and keeps #74's S1
    and S5 as they are. To be confirmed by the user (see the round's report).
  - **Under `--privsep` the record is in two parts:** root's, and a note of the I/O process in
    its state directory, since a volume id does not fit #74's 128-byte request. S3's "only
    root reads it" is narrowed to root's record there.
  - `node.sh` gains `restart`, which never deletes; `down` runs `flaminio-io cleanup` in both
    modes; `up` stays as #74 has it.
- **R1-S1 (High), R1-C11 (Medium): continuity.** The record holds the volume's `first-epoch`
  and the device's `size`. New safety rule: a resume goes only with the recorded first epoch
  and at exactly the run mark plus one. The run mark (#15) is the "last epoch this node's
  daemon durably saw", and it is written before each target starts, so no new field carries
  it. A gap, another first epoch or another size ends the device. The rule is
  `RunEpochs::resume` in `flaminio-repl`, with the simulator check `kept device resumed after
  epoch E ran elsewhere`, the injected bug `resume_over_gap`, unit and engine tests, and a lab
  case (moved to fl-b and back). A lost `Attach` reply then ends the device: named, not
  accepted.
- **R1-C2 (High): the loop's lock.** The loop never takes the plugin's node lock. The data
  plane's registry settles a resume against a detach, and `device()` leaves kept volumes to
  the loop. Go tests: an unmount blocked on a kept device, and the same with a second volume.
- **R1-C3 (High), R1-S7 (Medium): the lifetime tuple.** A resumed device keeps its recorded
  tuple: a recovery keeps the kernel's disk, so `rdev` and diskseq stay, and the instance and
  generation are kept as recorded. #79's record and rules need no change, and a stage made
  before the restart still unstages. This is the answer that asks least of the clear plans:
  the reviewer's rebind step would have #79 rewrite its record at each resume. #15's contract
  needs a revision for the two states and for one sentence (cross-plan). S6's "this run's
  instance and a new generation" is changed accordingly. `device-info`, `status`, `list` and
  `detach-device` are given for kept and resuming; the plugin sends no name-only detach; the
  successor opens the device at the resume's ready step.
- **R1-C4, R1-S3 (Medium): #83.** The loop, `node-restart` and the engine test use #83's
  challenge, nonce and handoff, added by whichever of #48 and #83 lands second, with the
  refusal's test. A quarantined data plane gives no challenge, so nothing is resumed until the
  operator restarts it; that restart is resumed as any other, which is #83's S5.
- **R1-C5 (Medium): the three clarifications** settled at approval are written into S5
  (quarantine: what its I/O does, and `flaminio-io del`), S8 (no enforcer, the mounted id) and
  S11 (the older plugin). The "needs clarification" sentences are gone. Intent's want 5 names
  the exception.
- **R1-C6 (Medium), R1-S12 (Low): the kernel's state and the `EBUSY` wait.** The scan
  classifies by `GET_DEV_INFO`: QUIESCED kept, DEAD deleted, LIVE kept without a wait, since
  the resume's `EBUSY` wait covers it. That wait is cut at `ADD_DEVICE_LIMIT` by a hunk in the
  vendored libublk (`start_user_recover_once`), so a resume stays inside #6's 50 s.
- **R1-C7 (Medium): the resume's steps.** A table: what is durable and what the kernel holds
  after each step, and what the next successor does. The `nr_queues_ready` rule holds in
  6.1.112 and 6.8.12; 6.6 and 7.0 are in the kernel check. *S6 is narrowed:* a failure after
  the kernel was asked to recover, or a kernel that refuses, ends the device. No `resuming`
  phase is recorded: the kernel's answer decides.
- **R1-C8 (Medium), R1-S10 (Low): one epoch rule.** The resume's admission is `RunEpochs::admit`
  through #15's `MarkEpoch`, unchanged; the record's `epoch` is information only. The
  mutation `resume_keeps_epoch` is removed; the kept-restart scenario also runs with #6's
  `restart_at_epoch` and #15's `volatile_run_mark`.
- **R1-C9, R1-S8 (Medium): contracts.** Written: the record's block and its compatibility both
  ways; `status`, `list` and `device-info` with two new states and no new field; one new line,
  `recovery`; each refusal of a resume; `cleanup --expired`'s output and exit status; the
  thread that stamps. *Cut:* the record's nonce, the list's `recoverable`, `last_epoch` and
  expiry fields, `end-kept`. A downgrade needs every volume detached first (S11).
- **R1-C10 (Medium): S9, S10, tests, lab.** A section for the six times and the command; a
  test per rule with file and case; the lab as numbered commands with pass criteria and
  restore.
- **R1-C12 (Low), the security report's Nit: the lock.** One scheme: #15's `flock` on the
  records directory. #12 plans no process lock, and the design no longer says so.
- **R1-C13, R1-S11 (Low): `/run/ublksrvd`.** #12's `emptyDir` stays; intent's fact is
  corrected; nothing reads libublk's JSON.
- **R1-C14 (Low), R1-S4 (Medium): SIGTERM.** *S4 is narrowed:* it applies while every attached
  device is recoverable; otherwise #12's S4 stays. #12's preStop hook and grace period are not
  changed: the hook ends with PID 1.
- **R1-C15, R1-S9 (Low): the pace.** A back-off per volume (100 ms doubled to 5 s), one
  `Attach` per step, none while a token is unused; a test counts the calls.
- **R1-S5 (Medium): the crash loop.** No stamp for 60 s after a resume, so a device whose
  server ends sooner keeps its first exit's deadline. One rule, no counter. A loop slower than
  that is named, not accepted.
- **R1-S6 (Medium): the ender.** With no data plane, the agent runs `flaminio-io cleanup
  --expired` (in the CSI image) under #15's `flock`, while the socket is silent. A start tries
  that lock for 5 s, so that a cleanup under way does not fail it. The scan and a deadline
  thread start before the store. With the lock held and the socket silent, the
  agent logs and overrules nothing: named, not accepted.

**Rules narrowed,** each because a finding shows that the rule cannot hold as stated: S3 (only
root reads it; R1-C1, R1-S2), S4 (R1-C14, R1-S4), S6's answer (R1-C3, R1-S7), S6's failed
resume (R1-C7), S6's token, from "above the record's" to the next epoch (R1-S1). S5's "differing
from its record" is quarantined, as settled at approval (R1-C5).

**Risks named, not accepted:** design, Risks.

**Nits.** N1 (60 s is the plugin's limit; an attach's is 50 s), N2 (the ADR's body reads as
the decision), N3 (S7 names #15's S1) and the security report's Nit (the lock) are fixed. None
is left.

## Changes after approval — plan review round 2 (2026-11-17)

Made after approval, in this round of the batch's review (reviewed commit `2d0611a`, the plan
as fixed at `8bd2a53`; one reviewer, both focuses). Nothing here is approved anew, no risk is
accepted, and no test or lab result exists. The take-over by record of round 1 stands: the
reviewer found it sound.

- **R2-B1 (Medium): `cleanup --expired` and a helper's record.** One table, "Who reads which
  record", says what each reader does with each form: `cleanup --expired` judges both forms by
  the scan's rows and leaves a kept device that is inside its bound; the mode-change rule is a
  node's or a helper's start only. Two tests; lab steps 7 to 9 run under the helper.
- **R2-B2 (Medium): the helper's exit stamp.** The helper stamps, at its tick and at its
  child's exit, only a device that this child was serving. A device taken over and not yet
  recovered, and one in the middle of a recovery, is never stamped, so R1-S5's property holds
  under `--privsep`. Tests: `child_exit_does_not_stamp_an_adopted_device`,
  `child_exit_during_recovery_does_not_stamp`,
  `a_crash_loop_under_the_helper_ends_at_the_first_exits_bound`.
- **R2-B3 (Medium): the tests beside the live node.** The records directory is an argument,
  `--records DIR`, of the helper, the node and `cleanup`; each kernel test passes its own, so
  it neither waits on the node's `flock` nor writes among its records. Added to #74's
  cross-plan item, `make privsep-check` included.
- **R2-B4 (Low):** the helper ends a device on a cleanup thread that nothing waits for, as
  #74's outcome 4 (c) sends its DEL_DEV. A test with a delete that blocks.
- **R2-B5 (Low):** the crash table's step 4 is worded by what the cited kernels do: a successor
  that dies there may leave its fetches, START_USER_RECOVERY then answers `EBUSY`, and the
  device ends at the bound. It is item 7 of the kernel check; the kernel tests assert the safe
  outcome and print which the kernel gave; a case with a partly fetched queue is added.
- **R2-B6 (Low):** the scan reads no errno. A `GET_DEV_INFO` that fails removes the record only
  when `/sys/class/ublk-char/ublkc<id>` is absent; otherwise the record is left and logged, not
  quarantined. A test with a kernel that answers `EOPNOTSUPP`.
- **R2-B7 (Low):** the helper's refusals for the new states are written, each with its TEXT and
  no kernel call (`kept DEV`, `not recovering DEV`, `not kept DEV`, `already recovered DEV`,
  `device limit`), with their tests and the fuzz target's state.
- **R2-B8 (Low):** after a resume the stamp starts again once the deadline the device had has
  passed, not 60 s after the resume. A crash loop still ends at its first exit's bound, and a
  device that was serving past that deadline is not ended by its next exit. S8 says so.
- **R2-B9 (Low):** `detach VOLUME`, by name, ends a kept entry and sets a resuming entry's
  detach flag. It is what the plugin's unstage sends until #79 is in. The two Go tests that
  need #79's record and client are added by whichever of #48 and #79 lands second; a test of
  the by-name unstage covers the time before.
- **R2-B10 (Low):** S3 is narrowed only as far as #74's request forces. The helper reads `dev`
  and diskseq itself and keeps them in root's record, so a difference is a quarantine, as in
  direct mode. The note carries only what the I/O process alone knows; S3 says that the I/O
  user writes it and its successor reads it. The reviewer's further option, a digest of the
  volume in `add`, is not taken: it would add a field to #74's request for no rule here.
- **R2-B11 (Low):** a device that the kernel returned with `UBLK_F_USER_RECOVERY` alone is
  ended by the scan, so "a restart ends it" holds. A test.
- **Nits, all fixed:** N1 (S6: "once the kernel began the recovery"), N2 (the ADR: the
  administrator's token resumes too), N3 (intent's want 4 and the ADR carry the narrowings of
  S4 and S3), N4 (the test's name says recoverable), N5 (`recovery` answers `unknown` under
  `--privsep` before the first `add`).

**Rules narrowed:** none anew. S3's narrowing of round 1 is made smaller (R2-B10).

**Risks named, not accepted:** the slow crash loop is reworded to the new stamp rule (a server
that outlives the device's old deadline, then ends, is resumed again); a successor that dies
during a recovery may leave the device unrecoverable until the bound (kernel check, item 7).

## Changes after approval — plan review round 3 (2026-11-17)

Made after approval, in this round of the batch's review (reviewed commit `5518e4f`, the plan
as fixed at `6ddc6bd`; one reviewer, both focuses). Nothing here is approved anew, no risk is
accepted, and no test or lab result exists.

- **R3-B1 (Medium): no second recovery.** The record gets a phase `recovering`, written before
  START_USER_RECOVERY is asked (under the helper, at `recover`, before the kernel call), and
  turned back to `running` when END_USER_RECOVERY has returned or when the try gives up on
  `EBUSY`. The scan ends a matching device in that phase, as it ends `starting` and
  `stopping`. So a successor that died in the middle of a recovery is never recovered again:
  in 6.8.12 a second START_USER_RECOVERY over one queue that is ready and one that never was
  puts a NULL task under the device's mutex. The crash table's steps 3 to 5, the scan's table,
  S2, S5 and S6, the tests (fake kernel and real) and the kernel check's items 2 and 7 follow.
  - **Round 2's risk becomes a rule.** "A successor that dies during a recovery may leave the
    device unrecoverable until the bound" (R2-B5) is no longer a risk named: such a device is
    ended at the next start, by rule. Round 1's choice of no recorded phase for a resume
    (R1-C7, "the kernel's answer decides") is withdrawn.
- **R3-B2 (Low): the stamp, stated once.** One table in Root records gives, for both modes, the
  tick's stamp and the helper's exit stamp in each case: added and serving; resumed with the
  old deadline not yet passed; resumed with it passed; taken over; recovering. The window is
  closed: after a resume the stamper's wait ends at the old deadline and it stamps then, and
  the helper's exit stamp goes by the clock. Tests named, the resumed-inside-the-old-deadline
  exit among them.
- **R3-B3 (Low): the table while a device is ended.** The entry stays, in a state `ending`,
  from before STOP_DEV until DEL_DEV returns, and holds the id: `kept` answers `ok none`,
  `add` `device limit`, `recover` `not kept DEV`, the others `not owned DEV`. The thread
  removes its own record, then the entry. A `recover` and the deadline settle under the table
  lock.
- **R3-B4 (Low): the start's cleanup thread** is made after #74's step 7, where the server
  thread is made; the start only classifies and marks. A kernel test reads every thread's
  capabilities.
- **R3-B5 (Low): quarantine under `--privsep`.** S5 is not narrowed: `kept` answers `ok
  quarantined DEV`, and the I/O process lists that id in `recovery`.
- **Nits, all fixed:** N1 (the last `ENODEV` of the scan's text), N2 (`start` after
  `end-recovery`: `already started DEV`), N3 (`--privsep` refuses `--records`; DIR is
  absolute), N4 ("without `UBLK_F_USER_RECOVERY`").

**Rules narrowed:** none. S2, S5 and S6 name the new case, which S6 already covered ("once the
kernel began the recovery, a failure ends the device").

**Risks named, not accepted:** none new. A SIGTERM or a crash in the middle of a resume ends
that device at the next start: that is the rule above, and it costs what a crash costs today.

## Plan review round 4 (2026-11-17): changes made after approval

Findings of the review of commit 69d93ad (one reviewer, design-code and design-security):
R4-B1 and R4-B2, both Low, and four Nits. Nothing here is approved by this record, and no
risk it names is accepted.

- **Under the helper the bound holds off from an accepted `recover`** (R4-B1). The helper
  learns of a resume only at `recover`, so it cannot skip a resume that is in its first two
  steps or between two busy `recover`s, as direct mode's deadline thread does. A resume that
  reaches `recover` after the bound is refused and the device is ended, as after a crash.
  Test `the_deadline_ends_a_device_between_two_busy_recovers`.
- **Tests that can fail** (R4-B2): the helper's stamp at the old deadline has its own test,
  with the child alive; the thread-capabilities test holds the ended device open, so the
  cleanup thread exists when the threads are read; `requests_on_an_ending_entry_are_refused`
  covers `kept`, `start`, `stop`, `delete` and `end-recovery`.
- **Nits,** each a wording: the phase through the `EBUSY` wait differs between the two modes
  on purpose (N1); `error busy` puts the helper's entry back to kept, any other failure does
  not (N2); the `successor_killed_*` tests kill the helper with its child (N3); the stamp's
  age at SIGTERM for a device resumed inside its old deadline (N4).

**Rules narrowed:** S8's "the bound never cuts a resume under way", under `--privsep` only,
because R4-B1 shows it cannot hold there as stated: within #74's boundary the helper sees a
resume first at `recover`.

**Risks named, not accepted:** under `--privsep` a resume that starts before the bound and
reaches `recover` after it (a slow start of the store, a fence that waits) ends the device,
where direct mode completes it.
