# Plan review: issue #48 (Live data-plane upgrade), round 4 (incremental), focuses design-code and design-security

Scope: `git diff 5518e4f..e3a973d -- docs/issues/48/` in `/home/jsfakian/Documents/src/flaminio-48`
(HEAD `69d93ad` adds only report.md). Checked against the kernel trees the design cites
(6.1.112 `eve-kernel`; 6.8.12, #74's tree) and #74's design on `origin/main`. No 6.6 or 7.0
source was read. Line numbers of design.md are at `e3a973d`.

Counts: Critical 0, High 0, Medium 0, Low 2, Nit 4.

## Earlier findings

- R3-B1 (Medium): fixed. See "What holds", the `recovering` phase.
- R3-B2 (Low): fixed in the text, for both modes. The helper's stamp at the old deadline has no
  test of its own: R4-B2 (a).
- R3-B3 (Low): fixed. Four of `ending`'s answers have no test: R4-B2 (c). What the new sentence
  states against S8: R4-B1.
- R3-B4 (Low): fixed. The test's precondition: R4-B2 (b).
- R3-B5 (Low): fixed. `ok quarantined DEV` fits #74's grammar (ASCII, single spaces, under 256
  bytes; three fields against seven). `recovery` then answers `ok unknown 1 ID`, so S5's "listed
  in the node's status" holds under `--privsep`. `recover`, `stop` and `delete` on that id stay
  refused (`not kept`, `not owned`): the child cannot act on a quarantined device.
- Nits N1 to N4: fixed.

**What holds** (asked for in the brief; no finding):
- **The `recovering` phase, crash by crash.**
  - Before the write: `running`, kept (table step 2).
  - Between the write and the kernel call, and during steps 3 to 5: `recovering`, ended. The
    first is conservative: the kernel began nothing.
  - Between END_USER_RECOVERY's return and the write back: ended. Only a death leaves that
    state, so nothing is serving the device then. The write back comes before step 6 and before
    `end-recovery`'s `ok`, so no device that serves is ended for a lost write back.
  - After the write back: kept, inside the old bound, with every queue ready.
  - The crash table (:245–247), the scan's table (:157), "Who reads which record" (the phase is
    in both forms), S2, S5, S6 and the ADR agree.
- **No device is recovered twice.** `EBUSY` is returned before `ublk_queue_reinit` (6.8.12
  `:2740–2747`; 6.1.112 `:1933–1941`), so the turn back to `running` follows a call that changed
  nothing. After a START_USER_RECOVERY that succeeded, a failure in the process ends the device
  and a death leaves `recovering`. So the next START_USER_RECOVERY only ever meets a device whose
  queues were all ready: the fault of R3-B1 is not reachable.
- **Ending the device in R3-B1's state is safe.** `ublk_stop_dev` takes the mutex, and
  `ublk_cancel_cmd` skips an io that is not active (6.8.12 `:1502`, `:1663–1681`); STOP_DEV also
  completes the stale fetches that kept the character device open.
- **The dead successor does not hold the `flock`.** The process's exit waits only for tracked
  requests (6.1.112 `io_uring.c:3003`, `:2973–2978`); the stale fetches stay with the ring's exit
  work. So the next start can run.
- **The helper deleting a `recovering` device at its child's exit** is #74's outcome 4 (c)
  unchanged (the pin closed, DEL_DEV from a cleanup thread, the exit within 9 s). The record
  stays; the next start removes it (first row), deletes a DEAD device, or ends it again.
  Take-over by record needs nothing more.
- **The stamp's table.** No row stamps before the old deadline. At that deadline the device is
  serving (stamped: it was resumed), kept (ended), or recovering (not stamped; ended if its
  server dies). So a crash loop ends at its first exit's bound. What is left is the stamper's
  wake-up at the deadline, which cannot be closed without stamping early.
- **The `ending` entry.** The server thread answers one request at a time, so only the main
  thread and the cleanup thread run beside it. `add` is refused until the record and then the
  entry are gone, in that order. The helper may exit with the thread running: the next helper
  finds the record with no device (removed), a DEAD device (deleted) or the device as it was
  (ended again, `ending` again).

### R4-B1 — Low
**Under the helper the bound can cut a resume under way, against S8.**
- S8: "The bound never cuts a resume under way, which ends within #6's limits" (spec.md:147–148);
  design.md:387–388 says the same, by the deadline thread that skips a resuming entry.
- The helper learns of a resume only at `recover`. Until then the entry is kept: through steps 1
  and 2 (the token, the mark, the fence, the reconciliation), and between two `recover`s that
  answered `error busy`. The new sentence says so: "the tick ends an entry … only while it is
  kept: one of the two wins" (design.md:500–503), and the new test accepts it
  (`a_recover_that_meets_the_deadline_is_refused_or_wins`, :590).
- So a resume that starts before the deadline and reaches `recover` after it (a slow start of
  the store, a fence that waits for a lost ISR member) completes in direct mode and is ended
  under `--privsep`, the lab's default. The outcome is safe: I/O errors, as after a crash.
- The behaviour predates this round ("unless a `recover` succeeded"); this round's sentence and
  test state it as intended, and decisions.md says "Rules narrowed: none".

Fix: narrow S8 for `--privsep`, in the spec, at design.md:387 and in decisions.md: under the
helper the bound holds off from a `recover` that the kernel accepted; a resume that reaches
`recover` after the bound is refused, and the device is ended. One test name for the wait on
`error busy` (`the_deadline_ends_a_device_between_two_busy_recovers`).

### R4-B2 — Low
**Three of the round's rules have no test that can fail without them.**
- **(a) The helper's stamp at the old deadline.** The rule is for both modes (design.md:123–125,
  :463–465). `the_stamp_comes_at_the_old_deadline_not_at_the_next_tick` is in `kept.rs` only
  (:736). Under the helper, `an_exit_right_after_the_old_deadline_is_inside_a_new_bound` (:584)
  passes by the exit stamp alone, with a plain 1 s tick. The stamp at the deadline is what
  covers a helper killed with its child, which is what a unit's stop does (#74, outcome 5).
- **(b) `threads_after_the_start_hold_only_sys_admin`** (:607–609) reads the threads "with a
  device ended at the start". If DEL_DEV has returned, the cleanup thread is gone, and the test
  passes with the thread made before step 5.
- **(c) `ending`'s answers.** `add` and `recover` have tests (:589–590). `kept` (`ok none`) and
  `start`, `stop`, `delete`, `end-recovery` (`not owned DEV`) have none (:508–511). `delete` is
  the one that matters: accepted, it removes the entry and lets an `add` in beside the thread,
  which is R3-B3 again.

Fix: (a) add the `kept.rs` test's twin to `serve.rs`, on the fake clock, with the child alive.
(b) the test holds `/dev/ublkb<ID>` open, as #74's `held_device_does_not_block_cleanup` does, so
the thread is in DEL_DEV when the threads are read. (c) `requests_on_an_ending_entry_are_refused`
(each of the five; no kernel call).

## Nits

- **N1.** design.md:253–260 against :456–458: direct mode keeps `recovering` through the whole
  `EBUSY` wait (10 s at most; a death there ends a device the kernel never began to recover),
  the helper only across each call. Say that the difference is meant, or turn back at each
  `EBUSY` in both.
- **N2.** design.md:500–503, :526–529: say that `error busy` puts the entry back to kept, and
  that any other failure of `recover` leaves it out of kept (`stop` and `delete` only).
- **N3.** design.md:604–606: under the helper a killed child's device is deleted by its own
  helper at the exit; "the next helper finds `recovering`" ends a device only when the helper
  is killed with its child. Say which is killed.
- **N4.** design.md:325–326: "the stamp is at most 1 s old" holds for an added device; after a
  resume inside the old deadline it is the first exit's (the stamp's table, second row).

Reviewed commit: 69d93ad86f4a516c83a9c1536ed354c5271f4e17 (the plan as fixed at `e3a973d`)
