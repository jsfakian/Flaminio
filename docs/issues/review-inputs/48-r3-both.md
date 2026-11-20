# Plan review: issue #48 (Live data-plane upgrade), round 3 (incremental), focuses design-code and design-security

Scope: `git diff 2d0611a..6ddc6bd -- docs/issues/48/` in `/home/jsfakian/Documents/src/flaminio-48`
(HEAD `5518e4f` adds only report.md). Checked against the code of that checkout, libublk 0.4.8,
the kernel trees the design cites (6.1.112 `eve-kernel`; 6.8.12, #74's tree), and the clear plans
#15, #74, #79 on `origin/main`. No 6.6 or 7.0 source was read. Line numbers of design.md are at
`6ddc6bd`.

Counts: Critical 0, High 0, Medium 1, Low 4, Nit 4.

## Earlier findings

- R2-B1 (Medium): fixed. "Who reads which record" gives each reader and each form one outcome;
  the two tests and the lab's mode are there.
- R2-B2 (Medium): fixed for an adopted device and for one mid-recovery. Residue: R3-B2.
- R2-B3 (Medium): fixed. `--records DIR`; #74's cross-plan item has it. Residue: Nit N3.
- R2-B4 (Low): fixed as asked (a cleanup thread nothing waits for). What it opens: R3-B3, R3-B4.
- R2-B5 (Low): partly fixed. The `EBUSY` cases are stated and are item 7. The row's "resumed"
  case still holds a kernel fault: R3-B1.
- R2-B6 (Low): fixed. One `ENODEV` is left in the text: Nit N1.
- R2-B7 (Low): fixed. One case without a TEXT: Nit N2.
- R2-B8 (Low): fixed, but for a window of up to 1 s: R3-B2.
- R2-B9 (Low): fixed. `detach VOLUME` on a kept and on a resuming entry agrees with #15's S8 and
  S9 and with `control/csi/node.go:140`; `dataplane.go:119` already accepts `is not attached
  here`.
- R2-B10 (Low): fixed. R2-B11 (Low): fixed; the next row's wording, Nit N4.
- Nits N1 to N4: fixed. N5: fixed, and it opens R3-B5.

**What holds** (asked for in the brief; no finding):
- **`cleanup --expired`** runs only with the `flock` free, so beside no node and no helper. By
  the rows it leaves a device inside its bound in both forms, and ends one past it. What it
  leaves past the bound is what the plan already names: a quarantined device, and a record
  whose `GET_DEV_INFO` failed while `ublkc<id>` exists, which the next run reads again.
- **The scan without errnos.** `ublkc<id>` exists from ADD_DEV to DEL_DEV's removal, and a
  deleted device that is still held answers `GET_DEV_INFO` (#15, S2). So "fails and absent"
  removes no record of a device that exists, and no record stays for ever.
- **`--records DIR`** is on root's command lines only (the unit's helper, `cleanup`, the tests).
  The child cannot change the helper's. #15's check (a directory, not a link, root's, 0700)
  cannot be met by a directory an unprivileged user fills. The node's and the agent's default
  stays `/run/flaminio/ublk`.
- **The refusals' TEXT** fits #74's grammar, its 128-byte requests and 256-byte replies.
- **The crash loop,** fast or slow, in direct mode: no stamp before the old deadline, so it
  ends at the first exit's bound; R3-B2 has the two residues.
- **The new kernel citations** (6.1.112 `:1128`, `:1136–1137`; 6.8.12 `:1536–1537`,
  `:2769–2790`) are at the lines given.

### R3-B1 — Medium
**Crash table step 4: the "resumed" case includes one where the next START_USER_RECOVERY makes
the kernel dereference NULL.**
- The row says: "Resumed where the kernel has released the dead successor's fetches and a queue
  was ready" (design.md:230). The paragraph under it and item 7 know only `EBUSY`
  (:243–248, :775–776).
- 6.8.12, a device with two or more queues, the successor dead when one queue has all its
  fetches in and another has sent none. libublk submits a queue's fetches together
  (`io.rs:2325–2336`), so a kill between two queue threads leaves exactly that.
  - The ready queue's commands are cancelled (`ubq_daemon` is set, `:1536` passes), the
    character device is released, `UB_STATE_OPEN` is cleared (`:1371`).
  - The next START_USER_RECOVERY passes every check: `nr_queues_ready` is 1 (`:2725`), not
    open, QUIESCED (`:2740`). It then runs `ublk_queue_reinit` on every queue. For the queue
    that never became ready `ubq_daemon` is NULL: `WARN_ON_ONCE` (`:2694`), then
    `put_task_struct(NULL)` (`:2699`), under `ub->mutex`.
  - So: an oops, or a panic with `panic_on_oops`. Without it, the device's mutex stays held:
    no later STOP_DEV or DEL_DEV returns, and the held I/O waits past every bound.
- 6.1.112 has the same lines (`:1889`, `:1894`) but does not reach them: there the fetches are
  never released (the design's own reading), so it answers `EBUSY`.
- The default is one queue (`node.rs:140`), where a queue is all or nothing; `--queues` goes
  to 64 (`node.rs:203`). Hence Medium, not High.
- The kernel tests cannot show this safely: a test of that case would oops the VM. The two
  named (:549–552, :669) are "all fetched" and "partly fetched".
- From memory, not checked here: the `nr_queues_ready` test that the step-3 row relies on
  (`EINVAL`) is itself a later fix of the same dereference. It is in 6.1.112 and in this
  6.8.12 tree; an early 6.6.y may lack it. Kernel-check item 2 should ask for it by name.

Fix: never send a second START_USER_RECOVERY over a recovery that did not finish. The record
gets a phase `recovering`, written before step 3 (under the helper, at `recover`, before the
kernel call) and turned back to `running` when END_USER_RECOVERY returns, or when step 3
gives up on `EBUSY`; `alive` is not touched. The scan ends a matching device in that phase, as
it ends `starting` and `stopping`. Steps 3 and 4 of the table then read "Ended", which is also
what S6 says ("once the kernel began the recovery, a failure ends the device"). It gives the
helper's write at `end-recovery` (:430–431) its purpose. Tests: a scan row case; on the fake
kernel, no second START_USER_RECOVERY after a death in steps 3 to 5; the kernel tests assert
"ended". Item 7 then records the fact as the reason.

### R3-B2 — Low
**The stamp after a resume: one case untested under the helper, and a window of up to 1 s in
both modes.**
- **The helper's exit stamp after a completed resume.** The rule is "only if the tick was
  stamping it", but its gloss is "that is, if this child was serving it" (:459–462). A child
  that finished its resume and dies inside the old deadline was serving, and the tick was not
  stamping. A stamp there moves the deadline at every cycle: R1-S5 again. No named test has
  that exit: the two `child_exit_…` tests and the crash loop's ("each child dying before or
  during its resume", :533–536) stop short of it.
- **The window.** Both stamps come from a 1 s tick (:103–112, :435–439). A device that is
  serving when its old deadline passes has a deadline in the past until the next tick. An exit
  in that second (SIGTERM "writes nothing", :300) is found expired, and the device is ended.
  The spec's "a device served past that deadline starts a new bound at its next exit" (S8) does
  not hold there. Under the helper the exit stamp does not close it: the tick was not yet
  stamping.

Fix: (1) the helper's exit stamp goes by the clock: written when the device was started by
this child, or recovered by it and the old deadline has passed at the exit; drop the gloss.
(2) After a resume the device thread's wait, and the helper's, ends at the old deadline, and
the stamp is written then. Tests: `child_exit_after_a_resume_inside_the_old_deadline_does_not_stamp`,
the crash loop with a child that dies after its resume, and
`an_exit_right_after_the_old_deadline_is_inside_a_new_bound` with the tick not yet run.

### R3-B3 — Low
**What the helper's table holds while the cleanup thread ends a device.** The thread may sit in
DEL_DEV with no bound, and "the record goes when DEL_DEV returns" (:472–476). Not stated:
whether the device is still owned meanwhile, and what `kept`, `add` and `recover` answer. The
helper has one id, so a later `add` writes its `starting` record (:430) at the path the
thread will remove.
- If the device counts as not owned, the `add` meets the old record under `O_EXCL`, or replaces
  it and removes it when ADD_DEV fails (#15, S10). An `add` that succeeds just after the
  release loses its record to the late thread; a kept device with no record is never ended.
- The deadline's end can also race a `recover` in flight: "unless a `recover` succeeded"
  (:470–471) does not cover one that the kernel is serving. The table lock is never held
  across a kernel call (#74).
- A kept device past its bound that a pod still has mounted is the usual case here.

Fix: the entry stays in the table, in a state `ending`, set under the table lock before
STOP_DEV, until DEL_DEV returns. Meanwhile `kept` answers `ok none`, `add` answers `error
device limit`, `recover` answers `error not kept DEV`; the thread removes the record and then
the entry. Tests: `add_while_an_end_is_in_flight_is_device_limit`,
`a_recover_that_meets_the_deadline_is_refused_or_wins`.

### R3-B4 — Low
**The cleanup thread of the helper's start is made before #74's capability drop.** The start
classifies "after the `flock` and before it spawns the child" (:466–469), and ends a device
on a cleanup thread then (:472–475). #74 runs steps 1 to 7 on the only thread because
capabilities are per thread, and drops to `CAP_SYS_ADMIN` at step 7 (#74 design, The helper;
spec S1: "the helper keeps only `CAP_SYS_ADMIN`"). A thread made before the spawn keeps
`CAP_SETUID` and `CAP_SETGID`, or every capability and no `no_new_privs` if made before step
5, and with a mounted device it lives as long as the helper. It shares the address space of the
thread that parses the child's requests. #74's `credentials_after_start` reads the main thread
only.

Fix: classify where the design says, but start that cleanup thread after step 7, as the server
thread is. Test: with a device ended at the start, every `/proc/<pid>/task/*/status` of the
helper shows `CapEff` `0x200000` and `NoNewPrivs` 1.

### R3-B5 — Low
**Under `--privsep` a quarantined device is in no status, against S5.** New in this round:
"Under `--privsep` … N is 0: the records are the helper's, which logs what it quarantines"
(:621–623). The spec says a quarantined device is "logged and listed in the node's status"
(spec.md:92–96). The helper is the lab's default. There a quarantine makes every `add` fail
(`adoption_quarantines_a_mismatch`), `recovery` answers `ok unknown 0` for good, and nothing
on the socket says why. decisions.md says "Rules narrowed: none anew".

Fix: `kept` answers `ok quarantined DEV` for it (no descriptor; it fits the grammar), and the
I/O process counts and lists it in `recovery`. Or narrow S5 for `--privsep` in the spec and in
decisions.md. A test either way.

## Nits

- **N1.** design.md:174–175: "The next start then removes the record (`ENODEV`)" still names an
  errno; it is the first row.
- **N2.** design.md:499: `start DEV` after `end-recovery` has no TEXT; `already started DEV`
  fits (the kernel answers `EEXIST`, 6.8.12 `:2265–2269`).
- **N3.** design.md:441–446: say that `flaminio-io node --privsep` refuses `--records` as the
  helper's, like `--id` (#74, Parse), and that DIR is absolute.
- **N4.** design.md:144: "neither recovery flag" leaves the reissue flag alone in no row; say
  "without `UBLK_F_USER_RECOVERY`".

Reviewed commit: 5518e4ff63bba98d8a486aee3178e0ae333f7b64 (the plan as fixed at `6ddc6bd`)
