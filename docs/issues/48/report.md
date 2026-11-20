# Issue #48 — report

## Review rounds

### Plan review round 1 — f7db824

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code); plan-reviewer (design-security).

- **R1-C1 — High:** #74's hand-off is not taken: under `--privsep` every rule from S1 to S8 is false, and the plan never names #74 or the helper. Evidence: #74 design, Owned devices, outcome 4 (line 114), Requests, `add` (`fixed_flags`); #74 intent lines 33–37; #74 spec line 63; kernel 6.1.112 `ublk_drv.c:1933–1937`; #15 design, S10 "direct mode only".
- **R1-C2 — High:** the agent's loop takes the plugin's lock, which a stage or unstage holds while it waits on the kept device's own I/O. Evidence: `control/csi/node.go:28`, `:89–126`, `:135–139`, `:167–202`, `:210–214`; #15 design, S9, Locking.
- **R1-C3 — High:** a resumed device has a new lifetime tuple, and nothing says what #15's contract and #79 do with it. Evidence: #15 `device-lifetime-contract.md`; #79 design lines 131–134; #79 spec S7; #15 design, S2 and S9.
- **R1-C4 — Medium:** #83's fresh-process handoff is not followed. Evidence: #83 `fresh-process-contract.md`; design.md, Agent, deadline and races, and Contracts.
- **R1-C5 — Medium:** the spec was not updated with the three settled clarifications, and contradicts the design. Evidence: spec.md S5, S8; design.md, Root records; decisions.md, Settled at approval; adr-proposal.md, Consequences.
- **R1-C6 — Medium:** the start's classification ignores the kernel's device state, and libublk's `EBUSY` wait is outside #6's limits. Evidence: 6.1.112 `ublk_drv.c:137`, `:1015–1044`, `:1933–1934`; libublk 0.4.8 `ctrl.rs:2606–2624`; `flaminio-io/src/control.rs:33–35`.
- **R1-C7 — Medium:** no step-by-step crash argument for the resume itself. Evidence: 6.1.112 `ublk_drv.c:1918–1919`, `:1943`, `:1979–1983`; design.md, Tests, row S3–S5.
- **R1-C8 — Medium:** the resume's epoch rule is a second copy of #6's and #15's, outside `flaminio-repl`, and its injected bug duplicates theirs. Evidence: #6 design, S6 (`flaminio-repl/src/run_epochs.rs`, `restart_at_epoch`); #15 design, S11; AGENTS.md, Principles.
- **R1-C9 — Medium:** the contracts are named, not written (status and list lines, the attach's errors, `end-kept`, the record's grammar and compatibility, the stamp's thread). Evidence: design.md, Contracts and files; #15 design, S8 and S10; #74 S6.
- **R1-C10 — Medium:** S9 and S10 have no approach, the tests have no names, and the lab plan is not runnable. Evidence: `scripts/guest/node.sh:23–30`, `:44`; `scripts/lab.sh:624–629`; design.md, Tests and Lab.
- **R1-C11 — Medium:** the record names its volume by name only. Evidence: `flaminio-net/src/token.rs:23`; #15's lifetime tuple; design.md, Root records.
- **R1-C12 — Low:** "#12 plans a single data-plane process lock" is not in #12; the lock is #15's `flock`, and the design's other locks are not related to it. Evidence: #12 decisions line 69; #15 design, S10, One owner at a time.
- **R1-C13 — Low:** libublk's recovery does not need its JSON. Evidence: libublk 0.4.8 `ctrl.rs:1170–1174`, `:2007–2011`, `:549–558`; intent.md, fact 7.
- **R1-C14 — Low:** S4 is unconditional, S1 has a fallback. Evidence: spec.md S1, S4; #12 spec S4.
- **R1-C15 — Low:** the retry of a failed resume has no pace. Evidence: design.md, Agent, deadline and races ("New epoch on each retry is permitted").
- **R1-S1 — High:** a kept device is resumed after another target served the volume, or for another volume of the same name. Evidence: `meta/service/service.go:714`, `:743–745`; `meta/domain/domain.go:217–220`; `pkg/captoken/captoken.go:49–51`; spec.md S6, S8.
- **R1-S2 — High:** #74's hand-offs are not taken; under `--privsep` nothing of the design can work, and `end-kept` is the root path #74 rules out. Evidence: `docs/issues/batch-2026-11-12.md:226`, `:250`; #74 intent, Which step; #74 spec S3, S5, S7.
- **R1-S3 — Medium:** the recovery loop omits #83's fresh-process handshake. Evidence: #83 `fresh-process-contract.md`; #79 spec S1, S7.
- **R1-S4 — Medium:** S4 exits at once even when the attached devices cannot be kept. Evidence: spec.md S1, S4, Q5; #12 spec S4.
- **R1-S5 — Medium:** the bound does not end a crash loop that reaches Running. Evidence: design.md, Root records ("Refresh alive …"); #15 S5.
- **R1-S6 — Medium:** who ends a kept device when no data plane serves is not designed, and a live but stuck successor is never overruled. Evidence: `scripts/dev/csi-image.sh:11–14`; #15 design, S10 (R2-B6); design.md, Agent, deadline and races.
- **R1-S7 — Medium:** the resumed device's lifetime tuple changes under #15's contract and #79's ownership. Evidence: #15 contract, revision 5; #79 spec S2, S7; #15 design, S9.
- **R1-S8 — Medium:** the new inputs have no grammar or bound (record v2, the socket's lines, `end-kept`'s arguments). Evidence: #15 design, S8 and S10; #74 S6.
- **R1-S9 — Low:** the loop can mint epochs without limit. Evidence: design.md, Agent, deadline and races.
- **R1-S10 — Low:** the resume's epoch checks do not name #15's run mark, and the injected bug's target is unclear. Evidence: #15 S11; #6 design, S6.
- **R1-S11 — Low:** the host-mounted `/run/ublksrvd` is not needed and adds a root-trusted input. Evidence: libublk 0.4.8 `ctrl.rs:1170–1173`, `:1185–1188`, `:547–555`.
- **R1-S12 — Low:** "a resume ends within #6's limits" does not hold with libublk's wrapper. Evidence: libublk 0.4.8 `ctrl.rs:2606–2624`; `flaminio-io/src/ublk.rs:697–703`.
- **Nits:** N1 (60 s is the plugin's `DefaultControlTimeout`, an attach's limit is 50 s), N2 (the ADR's body still reads as a proposal), N3 (S7 should name #15's S1), and the security review's Nit ("#12's process lock").

Changes/resolution (made after the approval of 2026-11-13; decisions.md, "Changes after approval — plan review round 1 (2026-11-17)"):
- **R1-C1, R1-S2:** a new design section, "With #74's helper". The helper probes the features and sets both recovery flags at `add`; it writes root's record and holds #15's `flock`; at its child's exit it keeps a recoverable running device and exits; the next helper takes the device over by the record. New requests `kept`, `recover` (START_USER_RECOVERY, then the new pin and its descriptor) and `end-recovery` (the child's pid); a kept device is ended by `stop` and `delete`, with no root `end-kept`. `node.sh` gains `restart`, which never deletes; `down` runs `cleanup` in both modes. Tests named. The helper does not outlive its child (respawning needs `CAP_SETUID`/`CAP_SETGID` kept, against #74's S1 and S5): the device and root's record outlive both. Under `--privsep` the fields only the I/O process knows are in a note in its state directory. Changes #74 needs are listed as cross-plan items.
- **R1-S1, R1-C11:** the record gains `first-epoch` and `size`. New safety rule, continuity: a resume goes only with the recorded first epoch and at exactly #15's run mark plus one; a gap, another first epoch or another size ends the device. It is `RunEpochs::resume` in `flaminio-repl`, with the simulator check `kept device resumed after epoch E ran elsewhere`, the injected bug `resume_over_gap`, unit and engine tests, and a lab case (moved to fl-b and back). What a lost `Attach` reply costs is stated.
- **R1-C2:** the loop never takes the plugin's node lock; the data plane's registry settles a resume against a detach; `device()` leaves kept volumes to the loop. Go tests `TestResumeWhileUnmountBlocks` and `TestResumeSecondVolumeWhileUnmountBlocks`.
- **R1-C3, R1-S7:** a resumed device keeps its recorded lifetime tuple (the kernel's disk, so `rdev` and diskseq, stay; the instance and generation stay as recorded), so #79's record still unstages it. Lines for kept and resuming in `device-info`, `status VOLUME` and `list`; `detach-device` on a kept or resuming entry; no name-only detach from the plugin; the successor opens the device at the resume's ready step. S6's answer is changed. #15's contract needs a revision (cross-plan).
- **R1-C4, R1-S3:** the loop, `node-restart` and the engine test use #83's challenge, nonce and handoff, added by whichever of #48 and #83 lands second; a quarantined data plane gives no challenge; the refusal of a resume without a handoff has a test.
- **R1-C5:** S5 (quarantine, what its I/O does, `flaminio-io del`), S8 (no enforcer, the mounted id) and S11 (the older plugin) carry what was settled; the "needs clarification" sentences are gone; a quarantine test is named.
- **R1-C6, R1-S12:** the scan classifies by `GET_DEV_INFO` (QUIESCED kept, DEAD deleted, LIVE kept, its wait being the resume's `EBUSY` wait). That wait is cut at `ADD_DEVICE_LIMIT` through a vendored-libublk hunk, `start_user_recover_once`. Tests per state and for the deadline.
- **R1-C7:** a table of the resume's six steps: what is durable, what the kernel holds, what the next successor does. S6 says that a failure after the kernel was asked to recover ends the device. Fake-kernel and real-kernel tests named.
- **R1-C8, R1-S10:** the resume's admission is `RunEpochs::admit` through #15's `MarkEpoch`; the record's `epoch` is information only; `resume_keeps_epoch` is removed; the kept-restart scenario also runs with #6's `restart_at_epoch` and #15's `volatile_run_mark`.
- **R1-C9, R1-S8:** written: the record's block, its bound and its compatibility both ways (detach before a downgrade); the socket's lines with two new states and no new field; the new `recovery` line; each refusal of a resume; `cleanup --expired`'s output and exit status; the thread that stamps. The record's nonce, `end-kept` and the list's extra fields are cut. Parser tests on both sides.
- **R1-C10:** a section for S9 (where each time is taken, how the pause is measured) and S10 (`node-restart`, its variables, the unit and pod paths); every rule has tests with file and case; the lab is ten numbered steps with pass criteria and a restore.
- **R1-C12 and the security Nit:** one lock scheme, #15's `flock`; a start tries it for 5 s so that a running `cleanup` does not fail it.
- **R1-C13, R1-S11:** #12's `emptyDir` at `/run/ublksrvd` stays; intent's fact 7 is corrected.
- **R1-C14, R1-S4:** S4 applies while every attached device is recoverable; otherwise #12's S4 stays. #12's preStop hook and grace period are not changed. Tests named.
- **R1-C15, R1-S9:** a back-off per volume (100 ms doubled to 5 s), one `Attach` per step, none while a token is unused; `TestOneAttachPerBackoffStep` counts the calls.
- **R1-S5:** no alive stamp for 60 s after a resume, so a crash loop ends at its first exit's bound. Tests named.
- **R1-S6:** with no data plane, the agent runs `flaminio-io cleanup --expired` (added to the CSI image) under #15's `flock`; the scan and a deadline thread start before the store; with the lock held and the socket silent the agent logs and overrules nothing. Tests named.
- **Nits:** N1, N2, N3 and the security review's Nit are fixed.

Rules narrowed, each because a finding shows that it cannot hold as stated: S3's "only root reads it" (under `--privsep`; R1-C1, R1-S2); S4 (R1-C14, R1-S4); S6's answer, from this run's instance and a new generation to the recorded ones (R1-C3, R1-S7); S6's failed resume (R1-C7); S6's token, from "above the record's" to exactly the next epoch (R1-S1).

Risks named, not accepted: a lost `Attach` reply, an agent restart with an unused token, or (#83) a data-plane restart between the `Attach` and the attach ends the device; the kernel facts are read in 6.1.112 and 6.8.12 only, not in 6.6 or 7.0 (the design's Kernel check, an open item of the ADR); a data plane stopped as a whole enforces no bound; a quarantined device waits for the operator; a crash loop slower than 60 s per cycle repeats; a downgrade leaves kept devices waiting; the CSI image carries `flaminio-io`; under `--privsep` the I/O process's note is in its disk's trust class.

Cross-plan items (not edited; each made by whichever plan lands second): #74 (`add`'s flags, outcomes 4 and 5, Ownership, the three requests, the helper's record and `flock`, two libublk hunks, `node.sh restart` and `down`); #15 (a revision of `device-lifetime-contract.md` for the states `kept` and `resuming` and for "Compare boot/instance against this daemon"; the records directory and its `flock` under `--privsep`; the start trying the `flock` for 5 s); #79 (pin that revision; an unbound operation met after a restart reads `device-info` before it takes an epoch); #83 (none to its plan); #12 (the CSI image gains `flaminio-io`; `should_exit`'s new case); #6 (`RunEpochs::resume` and its bug; `ADD_DEVICE_LIMIT` bounds the `EBUSY` wait).

Nits left: none.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 2 — 2d0611a

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

- **R2-B1 — Medium:** `cleanup --expired` on a helper's record: one reading ends the kept device inside the bound. Evidence: design.md:334–337, :416–418, :448–450 (at `8bd2a53`); the test `cleanup_expired_ends_only_what_the_scan_ends`; lab steps 7 to 9 name no mode.
- **R2-B2 — Medium:** the helper's stamp at its child's exit moves the deadline (reopens R1-S5 under `--privsep`). Evidence: design.md:396–397, :407–413; spec.md S8.
- **R2-B3 — Medium:** the helper's `flock` on the one records directory breaks #74's kernel tests, and #48's own. Evidence: design.md:380–382, :470–475, :727–731; #74 design, Lab ("Then, with the node running").
- **R2-B4 — Low:** which thread sends the helper's DEL_DEV when it ends a kept device. Evidence: design.md:141–143, :414–420; #74 design, Owned devices, outcome 3; #74 S5.
- **R2-B5 — Low:** the crash table's step 4 relies on a kernel fact that is not cited, and that the cited kernels contradict. Evidence: design.md:208, :682–691; 6.1.112 `ublk_drv.c:1128`, `:1137`; 6.8.12 `:1536`.
- **R2-B6 — Low:** the scan decides by `ENODEV`, which the design says cannot be trusted. Evidence: design.md:17–19, :130; libublk 0.4.8 `ctrl.rs:1499–1520`.
- **R2-B7 — Low:** the helper's refusals for the new states are not written. Evidence: design.md:439–440; 6.8.12 `ublk_drv.c:2769–2789`.
- **R2-B8 — Low:** an exit between `alive` + 60 s and the resume + 60 s ends the device at once. Evidence: design.md:104–106, :269.
- **R2-B9 — Low:** the plugin's detach before #79, and root's `detach VOLUME` on a resuming entry. Evidence: design.md:44, :256–257; `control/csi/node.go:140`.
- **R2-B10 — Low:** S3's narrowing gives up more than the finding forces, and the spec names only half of it. Evidence: spec.md:71–74; design.md:135.
- **R2-B11 — Low:** a device returned with one of the two flags is never ended. Evidence: design.md:57–59, :136; #15 design, S10.
- **Nits:** N1 (spec S6, "once the kernel was asked"), N2 (the ADR's "only with a token of the agent's"), N3 (intent's want 4 and the ADR's "The record" lack the narrowings of S4 and S3), N4 (the test `sigterm_exits_at_once_when_every_entry_is_kept`), N5 (`recovery` under `--privsep` before the first `add`).

Changes/resolution (made after the approval of 2026-11-13; decisions.md, "Changes after approval — plan review round 2 (2026-11-17)"):
- **R2-B1:** one table, "Who reads which record", beside the scan's: `cleanup --expired` judges both forms of record by the scan's rows and leaves a kept device that is inside its bound; the mode-change rule is a node's or a helper's start only. Tests `cleanup_expired_keeps_a_helpers_device_inside_the_bound` and `cleanup_expired_ends_a_helpers_device_past_the_bound`; lab steps 7 to 9 run under the helper, step 9 also with `PRIVSEP=0`.
- **R2-B2:** the helper stamps, at its tick and at its child's exit, only a device that this child was serving; a device taken over and not yet recovered, and one in the middle of a recovery, is never stamped. Tests `child_exit_does_not_stamp_an_adopted_device`, `child_exit_during_recovery_does_not_stamp`, `a_crash_loop_under_the_helper_ends_at_the_first_exits_bound`.
- **R2-B3:** the records directory is an argument, `--records DIR` (default `/run/flaminio/ublk`), of the helper, the node and `cleanup`; each kernel test passes its own. Added to #74's cross-plan item, `make privsep-check` included.
- **R2-B4:** the helper ends a device with STOP_DEV and DEL_DEV on a cleanup thread that nothing waits for, as #74's outcome 4 (c). Test `ending_a_held_device_blocks_neither_the_spawn_nor_the_exit`.
- **R2-B5:** step 4's row and a new paragraph state what 6.1.112 and 6.8.12 do: a successor that dies there may leave its fetches, START_USER_RECOVERY then answers `EBUSY`, and the device ends at the bound. Item 7 of the kernel check; the kernel tests assert the safe outcome and print which the kernel gave; a case with a partly fetched queue.
- **R2-B6:** no row reads an errno: a failed `GET_DEV_INFO` removes the record only when `/sys/class/ublk-char/ublkc<id>` is absent, else the record is left and logged. Test `a_failed_get_dev_info_decides_by_sysfs`.
- **R2-B7:** the refusals are written, each with its TEXT and no kernel call (`kept DEV`, `not recovering DEV`, `not kept DEV`, `already recovered DEV`, `device limit`); four tests and the fuzz target's state.
- **R2-B8:** after a resume the stamp starts again once the deadline the device had has passed. A crash loop still ends at its first exit's bound; a device that was serving past that deadline is not ended by its next exit. S8 and the ADR say so. Tests `no_stamp_until_the_old_deadline_after_a_resume`, `an_exit_after_the_old_deadline_is_inside_a_new_bound`.
- **R2-B9:** `detach VOLUME`, by name, ends a kept entry and sets a resuming entry's detach flag; it is what unstage sends until #79 is in. `TestUnstageAfterResume` and `TestDetachDeviceWithThePreRestartTuple` are added by whichever of #48 and #79 lands second; `TestUnstageByNameAfterResume` and `detach_by_name_ends_a_resuming_entry` cover the time before.
- **R2-B10:** `dev` and `diskseq` are in the helper's record, read by the helper from sysfs, so a difference is a quarantine; S3 says that the I/O user writes the note and its successor reads it. The digest of the volume in `add` is not taken.
- **R2-B11:** the scan ends a device that has `UBLK_F_USER_RECOVERY` without the reissue flag. Test `one_recovery_flag_alone_is_ended`.
- **Nits:** N1 to N5 are fixed.

Rules narrowed: none anew; S3's narrowing of round 1 is made smaller (R2-B10).

Risks named, not accepted: the slow crash loop, reworded to the new stamp rule; a successor that dies during a recovery may leave the device unrecoverable until the bound (kernel check, item 7).

Cross-plan items (not edited; each made by whichever plan lands second): #74 (`--records DIR` for the helper, passed by each test of `tests/kernel.rs` and by `make privsep-check`; the new refusals' TEXT); #15 (`--records DIR` for the node and `cleanup`; its S10 decides by `ENODEV` in three places, which libublk's second try can hide on a kernel without legacy opcodes); #79 (the two Go tests that need its record and client).

Nits left: none.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 3 — 5518e4f

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

- **R3-B1 — Medium:** crash table step 4: the "resumed" case includes one where the next START_USER_RECOVERY makes the kernel dereference NULL. Evidence: design.md:230, :243–248, :775–776 (at `6ddc6bd`); 6.8.12 `ublk_drv.c:1371`, `:1536`, `:2694`, `:2699`, `:2725`, `:2740`; 6.1.112 `:1889`, `:1894`; libublk 0.4.8 `io.rs:2325–2336`; `flaminio-io/src/node.rs:140`, `:203`.
- **R3-B2 — Low:** the stamp after a resume: one case untested under the helper, and a window of up to 1 s in both modes. Evidence: design.md:103–112, :300, :435–439, :459–462, :533–536; spec.md S8.
- **R3-B3 — Low:** what the helper's table holds while the cleanup thread ends a device. Evidence: design.md:430, :470–476; #15 design, S10; #74 design, Owned devices.
- **R3-B4 — Low:** the cleanup thread of the helper's start is made before #74's capability drop. Evidence: design.md:466–469, :472–475; #74 design, The helper, steps 5–7; #74 spec S1.
- **R3-B5 — Low:** under `--privsep` a quarantined device is in no status, against S5. Evidence: design.md:621–623; spec.md:92–96.
- **Nits:** N1 (design.md:174–175, the last `ENODEV` of the scan's text), N2 (`start DEV` after `end-recovery` has no TEXT), N3 (`--privsep` and `--records`; DIR absolute), N4 ("neither recovery flag").

Changes/resolution (made after the approval of 2026-11-13; decisions.md, "Changes after approval — plan review round 3 (2026-11-17)"):
- **R3-B1:** the record gets a phase `recovering`, written before START_USER_RECOVERY is asked (under the helper, at `recover`, before the kernel call) and turned back to `running` when END_USER_RECOVERY has returned or when the try gives up on `EBUSY`; `alive` is not touched. The scan ends a matching device in that phase, so no second START_USER_RECOVERY is sent over a recovery that did not finish. The crash table's steps 3 to 5 read "Ended"; the scan's table, S2, S5, S6 and the ADR follow; at its child's exit the helper deletes a `recovering` device. Tests: `starting_recovering_and_stopping_are_ended`, `no_second_recovery_after_a_death_in_steps_3_to_5`, `a_try_that_gives_up_on_busy_turns_the_phase_back`, and the kernel tests now assert "ended" with no second START_USER_RECOVERY. Kernel check: item 2 names the `nr_queues_ready` test; item 7 records the fact as the reason. Round 2's risk (R2-B5) is now a stated rule.
- **R3-B2:** one table, "When `alive` is stamped", for both modes, with the tick's stamp and the helper's exit stamp in each case (added and serving; resumed, old deadline not passed; resumed, old deadline passed; taken over; recovering). After a resume the stamper's wait ends at the old deadline and it stamps then; the helper's exit stamp goes by the clock. Tests `child_exit_after_a_resume_inside_the_old_deadline_does_not_stamp`, `an_exit_right_after_the_old_deadline_is_inside_a_new_bound`, `the_stamp_comes_at_the_old_deadline_not_at_the_next_tick`; the crash-loop test has a child that dies after its resume.
- **R3-B3:** the entry stays in the table, in a state `ending`, from before STOP_DEV until DEL_DEV returns: `kept` answers `ok none`, `add` `error device limit`, `recover` `error not kept DEV`, the others `error not owned DEV`; the thread removes its own record, then the entry. A `recover` and the deadline settle under the table lock. Tests `add_while_an_end_is_in_flight_is_device_limit`, `a_recover_that_meets_the_deadline_is_refused_or_wins`, `the_ending_thread_removes_only_its_own_record`.
- **R3-B4:** the start classifies on the main thread and only marks the entry; the cleanup thread is made after #74's step 7, where the server thread is. Kernel test `threads_after_the_start_hold_only_sys_admin`.
- **R3-B5:** S5 is not narrowed: `kept` answers `ok quarantined DEV`, and the I/O process lists that id in `recovery`. Tests `kept_answers_quarantined_for_a_quarantined_record`, `a_quarantined_device_is_listed_by_recovery`.
- **Nits:** N1 to N4 are fixed.

Rules narrowed: none.

Risks named, not accepted: none new. Round 2's "a successor that dies during a recovery may leave the device unrecoverable until the bound" is withdrawn as a risk: the device is ended at the next start, by rule.

Cross-plan items (not edited; each made by whichever plan lands second): #74 (the `kept` reply's `ok quarantined DEV`; the table's `ending` state and its answers; the start's cleanup thread made after step 7, and `credentials_after_start` reading every thread; `flaminio-io node --privsep` refusing `--records`).

Nits left: none.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 4 — 69d93ad

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Earlier findings: R3-B1 to R3-B5 and Nits N1–N4 fixed; residues are R4-B1 and R4-B2.

- **R4-B1 — Low:** under `--privsep` the helper's deadline can end a device whose resume is under way, before `recover` or between two busy `recover`s, against S8's "the bound never cuts a resume under way". Evidence: spec.md S8; design.md, The bound and With #74's helper, The deadline.
- **R4-B2 — Low:** three rules have no test that can fail without them: the helper's stamp at the old deadline; the thread-capabilities test, which passes when the cleanup thread has already gone; the `ending` entry's answers for `kept`, `start`, `stop`, `delete` and `end-recovery`.
- **Nits N1–N4:** the phase through the `EBUSY` wait differs between the modes; what `error busy` and other `recover` failures do to the entry; which process the `successor_killed_*` tests kill; "the stamp is at most 1 s old" for a device resumed inside its old deadline.

Changes/resolution (made after the approval of 2026-11-13; decisions.md, "Plan review round 4 (2026-11-17): changes made after approval"):
- **R4-B1:** S8 is narrowed under `--privsep`, as the reviewer recommends: the bound holds off from a `recover` that the kernel accepted; a resume that reaches `recover` after the bound is refused (`error not kept DEV`) and the device is ended. Spec S8, design (The bound; The deadline) and decisions.md say so. Test `the_deadline_ends_a_device_between_two_busy_recovers`.
- **R4-B2:** `the_stamp_comes_at_the_old_deadline_under_the_helper` (fake clock, child alive); the thread-capabilities test holds `/dev/ublkb<ID>` open so the cleanup thread is in DEL_DEV when the threads are read; `requests_on_an_ending_entry_are_refused` (the five requests, no kernel call).
- **Nits N1–N4:** fixed, each a sentence.

Nits left: none.

Rule narrowed: S8, under `--privsep` only (R4-B1 shows the helper sees a resume first at `recover`). Risk named, not accepted: under `--privsep` a resume that starts before the bound and reaches `recover` after it ends the device.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 5 — 75316ba625c43ed64581957dec8f8b95bd2d6d2d

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Reviewed commit 5902b14. Nothing of Low or worse. One Nit: the helper's test of the stamp at
the old deadline did not say that the tick has not run, so it could pass by the tick. Fixed
in the commit named above ("the next tick not yet due"), with no further round, under the
batch's rule for Nits.

Status: clear. Approval remains separate; no approval is inferred.
