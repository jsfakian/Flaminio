# Issue #6: a faster first attach after a restart — design

**Status** (plan review round 1, R1-C7): S2–S4 shipped in pull request #25 (3b5560b). Their
sections describe the code as it is on main, with the values it replaced marked "before #25".
Still to do: S1, S6 (added after approval, R1-S1) and the tests marked *new*. "Later plans" says
what #74 and #15 change in what this design states.

## Measurements

Lab, 2026-10-28, `vol1` (16 GiB, 64 chunks, replicas fl-a and fl-c) attached on fl-a, Linux
7.0, zero copy. Each run detached the volume everywhere, stopped the replica nodes named, then
timed three things on fl-a: the metadata service's attach, the control socket's `attach`, and a
direct read of block 0.

| Replica nodes stopped | Metadata attach | Control `attach` answered | First read, from the start |
|---|---|---|---|
| none | 0.045 s | 0.055 s | 0.115 s |
| fl-c | 0.046 s | 0.094 s | 5.820 s |
| none | 0.035 s | 0.051 s | 0.104 s |

With fl-c stopped, the first read waits for the takeover: `L × (1 + ρ)` = 5.5 s, before the
target may act without fl-c. The control socket answered within 0.1 s all the same. Starting the
device (`START_DEV`) did not wait for the kernel's partition scan, whose reads would have waited
as long. On this kernel the scan runs after the device has started.

## What an attach waits for

From kubelet, inside NodeStageVolume's 2-minute deadline, with the node plugin's lock (`n.mu`)
held throughout (`control/csi/node.go`):
1. `Status` on the control socket, which is instant.
2. `Meta.Attach`, the metadata service's attach. It gives each endpoint up to 10 s
   (`attemptTimeout`), so up to 30 s with three.
3. The control socket's `attach`, in the data plane (`attach_and_serve`):
   - **Target start:** the engine reads the volume and the nodes from the metadata service
     (`GetVolume`, `ListNodes`), then starts the target. Before #25 it waited up to 60 s
     (`drive_until`); since, `METADATA_LIMIT`, 30 s. The metadata client has one queue for all of the node's calls, and tries
     each endpoint for 3 s. When the service is unreachable, its heartbeats can queue up
     ahead of the attach.
   - **Adding the device:** `add_device` retries for 10 s, plus one more try at most, while a
     previous device with the same id goes away.
   - **Starting the device:** libublk's `UblkDev::new` waits up to 3 s for `/dev/ublkcN`.
     `START_DEV` itself takes milliseconds on Linux 7.0.
4. For a filesystem volume: `blkid`, `mkfs` if the volume is empty, and `mount`.

Before #25, the control socket's own limit (120 s) started at step 3. When it failed, its answer
came after kubelet's deadline, and the node plugin reported only kubelet's `DeadlineExceeded`.

## Time limits (S2, S4): shipped in #25

**Data plane** (`dataplane/crates/flaminio-io/src/ublk.rs`, `control.rs`). Named constants in
`ublk.rs`, `pub(crate)`:
- `METADATA_LIMIT` = 30 s: the target start's wait, replacing `drive_until`'s literal 60 s.
- `ADD_DEVICE_LIMIT` = 10 s: `add_device`'s retries, replacing its literal.
- `START_LIMIT` = 10 s: an allowance for starting the device. It covers libublk's 3 s wait, and,
  on a kernel that scans partitions inside `START_DEV`, a first read that waits for a takeover
  (5.5 s). It is not enforced: the kernel call cannot be cancelled. It is part of the sum below.

`control.rs` derives `ATTACH_LIMIT` = `METADATA_LIMIT + ADD_DEVICE_LIMIT + START_LIMIT` = 50 s,
replacing 120 s. `DELETE_LIMIT` is 50 s too (was 60 s). `detach` keeps its 30 s. `attach` takes
its limit as a parameter, so a unit test can pass a short one. The error names the limit it
waited, in seconds with at most one decimal: `error: no answer within 50 s`.

**A deadline for each request.** `Control::Attach` carries `deadline: Instant`: when the
control socket stops waiting, `ATTACH_LIMIT` after it sent the request, or sooner if the caller
said so. The node plugin's `Attach` sends its time left, from `min(Timeout, ctx deadline)`, less
`answerMargin` = 1 s, as a fourth word in milliseconds; the socket takes the smaller of that and
`ATTACH_LIMIT`. A data plane older than this answers the fourth word with its usage line; the
plugin then sends the attach again without it, so the two can be upgraded in either order.
- **When the fourth word cuts the limit** (R1-C5). A NodeStage that spent 30 s in `Meta.Attach`
  has 90 s of kubelet's 2 minutes left: the plugin sends `min(60 s, 90 s) − 1 s` = 59 s
  (`dataplane.go`, `deadline` and `Attach`), and the socket takes `min(59 s, 50 s)` = 50 s
  (`control.rs`, `attach_limit`). Under kubelet's deadline with up to three endpoints, the fourth
  word never cuts the socket's 50 s. It does only for a caller with less than 51 s left, such
  as one with a shorter deadline of its own, or one whose `Meta.Attach` tried more endpoints.
  Its answer then comes, with its cause, before that caller's own deadline.
- **Skipping.** The main thread skips a request that it takes after `deadline − START_LIMIT`,
  when no step would have any time left. It logs that, and answers with that reason, in case
  the caller still waits. Without this, a request that timed out while the node served another volume would
  run after the next detach, with a token nobody uses any more.
- **Bounding the steps.** `attach_and_serve` ends the target's start and the device's adding by
  `deadline − START_LIMIT`. It waits at most `min(METADATA_LIMIT, that − now)` for the metadata
  service, and `add_device` stops retrying at `min(now + ADD_DEVICE_LIMIT, that)`. A request
  that waited its turn gets less time, not the full limits, and the node's own error arrives
  before the socket gives up.
- **What remains.** Only starting the device can outlast the deadline, if it takes longer than
  `START_LIMIT`. The device then serves the volume with nobody told, and a retry finds it
  through `status`. That is direct mode's case; #74's `--privsep` ends the process instead
  (Later plans).
- **The attach at startup** (`--attach`, a lab and manual option) has no caller to retry it. It
  is sent before the control socket opens, so it comes first. After a failure, a thread sends
  it again every 5 s for up to 5 minutes, so that a metadata service still recovering after a
  cluster restart, whether it times out or refuses connections, does not leave the node without
  its volume. On main it stops at an `ok`, once a volume is attached (one volume per node), or
  after the 5 minutes (`ublk.rs:746–769`), and resends the same token after any other failure;
  S6 adds a stop at its refusal (R1-S1).
- **One attach of a volume at a time.** The engine refuses an `Attach` of a volume it is still
  attaching ("already attaching"), so every attach gets an answer.

A flag that the socket set when it gave up, checked at fixed points, would race with a device
starting at that moment, and would leave the steps their full limits.

**CSI node plugin** (`control/csi/dataplane.go`):
- `ControlSocket` gets a field `Timeout time.Duration`. Zero means `DefaultControlTimeout` =
  60 s, replacing `controlTimeout` = 150 s. A caller's earlier deadline still wins, as now. The
  effective limit is computed in one place, `ControlSocket.timeout()`, which the test checks.
  `main.go` sets no timeout, so it gets the default.
- The comment says what the 60 s is for: a backstop for a data plane that never answers, and
  the limit for callers without a deadline (the tombstone sweep). It must stay above the data
  plane's 50 s (`control.rs`).

**Follow-ups.** `docs/followups.md` §10, and its row in the table, say since #25:
- the limits are done, with these numbers;
- the partition scan no longer holds up an attach on Linux 7.0;
- scanning stays on until a libublk release includes ublk-org/libublk-rs#72.

They dropped the claim that the device was created with scanning off. S1's pull request changes
the third point (R1-C1): partition scanning is off in direct mode, through the vendored libublk's
hunk, and #72 is still offered upstream; under #74's `--privsep` the kernel suppresses it anyway.
S6's pull request adds the startup retry's fix to the same section.

## Dropping a timed-out attach (S3): shipped in #25

The engine (`dataplane/crates/flaminio-node/src/engine.rs`) keeps an attach in progress in
`attaching[volume]` until both metadata answers are in. Before #25 nothing removed it when the
frontend gave up, and a late answer started a target that the frontend never served.

- **Removing the entry.** `Engine::detach` removes `attaching[volume]` too, whether or not a
  target exists. `attach_and_serve` sends `Input::Detach` when the target start runs out of time.
  `AttachFailed` needs nothing, as the engine has already dropped the entry.
- **A generation for each attach.** `Attaching` gets `attempt`, from a counter in the engine, and
  `MetaWait::Attach` carries it. `on_meta` compares `attempt` with the entry that holds the volume
  now before it looks at the answer, and ignores an answer that does not match. The generation
  keeps a late answer from failing or filling in the next attach (R1-C6, R1-S-N1):
  - without it, data from the dropped attach would fill in the next attach of the same volume
    with an old view, at the old epoch. Since #30, `start_target` refuses a token whose epoch is
    not the view's (`engine.rs:1617–1623`), so the new attach would fail with `AttachFailed`
    ("the token is for v epoch 2 … the volume is at 1"), not start a target at the old epoch:
    the token check, not the generation, bars that;
  - a failure would end the next attach with the old attach's error.

## No automatic partition scan (S1): still to do

- **libublk** (R1-C1, R1-S5). S1 is one hunk in the vendored libublk 0.4.8
  (`dataplane/vendor/libublk/`, `[patch.crates-io]`), which #74 and #15 bring in, whichever lands
  first (#74, settled Q2; #15 design, S2, "A failed `ADD_DEV` deletes nothing"):
  `sys::UBLK_F_NO_AUTO_PART_SCAN` joins `UBLK_DRV_F_ALL` (`ctrl.rs:1107–1119`), against which
  `validate_new_params` checks a builder's flags (`:2251`). The constant is already in the
  bindings (`libublk-rs-sys` 0.1.2, `ublk_cmd.h:391`). The hunk is the change of
  [ublk-org/libublk-rs#72](https://github.com/ublk-org/libublk-rs/pull/72), still offered
  upstream; the copy drops it when a release takes it, as for #74's and #15's hunks. S1 lands
  with or after the plan that vendors libublk.
- **Feature check** (direct mode). The `ctrl_flags` that `add_device` builds get
  `sys::UBLK_F_NO_AUTO_PART_SCAN` when the kernel advertises it. That is #15's rewritten
  `add_device` (#15 design, S2: the id walk), or today's if S1 lands before #15; either way only
  its flags change. The node reads the features once per process (R1-C-N1): a `static`
  `OnceLock<bool>`, filled at the first add from `UblkCtrl::get_features()`. When the flag is
  missing, or the kernel answers no features, the initializer logs, once per process and not once
  per add (under #15 there are many): "the kernel scans the partitions of ublk devices: a
  partition table inside a volume shows on the host".
- **Under #74's `--privsep`,** nothing changes (R1-C2). The helper adds the device with its fixed
  flags (`ctrl_flags` 0, #74's `fixed_flags` test), and the I/O process never runs `add_device`.
  The kernel suppresses partition scanning for an unprivileged server and keeps it suppressed,
  so even `blockdev --rereadpt` fails (#74's `partition_scan_suppressed`). The helper adds no
  flag, and nothing is logged.
- **Lab check** (R1-S3, R1-C2, R1-C-N2, R2-B1, R3-B1). A new `scripts/checks/partscan.sh`, for
  direct mode. NODE must have no volume attached.
  - **The mode.** Until #74 lands, the lab runs only direct mode: no script has a `--privsep`
    or `PRIVSEP` setting. Once #74 lands, the check first reads the node unit's command line
    (`systemctl show -p ExecStart --value flaminio-node`), and if an argument is exactly
    `--privsep` (`grep -qE -- '(^| )--privsep( |;|$)'`, so `--privsep=0` or `--privsep-x` do not
    match; the flag name to confirm when #74 lands), fails up front:
    "partscan.sh needs direct mode: the node runs --privsep, where the kernel suppresses
    partition scans and re-reads (#74)".
  - It creates a scratch volume, `partscan-<time>`, of one chunk, as `resync.sh` creates its own.
    An `EXIT` trap detaches and deletes it, whatever happens. No volume with data is written, so
    nothing needs restoring.
  - It attaches the volume on NODE with a token from `metactl attach`, and writes a prepared
    512-byte MBR with one partition over its start with `dd conv=fsync`, buffered: the device's
    logical block is 4 KiB, which the table's LBAs count in. It detaches the volume.
  - It takes a new token with `metactl attach`, a new epoch, and attaches the volume again: S6
    refuses the first token in the same run of the node.
  - It polls for `/dev/ublkbNp1` for 5 s, and fails if it appears. A single check after `udevadm
    settle` would not do: Linux 7.0 scans after `START_DEV` returns, and the settle waits only
    for events already queued.
  - **Positive control:** it then runs `udevadm settle`, and `blockdev --rereadpt /dev/ublkbN`
    until `/dev/ublkbNp1` appears, for up to 5 s: the re-read fails with `EBUSY` while another
    process holds the device open, as udev's probe does just after the add. That shows the
    kernel parses the table, so that the check cannot pass on a table it ignores, and it tests
    S1's clause that an explicit re-read still creates the partitions. If no `p1` appears, it
    fails with "the re-read created no p1: the table is invalid, the node runs --privsep, or the
    device was busy", and the last `blockdev` error.
  - It reads the node's log of its current run, as `node.sh`'s `journal` does (the unit's
    `InvocationID`). If S1's line is there, the kernel lacks the flag: the check says so and
    fails, as it cannot show S1.

## One target per epoch on a node (S6): still to do

Found in plan review round 1 (R1-S1), after approval: a defect of #25's startup retry, on main.
The retry resends the same token after any failure (`ublk.rs:747–768`). A failure can come after
the target has started: `add_device` failing for 10 s (`ublk_drv` not loaded, `--zero-copy` on a
kernel without `UBLK_F_AUTO_BUF_REG`), libublk's 3 s wait for `/dev/ublkcN`, or `START_DEV`
failing. `attach_and_serve` then detaches the target (`ublk.rs:888–896`). The retry's `GetVolume`
still shows epoch E, so `start_target`'s check passes (`engine.rs:1617`), and a second target
starts at E, numbering its sessions afresh (`Target::new`, `target.rs:484–486`), up to about 20
times in the 5 minutes. Replicas would take its sessions and versions for the old one's
(`replica.rs:174–186`). The CSI path is not affected: each NodeStage takes a new epoch
(`node.go:54–70`).

- **The bar** (`flaminio-repl/src/run_epochs.rs`, new; R2-C1). The rule is a sans-I/O type in
  `flaminio-repl`, as AGENTS.md's Principles ask, so that the engine and the simulator run the
  same code. `RunEpochs<K: Ord>` holds, per volume key, the highest epoch at which a target of
  it started. `admit(&mut self, volume: K, epoch: Epoch) -> Result<(), Epoch>` refuses an epoch
  at or below the volume's highest, and returns that highest; otherwise it raises the highest to
  the epoch and returns `Ok`. Its test-only `Bugs { restart_at_epoch }`, like `target::Bugs`,
  admits every epoch, still raising. `lib.rs` declares the module.
- **In the engine** (`engine.rs`). `Engine` gets `ran: RunEpochs<String>`. `start_target`, after
  the token's claims check (`engine.rs:1617–1623`) and just before it builds the target (nothing
  after that fails), calls `ran.admit(volume, claims.epoch)`. A refusal answers `epoch: a target
  of VOLUME already started at epoch E on this node; attach with a new token`, which
  `attach_and_serve` answers as `error: attach VOLUME: epoch: …`. `Engine::detach` and a
  deposition leave `ran` alone. A target starts only in `start_target`, so the bar covers every
  attach in the run: the startup retry's, and an `attach` on the control socket with an old
  token. The comparison and the raise are `RunEpochs`'s, which the simulator checks; where the
  engine calls it is the engine test's.
- **What is not barred.** An attach that started no target (no answer from the metadata
  service, `AttachFailed` before the start, or a timeout, S3) leaves `ran` as it was: a retry
  after a metadata outage keeps its token, and `late_answers_to_a_dropped_attach_start_no_target`,
  which attaches again at the same epoch after an attach that started nothing, still passes.
  Another volume's epochs never bar an attach, and a volume made again under its name starts
  above its earlier epochs (#30).
- **The startup retry** (`ublk.rs`). The loop moves out of `serve_node` into
  `fn startup_retry(volume, first: Receiver<String>, every: Duration, until: Instant, send: impl
  FnMut() -> Option<Receiver<String>>, attached: impl Fn() -> bool)`, called with 5 s and 5
  minutes. It stops at `ok`, at an answer that starts `error: attach VOLUME: epoch: `, once a
  volume is attached, or at `until`; after any other failure it sends again after `every`. So
  after a device failure it sends one attach, which the engine refuses, and stops.
- **Until #15.** The bar lasts one run of the process. A node restarted with the old token
  (`--attach` with the same token file, the lab's `node.sh up`) can still start a target at that
  epoch, as it could before #25. #15's run mark (#15, S11) is the same comparison, made durable,
  loaded at start and checked at the socket before a slot is taken. It replaces `ran`, and its
  simulator check extends the one below across crashes.
- **Simulator** (`flaminio-repl/src/sim`):
  - each node keeps a `RunEpochs<VolumeId>` for the targets started on it since its last
    restart; a crash loses it, as the engine's is lost until #15. `start_target` calls
    `admit(VOL, view.epoch)` beside its check of a live target's epoch, and starts nothing on a
    refusal;
  - the simulator records every epoch a target started at. A second start at one fails the run:
    `two targets at epoch E`;
  - a scripted step, `Action::DeviceFails(n)`: node n's target ends without a crash, as when its
    device fails after the start, and 5 s later the node takes the last `Start` it got again, as
    the startup retry does with its token. A crash in between cancels it, as the retry thread
    dies with the process;
  - injected bug `restart_at_epoch`: `sim::Bugs` gets `run_epochs: run_epochs::Bugs`, which
    the simulator sets on each node's `RunEpochs`, as it sets `target::Bugs` on each target. So
    the bug turns off the comparison the engine ships, not a copy of it (R2-C1).

  The step is scripted, not a new kind of random fault: one would change every seed's run, and
  with it the recorded first catches of the rare mutations.
- **Docs** (R2-C3). `docs/replication-protocol.md` §9 gets:
  - a fourth check each run makes: no two targets ever start at one epoch (§3), failing with
    `two targets at epoch E`;
  - a third scripted scenario: a target whose device fails after its start, and the node's
    retry of its last start;
  - a row in the mutation table: "A target restarts at its epoch on the same node (§3) |
    device-failure scenario";
  - a sentence that this scenario's catch is `two targets at epoch`, a failure of neither
    linearizability nor convergence: the scenario test asserts that text itself, as the
    split-brain scenarios assert `not linearizable`;
  - two rewordings, so that the new scenario and its catch fit (R3-N1): the scenarios' intro
    ("reproduce the two split-brain cases exactly") names three scenarios, the two split-brain
    cases and the device failure's retry; and the mutation checks' counting rule ("Only a
    failure that breaks linearizability or convergence counts") also counts
    `two targets at epoch`, for that scenario's bug.

  `docs/followups.md` §10 gets the startup retry's fix (Follow-ups, above).

## Tests

Tests on main since #25 are named; *new* marks those still to write (R1-C3, R1-S2).

- **Go** (`control/csi/dataplane_test.go`): a Unix socket server, which reads the command, then
  never answers or answers after a delay. The socket goes in a directory from
  `os.MkdirTemp("", "fl")`, removed by `t.Cleanup`: macOS allows socket paths of only 104
  bytes, and a `t.TempDir()` path with the test's name can be longer.
  - `TestControlSocketGivesUpOnASilentDataPlane`: `Timeout` 50 ms with no answer: `Attach` fails
    within a second, and its error says no answer came.
  - `TestControlSocketKeepsTheCallersEarlierDeadline`: `Timeout` 10 s with a context deadline of
    50 ms: it fails within a second.
  - `TestControlSocketDefaultWaitsForTheDataPlane`: `ControlSocket{}.timeout()`, and with a
    negative `Timeout`, equals `DefaultControlTimeout`; with an answer after 200 ms, `Attach`
    returns `/dev/ublkb3`. It asserts equality only: that the default is above the data plane's
    limits is the Rust limits test's.
  - `TestControlSocketTellsTheDataPlaneItsTimeLeft`: a 3 s context gives a fourth word of 1500 to
    2000 ms.
  - `TestControlSocketFallsBackForAnOlderDataPlane`: a server that answers anything but three
    words with the old usage line; `Attach` returns its device.
- **Rust:**
  - **Control socket** (`control.rs`):
    - `an_attach_without_an_answer_times_out_at_its_deadline`: a live receiver that never
      answers, and a limit of 100 ms. It checks the answer, and that the request it received
      carries a deadline that has passed.
    - `an_attach_takes_the_callers_shorter_limit`: `attach_limit` with no fourth word, 20000,
      999999 and `soon`.
    - `an_attach_dropped_without_an_answer_says_so`: a thread takes the request and drops it.
  - **Limits** (`control.rs`), `the_limits_are_their_steps_and_below_the_csi_limit`: asserts
    `ATTACH_LIMIT == METADATA_LIMIT + ADD_DEVICE_LIMIT + START_LIMIT`, and that `ATTACH_LIMIT`
    and `DELETE_LIMIT` are below `DefaultControlTimeout`, which `csi_control_timeout()` reads from
    `control/csi/dataplane.go` (`include_str!`), not a literal 60 s.
  - *New:* **skipping and bounds** (`ublk.rs`). The main loop's body moves into
    `fn take_attach(now, request: Control, serve: impl FnOnce(AttachRequest,
    &mpsc::Sender<String>) -> Result<(), String>)`. It takes the reply sender out of the
    `Control::Attach`, computes `steps_by`, and answers and logs the skip, or calls `serve` with
    the request and the sender, and logs and answers its error. In the loop, `serve` is
    `attach_and_serve`, which needs the sender for `run_device`'s `ok /dev/ublkbN` (R2-C2). The
    sender is passed rather than moved into `AttachRequest`, so that `take_attach` still holds
    it to answer `serve`'s error. The bounds become
    `fn metadata_wait(now, steps_by) -> Duration` and `fn add_device_by(now, by) -> Instant`.
    - `a_request_past_its_steps_is_answered_and_never_begun`: a deadline 5 s away is answered
      `error: attach v: its caller's time ran out before it could begin`, and `serve` is never
      called, so no `Input::Attach` reaches the engine. A deadline 50 s away calls `serve` with
      `steps_by` 40 s away.
    - `the_steps_are_bounded_by_the_time_left`: with `steps_by` 40 s away, 30 s and `now + 10 s`;
      5 s away, 5 s and `now + 5 s`; already passed, zero and `steps_by` itself, as
      `(now + ADD_DEVICE_LIMIT).min(by)` returns `by` (R2-N1).
  - *New:* **the startup retry** (`ublk.rs`), `startup_retry` with an interval of 10 ms and a
    fake `send` that records each request:
    - `the_startup_attach_is_sent_again_after_a_failure`: after `error: attach v: no answer from
      the metadata service within 30 s`, it sends again after the interval.
    - `the_startup_attach_stops_when_it_must` (S6, R1-S1): after `ok /dev/ublkb0`, after
      `error: attach v: epoch: …`, once `attached` is set, and past `until`, it sends nothing
      more.
- **Engine** (`flaminio-node/tests/engine.rs`), driven by hand: the metadata answers sit in the
  input channel until the test lets the engine read them.
  - `late_answers_to_a_dropped_attach_start_no_target`: Attach, then Detach, then deliver the
    answers: there is no `Attached`, and no target (`Input::Io` gets `NotAttached`). A new Attach
    then gives `Attached`.
  - `late_answers_to_a_dropped_attach_leave_a_new_attach_alone`: Attach at epoch E, then Detach.
    Raise the model's epoch to E + 1, then Attach with an E + 1 token, then deliver all the
    answers: one `Attached`, at E + 1, and no `AttachFailed`. The old answers carry epoch E, so
    without the generation check they would fill in the new attach, and `start_target` would
    refuse its E + 1 token against the old view: an `AttachFailed`, which the test asserts does
    not come (R1-C6, R1-S-N1).
  - `late_failures_of_a_dropped_attach_leave_a_new_attach_alone`: as the last, but the test
    replaces the dropped attach's two answers in the channel with failures. It finds them as the
    first `GetVolume` and `ListNodes` answers in the channel, not by id: the engine's own calls,
    such as `RegisterNode`, come first. The new attach still gives one `Attached`, and no
    `AttachFailed`.
  - *New:* `a_second_attach_of_a_volume_still_attaching_is_refused`: two Attaches of `v` before
    any answer; the second gives `AttachFailed` (`already attaching`) at once; delivering the
    answers gives one `Attached`.
  - *New:* `a_target_never_starts_twice_at_one_epoch` (S6): Attach at epoch 1 gives `Attached`;
    Detach, as `attach_and_serve` does when the device fails; Attach with the same token gives
    `AttachFailed` whose reason starts `epoch: `, no `Attached`, and `Input::Io` gets
    `NotAttached`. Then an attach of a second volume at epoch 1 gives `Attached` (the fake service
    answers `GetVolume` with its one volume, whatever the id): the bar is per volume. Last, with
    the model's epoch raised to 2, an epoch-2 token for `v` gives `Attached` at 2.
- *New:* **`RunEpochs`** (S6, `flaminio-repl/src/run_epochs.rs`): epoch 3 is admitted; 3 again
  is refused with 3, and so is 2; 4 is admitted; another key's 3 is admitted.
- *New:* **simulator** (S6, `flaminio-repl/src/sim/tests.rs`):
  `scenario_device_failure_then_retry`: node 0 is attached, then `DeviceFails(0)`; the controller
  runs only after the script, so the retry comes before any failover. It passes, and the run
  settles at a newer epoch. With `run_epochs` bug `restart_at_epoch` on, it must fail with `two
  targets at epoch`.
- **Lab:** `scripts/checks/reconcile.sh` with `ZERO_COPY=1`: it passes, and its `device … ready`
  line, which times the attach on NODE from the metadata service's attach to the device, is
  under a second (S5; R1-C-N3). The runs with a replica's node stopped, in Measurements, were
  measurements before #25; no script repeats them, and no rule depends on them.
- **S1:** `scripts/checks/partscan.sh`, above.

## Files touched (still to do)

- **S1:** `dataplane/vendor/libublk/src/ctrl.rs` (the hunk, in the copy #74 or #15 brings);
  `dataplane/crates/flaminio-io/src/ublk.rs` (`add_device`'s flags, the features `OnceLock`);
  `scripts/checks/partscan.sh` (new); `docs/followups.md` §10.
- **S6:** `dataplane/crates/flaminio-repl/src/run_epochs.rs` (new: `RunEpochs`, its `Bugs`, its
  tests) and `src/lib.rs`; `flaminio-repl/src/sim/mod.rs` (each node's `RunEpochs`, the check,
  `Action::DeviceFails`, `Bugs::run_epochs`) and `src/sim/tests.rs` (the scenario);
  `dataplane/crates/flaminio-node/src/engine.rs` (`ran`) and `tests/engine.rs`;
  `dataplane/crates/flaminio-io/src/ublk.rs` (`startup_retry`); `docs/replication-protocol.md`
  §9; `docs/followups.md` §10.
- **The new S2 tests** (R1-C3): `flaminio-io/src/ublk.rs` (`take_attach`, `metadata_wait`,
  `add_device_by`, their tests); `flaminio-node/tests/engine.rs` ("already attaching").

## Later plans

#74 and #15 (approved, not implemented) change these statements of this design (R1-C8):
- **`START_LIMIT` "is not enforced"** ("What remains", Risks). Under #74's `--privsep`, a
  `start` reply not received within `START_LIMIT` ends the I/O process, and #74's cleanup
  follows (#74 design, the client). #15 spends `START_LIMIT` again on the udev wait in the ready
  callback, and stops the device itself when it runs out (#15 design, S2, step 1). A device that
  starts late, and that a retry finds through `status`, remains direct mode's case, as #15 keeps
  it (#15 design, S2, "A late-started device").
- **The startup retry's stop "once a volume is attached".** Under #15 it runs until *that*
  volume has an entry, or until an `epoch:` refusal, which is final; another volume's entry does
  not stop it (#15 spec, S10; design, S1's tests).
- **Adding the device "retried while a previous device with the same id goes away".** #15
  removes the retry at the same id and walks ids instead (#15 design, S2, Ids). S1's flag goes
  into that `add_device`.
- **S6's `ran`.** #15's run mark makes it durable: per volume, loaded at start, and checked at the
  socket before a slot is taken (#15 spec, S11). It can load and store the state of the same
  `RunEpochs`, so that the comparison stays the one the simulator checks (R2-C1). #15's answer
  `error: VOLUME: epoch: <reason>` replaces S6's `error: attach VOLUME: epoch: …`, and the
  startup retry's stop takes that form.

## Decisions

- **30 s for the target start, not 60 s.** It keeps the budget inside kubelet's 2 minutes: 30 s
  for `Meta.Attach`, plus 50 s, leaves 40 s for `mkfs` and `mount`. A metadata service that has
  not answered in 30 s is in an outage. With S3, the retry kubelet makes then works.
- **Derive `ATTACH_LIMIT` from its parts.** It should not be a third literal that has to be kept
  in step by hand.
- **The CSI limit is a field with a default, not a flag.** No deployment needs to change it. The
  field exists for tests.
- **A generation, not only removal.** Removing the entry alone would let a late answer fill in,
  or fail, the next attach of the same volume.
- **A deadline in the request, rather than a withdrawn flag.** A request that would run after
  its caller gave up would attach the volume with a stale token. The deadline also bounds each
  step by the time left, which a flag checked at fixed points cannot.
- **S1 is a hunk in the vendored libublk** (changed after approval, R1-C1, R1-S5). It first
  waited for a libublk release, as the project carried no patched copy. #74 and #15, approved,
  vendor libublk 0.4.8 and maintain it here (#74's settled Q2), so S1 waits for that copy, not
  for upstream; ublk-org/libublk-rs#72 stays offered upstream, and the hunk goes when a release
  takes it.
- **S6 refuses, in the engine** (added after approval, R1-S1). A retry with a fresh token would
  need the data plane to take a new epoch itself: `Meta.Attach` is the node plugin's call, and
  the data plane would need its own `auth.go` rule for it. Refusing needs a map and one check.
  The check is in `start_target`, where every target starts, rather than only in the retry
  thread, so it covers every attach in the run. The map and its check are one sans-I/O type in
  `flaminio-repl`, `RunEpochs`, which the simulator calls too (R2-C1), and which #15's run mark
  makes durable. The price is one refused attach, with its metadata calls, before the retry
  stops.

## Risks

- **A metadata service slower than 30 s to answer fails the attach.** That is an outage, not a
  slow answer, and kubelet retries.
- **A kernel that scans partitions inside `START_DEV`** would hold the start for as long as the
  scan's first reads wait: up to a takeover, 5.5 s. `START_LIMIT` allows for it, and S1 removes
  the scan.
- **An attach already under way when the socket gives up still finishes** (S2). A retry finds
  its device through `status`. Under #74's `--privsep`, a start past `START_LIMIT` ends the I/O
  process instead (Later plans).
- **Until #15, S6 lasts one run** of the process: a node restarted with an old token can start a
  target at that epoch again, as before #25. #15's run mark closes it.
