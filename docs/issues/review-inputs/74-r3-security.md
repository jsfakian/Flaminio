# Issue #74 plan review: round 3 (incremental), design-security

Scope: `git diff 96d792d..7245c69 -- docs/issues/74/ ':!docs/issues/74/report.md'` in
`/tmp/flaminio-plans-2026-11-12/74`: the round-2 fix (30458b9) and the approval (7245c69). I checked
it against the worktree, libublk 0.4.8 in the cargo registry, the Linux 6.8.12 tree
(`/home/dpoulios/ie3500/eve-kernelR/kernel/kernel-noble`) and #71's latest plan (51cdf54). The new
kernel claims I re-read hold: an open `/dev/ublkcN` keeps its parent device and so its id
(`fs/char_dev.c:348–355`, `:496`, `:608–624`; `ublk_drv.c:2150–2164`); DEL_DEV removes the device
and returns the `ublks_max` slot before it waits (`:2533–2568`, `:2226–2233`); STOP_DEV cancels
the fetches of a device that never started (`:1663–1680`); in 6.8 a server's death stops the
device through the uring_cmd cancel, not the file release (`:1519–1550`), so the pin does not
delay it; SIZE ≤ 2^63 − 4096 cannot overflow `(loff_t)sectors << 9`.

## Earlier findings
- R2-S1: fixed. The main thread waits on the pidfd, and process exit interrupts the io-wq waits.
- R2-S2: fixed. intent.md "Which step" says Kubernetes stays privileged until #12, and that #48 must route recovery through the helper.
- R2-S3: partly fixed. The child's exit now deletes owned devices, but a compromised child can still leave devices behind (R3-S2).
- R2-S4: fixed. SIZE is capped at 2^63 − 4096, and `bounds` tests it.
- R2-S5: fixed. Answers are capped at 4096 printable bytes, with tests.
- R2-S6: fixed. The invariant, Risks and intent's step 3 state it as open, and no document claims it closed (the ADR omits it: Nit 2).
- R2-S7: fixed by a cut. `ATTACH` is refused under the helper, and no lab target sets it (grep of `scripts/`, `Makefile`, `docs/`).
- R2-S8: not fixed. The pin stops id reuse, but a failed `add` still deletes a device the helper never owned (R3-S1).
- Round-2 security nits: fixed. The std order matches `unix.rs:307–330`, the ADR names UID 0, and Q4 names udev rules.
- R2-C1 to R2-C9 and the code nits: fixed as decisions.md records. The bound R2-C5 claims for `delete` does not hold (Nit 3).

### R3-S1 — Critical
**A failed `add` makes the helper delete any ublk device whose id the child names.** This bypasses
the ownership check.
The helper adds devices through `add_device`'s libublk builder and disowns the device only once the
build succeeds (design.md:56–60). In libublk 0.4.8, `UblkCtrlInner::new` runs
`handle_device_lifecycle(config.id)?` (`ctrl.rs:1258`). When ADD_DEV fails, the `?` drops the inner
control. Its `Drop` (`ctrl.rs:1085–1096`) sees `UBLK_DEV_F_ADD_DEV` without the disowned flag and
sends DEL_DEV with `dev_info.dev_id`. That is the requested ID, because the kernel copies nothing
back on failure (`ctrl.rs:1134`, `:1439–1441`). ADD_DEV of a taken id fails with `EEXIST`
(`ublk_drv.c:2134–2137`), and DEL_DEV needs only the helper's `CAP_SYS_ADMIN`.
So `add 1 1 4096 4096 N` deletes device N: another server's device (Q3), a root `flaminio-io serve`
device, or a device left by an earlier helper (outcome 5 says "never acts on them"). If N is
mounted, its disk is removed under the filesystem, and the server thread then blocks until unmount.
If N is one of the helper's own pinned devices, the server thread deadlocks on its own pin until
the child exits.
This contradicts the invariant ("nothing else"), S3, and the Ownership, outcome 5 and outcome 6
claims (design.md:73–78, :100–107). `foreign_device_refused` tries only start, stop and delete. The
fake `Kernel` hides libublk's `Drop`.
Today's direct mode depends on this side effect without saying so. Its `add` retry
(`ublk.rs:902–934`) deletes a crash leftover with the same id. `node.sh up` runs `down` only when
the unit is active (`node.sh:44`).
**Fix:** add a third change to the vendored libublk patch: on an ADD_DEV error, mark the control
deleted before returning, so that `Drop` sends nothing. (Alternatively, `Kernel::add` issues
ADD_DEV itself.) Under the helper, `node.sh up` deletes a leftover `/dev/ublkc$id` before it
starts the unit. Add tests:
- the fake `Kernel`: `failed_add_sends_no_delete`;
- `tests/kernel.rs`: `foreign_device_survives_add_of_its_id` (a root-added device N survives the
  child's `add ... N`, which gets `error kernel add: EEXIST`);
- an owned id passed to `add` leaves the table and the device unchanged.

### R3-S2 — Medium
**Outcome 4's cleanup can be defeated, so devices still leak across restarts (R2-S3).**
S5 says the helper "first deletes its owned devices within a bound". A compromised child gets
around that in four ways:
1. **A forked process keeps the socket.** fd 0 has no `CLOEXEC`, so a fork keeps the helper
   socket after the pidfd's process exits. The server thread keeps serving during the 5 s cleanup.
   Every `add` after the main thread "takes every entry" (design.md:92–94) is never deleted: up to
   4 more devices per life. If the entry is recorded before the reply goes out, the reply can also
   carry a descriptor number that the main thread just closed and the cleanup thread reused.
2. **An `add` in flight at the exit is left** (design.md:96–97), up to the 3 s open retry.
3. **One held device stalls the rest.** One cleanup thread sends DEL_DEVs one after another, each
   through libublk's synchronous call (`ctrl.rs:1576`). The first device with a holder makes that
   call wait (outcome 3 says so), and the other devices get no DEL_DEV before the 5 s exit. The
   holder can be a mount, or a dm or md device that udev autoactivation builds from contents the
   child wrote (Q4). 6.8 has no DEL_DEV_ASYNC.
4. **Only `add` is paced.** 6.8 restarts a stopped device: START_DEV checks only `LIVE` and
   `USED` (`ublk_drv.c:2266–2270`), and STOP_DEV sets `DEAD`. Start/stop cycles on one owned
   device therefore create and remove disks, with uevents and root udev probes, as fast as udev
   frees them.

None of these is tested. `child_exit_deletes_owned` runs on a fake kernel, and the kernel test has
one device.
**Fix:**
- At the child's exit, the main thread first sets `closing` under the table lock and calls
  `shutdown(SHUT_RDWR)` on the helper's end.
- The server thread records a device right after ADD_DEV (so outcome 4 covers an add in flight),
  sends the reply before giving up the descriptor, and deletes the device itself when `closing`
  is set.
- Each DEL_DEV in cleanup runs on its own thread.
- One `start` per owned device (`error already started`).

Tests: `requests_after_child_exit_refused` (a forked holder), `add_in_flight_at_exit_deleted`,
`held_device_does_not_block_cleanup` (two owned devices, the first held open by the test: both gone
within the bound), `second_start_refused`.

### R3-S3 — Medium
**#74 and #71 contradict each other on the telemetry socket, and neither plan owns the helper's
part.** #74's design Risks (design.md:287–289) says: "by #71's draft, under `--privsep` the helper
also creates #71's telemetry directory, binds its socket by #71's rules and passes it like
`control`; #71's plan owns that". decisions.md:129–130 says the same. #71's latest text says the
opposite: "Telemetry needs direct mode in this step … a privsep node's observer reports its
snapshot as not fresh until #74's helper provides the socket" (`71/contracts.md:32–35`; spec.md
S3; decisions.md:139–141; intent.md:48–50). Each plan names the other as the owner.
The helper's grammar has no such request (design.md:193–198). The lab node defaults to the helper
(S8), so telemetry is off there with no owner and no followup. Whoever adds it later would add a
root-created directory and a new request to the helper without this plan's review.
**Fix:** state #71's actual rule: under `--privsep` telemetry is unavailable in this step. Name
the owner of a future `telemetry` request (a later #74 step or a followups.md entry). That request
would need its grammar line, its descriptor rule, `#71`'s directory checks done by the helper,
tests and a fuzz corpus. Correct decisions.md:129–130.

### R3-S4 — Low
**A root `del` of an owned device leaves it startable, so a compromised child can hang
`node.sh down`.** Outcome 6 keeps an owned id in the table after a root `del` (design.md:105–107).
`node.sh down` runs exactly that while the unit runs (`node.sh:27–29`): it sends STOP_DEV, then
DEL_DEV, which removes the device and waits for the id. If the child now sends `start` for that id:
- START_DEV (`ublk_drv.c:2266–2270`) builds a new disk on the deleted device and holds
  `cdev_dev` (`:2292`).
- No later DEL_DEV removes that disk, because DEL_DEV skips `ublk_remove` once `UB_STATE_DELETED`
  is set (`:2543–2546`). Outcome 4's cleanup does not remove it either.
- The root `del` therefore waits forever. `make node-down` hangs, and `ublkbN` stays live with
  failing I/O until a second `del` stops it.

**Fix:** under `PRIVSEP=1`, `node.sh down` stops the unit first (outcome 4 deletes the owned
devices) and runs `del` only for leftovers. The one-`start` rule (R3-S2) closes the rest. Test:
`node-scripts-test.sh` checks that `down` stops the unit before `del` under the helper.

## Nits
1. The approval text contradicts itself. decisions.md:5–6 still says "None yet. Nothing in this
   plan is approved …; adr-proposal.md stays Proposed", right above the approvals.
   adr-proposal.md's Confirmation says "Nothing here is approved or run" under an "Accepted"
   status. design.md:296 says "New, Proposed".
2. adr-proposal.md Consequences lists what is retained without the disk descriptor's
   `SG_IO`/reservation/NVMe passthrough power, which the spec's invariant names. Add it next to
   "Retained".
3. design.md:280–281 says the `start` limit and the main thread's exit bound the server thread's
   waits. Neither bounds a DEL_DEV of a held device (outcome 3), and the client mutex keeps a
   detach's `stop` behind it. Say that this wait is unbounded, as today's drop is.
4. The pin is the same open file the child holds, so it keeps `UB_STATE_OPEN` set while the helper
   lives. START_USER_RECOVERY refuses while that bit is set (`ublk_drv.c:2740–2744`), and round 2
   gave this as the reason to close the copy. The ADR's "recovery stays available (#48)" should
   add that #48 must close the pin before recovery and reopen it afterwards.

Reviewed commit: 6a9d064dd0d493980cb51925c72409fdb1372f72 (diff 96d792d..7245c69; HEAD adds only report.md)
