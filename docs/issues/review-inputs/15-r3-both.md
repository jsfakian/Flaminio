# Plan review: issue #15, round 3 (incremental), focuses: design-code and design-security

Scope: `git diff fe35c40..0a2b1fe -- docs/issues/15/ ':!docs/issues/15/report.md'` (HEAD 28711dd
adds only report.md), and what it depends on. Checked against the worktree's code, libublk 0.4.8
in the cargo registry, and the #74 and #79 plans under `/tmp/flaminio-plans-2026-11-12/`.

## Earlier findings

- R2-B1 (Medium, a leftover run-mark `.tmp`): fixed. It is unlinked before each create and on a
  failed write, and stray ones are removed at start. All writes are on the one main thread, so
  there is no race on the `.tmp`. Tests added.
- R2-B2 (Low, the mark's durability tests): fixed. `MarkFs` order test, failed write, malformed
  mark, `lstat` check, `epochs/` fsync'd in its parent, and the restart test moved to `flaminio-io`.
  The trait's wording is a Nit below.
- R2-B3 (Low, `checkUblkDevice`): fixed. #74's function is unchanged (#74 design.md:188–192), and
  #15's comparison sits beside it. Both plans give `codes.Internal`. Opens R3-C2 (no test seam)
  and a Nit (step 4).
- R2-B4 (Low, the daemon's open step untested): fixed. The path and the sysfs root are
  parameters; symlink, char device and mismatched-rdev cases are tested.
- R2-B5 (Low, the routing-order test deadlocks): fixed. The hook releases after 200 ms.
  - Routing under the lock: the queue thread's `local_read_done` waits for the lock, and `Piece`
    is queued first.
  - Routing after the unlock: `Done` is routed during the pause, and the test fails.
- R2-B6 (Low, cleanup and the id walk): fixed as asked, and it opens R3-B1 and R3-C1.
  - The `flock`: a crash releases it, as `flaminio-io` spawns no children and std opens files
    `O_CLOEXEC`. So `node.sh down` after a crash (stop, then cleanup) still cleans up. Under
    `--privsep`, neither process takes it: the records are direct mode's only (design.md:547–549).
    The text does not say so (Nit).
  - The walk terminates: `ADD_DEVICE_LIMIT` is 10 s at one add per 250 ms (`ublk.rs:672`, `:876`).
    Skipping an id is harmless, as ids need not be dense. But the walk can stall on an id it
    should skip (R3-B1).
- R2-B7 (Low, #79's pin): fixed. The contract has a revision line, and decisions.md tells #79's
  owner to re-pin.
- R2-B8 (Low, `--privsep` and S5): fixed.
  - The S5 exception matches #74 (design.md:166–168). It is the only per-attach fault there that
    ends the process: a malformed reply fails only its call, and EOF means the helper is gone.
  - #74 tests it (`start_without_reply_times_out`, #74 design.md:288).
  - The variant's steps 1–7 match #74's Each attach (#74 design.md:173–180).
  - Two direct-mode steps are not carried over (R3-C3).
- Nits (#75's timer, S9's `--privsep` pointer, the record grammar): fixed.

### R3-B1 — Medium
**The id walk stalls on an id that a deleted but still-open device holds, so every attach on the
node fails until that device is closed.**
After `DEL_DEV`, the device is removed: `ublkc<N>` goes. Its id is freed only when the last open
of the device is closed. #74 relies on both: the id is freed at release (#74 design.md:84–85), and
`held_device_does_not_block_cleanup` sees `ublkc<ID>` gone while `/dev/ublkb<ID>` is held
(:133–134). #15 relies on it too: the next cleanup finds the device until the close
(design.md:445–448).
On such an id, `ADD_DEV` fails with no `ublkc<N>`, so the walk "tries the same id again"
(design.md:103–112) until the 10 s deadline. The walk never moves past it, so each later attach
does the same.
How it happens on 6.9 and later (the lab runs 7.0):
1. `DEL_DEV_ASYNC` removes the record at once (design.md:184).
2. `first_id` counts only `ublkc*` and the records (design.md:97–102). So at the next start the
   held id is invisible.
3. Example: V at id N (the highest id) is deposed. The device is deleted while the pod's mount, or
   kubelet's raw block loop device, still holds it. The data plane then restarts with `first_id`
   ≤ N.

This breaks spec S2's new "An id another device holds is skipped" (spec.md:47–48). The round-2
rule ("any errno moves on") skipped this id.
Fix:
- Move on when `ublkc<N>` exists or `GET_DEV_INFO` at N answers (it answers until the release,
  as the cleanup already assumes). Or cap the retries of one id, for example at 4.
- Say where the walk resumes after an attach that ran out of time at an id.
- Test on the lab VM (6.9 or later): the test adds a device at the node's next id, holds
  `/dev/ublkb<N>` open, and deletes it (`DEL_DEV_ASYNC`). The attach takes N+1 within its
  deadline.

### R3-C1 — Low
**The new `node.sh down` test contradicts #74's test in the same file, and the "without records"
branch it tests has no mechanism.**
- #15's test says `down` "keeps `del --id` before the stop" for a build without records
  (design.md:817–820).
- #74 makes `down`, "in both modes", stop the unit and then `del` (#74 design.md:209–211). Its
  `node-scripts-test.sh` asserts "`down` stops the unit before `del`" (#74 design.md:290).
  Whichever plan lands first creates that file.
- Nothing says how `down` tells whether a build writes records.
- The branch is not needed: `bench-ab` runs each revision's own `node.sh`, and main's keeps its own
  `del --id` (decisions.md:210–211; design.md:472–474).

Fix: drop the without-records branch from #15's `node.sh` and from its test. Test only: the mount
guard, the stop, then `flaminio-io cleanup`. If the branch stays, say how it is detected and that
it follows #74's order once #74 is in.

### R3-C2 — Low
**The plugin's new comparison cases cannot each be reached as the test is written.**
The S9 test refuses three cases (design.md:731–736):
- the reply's `major:minor` differs from sysfs;
- it differs from the fd's rdev;
- its diskseq differs.

Each case needs the other checks to pass:
- With the real `checkUblkDevice`, a host without ublk fails at the path or `Lstat`.
- With #74's `CheckDevice` hook stubbed, the read of `/sys/class/block/ublkb<N>/dev` fails for
  lack of the file, so the "differs from sysfs" case passes even with the comparison deleted.
- "Differs from the fd" while equal to sysfs needs the fd to differ from `Lstat`, which only a swap
  produces.

R2-B4 gave the daemon's open step a seam; the plugin's comparison has none.
Fix: the comparison takes the device path and the sysfs root as parameters. The test makes a block
node named `ublkb<N>` (a loop device's rdev) and a fake `ublkb<N>/dev`, and varies one field at a
time. Each case is refused with `codes.Internal` and no probe.

### R3-C3 — Low
**The `--privsep` device thread leaves out two of direct mode's steps without saying so.**
1. **The wait for `/dev/ublkbN`.** Direct mode waits for udev before it answers (design.md:126;
   decisions.md:78, "The node waits for the device node to appear before it answers"). Steps 1–7
   answer right after the sysfs read (design.md:564). The plugin's `Lstat` can then race udev, and
   fail with `Internal`. A `stat` opens nothing, so #74's "opens no ublk path" allows the wait.
2. **What follows the queue join.**
   - Direct mode purges the router and removes the arrivals counter, and ends or stops the entry
     under its lock (design.md:177–190).
   - In direct mode, step 4 does not wait for a `DEL_DEV` thread. Step 7's `delete` through the
     helper can block with no bound while the device is held (#74 outcome 3).
   - Placed before the entry's end, it leaves a deposed, mounted volume stopping rather than
     stopped.
   - "The same in both modes" (design.md:568–570) does not fix this order.

Fix: add the udev wait to the variant. List the post-join steps, with `delete` after the entry is
marked stopped, or state the difference. Add both to the variant's test.

## Nits
- design.md:406–411: "the node takes an exclusive `flock`" is not limited to direct mode. Under
  `--privsep` the I/O process cannot open the root 0700 directory, and writes nothing under
  `/run/flaminio`. Say: direct mode only.
- design.md:377–378: "Any failure of steps 2–4 is `codes.Internal`" includes step 4.
  - A changed second `device-info`, such as stopping or a new generation, is a state change, not
    "a device the kernel contradicts" (decisions, R2-B3).
  - The same stopping state at step 1 gets `FailedPrecondition` (design.md:362).
  - Limit `Internal` to steps 2–3 and the check after mounting.
- design.md:509–511: `MarkFs` lists four steps. The tests also drive these through it:
  - `epochs/`'s creation and its parent's fsync (design.md:785–788);
  - the failing create or write (:789).

  List the directory creation, the leftover `.tmp` unlink and the exclusive create in the trait.

Reviewed commit: 28711dd79f736eb9b014d691426184f611bcfb18
