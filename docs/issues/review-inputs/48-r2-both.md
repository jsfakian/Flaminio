# Plan review: issue #48 (Live data-plane upgrade), round 2 (incremental), focuses design-code and design-security

Scope: `git diff f7db824..8bd2a53 -- docs/issues/48/` in `/home/jsfakian/Documents/src/flaminio-48`
(design.md read whole at `8bd2a53`; `2d0611a` adds only report.md). Checked against the code of
that checkout, libublk 0.4.8, the kernel sources the design cites (6.1.112 `eve-kernel`; 6.8.12,
the tree #74's decisions name), and the clear plans #6, #12, #15, #74, #79, #83. No 6.6 or 7.0
source was read. Line numbers of design.md are at `8bd2a53`.

Counts: Critical 0, High 0, Medium 3, Low 8, Nit 5.

## Earlier findings

- R1-C1, R1-S2 (High): fixed in substance. Take-over by record is sound: see "What holds" below.
  What it opens: R2-B1, R2-B2, R2-B3, R2-B4, R2-B7.
- R1-C2 (High): fixed. The loop takes no `n.mu`; the registry's entry lock and the kept tuple
  settle a resume against a detach; `device()` takes no epoch for a kept volume.
- R1-C3 (High), R1-S7: fixed. The kept and resuming lines have the running line's arity; the
  pre-restart tuple detaches. Residue: R2-B9.
- R1-S1 (High), R1-C11: fixed. The rule excludes both cases (below); the check is buildable.
- R1-C4, R1-S3: fixed. R1-C5: fixed. R1-C6, R1-S12: fixed. R1-C8, R1-S10: fixed.
- R1-C7: fixed, but for one row of the table: R2-B5.
- R1-C9, R1-S8: fixed, but for R2-B6 and R2-B9.
- R1-C10: fixed. R1-S4, R1-C14: fixed. R1-S6: fixed in direct mode; under the helper, R2-B1.
- R1-S5: fixed in direct mode; reopened under the helper (R2-B2); a side effect, R2-B8.
- Round-1 Lows: all fixed.

**What holds** (asked for in the brief; no finding):
- **Between two helpers** the device is the kernel's, named by root's record. Control commands on
  an ordinary device need `CAP_SYS_ADMIN`, and `/dev/ublk-control` and `/dev/ublkc<ID>` are
  root's, so only root (`cleanup`, `del`, the next helper) reaches it. The disk's `O_EXCL` and
  the `flock` keep a second helper out while the old child lives.
- **The pin rule holds:** the pin is closed at the child's exit, a kept device has none, and
  `recover` opens the new one only after START_USER_RECOVERY (6.8.12 `ublk_drv.c:2740–2744`).
- **The requests** fit #74's grammar and bounds (128 bytes, no descriptor, one in the reply).
- **Continuity.** Tokens name the node (`meta/service/service.go:743–745`), #15 checks it, and
  only `Header.Attach` moves the epoch (`meta/domain/domain.go:217–220`). So mark + 1 was minted
  for this node by the one `Attach` after the mark: no other target ran. A volume made again
  starts above every earlier epoch (`service.go:461–465`), so it is always a gap as well. The
  rule does not depend on the 60 s.
- **The kernel facts** cited for 6.1.112 and 6.8.12 are at the lines given.
- **`cleanup --expired`** reads root's record only, under the `flock`; the note cannot make it
  end or keep a device.

### R2-B1 — Medium
**`cleanup --expired` on a helper's record: one reading ends the kept device inside the bound.**
- It "runs the start's scan, ends the devices that the scan would end" (design.md:334–337).
- The scan's table has no row for `recovery 2`. The only rule is: "`recovery 2` met by a
  direct-mode node is a mode change: its device, if it matches, is ended" (:416–418).
- `cleanup` is called "a direct-mode command" that "ends a helper's kept device by the record
  alone" (:448–450).

So with shared scan code, the agent's run, 2 s after the socket goes silent, ends the kept device
whenever no helper holds the lock: `PHASE=stop`, a unit slow to return. The lab's default mode is
the helper. The named test, `cleanup_expired_ends_only_what_the_scan_ends`, pins that reading.
Lab steps 7 to 9 do not say which mode they run in.

Fix: state that `cleanup --expired` classifies both forms of record by the same rows (boot,
`dev_info`, phase, state, `alive`), and that the mode-change rule is a node's or a helper's
start only. Add `cleanup_expired_keeps_a_helpers_device_inside_the_bound` and its expired twin;
give lab steps 7 to 9 a mode.

### R2-B2 — Medium
**The helper's stamp at its child's exit moves the deadline** (reopens R1-S5 under `--privsep`).
At the child's exit "a recoverable device whose record says `running` is kept, a recovery under
way included ... the record gets a last stamp, unless the 60 s rule forbids it" (:407–413). The
60 s rule is only "not for 60 s after an `end-recovery`" (:396–397). An adopted device that was
never recovered, and one between `recover` and `end-recovery`, still say `running`, so each
helper that exits stamps them anew. A successor that keeps dying before or during its resume (a
store that fails to open, an `end-recovery` cut by `START_LIMIT`) then never reaches the bound,
and the held I/O waits. Direct mode has no exit stamp, and S8 says "a crash loop ends at its
first exit's bound".

Fix: the exit stamp is written only for a device this helper's child was serving and had stamped
(started here, or recovered more than 60 s ago). Tests:
`child_exit_does_not_stamp_an_adopted_device`, `child_exit_during_recovery_does_not_stamp`.

### R2-B3 — Medium
**The helper's `flock` on the one records directory breaks #74's kernel tests, and #48's own.**
"The helper takes #15's `flock` at its start ... so one helper or one direct-mode node runs on a
node" (:380–382). #74's `make privsep-check` runs `flaminio-helper/tests/kernel.rs` beside the
running lab node, each test's helper at its own `--id`, 100 and up (#74 design, Lab: "Then, with
the node running"). Each of those helpers would wait 5 s on the node's lock and refuse to start,
or write records into the node's directory. #48 adds its kernel tests to that file (:470–475).
The cross-plan list for #74 (:727–731) does not have it.

Fix: the helper takes its records directory as an argument (default `/run/flaminio/ublk`), the
tests pass their own; add it to #74's cross-plan item and to `privsep-check`.

### R2-B4 — Low
**Which thread sends the helper's DEL_DEV when it ends a kept device.** "Its main thread's 1 s
tick ends a kept device" (:419–420), and the start classifies and ends before the child is
spawned (:414–418). A kept device past its bound is usually still mounted, and #74's DEL_DEV
waits for the last reference with no bound (#74 design, Owned devices, outcome 3). On the main
thread it stops the pidfd wait (#74's S5), and at start the node gets no data plane until the
pod's mount goes. "Ended" (:141–143) names #15's delete, which is the node's code, not the
helper's.

Fix: say that the helper ends a device as #74's outcome 4 (c) does, on a cleanup thread it does
not wait for (or `DEL_DEV_ASYNC` from 6.9). Test on the fake `Kernel` with a delete that blocks:
the child is still spawned, and its exit still ends the helper.

### R2-B5 — Low
**The crash table's step 4 relies on a kernel fact that is not cited, and that the cited
kernels contradict.** "Kept if a queue was ready" (:208), with the kernel test
`a_successor_killed_after_its_queues_fetched_is_resumed`, assumes that a successor dying in
recovery releases the character device.
- 6.1.112: nothing cancels its fetches while the device is QUIESCED. The monitor is cancelled
  at the quiesce (`ublk_drv.c:1128`) and the quiesce work returns unless LIVE (`:1137`). So
  `UB_STATE_OPEN` stays, and START_USER_RECOVERY answers `EBUSY` until a STOP_DEV.
- 6.8.12: the cancel function returns without cancelling for a queue that is not fully ready
  (`:1536`, `ubq_daemon` is set only then). Several queues, one ready and one partly fetched,
  give the same `EBUSY`.

The outcome is safe: the device stays kept and ends at the bound. But the row and the test's
expectation are wrong there, and the Kernel check (:682–691) does not list it.

Fix: word the row by what is certain ("kept; resumed where the kernel releases the dead
successor's fetches, else ended at the bound"), add the fact as item 7 of the Kernel check, and
add a kernel case with a partly fetched queue.

### R2-B6 — Low
**The scan decides by `ENODEV`, which the design says cannot be trusted.** The facts state that
libublk sends a failed command again with its legacy opcode, so "the errno is `EOPNOTSUPP`
whatever failed ... Nothing here decides by errno, `EBUSY` apart" (:17–19). GET_DEV_INFO is
retried that way (`ctrl.rs:1499–1520`, opcode 0x02). The table's first row is "`GET_DEV_INFO`
gives `ENODEV`" (:130), and no row says what any other failure does. On such a kernel a record
whose device is gone never matches the first row.

Fix: a row for "GET_DEV_INFO fails": what proves that the device is gone without the errno, and
what the record becomes otherwise (left, as #15 has it, not quarantined). A test with a fake
kernel that answers `EOPNOTSUPP`.

### R2-B7 — Low
**The helper's refusals for the new states are not written.** TEXT gains `busy`, `not kept`,
`already recovered` (:439–440). Not stated: `start DEV` on an adopted device; `end-recovery`
with no `recover` before it, or a second one ("at most once per `recover`", with no TEXT);
`add` while a kept device is owned. `end-recovery` passed to the kernel without a recovery
finds the old completion done and turns the device LIVE with no server (6.8.12 `:2769–2789`).
The child is the untrusted party.

Fix: one line per case with its TEXT and "no kernel call", and tests
`end_recovery_without_recover_refused`, `start_on_a_kept_device_refused`,
`add_with_a_kept_device_is_device_limit`; the fuzz target's state included.

### R2-B8 — Low
**An exit between `alive` + 60 s and the resume + 60 s ends the device at once.** After a resume
the record is not stamped for 60 s (:104–106), and SIGTERM writes nothing (:269). A server that
resumed at T+20 s and exits at T+65 s, a second planned restart included, leaves `alive` = T: the
successor's scan finds the bound passed and ends a device that was serving.

Fix: stamp again once the old deadline (`alive` + 60 s) has passed, not 60 s after the resume. A
crash loop still ends at its first exit's bound. Test the window.

### R2-B9 — Low
**The plugin's detach before #79, and root's `detach VOLUME` on a resuming entry.** "The plugin
sends no name-only detach (#79, S7)" (:256), but the design's prerequisites are #12 and #15
(:44), and today's unstage sends `detach VOLUME` (`control/csi/node.go:140`). The design gives
that command's effect on a kept entry only (:256–257); on a resuming one it has none (#15
answers a starting one `is attaching`). `TestUnstageAfterResume` and
`TestDetachDeviceWithThePreRestartTuple` need #79's record and client.

Fix: state `detach VOLUME` on a resuming entry, with a test, and mark those two Go tests "by the
second of #48 and #79 to land".

### R2-B10 — Low
**S3's narrowing gives up more than the finding forces, and the spec names only half of it.**
The 128-byte request shows that the helper cannot be told the volume's id. It does not show that
`dev` and diskseq must be the I/O user's: the helper is root and can read
`/sys/class/block/ublkb<DEV>/{dev,diskseq}` after `start`. With them in root's record, adoption
proves the disk as direct mode's scan does, where a mismatch is a quarantine (:135) and not an
end by the child. The spec says the note is one "which its user reads" (spec.md:71–74); that its
user also writes it, and so names the volume its successor serves, is only in the design's Risks.

Fix: put `dev` and `diskseq` in the `recovery 2` block; say in S3 that the I/O user writes the
note. If the volume is to be root's too, a digest of volume and first epoch fits `add`.

### R2-B11 — Low
**A device returned with one of the two flags is never ended.** "A device whose returned flags
lack either one is unrecoverable ... and a restart ends it" (:57–59). With
`UBLK_F_USER_RECOVERY` alone the kernel quiesces it at the server's exit; the scan then applies
"#15's rule for a device not yet dead" (:136), and #15 deletes only a DEAD device without that
flag. It waits with no bound. The design names the case itself (`off-flags`, zero copy).

Fix: an add that returns exactly one flag is deleted and made again with neither, or the scan
ends such a device. A test.

## Nits

- **N1.** spec.md:116–117: "once the kernel was asked, a failure ends the device" contradicts the
  design's `EBUSY` past its deadline, which stays kept (:215–217). Say "once the kernel began
  the recovery".
- **N2.** adr-proposal.md:28–29: "only with a token of the agent's `only_if_target` attach". The
  administrator's token resumes too (design.md:185).
- **N3.** intent.md, want 4, and the ADR's "The record" do not carry the narrowings of S4 and
  S3.
- **N4.** design.md:589: `sigterm_exits_at_once_when_every_entry_is_kept` tests recoverable
  entries, not kept ones.
- **N5.** design.md:60–61: what `recovery` answers under `--privsep` before the first `add`
  (the I/O process cannot probe) is not said.

Reviewed commit: 2d0611a3d3f4b5600b14775082782df4880466d2 (the plan as fixed at `8bd2a53`)
