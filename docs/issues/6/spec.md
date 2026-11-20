# Issue #6: a faster first attach after a restart — specification

**Status** (plan review round 1, R1-C7): S2–S4 shipped in pull request #25 (3b5560b), and are
described here as they are on main; the values they replaced are marked "before #25". Still to
do: S1; S6, which fixes a defect of #25's startup retry and was added after approval; and the
tests marked *new* below.

## Behaviour

**S1. No automatic partition scan** (still to do; it lands with the vendored libublk, intent,
"Dependency"). In direct mode, the node adds its ublk device with `UBLK_F_NO_AUTO_PART_SCAN` when
the running kernel advertises the flag (`UBLK_U_CMD_GET_FEATURES`). Otherwise it adds the device
as before, and logs that the kernel scans its partitions: once per process, not once per device.
The flag stops the automatic scan when the device starts: a partition table inside a volume no
longer creates partitions on the host when the volume is attached. An explicit re-read
(`blockdev --rereadpt`, `partprobe`) still does. Reading the device works as before.
- **Under #74's `--privsep`,** S1 adds nothing (R1-C2). The helper adds the device with flags it
  fixes, and the kernel already suppresses partition scanning for an unprivileged server, and
  keeps it suppressed: an explicit re-read fails there (#74, `partition_scan_suppressed`). The
  node adds no flag in that mode, and logs nothing.

**S2. Time limits on an attach.** Kubelet runs NodeStageVolume under a 2-minute deadline, which
reaches the CSI node plugin through the gRPC context. Within it, the node plugin calls:
1. `Meta.Attach` on the metadata service, which tries each endpoint for up to 10 s: up to 30 s
   with three;
2. the data plane's control socket, `attach <volume> <token>`;
3. on a filesystem volume, `blkid`, `mkfs` if the volume is empty, and `mount`.

The data plane must answer (2) in time for its answer to reach kubelet. Before #25 it waited up
to 120 s, starting only after (1), so when it failed, kubelet saw only its own deadline, with no
cause. Since #25, the data plane answers every control command within 50 s. An attach takes
these steps:
- **Target start:** up to 30 s (60 s before #25) for the metadata service to answer the
  target's start: the engine reads the volume and the nodes.
- **Adding the device:** up to 10 s, plus one more try, while a previous device with the same
  id goes away, as before #25.
- **Starting the device:** up to 10 s. libublk waits up to 3 s for the device's character
  device to appear. On a kernel that scans partitions after the device has started, as Linux
  7.0 does, starting does not wait for reads. On one that scans inside the start, the scan's
  first read can wait for a takeover: 5.5 s.

So the socket answers an attach within 50 s, or within the time its caller says it still waits,
if that is shorter: the node plugin adds it to the command, in milliseconds, less 1 s for the
answer to come back (`attach <volume> <token> <ms>`). The request carries that deadline. The node skips
a request it would begin with no time left for its steps (less than the 10 s before the
deadline), logs that it did, and answers with that reason. It ends the target's start
and the device's adding by the deadline less the 10 s for the start, waiting less than 30 s
and 10 s if the request waited its turn. So its own answer, or the reason there is none,
reaches the socket in time, unless starting the device itself takes more than 10 s. When the
deadline passes, the socket answers `error: no answer within 50 s`. An attach whose device then
starts anyway is found by a retry through `status`. `delete` answers within 50 s too (60 s before #25), and `detach` within 30 s, as before.

The rest of the attach's contract, shipped in #25 and stated here since plan review round 1
(R1-C4):
- **The time left, as a fourth word.** A data plane older than #25 answers
  `attach <volume> <token> <ms>` with its usage line. The node plugin then sends
  `attach <volume> <token>` again, without the fourth word, so the two can be upgraded in either
  order. A data plane from #25 on takes both forms; without the fourth word, its limit is 50 s.
  The refused form never began an attach, so the second is not a retry of one.
- **An attach dropped without an answer.** When the node drops an attach without answering it
  (its reply channel closes, as when the attach panics), the socket answers `error: the node
  dropped the attach without an answer (see its log)` at once, not at its limit.
- **One attach of a volume at a time.** The engine refuses an attach of a volume it is still
  attaching (`already attaching`), so every attach gets an answer, and a second one never
  replaces the first.
- **The attach at startup** (`--attach`, a lab and manual option) has no caller to retry it. It
  is sent before the control socket opens, so it comes first. After a failure, the node sends it
  again every 5 s, for up to 5 minutes from its first try, so that a metadata service still
  recovering after a cluster restart, whether it times out or refuses connections, does not
  leave the node without its volume. It stops at an `ok`, once a volume is attached (one volume
  per node), at S6's refusal, or after the 5 minutes. Once a target has started at the token's
  epoch, a retry with that token is refused (S6), and the retries end (R1-S1).

**S3. A timed-out attach leaves nothing behind.** When the target's start runs out of time, the
data plane drops the attach in the engine. An answer from the metadata service that comes later
is ignored, whether it carries data or a failure. It neither starts a target nor ends another
attach, so the next attach of the volume works, even when it comes before the late answer. Before #25 such an answer started a
target with no device behind it, and every later attach of the volume failed with "already
attached" until the node restarted.

**S4. The CSI node plugin's limit.** It waits up to 60 s for a control command (150 s before #25),
unless the caller's deadline is earlier. Every call from kubelet has a deadline, so the 60 s is
a backstop for a data plane that never answers. It is also the limit for callers without a
deadline, such as the tombstone sweep's `delete`. It is above the data plane's 50 s, so the data
plane's own answer comes first and names the cause.

The budget, with three metadata endpoints: (1) 30 s, plus (2) 50 s, leaves 40 s of kubelet's
2 minutes for (3).

**S5. Unchanged.** How long an attach takes when nothing fails: under 0.1 s on the lab, from the
metadata service's attach to the control socket's answer. How long the first reads wait
afterwards. And `scripts/checks/reconcile.sh`, which passes.

**S6. One target per epoch on a node** (still to do; added after approval, plan review round 1,
R1-S1; replication-protocol §3, "One target per epoch, for its whole life"). The node never
starts a second target of a volume at an epoch at which a target of that volume already started
in this run of the process. Such an attach is refused, with the reason `epoch: a target of
<volume> already started at epoch <E> on this node; attach with a new token`, and starts no
target.
- **What it closes.** On main, the startup retry (S2) sends the same token again after any
  failure, including one that comes after the target has started: adding the device failing for
  10 s, libublk's wait for the device's character device, or the device's start failing. The
  node then detaches that target, the retry finds the volume still at the token's epoch, and a
  second target starts at it, numbering its sessions afresh. The CSI path is not affected: each
  NodeStageVolume takes a new epoch.
- **What it leaves alone.** An attach that started no target (the metadata service did not
  answer, the engine refused it before the start, or it timed out, S3) bars nothing, so a retry
  after a metadata outage keeps its token. Another volume's epochs never bar an attach.
- **Until #15.** The bar lasts one run of the process. A node restarted with the old token
  (`--attach`, the lab's `node.sh up`) can still start a target at that epoch, as before #25.
  #15's run mark (#15, S11), durable and checked before an attach takes a slot, closes that once
  #15 lands.

## Tests

Tests on main since #25 are named; *new* marks those still to write (R1-C3, R1-S2).

- **Go** (`control/csi/dataplane_test.go`):
  - `TestControlSocketGivesUpOnASilentDataPlane`: with `Timeout` 50 ms and a socket that never
    answers, `Attach` fails within a second, and its error says no answer came.
  - `TestControlSocketKeepsTheCallersEarlierDeadline`: with `Timeout` 10 s and a context deadline
    of 50 ms, it fails within a second too.
  - `TestControlSocketDefaultWaitsForTheDataPlane`: with `Timeout` zero, as `main.go` leaves it,
    or negative, the effective limit equals `DefaultControlTimeout`, and a socket that answers
    `ok /dev/ublkb3` after 200 ms gives `/dev/ublkb3`. That `DefaultControlTimeout` is above the
    data plane's 50 s is checked by the Rust limits test, which reads it.
  - `TestControlSocketTellsTheDataPlaneItsTimeLeft`: with a 3 s context, the command's fourth
    word is about 2000 ms.
  - `TestControlSocketFallsBackForAnOlderDataPlane`: a socket that answers a four-word attach
    with the old usage line gets the three-word attach, and `Attach` returns its device.
- **Rust:**
  - **Control socket** (`control.rs`):
    - `an_attach_without_an_answer_times_out_at_its_deadline`: with a control channel whose
      receiver is kept but never answers, and a limit of 100 ms, `attach` answers
      `error: no answer within 0.1 s`. The request it sent carries a deadline that has passed by
      then.
    - `an_attach_takes_the_callers_shorter_limit`: no fourth word gives `ATTACH_LIMIT`; a smaller
      one is taken, a larger one is cut to `ATTACH_LIMIT`, and one that is not a number is
      refused.
    - `an_attach_dropped_without_an_answer_says_so`: the node takes the request and drops its
      reply channel; the answer says the attach was dropped.
  - **Limits** (`control.rs`), `the_limits_are_their_steps_and_below_the_csi_limit`:
    `ATTACH_LIMIT` is `METADATA_LIMIT + ADD_DEVICE_LIMIT + START_LIMIT`, and it and `DELETE_LIMIT`
    are below `DefaultControlTimeout`, read from `control/csi/dataplane.go`.
  - *New:* **skipping and bounds** (`ublk.rs`):
    - a request taken after its deadline less `START_LIMIT` is answered `error: attach <volume>:
      its caller's time ran out before it could begin`, and is never begun: no `Input::Attach`
      reaches the engine. A request with time left is begun, its steps ending at its deadline less
      `START_LIMIT`;
    - the steps' bounds, as functions of that end and the time: the metadata wait is the smaller
      of 30 s and the time left, and adding the device ends at the earlier of 10 s from now and
      that end.
  - *New:* **the startup retry** (`ublk.rs`), with its interval and limit as parameters: after a
    failure from the metadata service, it sends the attach again after the interval. It sends
    nothing more after an `ok`, after S6's refusal, once a volume is attached, or past its limit.
- **Engine** (`flaminio-node/tests/engine.rs`):
  - `late_answers_to_a_dropped_attach_start_no_target`: an attach is cancelled before the
    metadata service answers. The late answers start no target and give no `Attached`. A new
    attach of the volume then completes.
  - `late_answers_to_a_dropped_attach_leave_a_new_attach_alone`: the same, with the new attach
    made before the late answers arrive, at a newer epoch: it completes once, at the newer epoch,
    and nothing fails.
  - `late_failures_of_a_dropped_attach_leave_a_new_attach_alone`: the same again, with the
    dropped attach's late answers failures: the new attach still completes, and no
    `AttachFailed` comes.
  - *New:* a second attach of a volume still attaching gets `AttachFailed` (`already attaching`),
    and the first still completes, once.
  - *New* (S6): an attach at epoch 1 completes, and the volume is detached, as when its device
    fails. An attach with the same token then gets `AttachFailed` with S6's reason, gives no
    `Attached`, and starts no target (I/O gets `NotAttached`). An attach of another volume at
    epoch 1 completes, and so does one with an epoch-2 token once the volume is at epoch 2.
- *New:* **simulator** (S6): a node whose target ends without a crash, as when its device fails,
  takes its last start again 5 s later, as the startup retry does. The run fails if two targets
  ever start at one epoch. S6's comparison is code that the engine and the simulator share, and
  an injected bug in it, which skips the refusal, must be caught (R2-C1).
- **Lab:** `scripts/checks/reconcile.sh` with zero copy passes, and its own timing of the attach
  (its `device … ready` line, from the metadata service's attach to the device) is under a
  second: S5, with nothing failing (R1-C-N3).
- **S1, once it lands** (direct mode; R1-C2, R1-S3, R1-C-N2, R2-B1, R3-B1): a lab check
  creates a scratch volume, which it detaches and deletes on exit, whatever happens, so that no
  data is overwritten. It attaches the volume, writes a prepared 512-byte MBR with one partition
  over its start with `dd conv=fsync`, and detaches it. It attaches it again with a new token (S6
  refuses the first one), and checks for 5 s that no `/dev/ublkbNp1` appears. Then, after
  `udevadm settle`, `blockdev --rereadpt`, retried for 5 s while the device is busy, must create
  `/dev/ublkbNp1`: the kernel parses the table, and an explicit re-read still creates the
  partitions (S1). Otherwise the check fails, naming the likely causes: an invalid table, a node
  under `--privsep`, or a busy device. Until #74 lands the lab runs only direct mode; once it
  does, the check reads `--privsep` from the node unit's command line and fails up front, naming
  the mode. It reads the node's log of its current run: if S1's line is there, the kernel lacks
  the flag, and the check says so and fails, as it cannot show S1.
