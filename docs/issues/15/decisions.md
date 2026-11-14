# Issue #15: more than one volume per node — decisions

Settled in plan reviews 1–4. Reviewers take these as given. Each line gives the choice, the
reason, and where it came from: rounds 1–4, prefixed F, M, Q and U; "author" for the first draft.

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.
- Plan review round 1 (2026-11-13) fixes, made after approval: see below.
- Plan review round 2 (2026-11-14) fixes, made after approval: see below.
- Plan review round 3 (2026-11-14) fixes, made after approval: see below.
- Plan review round 4 (2026-11-14) fixes, made after approval: see below.
- Plan review round 5 (2026-11-14) fixes, made after approval: see below.
- Plan review round 6 (2026-11-14) fixes, made after approval: see below.
- Plan review round 7 (2026-11-14) fixes, made after approval: see below.
- Plan review round 8 (2026-11-14) fix, made after approval: see below.

Approved with the recommended answer to each open question (see Settled at approval). The plan's
review loop continues after approval; later fixes are recorded as changes made after approval.
Nothing is implemented. Approved before its plan review cleared (status: pending).

## Settled at approval

- **The per-node volume limit's default** ("to be approved"): 8. `spec.md` S1 (the limit);
  `design.md` S1 (`--max-volumes`, default 8); here, Limits and the control socket.
- **The engine thread's hold bound** ("to be approved"): its longest lock wait and its
  heartbeats' lateness stay under 100 ms with two busy volumes. `spec.md` S3 (holds are short);
  here, The engine thread.
- **One volume's performance bounds against main** ("bounds to be approved"): reads (zero copy,
  4K QD1, 4K QD32, 1M QD8) at most 3% lower; writes 4K QD1 at RF 1 and RF 2 at most 5% lower;
  1M QD8 at RF 1 at most 5% lower, with #10's rule; a job whose spread is too large repeated once,
  then reported inconclusive. `spec.md` S12; `design.md` Measurement (S12); here, Performance.
- **The proposed cross-issue amendment from #79 R1:** taken as it states: the additive
  `device-info` and `detach-device` commands, their strict grammar, locks and tests, as
  [device-lifetime-contract.md](device-lifetime-contract.md) gives them; #79's lifecycle effects
  fail closed on a data plane without them; an ownership mismatch leaves foreign mounts in place.
  Here, Proposed cross-issue amendment from #79 R1; `design.md` S8 and S9.

## Choices

**Scope and dependencies**
- Built after #10, and measured against main with #10. #10 makes the arrivals and the commit
  settings per device, and they become per node here. (F12)
- Depends on #30: a volume created again starts above every earlier epoch of its name. Tokens and
  fences of an earlier volume of the same name are #30's, not this issue's. (U7)
- Targets without a device follow from the engine thread. The engine already runs them in tests,
  and an attach without a device is #17's. (F13)

**The engine thread**
- It alone owns the inputs, and waits on the node's eventfd with a nanosecond timeout: the
  earliest of the turn and the gather, hold and arrivals deadlines, and #75's QoS release (added
  after approval: R1-C8). A thread that arms a deadline signals it. (F1; Q-round Low:
  `ARRIVALS_LIMIT` was never armed)
- Queue threads keep submit and pump under the engine's lock: no hand-off on the write path.
  (F1)
- Fast-read admission stays under the engine's lock, as R3 built it. A lock-free check is the
  next step only if the bounds fail, simulator first. (author)
- The engine thread handles at most 64 inputs per hold. It logs its longest lock wait and its
  heartbeats' lateness. A fair hand-off is added only if the 100 ms test fails (follow-up).
  (F1, M7, Q9, U5)
- One more wake-up per commit completion and remote reply is accepted. The bounds measure it.
  Queue 0 does not also poll the eventfd: two consumers would steal wake-ups. (Q10)
- Commits are asynchronous while a device runs, and synchronous on a node with none, as today.
  Turning them on for replica-only nodes is a follow-up, after the full RF 2 set is measured.
  (F3)
- No device control runs on the engine thread, nor under the engine's or the registry's lock.
  Stopping and deleting run on the device's own thread. (M1)

**Devices and identity**
- A registry entry `(volume, id, generation, state)` per attach, under one lock; every action
  names its generation. (F4)
- Generations go end to end: on attach, detach and I/O inputs, on every output, and through the
  fast-read path. `Detach` always acknowledges. Stale outputs are dropped and logged. (M2,
  security Low 7, U Low)
- Op ids are unique node-wide and never reused in a run. (F5)
- Ids only increase, starting above the highest ublk id present. An id the kernel refuses is
  skipped, whatever the errno, and the refused add deletes nothing (changed after approval:
  R1-C1). `add_device`'s own-id retry and a "highest id" rule were cut. (F4, M3) Round 2, after
  approval (R2-B6): the walk moves on only when another device now holds the id
  (`ublkc<N>` exists), one add per 250 ms, until `add_device`'s deadline. Round 3, after
  approval (R3-B1): held means `GET_DEV_INFO` answers, which covers a deleted device still open;
  an id not held is tried at most 4 times. Round 4, after approval (R4-B2): an id not held is
  tried until the attach's deadline, and skipped only as the attach fails; no id above 1048575
  is tried, and once none is left every attach is refused until a restart. Round 5, after
  approval (R5-1, R5-2): every add is paced, held id or not; the refusal carries `limit:`.
  Round 6, after approval (R6-1): at most 41 adds per attach, as today's loop; the ceiling's
  refusal comes without the pace's wait. Round 7, after approval (R7-L1, R7-L2): the next id
  is chosen, and the ceiling checked, before the deadline check and the wait; the ceiling's
  text wins.
- The node holds a read-only fd on each device from ready until the CSI's detach, not on
  deposition, so its minor stays taken while it may be published. (M3)
- The node waits for the device node to appear before it answers. (Q Low)
- A random instance id per run, of 64 bits or more. (M3, Q nit)
- Deletion of a device is never synchronous. Where the kernel lacks the asynchronous delete, the
  synchronous one runs outside every lock. (Q2)
- Startup deletes only recorded, dead devices whose recorded dev_info matches, without user
  recovery, never by pid. Records are written before the device is added, and removed after the
  delete; a crash's leftover libublk JSON goes with them. (M4, Q3, U Low)
- Stopping entries hold a slot, an fd and a thread until the CSI's detach. They are listed and
  logged when old. Root's detach is the remedy. (security Low 8, U Low)

**Limits and the control socket**
- A per-node volume limit, 8 by default (settled at approval), reserved atomically. It counts
  every entry and late-started devices. (author, F9) It is 1 to 16, and 1 under #74's
  `--privsep` (changed after approval: R1-S5, R1-S1).
- Attaches are taken one at a time. Overlapping metadata phases are a follow-up. (F9)
- The token's signature, volume and node are checked before a slot is reserved, and the epoch
  after `GetVolume`. (M8)
- Volume names are checked by the metadata service's rule everywhere, in the CSI node plugin
  too. (Q1)
- `attach`'s answer and plain `status` keep today's format, so an older CSI node plugin works on
  one volume. A per-volume status and a list are new. (F11, author)
- A stable prefix marks the limit refusal: `error: VOLUME: limit:` (F11; changed after approval:
  R1-C9, R1-S11).
- The control socket's hardening (a read timeout, a connection cap) is not done here: the socket
  is root-only, and #12 moves it. (M-round Low)

**Tokens and epochs**
- Attach tokens are verified: signature, volume, node and epoch. This fixes a gap that predates
  this issue. (F6)
- An attach is refused at an epoch at or below the volume's durable run mark (it replaced the
  in-memory `ran_epochs`: changed after approval, R1-S2, R1-S4), or at or below the local
  replica's durable fence for the volume. After a crash, the restart path gets a new token. (Q6,
  U1)

**Deletes**
- The engine's existing refusal while a target or an attach is pending is kept. The socket also
  refuses while the volume has any registry entry: a starting entry has nothing in the engine yet.
- `begin_delete` first makes a fence at `u64::MAX` durable, then forgets the volume. A crash
  mid-delete leaves the chunks unusable, even by a volume made again under the same name (#30).
- Each turn deletes `k` chunks in one commit, with `k` sized so that a 4 TiB volume finishes
  within #6's 50 s.
- A chunk's fence stays durable until the chunk is gone.

Rounds Q7 and U4, and the full review; "still deleting" timeouts were cut.

**Panics**
- Panics abort: in the release profile, and through a hook installed before any thread. A host
  test, not a lab test. (F7, Q10)

**The CSI node plugin**
- `MaxVolumesPerNode` comes from the data plane's list. It is 1 on an older data plane, and an
  error when there is no answer. (F11, Q Low)
- Identity is checked by `fstat` of an opened fd, a second status, and the new mount's device
  checked after mounting by path. `/dev/fd/3` for mkfs was cut: the held fd and ids never reused
  cover it. (Q5, U5) Superseded after approval by the
  [contract](device-lifetime-contract.md) (R1-C3): `device-info` twice around the open, with
  diskseq and the ublk-disk check; the tools take the path because the plugin's fd holds the
  device's ublk id, not because ids are never reused (that holds only within a run).
- Existing mounts are checked against the volume's status, and refused on mismatch. Filesystem
  mounts are checked by mountinfo, and raw block by `stat` of the target. (U2) Changed after
  approval (R1-C3): against the contract's checked device; nothing is unmounted.
- Raw block across restarts: kubelet's loop device is expected to pin the minor; a lab test
  decides. If it does not, the plugin re-opens its raw publishes' fds at its start. (Q4, U3)
- One lock per node stays: attaches are serialised anyway. (M-round cut)

**Performance**
- `bench-ab`: 3 repetitions, A and B alternating between repetitions, reads before writes.
  Jobs: reads QD1, QD32, 1M; writes 4K QD1 at RF 1 and RF 2 (one raw run for both), and 1M QD8.
  Reads take under an hour; writes are 24 runs of about 25 minutes, about 10 hours.
  (F2, M6, Q8, U6)
- Bounds relative to main's share of raw: reads 3%, writes 5%, and #10's 1M rule. One more
  repetition when 2 × spread exceeds the bound; then inconclusive, and the owner decides.
  (F2, M6, Q8)
- The two-volume read p99 is recorded, not gated. (U5)

## Accepted risks

- **Deleting a live volume's replicas** through the socket is data loss, as today. Who may delete
  is #12's. (M8, U4)
- **No fairness or QoS between volumes:** a volume's writes may wait for a commit that holds
  another's. (F, author)
- **Settling** holds the engine's lock for the commits in flight and queued, stalling every
  volume, as one volume today. Recorded in the node fault test. (code Low S5)
- **Space is node-wide:** a volume that fills the disk fails other volumes' writes there. (security
  Low)
- **Panics are not isolated:** the node aborts, and every volume fails over. (F7)
- **A store error** fails every volume's writes on the node. (author)
- **A restart ends every device.** There is no user recovery. (author)
- **The raw block gap** while the CSI node plugin restarts, if kubelet does not pin the device.
  (Q4) Its impact, restated after approval (R1-S10): ublk disks take dynamic `blkext` minors
  (major 259) from the pool NVMe namespaces use (on the drafting host `nvme0n1` is 259:0), so a
  stale bind in the gap may open any `blkext` disk on the host, a host NVMe namespace included.
  Under #74's `--privsep` the node holds no device fd, so the gap also opens when a deposed raw
  block volume's device goes (R1-S1). Re-accepted as restated under the user's instruction of
  2026-11-13 ("Continue unattended and choose the recommendations"), the plan's own position
  since Q4, as the batch's orchestrator applied it; listed for the user to check.
- **The lab's 1M write campaign** wears the drive by about 7.5 TB. (Q8)

## Added in the full review

- **The device thread's control loop is libublk's ready callback.** `run_target` runs that
  callback before it joins the queues, so the loop can `kill_dev` from there.
- **One router in `Node` delivers every output:**
  - I/O by op id;
  - attach results by `(volume, generation)`;
  - `Deposed` to the device's stop channel.

  Queue threads no longer drain inputs. Changed after approval (R1-C2, R1-C10): the I/O routes
  live under the engine's lock, and each thread routes I/O outputs before it lets that lock go.
- **The main thread waits for an attach's result,** and on a timeout sends `Detach` with the
  generation.
- **The router purges a generation's ops** when its `run_target` returns.
- **No automatic re-attach after a crash.** The old token is refused. The lab attaches again with
  `cmd_vol attach`, which takes a new token.
- **The startup `--attach`** takes the socket's reserve-and-token path.
- **Its retry stops** once that volume has an entry, or at an epoch refusal. Today it stops once
  any volume is attached.
- **A detach ends the target before `kill_dev`,** as today, so that a target stuck on its ISR
  cannot block the device's stop. The socket waits 30 s at most.
- **One lock per entry covers its state and its detach flag.** The device thread's last step
  ends the entry if a detach was requested, or else marks it stopped. A detach ends a stopped
  entry itself, and otherwise sets the flag. A timed-out detach leaks nothing, and a detach
  waits only on the generation it found.
- **Shutdown uses `process::exit`,** and does not join a thread blocked in `kill_dev`. Startup
  cleanup retries a recorded device that is not yet dead for a few seconds, then keeps its
  record and logs it.
- **A record goes only when its delete returns `Ok` or `ENODEV`:** at once with
  `DEL_DEV_ASYNC`, and when the `DEL_DEV` thread returns before 6.9. Otherwise it stays, for the
  startup cleanup.
- **`DEL_DEV_ASYNC` is chosen by the kernel's release** (`uname`'s major and minor, 6.9 and
  later). Otherwise, or if it cannot be parsed, the control handle is disowned. `flaminio-io
  del` takes the same rule (changed after approval: R1-C11).
- **`bench-ab` runs each revision's own `node.sh`.** Main writes no records, and needs its
  `del --id`, which main's own `node.sh` keeps; #15's `node.sh` has no such branch (round 3,
  after approval: R3-C1).
- **The startup record:**
  - written `O_EXCL`, so an id whose record exists is skipped; removed on any `ADD_DEV` failure,
    and when `GET_DEV_INFO` gives `ENODEV`;
  - written before `ADD_DEV` with the volume, instance and generation, then replaced atomically
    with the `dev_info` that `ADD_DEV` returned, which the kernel rewrites. All of it is compared
    at cleanup. A record without `dev_info` is logged and left, unless its device is gone;
  - `first_id` counts the recorded ids, is at least 0, and `--id -1` sets no floor.
  - its trust, added after approval (R1-S6): a root-owned 0700 directory checked by `lstat`,
    `O_NOFOLLOW` opens, `O_EXCL` for the `.tmp`, names `^[0-9]{1,7}$`, at most 4096 bytes; `node.sh
    down` deletes by the startup cleanup's rule (`flaminio-io cleanup`). Round 2, after
    approval: names `^(0|[1-9][0-9]{0,6})$` (R2-N3); an exclusive `flock` on the directory for the
    node's life, which `cleanup` must take (R2-B6). Round 3, after approval: the `flock`, like
    the records, is direct mode's only (R3-N1).
- **Lock order:** the registry, then the engine. The arrivals counters live in the engine.
- **The device thread's failures** (`add_device`, `start_dev`, `START_LIMIT`) each answer the
  attach, send `Detach`, remove the record, and end the entry.
  - **Before a queue's loop:** fallible setup moves before `run_target` where it can. A queue that
    still fails tells `start_dev`: by `notify_buffer_registration_complete(true)` without zero
    copy, and by a `START_LIMIT` watchdog with zero copy. If the watchdog cannot end `START_DEV`,
    the node aborts. Superseded after approval (R1-C5): `notify_buffer_registration_complete`
    in both modes, with the vendored libublk waiting under `UBLK_F_AUTO_BUF_REG`; no watchdog,
    no abort.
  - **After `run_target` returns `Err`:** a stop flag and each queue's eventfd end the queue
    loops. Exits are counted by drop guards, and the device is deleted once all are counted.
    Changed after approval (R1-C7): `kill_dev` ends the pending FETCHes, and `event_loop` breaks
    on the stop flag itself.
- **A late-started device** reached running after the socket had answered its attach as timed
  out. It stays running, and counts against the limit.
- **`pending_delete`** clears only when a delete finishes, even after a store error that does not
  poison the store. A running delete is tracked apart from it, so a later `delete` restarts one
  that a store error stopped.
- **One wire prefix for every epoch refusal:** `error: VOLUME: epoch: <reason>`. The CSI node
  plugin maps it to `FailedPrecondition`.
- **The CSI node plugin maps an epoch refusal to `FailedPrecondition`.** Recovering takes a new
  `ControllerPublishVolume`.
- **`write-load.sh`** runs by size, not by runtime, so that its verify reads back everything
  written, and keeps one output per run.
- **`node-fault` with `OTHER`** checks the other volume through fl-b's restart only. Running it
  with the volumes swapped covers both failovers.
- **The CSI node plugin:**
  - it keeps step 2's fd open through `mkfs`, the mount and the check;
  - an attach's timeout is `DeadlineExceeded`, and a starting entry `Unavailable`, not
    `Internal`.
- **The running-device count** is kept under the registry's lock, and decides the asynchronous
  commits.
- **Lab:**
  - `make multi-vol` checks two volumes, and the deposition of one;
  - `make node-fault` takes `VOL` and `OTHER`;
  - `bench-ab` builds each revision with `git archive` into its own path, and passes the binary
    through `node_env`. Changed after approval (R1-S8): revisions resolved to their hash, which
    names the path; new values validated and quoted with `printf %q`.
- **No token expiry is checked.** Tokens carry `issued_at` only, as for sessions today.

## Review

- **The review loop ended with only Lows,** all fixed in the last revision.


## Proposed cross-issue amendment from #79 R1

The earlier review choices/records above remain historical; this amendment records no approval.
Reviewers review79_sec_r1 and review79_code_r1 found #79's status/open and name-only detach
races (R1-S2/R1-S3). The proposed [device lifetime contract](device-lifetime-contract.md) adds
expected FirstEpoch/boot/instance/generation/rdev/diskseq and server-conditional detach once here.
The earlier new-plugin/old-daemon fallback is restricted to read-only discovery when #79 is
used: lifecycle effects fail closed without this profile. Ownership mismatch preserves foreign
mounts, superseding the earlier unconditional mismatch-unmount recommendation for #79 paths.
No runtime implementation, test result, accepted risk or new architectural acceptance is implied.

## Changes after approval: plan review round 1 (2026-11-13)

Made after the approval of 2026-11-13, in round 1 of this batch's plan review of commit `976e7d5`
(plan-reviewer (design-code), plan-reviewer (design-security)). Ids here are that round's, not
#79's R1 ids above. Nothing is implemented or run; no approval or accepted risk is added.

**Narrowed, because the finding shows the rule cannot hold as stated:**
- **R1-S1 (High):** #74 (approved) forbids under `--privsep` what #15 put in the I/O process:
  choosing ublk ids, opening `/dev/ublkbN`, writing under `/run/flaminio`, and owning more than
  one device (#74 S2, S3). So under `--privsep` the node takes one volume (`--max-volumes`
  defaults to 1 there, and a larger value is refused at parse); S2's ids and held fd and S10's
  records and cleanup are direct mode's; add, start, stop and delete go through #74's helper
  requests; `device-info` reads `major:minor` and diskseq from sysfs. Whichever of #15 and #74
  lands second adds the parse refusal and, in `node.sh`'s `PRIVSEP=1` branch, the run mark's
  directory. One owned id per volume is #74's next step. The lab's checks with several volumes or
  `ATTACH` run with `PRIVSEP=0`. Spec header and S1; design, With #74's helper; the contract.
- **R1-S5 (Low):** `--max-volumes` is 1 to 16, so that `list`'s longest answer (2 220 bytes)
  fits the 4096-byte cap #74 puts on what `NodeGetInfo` reads. Chosen over a fixed-size `limit`
  command: the bound adds no command. Spec S1; design S1, S8.

**Fixed without narrowing a rule:**
- **R1-C1 (High):** a failed `ADD_DEV` deletes nothing: the vendored libublk 0.4.8 with #74's
  failed-add hunk, which whichever of #15 and #74 lands first vendors. Any errno moves to the next
  id, as libublk's legacy-opcode retry turns `EEXIST` into `EOPNOTSUPP` (narrowed in round 2,
  R2-B6: only when `ublkc<N>` now exists). Kernel-chosen ids (`-1`)
  were not taken: the kernel hands freed ids out again, against S2. Test: a foreign device at the
  next id survives, and on 6.8 nothing blocks. Spec S2; design S2, tests.
- **R1-C2 (High):** each thread routes `Done`, `LocalRead` and `Piece` before it lets the engine's
  lock go, so a request's outputs reach it in production order. New rule in S2 (a read's data
  before its end); test with the engine thread paused before it routes, which fails if routing
  moves after the unlock. Design, The router, S3, S4.
- **R1-C3 (Medium):** the boundary with #79 is stated (design S9): #15 ships the plugin's device
  check through the contract, its check of a mount in place against that device, and the legacy
  one-volume path on a data plane without `device-info`; #79 ships ownership, mount topology,
  tools on the inherited fd, the conditional-detach client, and the end of the legacy path for
  effects. "#79 enabled" is the plugin build that includes #79, with no flag or probe. Q5/U5 are
  superseded by the contract (Choices, The CSI node plugin). #74's `checkUblkDevice` and answer
  cap: whichever lands first adds them.
- **R1-C4 (Medium):** the reservation is a guard the queued request owns; a request skipped for
  its deadline is answered, and dropping any request never begun ends its entry. Spec S1; test.
- **R1-C5 (Medium):** no watchdog and no abort. A zero-copy queue that fails before its loop
  tells `start_dev` as without zero copy, through a second vendored-libublk hunk that waits for
  queue registration under `UBLK_F_AUTO_BUF_REG` too; `START_DEV` is never sent. Chosen over
  interrupting `START_DEV`: libublk's control ring is the blocked thread's own. Spec S5 names the
  case; test.
- **R1-C6 (Medium):** named tests for S5's space case and its log line, S2's stopping-entry log,
  and S7's 4 TiB bound (a store test of the commit count, a lab timing on fl-b).
- **R1-C7 (Low):** after `run_target` returns `Err`, `kill_dev` ends the pending FETCHes (also on
  a device never started), and `event_loop` breaks on the stop flag itself.
- **R1-C8 (Low):** `next_wakeup` covers every engine deadline, #75's QoS release included; arming
  any deadline signals the eventfd. Whichever of #15 and #75 lands second folds it in.
- **R1-C9 (Low), R1-S11 (Nit):** one root cause: the limit error is `error: VOLUME: limit: N
  ...`, and the plugin matches the exact prefix with the requested volume, as for `epoch:`.
- **R1-C10 (Low):** the I/O routes live under the engine's lock, which every submit and pump
  already takes: no lock is added to the I/O path. Per-device maps were not taken, as routing
  under the engine's lock is also what gives R1-C2's order.
- **R1-C11 (Low):** `flaminio-io del` takes the release rule; tested on 6.8 and 7.0.
- **R1-S2 (Medium):** the run mark: per volume, the highest epoch a target of it started at here,
  in `--state-dir`, made durable by the main thread between the engine's checks and the target's
  start, so before any `Hello`, local or remote. It holds for a target with no local chunk. New
  simulator check (two target incarnations at one epoch fail the run, with stale `Start`s after
  restarts) and injected bug `volatile_run_mark`; engine test with no local chunk. The finding's
  other choice, narrowing S11 with an accepted risk, would need an acceptance this round cannot
  give. Spec S11; design S11.
- **R1-S3 (Medium):** the contract's DEVICE is `/dev/ublkb<N>`, with sysfs's `dev` equal to both
  the reply's and the opened fd's; the daemon opens `O_NOFOLLOW` and checks `S_ISBLK` and sysfs
  before running. Test: a reply naming a loop device is refused with no effects.
- **R1-S4 (Low):** resolved by R1-S2's cut of `ran_epochs`: the run mark is keyed by volume.
  Test: B taken at an epoch A ran, A refused again.
- **R1-S6 (Low):** the records' trust model (design S10) and `node.sh down` through `flaminio-io
  cleanup`, the startup cleanup's rule; host tests, a symlinked `.tmp` among them.
- **R1-S7 (Low):** lab tests of the matching branch: a dead device at a recorded id with another
  `dev_info`, and one with no record, are both left.
- **R1-S8 (Low):** `bench-ab` revisions resolved to a hash, `DEV`, `VOL2`, `OTHER` validated,
  each quoted with `printf %q`, `FLAMINIO_IO` kept under the bench root; script tests.
- **R1-S9 (Low):** S11 and intent 7 restated against main: `decode_unverified`, its test and the
  `unverified` feature go; `first_epoch` comes from the verified claims.
- **R1-S10 (Low):** the raw block gap's impact restated (Accepted risks), and re-accepted as
  restated under the user's instruction of 2026-11-13 (the orchestrator's call, listed for the
  user). The raw block lab test also checks that the stale bind is no other disk's.
- **Nits (code):** spec S4 names S12; "to be approved" dropped from the limit's line here; the
  design says where the commit settings are applied (The shape).

## Changes after approval: plan review round 2 (2026-11-14)

Made after the approval of 2026-11-13, in round 2 of this batch's plan review of commit `fe35c40`
(plan-reviewer (design-code and design-security, one agent)). Each takes the reviewer's
recommended fix; where it offered options, the simplest safe one, with the reason. One rule
gains an exception: S5 under `--privsep` (R2-B8), as #74's approved rule makes it unable to hold
there. Nothing is implemented or run; no approval or accepted risk is added.

- **R2-B1 (Medium):** a leftover run-mark `.tmp` is unlinked before each exclusive create and on
  any failed write, and stray ones are removed at start, as the records' are. It never became the
  mark, so removing it is safe. Tests: a leftover `.tmp`, and a failed write in a read-only state
  directory, each leave the next attach at a higher epoch working. Design S11.
- **R2-B2 (Low):** the mark's file steps go through a `MarkFs` trait; a test sees the write, the
  file's `fsync`, the rename and the directory's `fsync` in order before `Input::Marked`, so
  dropping an `fsync` fails it. Each directory the node creates for marks is `fsync`ed in its
  parent. New `flaminio-io` tests for a failed write (answer, `Detach`, entry ended, no `Hello`)
  and a malformed mark; the across-restart refusal test moves to `flaminio-io`, where marks are
  read; the engine test keeps the held target and the absent `Hello`.
- **R2-B3 (Low):** #74's `checkUblkDevice(path)` is kept as #74 defines it (approved,
  review-clear). #15's comparison is separate: the reply's MAJ:MIN against sysfs and against the
  fd's rdev (`fstat`), and the diskseq. One code for a device that fails any of these:
  `codes.Internal`, #74's. It fits: the data plane answered a device the kernel contradicts, a
  fault in the node, not in the request, and kubelet retries either way. So #74 needs no change. A
  mount in place on another device stays `FailedPrecondition` (another volume's target). Spec S9,
  design S9, the contract, tests asserting the codes.
- **R2-B4 (Low):** the daemon's open step takes the path and the sysfs root as parameters; a
  test with a symlink, a character device and a block node under a mismatched sysfs `dev` fails
  each attach, keeping no fd.
- **R2-B5 (Low):** the routing-order test's hook releases after a fixed 200 ms, and never waits
  for the queue thread. Chosen over detecting the queue thread blocked on the lock: simpler, and
  still fails if routing moves after the unlock, without deadlocking when it does not.
- **R2-B6 (Low):**
  - The node holds an exclusive `flock` on the records directory for its life, and
    `flaminio-io cleanup` refuses while it is held (the reviewer's first option, which makes the
    unsafe case impossible rather than unsupported).
  - Before 6.9, `cleanup` waits up to 5 s in all for its `DEL_DEV` threads, then keeps the
    records of devices still held.
  - `node.sh down` keeps today's mount guard, for `$dev` and every recorded device.
  - Script test of `down`.
  - The id walk moves on only when `ublkc<N>` now exists, at one add per 250 ms, until
    `add_device`'s deadline. The reviewer marked this optional; taken because it is what keeps
    an unrelated failure from burning ids.
- **R2-B7 (Low):** the contract carries a revision marker (revision 3 is this round's commit).
  For #79's owner: #79 must re-pin from `8af7aeb` to revision 3, and add to its client #15's
  comparison of the reply's MAJ:MIN with sysfs and with the fd, beside #74's `checkUblkDevice`.
  Round 3 made it revision 4, and round 4 revision 5: #79 must pin revision 5, commit `95a1dba`
  (R3-N2, R4-N1, R5-N1).
- **R2-B8 (Low):** the device thread's `--privsep` variant (#74's attach steps in place of
  `run_target`) is stated in the design, not left to #74's follow-up item 5: it belongs to the
  second of #15 and #74 to land, with its tests, the sysfs `device-info` among them. Without it,
  the lab's default mode would not attach once both are in. #74's fatal `start` limit is listed
  in S5 as an exception under `--privsep`. It is #74's approved rule, not a new acceptance.
- **R2-N1:** #75's QoS timer is described as #75's design, not as today's code.
- **R2-N2:** S9's raw block rule points to its `--privsep` exception (Accepted risks).
- **R2-N3:** record names `^(0|[1-9][0-9]{0,6})$`, #74's grammar; `01` is refused.

## Changes after approval: plan review round 3 (2026-11-14)

Made after the approval of 2026-11-13, in round 3 of this batch's plan review of commit `28711dd`
(plan-reviewer (design-code and design-security, one agent)). Each takes the reviewer's
recommended fix; where plans differ, #74's, which is approved and review-clear. No rule is
narrowed. Nothing is implemented or run; no approval or accepted risk is added.

- **R3-B1 (Medium):** the id walk moves on when the id is held, which now means that
  `GET_DEV_INFO` at it answers: a live device, and a deleted one still open, whose `ublkc<N>` is
  gone but whose id is freed only at the last close. An id not held is tried at most 4 times,
  then skipped (ids need not be dense; the cap ends any stall neither check sees). The next
  attach starts above the last id tried. Both of the reviewer's options are taken, as each covers
  what the other might miss. Tests: the walk on a fake kernel; on the lab's 7.0, a deleted but
  open device at the next id, which the attach passes for N + 1 within its deadline.
- **R3-C1 (Low):** following #74, `node.sh down` stops the unit before it deletes, in both modes.
  #15's branch for a build without records is dropped (main's own `node.sh` keeps its `del --id`
  for `bench-ab`), and so is its test. The `down` test checks the mount guard, then the stop, then
  `flaminio-io cleanup`.
- **R3-C2 (Low):** the plugin's comparison takes the device path and the sysfs root. A test with a
  block node named `ublkb<N>` (a loop device's rdev) and a fake sysfs `dev` reaches each of the
  three cases. Each is `codes.Internal` with no probe, and the sysfs case fails the test if the
  comparison is removed.
- **R3-C3 (Low):** the `--privsep` device thread waits for `/dev/ublkb<DEV>` by `stat` before it
  answers, as direct mode does. After the queue join it purges the router, removes the arrivals
  counter, and ends or stops the entry under its lock, and only then calls `delete` through the
  helper. That wait is bounded only by the device's last close, as #74 accepts (outcome 3), and
  nothing of #15's waits for it. Tests for the late node and for a blocking `delete`.
- **R3-N1:** the records directory's `flock` is direct mode's only.
- **R3-N2:** `codes.Internal` covers only the device checks (steps 2 and 3, and after mounting).
  A second `device-info` reply that differs is a change of state, answered as step 1's state
  would be. This is changed in the design, the spec and the contract, which is now revision 4.
- **R3-N3:** `MarkFs` lists every operation the tests drive: a directory's creation and its
  parent's `fsync`, the `.tmp` unlink, the exclusive create, the write, the file's `fsync`, the
  rename, and the directory's `fsync`.

## Changes after approval: plan review round 4 (2026-11-14)

Made after the approval of 2026-11-13, in round 4 of this batch's plan review of commit `baee5c8`
(plan-reviewer (design-code and design-security, one agent)). Each takes the reviewer's
recommended fix, cutting where it could. No rule is narrowed. Nothing is implemented or run; no
approval or accepted risk is added.

- **R4-B1 (Low):** `node.sh down` is stated per mode. Both modes check the mount guard and stop
  the unit before deleting. Direct mode then runs `flaminio-io cleanup`; `PRIVSEP=1` runs #74's
  `del` of `/dev/ublkc<ID>` (its outcome 5). The test asserts each mode's order. The second of #15
  and #74 to land turns #74's direct-mode assertion into `cleanup`, and keeps its `PRIVSEP=1`
  case for `del`.
- **R4-B2 (Low):** the walk's ceiling. Chosen by cutting, from the reviewer's options: the round-3
  cap of 4 tries goes. An id not held is now tried until the attach's deadline, and skipped only
  as the attach fails, so an unrelated failure burns one id per failed attach. One attach skips
  at most 40 ids (the deadline over the pace), all held but the last. No id above 1048575 (the
  kernel's `UBLK_MAX_UBLKS` and the records' grammar) is tried. Once none is left, every attach
  is refused (`no ublk id left in this run; restart the node`); wrapping was not taken, as it
  would reuse ids within a run, against S2. Spec S2; test on the fake kernel.
- **R4-B3 (Low):** on the `--privsep` failure path, after `stop`, the join and dropping the
  `UblkDev`, the device thread answers the attach, sends `Detach`, purges the router, removes the
  counter and ends the entry. Only then does it send `delete`, the success path's order. This
  only moves #74's outcome-2 report before its `delete`. Tested with a blocking `delete`.
- **R4-B4 (Low):** a 6.8 test, on Centaur's disowned path: a synchronous `DEL_DEV` on the test's
  thread with the device held open, and the attach takes N + 1. The kernel source available here
  is 6.1 (`eve-kernel`, 6.1.112). There `GET_DEV_INFO` answers by `idr_find` until the release,
  which bears out the walk's check. But a synchronous `DEL_DEV` holds `ublk_ctl_mutex` across its
  wait, and `ADD_DEV` takes that mutex. Whether 6.6–6.8 do the same is unchecked. If they do,
  attaches there wait for a held device's last close, and the test shows it. Stated under Risks,
  not accepted.
- **R4-N1:** a changed second reply gets "the state's code: stopping `FailedPrecondition`; none,
  starting or another lifetime `Unavailable`", without "when first seen". This is changed in the
  spec, the design and the contract, which is now revision 5.

## Changes after approval: plan review round 5 (2026-11-14)

Made after the approval of 2026-11-13, in round 5 of this batch's plan review of commit `dea201b`
(plan-reviewer (design-code and design-security, one agent)). Nothing is implemented or run; no
approval or accepted risk is added.

**Narrowed, conditionally, because the finding shows the rule cannot hold as stated:**
- **R5-3 (Low):** the coordinator's decision, under the user's "otherwise figure it out". In the
  6.1 source, a synchronous `DEL_DEV` holds `ublk_ctl_mutex` across its wait for the last close,
  and `ADD_DEV` waits on that mutex. On such a kernel, one device's deletion blocks every other
  volume's attach inside `ADD_DEV`, where no deadline runs, so S5 (and S1's deadlines) cannot
  hold there with several volumes. So:
  - S2's 6.8 lab test becomes a gate before #15 ships, with its result in the report.
  - **Pass** (6.8 does not hold the mutex): nothing changes; 6.6 and 6.7 stay unchecked, and the
    risk stays named for them.
  - **Fail**: on a kernel before 6.9 (as read for `DEL_DEV_ASYNC`; an unparsable release counts
    as before), the default limit is 1, and `--max-volumes` above 1 is refused at start, with a
    stated error. Direct mode on 6.9 and later is unchanged.
  - #74's reading of 6.8.12 (an interruptible wait) does not settle it, as the wait says nothing
    of the mutex.
  - Spec S1; design S1, The 6.8 gate, Tests, Risks.

**Fixed without narrowing a rule:**
- **R5-1 (Low):** every `ADD_DEV` is paced, one per 250 ms, held id or not. So the bound "at most
  40 ids per attach" is true. This was chosen over restating two bounds because it is simpler:
  one rule for every add, and one bound the test checks. A held id costs 250 ms, and held ids
  above `first_id` are rare. The S2 test changes accordingly.
- **R5-2 (Low):** the ceiling's refusal is `error: VOLUME: limit: no ublk id left in this run;
  restart the node`. It reuses the limit's prefix, so the plugin's existing mapping gives
  `ResourceExhausted` with no new prefix; spec S2 names it. Tests are added for a held id at
  1048575, a `first_id` above 1048575 at start, and a successful add at 1048575, beside the
  failing add there.
- **R5-N1:** the contract names revision 5 by its commit, `95a1dba`, and #79 pins `95a1dba`. This
  round changed only the contract's revision line.
- **Format (noted, not a finding):** the orphaned `node-scripts-test.sh` bullet in the design's
  Order rejoined its list.

## Changes after approval: plan review round 6 (2026-11-14)

Made after the approval of 2026-11-13, in round 6 of this batch's plan review of commit `5b20c61`
(plan-reviewer (design-code and design-security, one agent)). No rule is narrowed beyond round
5's. Nothing is implemented or run; no approval or accepted risk is added.

- **R6-1 (Low):** the bound now matches the loop it keeps. Today's loop (`ublk.rs:891–899`)
  checks the deadline after each failed add and waits 250 ms only before it. So one attach tries
  at most 41 adds (40 waits, the last at about 10 s), and skips at most 41 ids. Chosen over
  checking the deadline before each add because it keeps today's loop unchanged; the S2 test
  says 41. The ceiling is checked when the next id is chosen, so its refusal after a held skip
  at 1048575 comes at once, without the pace's wait; the test checks that.
- **R6-2 (Low):** spec S1 states the failing gate's outcome in full: the default is 1 there, a
  larger `--max-volumes` is refused at start with its stated error, and a kernel release that
  does not parse counts as before 6.9. That branch, and its tests, are written only if the gate
  fails (spec S1, design S1), so a passing gate leaves no dead tests.

## Changes after approval: plan review round 7 (2026-11-14)

Made after the approval of 2026-11-13, in round 7 of this batch's plan review of commit `cd787d9`
(plan-reviewer (design-code and design-security, one agent)). No rule is narrowed beyond round
5's. Nothing is implemented or run; no approval or accepted risk is added.

- **R7-L1 (Low):** the loop's order is pinned. After a failed add:
  1. the next id is chosen (the next one if the id is held, the same one if not);
  2. the ceiling is checked;
  3. the deadline is checked;
  4. otherwise the 250 ms wait, then the add.

  So the ceiling's refusal after a held add at 1048575 comes at once. Design S2 no longer says
  "at the next paced attempt". The S2 test asserts, on a fake clock, that no wait passes between
  that held add and the refusal.
- **R7-L2 (Low):** the ceiling is checked before the deadline, so the ceiling's text wins when the
  held add at 1048575 is also the 41st, past the deadline. "Fails with the last error" and "never
  the last add's error" now agree. There is a new S2 case for it.

## Changes after approval: plan review round 8 (2026-11-14)

Made after the approval of 2026-11-13, in round 8 of this batch's plan review of commit `fef9ba1`
(reviewer: plan-reviewer (design-code and design-security, one agent)).

- **R8-L1 (Low):** the S2 id test still said a held id "moves to the next id at the next
  attempt", the phrase R7-L1 removed from the design body. It now says "straight after the
  failed add", as the pinned order does. Wording only.
