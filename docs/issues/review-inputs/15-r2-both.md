# Plan review: issue #15, round 2 (incremental), focuses: design-code and design-security

Scope: `git diff 976e7d5..fe35c40 -- docs/issues/15/ ':!docs/issues/15/report.md'` and what it
depends on. Checked against the worktree's code (`issue/15-volumes-per-node`), main (for #30's
engine and the simulator), libublk 0.4.8 in the cargo registry, and the #74, #75 and #79 plans
under `/tmp/flaminio-plans-2026-11-12/`. The re-acceptance of the raw block risk (fe35c40) is
taken as a decision.

## Earlier findings

- R1-C1 (High, a failed ADD_DEV deletes the id's holder): fixed. The vendored hunk matches #74's
  (`mark_deleted` when `handle_device_lifecycle` fails; libublk `ctrl.rs:1258`, where the only
  failing step is `add()`). Lab test added. The new "any errno" rule opens R2-B6.
- R1-C2 (High, `Done` before `Piece`): fixed. Taking and routing a batch under one lock hold
  keeps the output FIFO's order across threads. Checked for local, remote and zero-copy reads:
  each tag handles its events one at a time, and awaits a local read or `stage_to_request` before
  it reads the next event (`ublk.rs:1184–1195`, `:1341–1354`, `:1237–1285`, `:1397–1413`). This
  adds no new lock: the `waiters` lock goes, and the engine lock is held a little longer, for one
  `try_send` per I/O output. The test as worded opens R2-B5.
- R1-C3 (Medium, S9 boundary with #79): fixed. The contract's revision opens R2-B7.
- R1-C4 (Medium, reserved attach never begun): fixed (a guard owned by the request; test).
- R1-C5 (Medium, abort on a zero-copy queue failure): fixed (the registration-wait hunk; the
  `wait_for_buffer_registration` skip is at `io.rs:1108–1115`; test with zero copy).
- R1-C6 (Medium, tests for S5 space, the S2 log, the S7 bound): fixed.
- R1-C7 (Low, the stop flag did not end `event_loop`): fixed.
- R1-C8 (Low, #75's QoS release): fixed. Wording issue: see Nits.
- R1-C9 (Low, the limit error did not start with its volume): fixed.
- R1-C10 (Low, a node-wide `Mutex<Routes>`): fixed (routes under the existing engine lock).
- R1-C11 (Low, `del` always asynchronous): fixed.
- R1-C Nits (S4's reference, "to be approved", the commit settings): fixed.
- R1-S1 (High, #74's `--privsep`): fixed by the narrowing. R2-B8 lists what is left open.
- R1-S2 (Medium, no durable bar without a local chunk): fixed. The run mark bars the old token
  whether or not a chunk is local. The mark is written to `.tmp`, the file fsync'd, renamed, and
  the directory fsync'd. In the simulator, `volatile_run_mark` makes a replayed `Start` start a
  second target at E (`sim/mod.rs:815–819` refuses only against the in-memory target, which a
  crash clears, `:1107–1121`), and without the bug the mark refuses it. Its failure paths open
  R2-B1 and R2-B2.
- R1-S3 (Medium, DEVICE not required to be a ublk disk): fixed. A crafted reply cannot name a
  non-ublk disk: only ublk names `ublkb%d`, sysfs must match both the reply and the fd, and
  diskseq rules out a reused minor. Under `--privsep` it can still name another ublk server's disk,
  which is #74's accepted risk. Two gaps remain, R2-B3 and R2-B4.
- R1-S4 (Low, key of `ran_epochs`): fixed (the mark is keyed by volume; test).
- R1-S5 (Low, `list` over 4096 bytes): fixed (1–16; 11 + 16 × 138 + 1 = 2 220 bytes, checked).
- R1-S6 (Low, records' trust): fixed. `flaminio-io cleanup` opens R2-B6.
- R1-S7 (Low, S10's matching branch untested): fixed.
- R1-S8 (Low, lab values in remote shell strings): fixed.
- R1-S9 (Low, S11 written against pre-#30 code): fixed. Main's `engine.rs:1612–1621`
  and `flaminio-node/Cargo.toml:16` match, and nothing else uses `decode_unverified`.
- R1-S10 (Low, raw block impact): fixed (restated, re-accepted by decision, lab check added).
- R1-S11 (Nit, the limit error's volume): fixed.

### R2-B1 — Medium
**A leftover run-mark `.tmp` makes every later attach of that volume on the node fail.**
The mark is written through `.<volume>.tmp`, opened with `O_CREAT|O_EXCL`. Two cases leave that
file behind:
- a crash between the create and the rename;
- a write or fsync that fails. A full or failing `/var` is enough: the design answers
  `error: VOLUME: run mark: <reason>` and ends the attach, but never unlinks the `.tmp`.

Every later attach of that volume then gets `EEXIST` at the create. The CSI retries for ever, and
the volume can never be attached on that node again. No remedy is stated. The records handle the
same case ("A leftover `.tmp` is unlinked first", design.md:393–394), but the run mark only
borrows the records' directory check (design.md:476–478).
Evidence: design.md:463–468 and :472–478; spec.md:163–171.
Fix: unlink a leftover `.<volume>.tmp` before the exclusive create, unlink it on any failed
write, and remove stray `.tmp` files at start. A `.tmp` never became the mark, so removing it is
safe. Test: a leftover `.tmp`, and a failed write (a read-only state directory), each leave the
next attach at a higher epoch working.

### R2-B2 — Low
**The run mark's durability and failure paths have no test that can fail, and the refusal test
names the wrong layer.**
- No test fails if the fsyncs are dropped or reordered. "A new engine over the same store and
  state directory" passes from the page cache alone.
- No test covers a failed mark write: the answer, `Input::Detach`, the entry ending, and no
  `Hello`.
- No test covers a malformed mark (it refuses its volume's attaches), or the state directory's
  `lstat` check.
- The new `epochs/` directory is never made durable in its parent. On a filesystem that needs
  this, a power loss after the first mark can lose the whole directory. The startup token file
  is on tmpfs (`node.sh:59–61`), but kubelet keeps the CSI publish context, so the old token
  survives a reboot.
- The refusal test is listed under "S11 (engine)", but the engine never reads the state
  directory. The registry loads the marks, the socket and the main thread check them, and the
  main thread writes them (design.md:461–468).

Evidence: design.md:459–479, :699–702.
Fix:
- Put the mark's file steps behind a small trait, and test that `write`, `fsync(file)`, `rename`
  and `fsync(dir)` all happen before `Input::Marked` is sent.
- Add `flaminio-io` tests for a failed write and a malformed mark.
- fsync the parent after creating `epochs/`.
- Move the across-restart refusal test to `flaminio-io` (registry and socket).

### R2-B3 — Low
**#15's "`checkUblkDevice(path)`" is not #74's function, and the two plans give different codes for
the same refusal.**
#74 defines `checkUblkDevice(dev)` as: the path regex, `Lstat(dev)` is a block device, and sysfs
holds *its* rdev. Its failure is `codes.Internal` (#74 design.md:188–192). #15's step 2 says the
same function checks that sysfs equals *the reply's* MAJ:MIN. #15 says "whichever lands first adds
them" and "#15's check calls `checkUblkDevice`". But spec S9 maps any mismatch to
`FailedPrecondition` (spec.md:137). An implementer would either change #74's signature, or skip
the reply-to-sysfs comparison that the contract requires (device-lifetime-contract.md:22–24).
Evidence: design.md:343–345, :360–361; #74 design.md:188–192.
Fix: keep #74's `checkUblkDevice(path)`, and state the separate comparison of the reply's MAJ:MIN
with sysfs and with `fstat`. Settle one code for a non-ublk or mismatched device in both plans,
with the test asserting it.

### R2-B4 — Low
**The daemon's own device check has no test.**
After this change the device thread must:
- open `/dev/ublkbN` with `O_NOFOLLOW`;
- require `S_ISBLK`, and `major:minor` equal to `/sys/class/block/ublkbN/dev`;
- otherwise fail the attach.

No test covers it. The contract's loop-device test is the plugin's. The S2 lab tests cover the
minor and the id, not a substituted node.
Evidence: design.md:122–125; tests at :590–610.
Fix: test the open step, on a temp node, or with the path and sysfs root injected. A symlink at the
path, a character device, and a block node whose rdev differs from sysfs each fail the attach, and
no fd is kept.

### R2-B5 — Low
**The routing-order test cannot run as worded against the fix.**
The test pauses the engine thread "just before it routes" the remote reply's batch. Then "the
queue thread then ends the local read (`local_read_done`)". With routing under the engine lock,
the paused thread holds that lock, and `local_read_done` needs it (`ctx.engine()`,
`ublk.rs:1275–1277`). So the queue thread cannot end the local read until the hook lets go. A test
that waits for `local_read_done` before releasing the hook deadlocks on a correct implementation.
An implementer who "fixes" the test by moving the hook outside the lock would lose the test's
power to fail.
Evidence: design.md:596–600, :31–37.
Fix: the hook releases after a fixed delay, or once the queue thread is seen blocked on the lock.
Then assert `Piece` before `Done`. With routing after the unlock, the queue thread routes `Done`
during the pause, and the test fails.

### R2-B6 — Low
**`flaminio-io cleanup`, and the id walk on "any errno", are under-specified.**
- **cleanup beside a live node.** ublk reports `UBLK_S_DEV_DEAD` between `ADD_DEV` and
  `START_DEV`. At that point the record already holds the returned `dev_info` (design.md:399–402).
  So a `flaminio-io cleanup` run while the node runs finds a device mid-attach "recorded, dead and
  matching", and deletes it. Nothing excludes the two. #74 at least states a root `del` while the
  unit runs is unsupported (#74 report, R3-S4).
- **cleanup before 6.9.** It is not said whether cleanup waits for its `DEL_DEV` thread, which
  blocks until the last close. `node.sh down` loses today's guard ("deleting it would wait
  forever", `node.sh:22–25`), so with a staged, mounted volume on 6.8 `down` may hang.
- **No script test** that `node.sh down` runs `cleanup` (and keeps `del --id` for a build without
  records).
- **The id walk.** "Any errno" moves to the next id until the deadline (design.md:103–107). A
  failure that has nothing to do with the id then burns ids: `ublk_drv` not loaded, ENOMEM,
  `ublks_max` reached, or a flag the kernel refuses. No pace is stated. Today's loop sleeps 250 ms
  (`ublk.rs:896–899`), and #74's helper paces adds the same way.

Fix: hold an exclusive `flock` on the records directory for the node's life; `cleanup` takes it
non-blocking and refuses if held (or state it unsupported, as #74 does). Say that `cleanup` does
not wait for a `DEL_DEV` thread, or keep the mount guard for every recorded device. Keep the
250 ms pace. Optionally, move to the next id only when `/sys/class/ublk-char/ublkc<N>` now exists.
Add the `node.sh` script test.

### R2-B7 — Low
**#79 pins the contract at `8af7aeb`, and this round changed it.**
#79's design and decisions cite the contract "commit `8af7aeb`" (79 design.md:30–35;
decisions.md:33, :53). Since then the contract has added:
- the `/dev/ublkb<N>` and sysfs rule;
- the identity from sysfs under `--privsep`;
- the tools-by-path rule;
- the legacy path until #79.

#79's text still lists only "open no-follow, compare block/rdev/diskseq". The contract says #79 must
depend on it, but it carries no revision marker, and #15 records no note that #79's pin moves.
Evidence: device-lifetime-contract.md:5–8; #79 design.md:30–35.
Fix: give the contract a revision line (the commit of this change). Record in decisions.md, for
#79's owner, that #79 must re-pin and add the sysfs comparison to its client.

### R2-B8 — Low
**Under `--privsep`, the device thread and S5 are not reconciled with #74.**
#15's device thread is `run_target`'s ready callback (design.md:118–135). Under #74 there is no
`run_target`. The I/O process instead runs:
- `from_cdev`;
- queue threads as `create_queue_handlers`;
- `start` after `wait_for_buffer_registration`;
- its own affinity step.

A `start` reply missing past `START_LIMIT` is fatal: the process exits (#74 design.md:165–179).
"With #74's helper" lists only the four requests. "Order" gives the second lander the parse
refusal and the run mark's directory, but not this restructuring. Nor does it give the `device-info`
read from sysfs, which has no test. S5's list of faults that "end only their own volume" makes no
exception for #74's fatal `start`. Even with one volume, that exit ends the replica service other
nodes' volumes use.
Evidence: design.md:500–529; spec.md:84–86.
Fix: in "Order", give the second lander the privsep variant of the device thread, with its tests
(the sysfs `device-info` included). List #74's fatal `start` limit as an S5 exception under
`--privsep`.

## Nits
- design.md:218: "which `Engine::take_qos_timer` arms **today**": #75 is a plan, not code. Say
  "in #75's design".
- spec.md:139–140: S9's "its `major:minor` never serves another volume while published" reads as
  unconditional. Under `--privsep` the node holds no fd (decisions, Accepted risks). Point to that
  exception there.
- design.md:392: the record names `^[0-9]{1,7}$` accept leading zeros, so `01` and `1` name one id.
  #74's grammar for the same id space is `(0|[1-9][0-9]{0,6})`; use it.

Reviewed commit: fe35c4024a13c3d08046c22517b2395888c9b2b5
