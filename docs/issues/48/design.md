# Issue #48: Recover existing ublk devices under a fresh epoch — design

Approved 2026-11-13; not implemented. [Spec](spec.md), [intent](intent.md).
Changed after approval, plan review rounds 1 to 3 (2026-11-17): see [decisions](decisions.md).
Built on #12/#15 planned contracts, not code already present. **#48 stays open** for disruption
guard, N−1/N negotiation, format finalize and complete rolling-upgrade qualification.
Policy: [issue-local proposed ADR](adr-proposal.md).

The facts it starts from:
- **The node's add flags omit recovery** (`flaminio-io/src/ublk.rs:906–910`, in `add_device`,
  `:902–934`).
- **libublk 0.4.8** (pinned) has `get_features` (`ctrl.rs:2507`), `start_user_recover`
  (`:2606–2624`), `disown` (`:2790`) and `UBLK_DEV_F_RECOVER_DEV`. On a quiesced device its
  `start_dev` sends END_USER_RECOVERY with its own pid (`:2637–2651`). To handle:
  - `start_user_recover` retries `EBUSY` every 100 ms for 30 s and cannot be cut; the single
    command is private (`:1972`);
  - a failed control command, `EBUSY` apart, is sent again with its legacy opcode
    (`:1499–1527`), so on a kernel without legacy opcodes the errno is `EOPNOTSUPP` whatever
    failed (#74, Owned devices, Errors). Nothing here decides by errno, `EBUSY` apart;
  - a recovery needs no JSON from before: a failed reload is printed and ignored (`:1170–1173`,
    `:1185–1188`), the parameters are not sent again for a quiesced device, and the start
    writes its JSON anew (`:2000–2011`);
  - `run_target` sends STOP_DEV when its queue threads end (`:2960–2985`).
- **The kernel,** read in 6.1.112 (`eve-kernel`, `drivers/block/ublk_drv.c`; first numbers) and
  in 6.8.12 (#74's tree, its decisions.md, Kernel facts; second numbers):
  - START_USER_RECOVERY answers `EBUSY` while the character device is open or the device is not
    QUIESCED (`:1933–1937`; `:2740–2744`), and `EINVAL` when no queue became ready since the
    last one (`:1918–1919`, `:1943`; `:2725–2726`, `:2750`);
  - END_USER_RECOVERY waits for every queue's fetches, then lets the held requests go at once
    (`:1962`, `:1979–1983`; `:2769`, `:2786–2790`);
  - the character device opens once (`:909–918`; `:1356–1365`);
  - a dead server's device is quiesced by a monitor that runs every 5 s in 6.1 (`:137`,
    `:1015–1044`), and when its commands are cancelled in 6.8 (`:1547–1548`);
  - only a stop deletes the disk (`ublk_stop_dev`, `:1166–1185`; `:1663–1680`). A recovery keeps
    the disk, and so its `major:minor` and diskseq.

  Linux 6.6 (the baseline) and 7.0 (the lab) could not be read here: Kernel check, below.
- **#15 plans** `/run/flaminio/ublk/<id>` records, an exclusive `flock` on that directory held
  for the node's life (direct mode; `flaminio-io cleanup` takes it too), the run mark
  (`--state-dir`, `epochs/<volume>`), generation-tagged routing, and the device lifetime
  contract (`device-info`, `detach-device`). **#12 plans** the pod, with the agent's credentials
  only in the CSI node plugin and an `emptyDir` at `/run/ublksrvd`. **#74 plans** the helper.
  **#6 plans** `RunEpochs` and the attach's limits. **#83 plans** the fresh-process handoff.
  #12 and #15 must land before this design is implemented.
- **`Attach` with `only_if_target`** is the node agent's, not `io`'s (`meta/service/auth.go:66–70`).
  Every `Attach` adds exactly one to the epoch (`meta/domain/domain.go:217–220`), and after
  the volume's creation (`Header.Start`, `:210–214`) nothing else changes it.

## Kernel capability and ownership (S1, S2, S11)

- **The probe.** At start the node asks `UblkCtrl::get_features`. A device is added to be kept
  only when the kernel has **both** `UBLK_F_USER_RECOVERY` and `UBLK_F_USER_RECOVERY_REISSUE`;
  `add_device` then asks for both, with the zero-copy flags when `--zero-copy` is set.
  `UBLK_F_QUIESCE` is never asked for. Both flags are in the [Linux 6.6 UAPI][uapi]; the
  [kernel's documentation](https://docs.kernel.org/block/ublk.html) tells keeping pending
  requests from issuing delivered ones again.
- **What decides is what `ADD_DEV` returned.** The kernel rewrites the flags. A device whose
  returned flags lack either one is **unrecoverable**: it is served as today, and a restart
  ends it. The node then stops asking (S1's fallback), and logs it. With
  `UBLK_F_USER_RECOVERY` alone the kernel would hold the device at the exit: the scan ends it
  (its table, below), so a restart ends that one too (R2-B11).
- **Status.** `recovery` on the control socket (Contracts) answers `on`, `off-kernel` (the
  probe) or `off-flags` (an add's returned flags, as zero copy may cause). Under `--privsep`
  the I/O process cannot probe: it answers `unknown` until its first `add`, then `on` or
  `off-flags` by that reply's FLAGS.
- **No delete on exit.** A recoverable add disowns libublk's control at once (`disown`), so its
  drop deletes nothing; only a detach, an end (S8) or the cleanup sends STOP_DEV and DEL_DEV.
  The node ends with `process::exit` (#15), so `run_target` never returns to send its STOP_DEV.
  A kill drops everything the process held; the two flags keep the delivered requests.
- **One data plane at a time.** #15's `flock` on the records directory is the one lock. A
  successor that cannot take it does not start, so it never recovers a device a live
  predecessor serves. No second process is started as a handover.

[uapi]: https://raw.githubusercontent.com/torvalds/linux/v6.6/include/uapi/linux/ublk_cmd.h

## Root records and crash points (S3–S5, S8)

**The record** is #15's (`/run/flaminio/ublk/<id>`, its directory, names, opens and 4096-byte
bound unchanged). #48 appends one block to #15's fields, one `name value` line each, in this
order, every line present, numbers decimal without sign or leading zero:

```text
recovery 1
boot BOOT_UUID            the host's boot id, canonical, as #15's contract writes it
phase starting|running|recovering|stopping
first-epoch N             the token's first_epoch; 0 for a volume from before #30
size BYTES                the device's size
dev MAJ:MIN               0:0 until the device is ready
diskseq N                 0 until the device is ready
epoch N                   the last epoch the device was served at; information only
alive NS                  CLOCK_BOOTTIME, in ns, when its server last stamped it; 0 before
```

- **No token, certificate or key.** With #15's fields (the volume, the instance, the generation
  and the returned `dev_info`) the record is under 600 bytes.
- **Written** as #15 writes it: `.<id>.tmp` with `O_CREAT|O_EXCL|O_NOFOLLOW`, then a rename.
  - before `ADD_DEV`: `O_EXCL`, phase `starting` (#15);
  - after `ADD_DEV`: the returned `dev_info` (#15);
  - in #15's ready step, before the entry turns running and the attach is answered: `dev`,
    `diskseq`, `epoch`, `alive`, phase `running`;
  - before a resume asks the kernel for START_USER_RECOVERY: phase `recovering`; back to
    `running` when END_USER_RECOVERY has returned, or when the try gives up on `EBUSY`
    (Resume, step 3). Neither write touches `alive`;
  - before a deliberate detach or end sends STOP_DEV: phase `stopping`;
  - removed after the delete, on #15's `Ok` or `ENODEV` rule.
- **Who stamps.** Each running entry's device thread replaces its own record, from its wait
  on the entry's stop channel (#15, S2, step 4), which becomes a timed wait. It is not a queue
  thread nor the engine thread, and it takes no lock of the engine or the registry. Under the
  helper it is the helper's main thread (With #74's helper). `/run` is a tmpfs that dies with
  the boot, as the devices do, so no `fsync` is needed.
- **When `alive` is stamped** (R1-S5, R2-B2, R2-B8, R3-B2), in both modes. The old deadline is
  the `alive` the record had at the resume, plus 60 s. The exit stamp is the helper's only, at
  its child's exit, and goes by the clock at that exit; direct mode has none.

  | The device, under the process that serves it | Tick | Exit stamp |
  |---|---|---|
  | Serving, added by this process | Every second | Yes |
  | Serving, resumed by this process, the old deadline not yet passed | None | No |
  | Serving, resumed by this process, the old deadline passed | At that deadline, then every second | Yes |
  | Kept, taken over and not recovered | None | No |
  | Phase `recovering` | None | No |

  - **The stamp at the old deadline is not left to the next tick:** after a resume the
    stamper's wait ends at the old deadline, and it stamps then. So a serving device has no
    deadline in the past, and an exit right after that moment finds it inside a new bound.
  - **A crash loop does not move the deadline:** a device whose server ends before the old
    deadline, before, during or after its resume, still carries the stamp of its first exit,
    and ends at that exit's deadline.
- **Compatibility.**
  - A record without the block is #15's: the node treats it by #15's rule (dead and matching:
    deleted; anything else: logged and left).
  - A block that does not parse (another `recovery` number, a missing or extra line, an unknown
    phase, a bad number, over 4096 bytes) is a record that proves nothing: quarantine, below.
  - A build without #48 logs and leaves a record with the block (#15: a malformed record), and
    deletes no device with `UBLK_F_USER_RECOVERY`. So after a downgrade a kept device waits with
    no bound: **every volume is detached before a downgrade** (S11, `docs/lab.md`).

**The start** (S5), in this order, before the store is opened:
1. the `flock`, #15's, tried for up to 5 s (Who ends a kept device);
2. the scan: each record, with `GET_DEV_INFO` and `/sys/class/block/ublkb<id>/{dev,diskseq}`,
   which open no device;
3. the **deadline thread**, below;
4. then the store, the engine, and the socket. The kept entries are built at step 2 and handed
   to #15's registry when the node is built.

So a store whose recovery is slow or hangs delays no deadline (R1-S6).

**The scan's cases,** tried in this order:

| The record and the kernel | What the start does |
|---|---|
| `GET_DEV_INFO` fails and `/sys/class/ublk-char/ublkc<id>` is absent, or `boot` is not this boot | The record is removed; no device is touched |
| `GET_DEV_INFO` fails and `ublkc<id>` is there | The record is left and logged, as #15 leaves what it cannot judge; not quarantined |
| #15's record (no block) | #15's rule |
| The block does not parse; no `dev_info`; a `dev_info` field (#15's list) differs from the kernel's | **Quarantined** |
| Matching, state DEAD | Deleted, the record removed |
| Matching, phase `starting`, `recovering` or `stopping` | Ended |
| Matching, phase `running`, `dev` or `diskseq` differs from sysfs | **Quarantined** |
| Matching, phase `running`, `UBLK_F_USER_RECOVERY` without the reissue flag | Ended |
| Matching, phase `running`, without `UBLK_F_USER_RECOVERY` | #15's rule for a device not yet dead |
| Matching, phase `running`, `alive` + 60 s passed, or the clock unreadable or behind `alive` | Ended |
| Matching, phase `running`, QUIESCED or LIVE, inside the bound | **Kept**: an entry with the recorded lifetime |
| A device with no record | Never touched |

- **No row reads an errno** (R2-B6): libublk's second try can turn any failure into
  `EOPNOTSUPP` (facts). A device that is gone has no `ublkc<id>` in sysfs.
- **Who reads which record** (R2-B1). This table is the one place that says it. "The rows" are
  the scan's, above; they read only what both forms of record hold.

  | Reader | `recovery 1`, a node's | `recovery 2`, a helper's |
  |---|---|---|
  | A direct-mode node's start | The rows | A mode change: a matching device is ended |
  | A helper's start, at its `--id` only | A mode change: a matching device is ended | The rows |
  | `flaminio-io cleanup --expired` | The rows; a kept device is left | The rows; a kept device is left |
  | `flaminio-io cleanup` | Every matching device is ended | Every matching device is ended |

  So the agent's `cleanup --expired` ends no helper's device that is inside its bound. The
  mode-change rule is a start's only.
- **Ended** is STOP_DEV, which fails the held requests (6.1.112 `:1145–1185`), then #15's
  delete (`DEL_DEV_ASYNC` from 6.9, `DEL_DEV` on a thread before), outside every lock. The
  record goes on #15's rule.
- **LIVE** is a predecessor the kernel has not reaped yet: the `flock` shows that no node
  serves it. The start does not wait: the entry is kept, and the resume's START_USER_RECOVERY
  answers `EBUSY` until the kernel has quiesced it (Resume, step 3).
- **Kept** entries count towards #15's limit, and `first_id` stays above every recorded id.
- **Quarantine** (settled at approval, 2026-11-13: never deleted). A record that cannot prove
  that the device is this node's grants nothing: the device is not deleted, stopped or resumed,
  and its record is left. The node logs its id at each start, and `recovery` lists it. If the
  kernel holds it QUIESCED, its requests wait until the operator ends it:
  `flaminio-io del --id <id>`, which exists today. The next start then removes the record
  (the scan's first row). The bound does not apply to it.
- **The attach at startup** (`--attach`) never resumes: its token is at or below the run mark.

**The deadline thread** wakes once a second. For each kept entry that is not resuming and whose
`alive` + 60 s has passed, it moves the entry to stopping under the entry's lock (#15), then
ends the device outside it. It needs the records and the ublk control device only, so it runs
whatever the store or the engine does.

## Resume state machine and request lifetime (S4, S6, S8)

#15's registry gains two states, **kept** and **resuming**. An attach of a volume that has a
kept entry is a resume; any other attach adds a device at a new id (#15, S2), so an attach of
another volume never takes a kept device.

**The epoch rules** (R1-C8, R1-S10, R1-S1).
- **One copy.** A resume passes #15's token check and `MarkEpoch` unchanged: the bar "no second
  target at an epoch" is `RunEpochs::admit` (`flaminio-repl/src/run_epochs.rs`, #6), made
  durable by #15's run mark before the first `Hello`. The record's `epoch` is information only,
  and no comparison reads it.
- **Continuity,** new, in the same type: `RunEpochs::resume(&self, volume, kept_first_epoch,
  token_first_epoch, epoch) -> Resume`, which only reads. In this order:
  - `Stale`: the epoch is at or below the volume's run mark. The attach is refused with #15's
    `epoch:` answer, and the device stays kept.
  - `End(OtherVolume)`: the token's `first_epoch` is not the recorded one: a volume made again
    under the name (#30).
  - `Go`: the epoch is the mark plus exactly one.
  - `End(Gap)`: any other epoch, or no mark for the volume.
- **Why the next epoch proves continuity.** The mark M is the epoch at which this node's last
  target of the volume started, so the target at M was this node. Every `Attach` adds one, so
  M + 1 was minted by a single `Attach`, whose token names this node. No epoch lies between
  them, so no other node was the target, and no other target ran, since the device last
  served. Any larger epoch means an `Attach` this node did not see.
- **Enforced by the data plane itself,** for every caller, the administrator's attach included.
  The socket calls `resume` before it moves the entry to resuming, and the main thread calls
  it again before it writes the mark, as #15 checks the mark again. `End` ends the device and
  answers with the cause (Contracts); the volume then attaches as after a crash today.
- **In the engine.** `Input::Attach` says that it resumes, with the recorded first epoch and
  size. The engine checks the verified claims' `first_epoch` against it, and after `GetVolume`
  the volume's size: a difference fails the attach, and the device is ended (R1-C11). Its
  `MarkEpoch` says that it resumes, so that the main thread applies `resume`, not only #15's
  "above the mark".
- **What a lost `Attach` reply costs.** The epoch moved and no token came, so the next token is
  at M + 2: `End(Gap)`. The device is ended and its I/O fails, as after a crash today. The same
  holds for a token lost with an agent that restarted, and, once #83 is in, for a data plane
  that restarts between the `Attach` and the socket's attach, whose handoff is then refused.
  The data plane cannot tell any of these from an attach elsewhere. Risks names it.

**The steps of a resume,** on the entry's device thread unless said. "Next" is what the next
successor finds if the process dies right after the step.

| # | Step | Durable, and what the kernel holds | Next |
|---|---|---|---|
| 1 | Socket: the token verifies; #83's handoff where it is in; `resume` answers `Go`; the entry turns resuming | Nothing new | Kept |
| 2 | Engine: `GetVolume`, the size, `MarkEpoch`; the main thread makes the mark durable; the target starts, fences and reconciles | The mark is E. The device is QUIESCED | Kept; the next token must be E + 1 |
| 3 | The record's phase turns `recovering`; then START_USER_RECOVERY | The record says `recovering`. `nr_queues_ready` is 0 | Ended: the scan ends a `recovering` device, and sends no second START_USER_RECOVERY |
| 4 | The recover control (`UBLK_DEV_F_RECOVER_DEV`, the recorded id) opens `/dev/ublkc<id>`; the queue threads fetch every tag | Some or all queues ready | Ended, as step 3 |
| 5 | END_USER_RECOVERY (libublk's `start_dev`): the kernel lets the held requests go; then the record's phase turns `running` again | LIVE; the requests flow | Ended while the record still says `recovering`; kept once it says `running` |
| 6 | #15's ready step: `/dev/ublkb<id>` opened no-follow; `major:minor` and diskseq must equal the record's; the fd is held; the record gets `epoch`; the entry turns running; the attach is answered | The record | Kept |

- **Step 2 comes before step 3,** as today's attach starts the target before it adds the device.
  So the mark is durable and the fence under way before any held request can flow, and each
  reissued request waits for its chunk, as any I/O does (replication-protocol §6).
- **Step 3's `EBUSY`** (a device still LIVE, or its character device not yet released) is
  tried again every 100 ms until the device step's deadline: `ADD_DEVICE_LIMIT`, 10 s, and not
  past the attach's own (#6). Past it the attach fails and the device stays kept. The vendored
  libublk (#74, #15) gains `UblkCtrl::start_user_recover_once`, the one command without the
  30 s loop. So a resume runs within #6's limits, 50 s in all (`control.rs:33–35`), as an
  attach does (R1-C6, R1-S12).
  A try that gives up there turns the record's phase back to `running`: the kernel began
  nothing. In direct mode the phase stays `recovering` through the whole wait, so a death
  inside it ends a device the kernel never began to recover. The helper turns it back at
  each `error busy`, because it cannot know that the child will ask again. The difference is
  meant: both are safe.
- **No second recovery over one that did not finish** (R2-B5, R3-B1). A successor that dies in
  steps 3 to 5 leaves the record at `recovering`, and the next start ends that device: it is
  never asked to recover again. The kernels read give the reason:
  - its fetches may stay in the kernel, and with them the character device open. In 6.1.112
    nothing cancels them while the device is QUIESCED (the monitor is cancelled at the
    quiesce, `:1128`, and the quiesce work returns unless LIVE, `:1136–1137`). In 6.8.12 the
    cancel function returns without cancelling for a queue that is not fully ready
    (`:1536–1537`). START_USER_RECOVERY would answer `EBUSY` for good;
  - in 6.8.12, with one queue fully fetched and another that sent nothing, the device is
    released and START_USER_RECOVERY passes its checks, then resets every queue: for the one
    that never was ready it warns and puts a NULL task (`ublk_queue_reinit`, `:2694`,
    `:2699`; the same lines in 6.1.112, `:1889`, `:1894`, which does not reach them). libublk
    sends a queue's fetches together (`io.rs:2325–2336`), and `--queues` goes to 64
    (`node.rs:203`), so a kill between two queue threads leaves exactly that.
- **Any other failure of step 3** means that the kernel will not recover the device: it is
  ended.
- **A failure after step 3 succeeded** (a queue's setup, END_USER_RECOVERY, the device node,
  step 6's comparison) ends the device: the process stops it and deletes it, as a failed
  attach does (#15, S2, Failures).
- **A failure before step 3** (the token, the metadata service, the fence, `EBUSY` past its
  deadline) leaves the device kept, for the next try or the bound.
- **A detach during a resume** sets the entry's detach flag (#15): the resume ends at its next
  step, and the device is ended.
- **Under `--privsep`** steps 3 to 5 are the helper's requests (With #74's helper).

**The lifetime tuple** (R1-C3, R1-S7). #15's contract names a device's lifetime
`(volume, first_epoch, boot_uuid, instance, generation, rdev, diskseq)`. A recovery keeps the
kernel's disk, so `rdev` and diskseq stay; the boot, the volume and its first epoch stay. So
**a resumed device keeps its recorded tuple:** the instance and the generation are those of the
run and the attach that added the device.
- **Why:** #79's stage record holds that tuple and unstages with `detach-device` and all of it
  (#79 design, lines 131–134). A stage made before the restart therefore still unstages, and
  #15's raw-block rule ("whose instance and generation still match") still reopens its fds.
  #79's record and rules need no change.
- **Still unique:** the instance is drawn per run, so no entry this run adds can share a
  resumed entry's pair.
- **Routing** is apart from it: a resumed entry routes its I/O under a new generation of this
  run's counter. The router never sees the recorded one.
- **The held fd.** A kept entry holds none: its `dev` and diskseq are the record's, checked
  against sysfs at the scan. The successor opens the device at step 6, as #15's ready step
  does, and holds it from then to the detach.
- **`device-info`** answers a kept or resuming entry with the running line's arity:
  `ok VOLUME kept|resuming BOOT_UUID INSTANCE GEN FIRST_EPOCH DEVICE MAJ:MIN DISKSEQ`.
  `status VOLUME` answers `ok VOLUME kept|resuming /dev/ublkbN MAJ:MIN INSTANCE GEN`, and
  `list` pairs the volume with `kept` or `resuming`.
- **`detach-device`** with the whole tuple, under #15's registry and entry locks: on a kept
  entry it ends the device and answers `ok detached`; on a resuming one it sets the detach
  flag, waits for that entry as #15's detach does, and answers the same; any differing field
  is `stale attachment`, untouched.
- **`detach VOLUME`,** by name (R2-B9): on a kept entry it ends the device; on a resuming one
  it sets the detach flag and waits for that entry, as on a running one (#15, S8). It is what
  the lab sends, and what the plugin's unstage sends until #79 is in (`control/csi/node.go:140`;
  #15, S9). With #79 the plugin sends `detach-device` only (#79, S7).
- **What #15's contract needs** is under Cross-plan items: the two states, and its sentence
  "Compare boot/instance against this daemon" read against the entry's lifetime.

**Request lifetime.** Nothing of the predecessor's is used again: no op id, read plan, pointer,
registered buffer, SQE or completion. Zero copy registers the successor's buffers and request
mappings before its queues fetch. A reissued write carries the kernel's data under the new
epoch, session and version; reconciliation has settled an unanswered write of the predecessor
first. An old acknowledgement answers nothing. Acknowledged writes stay durable.

**SIGTERM** (S4; R1-C14, R1-S4). #12's `should_exit(requested, state)` gains one case: it is
also true when SIGTERM was requested and every running entry is recoverable. The node then
exits at once with `process::exit`, sends no STOP_DEV, and writes nothing: the stamp is at most
1 s old, or, for a device resumed inside its old deadline, its first exit's (When `alive` is
stamped, second row). `begin_attach` refuses after the signal, as in #12.
- **Where it is checked:** on #12's `sigwait` thread, when the signal comes, under the
  registry's lock alone, and not at the main thread's next turn: an attach in progress does
  not delay the exit. Its entry is still starting, and the successor ends its device (S5), as
  it ends one whose resume was between steps 3 and 5.
- **With an unrecoverable entry** (S1's fallback) #12's S4 stays whole: the signal is
  remembered until the detach.
- **The pod's manifests do not change.** #12's preStop hook signals PID 1 and loops; when PID 1
  exits, the hook dies with the container (#12 design, The DaemonSet). So a deletion ends within
  seconds when every device is kept, and waits as in #12 when one is not. The grace period
  stays 3600 s.
- **A crash** is resumed the same way. `node.sh down` still refuses a mounted device and ends
  the others.

## Agent, deadline and races (S7–S11)

**The loop** runs in the CSI node plugin from its start, with no kubelet call.
- **What it holds** (R1-C2). Never the plugin's node lock, `n.mu` (`control/csi/node.go:28`),
  which a stage, publish, unpublish or unstage holds across `mkfs`, `mount` and `umount`
  (`:89`, `:135`, `:167`, `:210`): an unmount of a kept device blocks in the kernel until the
  resume, so a loop that waited for that lock would never resume it. The loop has a small lock
  of its own for its per-volume state, never held across a socket or metadata call.
- **What settles a resume against a detach** is the data plane's registry: the entry's lock and
  its state, and the tuple that `detach-device` compares. An unstage's `detach-device` carries
  the stage's tuple, which a resume keeps.
- **`device()` leaves kept volumes to the loop:** on `kept` or `resuming` it answers
  `Unavailable` and calls no `Attach`. A second caller taking epochs would make a gap.
- **A pass,** every 100 ms: `list`; for each volume in state kept with no try in flight and
  its back-off over, one try:
  1. where #83 is in, `attach-challenge` on the socket;
  2. `Attach(volume, node, only_if_target)` as the agent, with #83's `target_nonce`, unless the
     loop still holds a token for the volume that the data plane has not used;
  3. the socket's attach, with the token and #83's handoff, in #79's `attach-operation` form
     where #79 is in.

  The agent's key never reaches `io`. No token is written anywhere.
- **The token is kept, in memory,** until the attach succeeds, the volume is no longer kept,
  the answer starts `error: VOLUME: epoch: ` (#15's prefix: the mark has reached it, or the
  volume's epoch moved), or, with #83, the challenge read at step 1 is no longer the one its
  handoff was made for. Any other failure tries the same token again. So one failed try does
  not make a gap.
- **Pace** (R1-C15, R1-S9): per volume, the wait after a failed try is 100 ms, doubled each
  time, 5 s at most, and back to 100 ms once the volume leaves kept. One `Attach` per volume
  per step, none while a token is unused, two metadata requests in flight in all. In a bound's
  60 s that is at most 17 tries, so 17 epochs, for a volume.
- **A refusal.** `ErrNotTarget` or `NotFound` from the metadata service: the loop reads
  `device-info` and sends `detach-device` with that tuple, so only that kept lifetime is ended.
  The loop sends the line itself (#15 ships the daemon's side), through #79's client once
  that is in.
- **#83** (R1-C4, R1-S3). The loop, the lab's command (S10) and the engine test use #83's
  sequence: the challenge, the nonce, the handoff. Whichever of #48 and #83 lands second adds
  it to them, with the refusal's test. A quarantined data plane still runs: its entries are not
  kept, its `attach-challenge` refuses, and the loop takes no epoch. Once the operator restarts
  it, the successor is a fresh process as any other, and the loop's handshake is the recovery
  step #83's S5 asks for. A successor whose clock profile refuses gives no challenge, and its
  kept devices end at the bound.
- **An older plugin** resumes nothing, and refuses the states it does not know before any
  effect (#15's contract: exact parse). Under it kept devices end at the bound.

**The bound** is 60 s from the record's `alive` on the same boot (settled at approval,
2026-11-13), on `CLOCK_BOOTTIME`. A clock that fails or reads behind the stamp counts as
expired. A resume under way is not cut: the deadline thread skips a resuming entry, and the
resume ends within #6's limits. Under the helper a resume is under way from the `recover` that
the kernel accepted (With #74's helper, The deadline; R4-B1).

**Who ends a kept device** (R1-C12, R1-S6).
- **A data plane that runs:** its deadline thread (direct mode), or the helper (With #74's
  helper). Both start before the store is opened.
- **No data plane:** the agent runs `flaminio-io cleanup --expired`, #15's command with one
  flag. It takes #15's `flock`, classifies each record, of either form, by the scan's rows
  (Who reads which record), ends the devices those rows end, leaves the kept ones that are
  inside the bound, and exits. There is no `end-kept`, no
  nonce, and no second parser of records in Go.
  - **When:** once the socket has been silent for 2 s, then every 10 s while it stays silent.
    It ends only what is past its bound, so a run during an ordinary restart ends nothing.
  - **Output:** one line per record, `ID ended|kept|quarantined|left`. **Exit:** 0 when it ran;
    3 when the lock is held (`the node is running`); 1 otherwise.
  - **The binary:** `scripts/dev/csi-image.sh` adds `flaminio-io` to the CSI image, from a data
    VM's release build, as #12's `dataplane-image.sh` takes it. The plugin's pod is privileged
    and mounts `/dev` and `/run/flaminio` already (`deploy/csi/driver.yaml:126–132`).
  - **Versions:** the plugin is upgraded first (S11), so its `flaminio-io` is the node's or
    newer. A record it cannot parse is quarantined, never deleted.
- **One lock scheme:** #15's `flock` on the records directory, held by the node or the helper
  for its life and by `cleanup` while it runs. Inside the node, #15's entry lock orders a
  resume against an end. There are no other lock files.
  - A start that meets a running `cleanup` must not fail for it. The node, and the helper,
    try the `flock` every 100 ms for up to 5 s, the longest a `cleanup` holds it (#15), and
    only then refuse to start. `cleanup` itself still refuses at once. This is part of the
    start that S5 replaces in #15's S10.
- **The lock held, the socket silent, the bound passed:** `cleanup` exits 3. The loop logs
  `the data plane holds its lock and does not answer`, once a minute, and ends nothing: it
  never overrules a live process. That process's deadline thread still ends its kept devices,
  whatever its store and engine do. A process stopped as a whole enforces nothing: Risks.
- **Settled at approval, 2026-11-13,** and carried into the spec's S8 and S11: no deadline is
  enforced while neither a data plane nor the agent runs; an older agent is a rollout limit;
  the bound covers ending the service and the waiting I/O, not the kernel id, which a mount
  may hold after it.

## With #74's helper (`--privsep`)

#74 (review-clear, not implemented) hands recovery to #48: recovery commands go through the
helper, and its pin is closed before START_USER_RECOVERY and taken again after (#74 intent,
Which step). #74 is not edited: each change below that touches what #74 states is made by
whichever of #74 and #48 lands second (Cross-plan items). #74's bounds stay: only the helper's
child calls it; requests are ASCII, single-spaced, 128 bytes at most, with no descriptor;
replies carry one descriptor at most; a device the helper does not own is refused.

- **What outlives the I/O process is the device and root's record,** not the helper. The
  helper still exits with its child (#74, S5), and keeps only `CAP_SYS_ADMIN` after its start
  (#74, S1): a helper that started a second child would need `CAP_SETUID` and `CAP_SETGID`
  again. A unit's stop and a pod's deletion end the helper too, so the next helper must be
  able to take the device over in any case. That one path serves every exit.
- **The probe and the flags.** The helper asks `get_features` at its start. With both flags,
  `add` uses them as its fixed `ctrl_flags`; without, 0, as #74 states. The child still names
  no flag, and the reply's FLAGS shows what the kernel returned.
- **The record** is root's, written by the helper at `/run/flaminio/ublk/<ID>`, under #15's
  rules for that directory. The helper takes #15's `flock` at its start and holds it for its
  life, so one helper or one direct-mode node runs on a records directory. It is the
  direct-mode record without the fields only the I/O process knows:

  ```text
  #15's returned dev_info fields
  recovery 2
  boot BOOT_UUID
  phase starting|running|recovering|stopping
  size BYTES
  dev MAJ:MIN
  diskseq N
  alive NS
  ```

  - written before ADD_DEV (`starting`) and after it (the returned `dev_info`), at `start`
    (`running`), at `recover`, before the kernel call (`recovering`; back to `running` at an
    `error busy`), at `end-recovery` (`running`), at `stop` (`stopping`); removed after
    `delete`;
  - `dev` and `diskseq` are root's (R2-B10): the helper reads them itself from
    `/sys/class/block/ublkb<DEV>/{dev,diskseq}` after `start`, so its start proves the disk as
    the direct-mode scan does, and a difference is a quarantine;
  - **stamped** by the main thread, whose wait on the child's pidfd gets a time-out, by the
    one table of Root records ("When `alive` is stamped"): the tick, the stamp at the old
    deadline, and the exit stamp;
  - never written under the table lock.
- **The records directory is an argument** (R2-B3): `--records DIR`, default
  `/run/flaminio/ublk`, for `flaminio-helper`, `flaminio-io node` and `flaminio-io cleanup`.
  DIR is absolute, and checked as #15 checks the default. `flaminio-io node --privsep` refuses
  `--records` as the helper's, as it refuses `--id` (#74, Parse). #74's kernel tests run
  beside the live lab node, each helper at its own `--id`: each passes a directory of its own
  (root's, 0700, removed after), so it neither waits on the node's `flock` nor writes among
  its records. #48's kernel tests do the same. The agent's `cleanup` reads the default only.
- **The I/O process's note.** The volume, the instance, the generation, the first epoch, `dev`,
  diskseq and `epoch` are the I/O process's knowledge, and a volume's id does not fit a
  128-byte request. It keeps them in `kept/<ID>` of #15's `--state-dir`, beside the run marks
  and under their rules (0700, the I/O user's, no-follow, `.tmp` and rename, `fsync`):
  `note 1`, `boot`, `volume`, `instance`, `generation`, `first-epoch`, `size`, `dev`,
  `diskseq`, `epoch`, one line each. The I/O process writes it, before the entry turns running;
  replaces it at a resume; removes it after `delete`; and its successor reads it. Nobody else
  does: the helper, `cleanup` and the plugin decide by root's record and the kernel. So S3's
  "only root reads it" holds for root's record, the device's identity included; what the note
  alone carries is the volume and its lifetime (Risks).
- **The child's exit** (#74, Owned devices, outcome 4). Steps (a) and (b) stay. Then the
  helper closes the pin, and:
  - a recoverable device whose record says `running` is **kept:** no DEL_DEV. One whose
    record says `recovering` is not: it is deleted with the others; if the helper was killed
    with its child, the next start ends it (R3-B1). A kept device's record gets a last stamp
    by the stamp's table, judged by the clock at the exit: for a device this child started,
    or one it recovered whose old deadline has passed (R2-B2, R3-B2). So the deadline of a
    kept device never moves without a resume that was completed and outlived the old
    deadline;
  - any other device is deleted, as #74 states.

  Then it exits with the child's status.
- **The helper's start,** after the `flock` and before it spawns the child, classifies the
  record at its `--id` as the direct-mode scan does (the table above, on `dev_info`, the boot,
  the phase, `dev` and diskseq, the flags, the state and `alive`). A kept device is **owned
  by its record,** with no pin. A record of the other form: Who reads which record.
- **The deadline** is the helper's own: its main thread's tick ends a device that is still
  kept when its `alive` + 60 s has passed. It does not depend on the child. A `recover` moves
  the entry out of kept under the table lock before its kernel call, and the tick ends an
  entry under the same lock only while it is kept: one of the two wins.
  - **`error busy` puts the entry back to kept;** any other failure of `recover` leaves it
    out of kept, for `stop` and `delete` only.
  - **So the bound holds off only from a `recover` that the kernel accepted** (R4-B1; S8,
    narrowed under the helper). The helper learns of a resume at `recover`, not at the
    resume's steps 1 and 2, and between two busy `recover`s the entry is kept. A resume that
    reaches `recover` after the bound finds the entry `ending` and is refused
    (`error not kept DEV`); the device is ended, as after a crash. Direct mode's deadline
    thread sees the resuming entry from its first step and skips it.
- **How the helper ends a device** (R2-B4), at its start or at the deadline: STOP_DEV and
  DEL_DEV on a cleanup thread, as #74's outcome 4 (c) sends its DEL_DEV, which nothing waits
  for. A mounted device holds DEL_DEV with no bound (#74, outcome 3). So the start still spawns
  the child, the main thread stays on the pidfd, and the helper still exits with its child.
  - **The table meanwhile** (R3-B3): the entry stays, in a state `ending`, set under the table
    lock before STOP_DEV, until DEL_DEV returns. It holds the helper's one id: `kept` answers
    `ok none`, `add` answers `error device limit`, `recover` answers `error not kept DEV`, and
    `start`, `stop`, `delete` and `end-recovery` answer `error not owned DEV`. When DEL_DEV
    has returned the thread removes that entry's record, then the entry; it removes no other
    record. An exit before that leaves the record for the next start.
  - **The thread is made after #74's step 7** (R3-B4), where the server thread is made: the
    start classifies on the main thread, before the spawn, and only marks the entry `ending`.
    So the thread has `CAP_SYS_ADMIN` alone and `no_new_privs`, as #74's S1 asks of the
    helper, and none of the capabilities of steps 5 and 6.
- **Requests,** added to #74's table:

  | Request | Reply |
  |---|---|
  | `kept` | `ok none`, `ok quarantined DEV`, or `ok DEV QUEUES DEPTH IO_BUF FLAGS SIZE`; no descriptor |
  | `recover DEV` | `ok DEV QUEUES DEPTH IO_BUF FLAGS` + `/dev/ublkc<DEV>` |
  | `end-recovery DEV` | `ok` |

  - **`recover`:** START_USER_RECOVERY, once (`start_user_recover_once`); then the helper opens
    `/dev/ublkc<DEV>` `O_RDWR|O_CLOEXEC`, which is the new pin, and replies with it. `EBUSY`
    answers `error busy DEV` and opens nothing: the child asks again every 100 ms until its
    device step's deadline. Any other failure answers `error kernel recover: ERRNO`.
  - **`end-recovery`:** END_USER_RECOVERY with the pid kept at the spawn, as `start` sends
    START_DEV (`UblkCtrl::end_user_recover_for(pid)`, beside #74's `start_dev_for`). It waits
    for every queue's fetches, so the client's `START_LIMIT` rule covers it as it covers
    `start`. At most once per `recover`.
  - **`stop` and `delete`** act on a kept device too: that is how the I/O process ends one.
    There is no other request for it, and no root `end-kept` beside a running helper.
  - **New TEXT:** `busy DEV`, `not kept DEV`, `already recovered DEV`, `kept DEV`,
    `not recovering DEV`; OP gains `recover` and `end-recovery`. The longest reply, `kept`'s,
    is under 100 bytes.
  - **Refusals** (R2-B7), each with no kernel call; the child is the untrusted party:
    - `start DEV` on a kept device, or on one being recovered: `error kept DEV`; after its
      `end-recovery`: `error already started DEV`, #74's;
    - `end-recovery DEV` with no `recover` before it, or a second one: `error not recovering
      DEV`. The kernel would find the old completion done and turn the device LIVE with no
      server (6.8.12 `:2769–2790`);
    - `recover DEV` on a device that is not kept: `error not kept DEV`; a second `recover`:
      `error already recovered DEV`;
    - `add` while a kept device is owned: `error device limit`, #74's.
- **A quarantined device is in the status here too** (R3-B5; spec, S5): for a record at its
  `--id` that the start quarantined, `kept` answers `ok quarantined DEV`, and the I/O process
  counts and lists that id in `recovery`. Every `add` then fails, and the socket says why.
- **The I/O process.** At start, after `control`, it sends `kept`. A device with a note whose
  boot, `dev` and diskseq equal `/sys/class/block/ublkb<DEV>/{dev,diskseq}` becomes a kept
  entry with the note's lifetime; without one it cannot name the volume, and ends the device
  (`stop`, `delete`). A resume runs the table's steps with `recover` at step 3,
  `UblkDev::from_cdev` and the queue threads at step 4, `end-recovery` at step 5, and #15's
  sysfs read at step 6. A failure after `recover` succeeded sends `stop`, joins the queues and
  sends `delete` (#74, outcome 2).
- **With no helper running,** root's `flaminio-io cleanup` is what it is in #15, a direct-mode
  command under the `flock`, as `del` is in #74's `node.sh down`. It judges a helper's
  device by root's record alone (Who reads which record). Beside a running helper it refuses.
- **`node.sh`:**
  - `up` still runs `down` first (#74), and so never keeps a device. It is not the restart;
  - `down`: the mount guard, the unit's stop, then `flaminio-io cleanup` in both modes, which
    ends every recorded device, kept ones too; under `PRIVSEP=1`, #74's `del` of a remaining
    `/dev/ublkc<ID>` after it;
  - `restart` (new, S10) signals the unit, waits for its end, and starts it again. It runs no
    `down`, `del` or `cleanup`.
- **Tests** (by the second of #74 and #48 to land):
  - `flaminio-helper/src/serve.rs`, fake `Kernel`: `fixed_flags` (0 without both features,
    both with them, never zero copy), `child_exit_keeps_a_running_recoverable_device` (no
    DEL_DEV, the pin closed, the record stamped), `child_exit_deletes_any_other_device`,
    `adopts_a_kept_device_from_its_record`, `adoption_quarantines_a_mismatch` (no kernel
    write; the next `add` fails and deletes nothing),
    `adoption_ends_dead_starting_recovering_stopping_or_expired`,
    `recover_writes_recovering_before_the_kernel_call`, `busy_recover_turns_the_phase_back`,
    `recover_starts_recovery_then_pins`,
    `busy_recover_opens_nothing`, `second_recover_refused`, `end_recovery_uses_the_child_pid`,
    `stop_and_delete_end_a_kept_device`, `kept_device_ends_at_the_bound` (a fake clock),
    `no_stamp_until_the_old_deadline_after_recovery`,
    `child_exit_does_not_stamp_an_adopted_device`,
    `child_exit_during_recovery_does_not_stamp`,
    `child_exit_during_recovery_deletes_the_device`,
    `child_exit_after_a_resume_inside_the_old_deadline_does_not_stamp`,
    `an_exit_right_after_the_old_deadline_is_inside_a_new_bound` (the tick not yet run),
    `a_crash_loop_under_the_helper_ends_at_the_first_exits_bound` (four helpers in turn, their
    children dying before, during and after a resume, each inside the old deadline: `alive`
    never moves),
    `ending_a_held_device_blocks_neither_the_spawn_nor_the_exit` (a delete that blocks),
    `add_while_an_end_is_in_flight_is_device_limit`,
    `a_recover_that_meets_the_deadline_is_refused_or_wins`,
    `the_deadline_ends_a_device_between_two_busy_recovers`,
    `the_stamp_comes_at_the_old_deadline_under_the_helper` (the fake clock, the child alive,
    the next tick not yet due, no exit stamp),
    `requests_on_an_ending_entry_are_refused` (`kept`, `start`, `stop`, `delete` and
    `end-recovery`, each with its answer and no kernel call),
    `the_ending_thread_removes_only_its_own_record`,
    `kept_answers_quarantined_for_a_quarantined_record`,
    `start_after_end_recovery_is_already_started`,
    `end_recovery_without_recover_refused`, `second_end_recovery_refused`,
    `start_on_a_kept_device_refused`, `add_with_a_kept_device_is_device_limit`,
    `dev_and_diskseq_are_read_by_the_helper`, `holds_the_records_lock`,
    `records_directory_is_an_argument`;
  - `flaminio-helper/src/lib.rs`: `grammar` and `round_trip` with the three requests; the
    fuzz target `helper_request`, which also feeds its requests to a table that owns a kept
    device, on the fake `Kernel`: a refused request makes no kernel call;
  - `flaminio-helper/tests/kernel.rs` (`#[ignore]`, root): `kept_device_survives_its_child`
    (the child killed; the helper exits 137; `ublkc<ID>` stays; a second helper adopts it;
    after `recover` and `end-recovery` a write left unanswered completes),
    `successor_killed_before_a_queue_is_ready`, `successor_killed_after_its_queues_fetched`
    and `successor_killed_with_a_queue_partly_fetched` (in each the helper is killed with
    its child, as a unit's stop does, #74's outcome 5, so no helper deletes the device at
    the exit; the next helper finds `recovering`, sends no START_USER_RECOVERY, and the
    device is gone),
    `threads_after_the_start_hold_only_sys_admin` (with a device ended at the start whose
    `/dev/ublkb<ID>` the test holds open, as #74's `held_device_does_not_block_cleanup`
    does, so the cleanup thread is in DEL_DEV when the threads are read: every
    `/proc/<pid>/task/*/status` of the helper shows `CapEff` `0000000000200000` and
    `NoNewPrivs` 1). Each test's helper gets its own `--records` directory;
  - `flaminio-io/src/privsep.rs`: `resume_sends_kept_recover_end_recovery_in_order`,
    `busy_recover_is_retried_until_the_device_limit`, `a_missing_note_ends_the_device`,
    `a_note_of_another_diskseq_ends_the_device`,
    `a_quarantined_device_is_listed_by_recovery`, `privsep_refuses_records`;
  - `scripts/tools/node-scripts-test.sh`: the `node.sh` cases under S10's tests.

## The pause and the lab's command (S9, S10)

**The six times** (S9), each `CLOCK_BOOTTIME` in ms:

| Time | Where it is taken |
|---|---|
| process | first statement of `node::main` |
| store | when the store's open returns |
| socket | the control thread, at its first answer |
| attach | the control thread, when it takes the resume's attach |
| served | the device thread, when END_USER_RECOVERY (or `end-recovery`) returns |
| first | a queue thread, at the device's first completion after `served` |

- **`first` is on the I/O path:** one relaxed load of a per-device atomic at each completion,
  and a single store of the clock while it is zero. No lock, no log. The device thread reads
  the atomic in its 1 s wait and writes the line:
  `flaminio-io: resume VOLUME epoch E: process=… store=… socket=… attach=… served=… first=…`.
- **The pause** is measured by the workload, not synthesized: fio logs each completion
  (`--write_lat_log`, `--log_unix_epoch=1`, no averaging), and the pause is the largest gap
  between two consecutive completions that spans the signal. The check prints it beside the
  line above.

**`make node-restart`** (`cmd_node_restart` in `scripts/lab.sh`; the Makefile's help).
- **Variables:** `VM` (the lab's selector; here one VM, default `a`); `SIGNAL` (`TERM`, the
  default, or `KILL`; anything else is refused before any remote command); `PHASE` (`both`,
  the default, `stop` or `start`); `DATAPLANE` (`unit` or `pod`, #12's). Volume names read
  from the socket must pass the id grammar before they reach a command line.
- **A unit node:** `node.sh restart`, with `node_env`'s variables:
  1. passes #12's pod-node guard, as `down` does; dies on `FORMAT=1`, on `ATTACH`, and on a
     `PRIVSEP` that differs from the running unit's mode (`node-restart cannot change PRIVSEP:
     use make node-up`);
  2. `sudo systemctl kill -s "$SIGNAL" "$unit"`, which reaches every process of the unit;
  3. waits up to 10 s for the unit to be inactive, then `reset-failed`;
  4. starts the unit with the arguments `up` builds (one shared function), and waits for
     `serving replicas`, as `up` does. It never runs `down`, `del` or `cleanup`.
- **The resume on a unit node** is done by one party, as two would take epochs against each
  other:
  - the CSI node plugin's loop, when its pod is ready on the node;
  - otherwise the command: for each volume that `node.sh kept` prints (`list`, state kept),
    `AS=agent metactl "$vm" "$VM_A" attach "$v" "$vm" only-if-target` (the node's agent
    identity, `lab.sh:587`), then `node.sh attach` with that token; with #83's challenge and
    handoff through #83's `metactl` and `node.sh` changes, whichever lands second.
- **A pod node:** `TERM` deletes the node's data-plane pod (`kubectl delete pod`, no wait);
  `KILL` runs `sudo pkill -KILL -x flaminio-io` on the VM, and kubelet restarts the container.
  The plugin's loop resumes.
- **Then** it waits up to 60 s until `list` shows no kept or resuming entry, prints each
  volume's state and the resume lines, and exits non-zero if a device was ended.

## Contracts and files

**The control socket** (#15's S8 block, with #48's lines; every answer fits #74's 4096 bytes):

```text
status VOLUME   ->  ok none | ok VOLUME STATE [/dev/ublkbN MAJ:MIN INSTANCE GEN]
list            ->  ok limit N [VOLUME STATE ...]
recovery        ->  ok on|off-kernel|off-flags|unknown N [ID ...]
```

- **STATE** gains `kept` and `resuming`; the arity of `status VOLUME` and the pairs of `list`
  do not change, so 16 entries still fit. `device-info` and `detach-device`: The lifetime
  tuple, above.
- **`recovery`** (new, read-only): the node's mode (Kernel capability), the number N of
  quarantined records, and their ids, ascending, 16 at most. The longest answer is under 200
  bytes. A build without #48 answers its usage line. Under `--privsep` the mode is `unknown`
  before the first `add`, and the quarantined id is the one the helper's `kept` named
  (R3-B5).
- **`detach VOLUME`** on a kept or a resuming entry: The lifetime tuple, above.
- **`--records DIR`** (`flaminio-io node`, `flaminio-io cleanup`, `flaminio-helper`): With
  #74's helper.
- **A plugin that does not know a state** refuses it before any effect (#15's contract).
- **An attach of a kept volume** answers `ok /dev/ublkbN`, the same path, or one of:
  - `error: VOLUME: epoch: …`, #15's own, for a token at or below the run mark: still kept;
  - `error: VOLUME is resuming`, for a second attach meanwhile;
  - `error: VOLUME: resume: busy: the kernel still holds the device; still kept`;
  - `error: VOLUME: resume: gap: epoch E, run mark M; the device is ended`;
  - `error: VOLUME: resume: another volume: first epoch F, recorded G; the device is ended`;
  - `error: VOLUME: resume: size S, recorded R; the device is ended`;
  - `error: VOLUME: resume: the kernel refused the recovery; the device is ended`;
  - #83's refusal of a missing or foreign handoff, unchanged: still kept.

  Every `resume:` line starts `error: VOLUME: resume: `. The loop does not parse them: it
  reads `list` again.
- **`flaminio-io cleanup [--expired]`:** Who ends a kept device, above. Without the flag it
  ends every recorded matching device, as `node.sh down` needs.
- **The record** and its compatibility: Root records. **The helper's requests and the note:**
  With #74's helper.
- **libublk, vendored** (#74, #15): `start_user_recover_once`, `end_user_recover_for(pid)`.
- **`flaminio-repl`:** `RunEpochs::resume` and its `Resume`.
- No metadata RPC, `auth.go` rule, replication frame or on-disk format changes.

**Touched:**
- `dataplane/crates/flaminio-repl/src/run_epochs.rs` (#6's), `src/sim/{mod,tests}.rs`;
- `flaminio-node/src/engine.rs` (`Input::Attach` and `MarkEpoch` say that they resume; the
  first epoch and the size);
- `flaminio-io/src/{node,ublk,control,cli}.rs`, `record.rs` and `kept.rs` (new),
  `privsep.rs` (#74's), `tests/recovery.rs` (new);
- `flaminio-helper/` and `dataplane/vendor/libublk/` (#74's);
- `control/csi/{node,dataplane,server}.go`, `resume.go` and `resume_test.go` (new);
- `scripts/dev/csi-image.sh`, `scripts/guest/node.sh`, `scripts/lab.sh`,
  `scripts/checks/node-restart.sh` (new), `scripts/tools/node-scripts-test.sh`, `Makefile`;
- `docs/ARCHITECTURE.md`, `docs/replication-protocol.md` §9, `docs/lab.md`,
  `docs/followups.md`, `README.md`.

#12's manifests are not touched: `/run/ublksrvd` stays an `emptyDir`, and the preStop hook and
the grace period stay (R1-C13, R1-S11, R1-C14).

## Tests: rule → case

| Rules | Test (file: case) | Level |
|---|---|---|
| S1, S2 | `flaminio-io/src/ublk.rs`, fake control: `add_asks_both_flags_or_neither`, `returned_flags_decide`, `zero_copy_without_the_flags_turns_recovery_off`, `a_recoverable_add_is_disowned`, `exit_sends_no_stop_or_delete`; `control.rs`: `recovery_line` | unit |
| S1, S2, S6 | `flaminio-io/tests/recovery.rs` (`#[ignore]`, root, a disposable device; Linux 6.6 and the lab's 7.0): `requests_in_flight_are_issued_again`, `recovery_alone_fails_them` (the one flag), `the_tuple_survives_a_recovery` (`major:minor`, diskseq), `a_successor_killed_before_a_queue_is_ready_ends_the_device`, `a_successor_killed_after_its_queues_fetched_ends_the_device` and `a_successor_killed_with_a_queue_partly_fetched_ends_the_device` (the next start finds `recovering`, sends no START_USER_RECOVERY, and the device is gone); each with its own `--records` directory | kernel |
| S3 | `record.rs`: `round_trip`, `grammar_refuses` (over 4096 bytes, another `recovery` number, an unknown phase, a missing or extra line, a leading zero), `a_record_without_the_block_is_fifteens`, `holds_no_secret`, `modes_and_no_follow`, `stamp_replaces_atomically` | unit |
| S4 | `node.rs`: `sigterm_exits_at_once_when_every_entry_is_recoverable`, `sigterm_waits_with_an_unrecoverable_entry`, `no_attach_after_sigterm`; #12's manifest test unchanged (preStop, 3600 s) | unit |
| S5 | `kept.rs`, fake kernel, one case per row of the scan's table: `no_device_drops_the_record`, `another_boot_drops_the_record`, `fifteens_record_takes_fifteens_rule`, `a_mismatch_is_quarantined` (no kernel write; listed by `recovery`), `dead_is_deleted`, `starting_recovering_and_stopping_are_ended`, `unrecoverable_takes_fifteens_rule`, `one_recovery_flag_alone_is_ended`, `past_the_bound_is_ended`, `quiesced_or_live_is_kept`, `a_failed_get_dev_info_decides_by_sysfs` (a fake kernel that answers `EOPNOTSUPP`: removed without `ublkc<id>`, left with it), `the_other_form_is_a_mode_change_at_a_start_only`, `an_unrecorded_device_is_untouched`; `kept_counts_towards_the_limit`, `the_deadline_starts_before_the_store`, `startup_attach_never_resumes` | unit |
| S6 | `ublk.rs`, fake queues: `resume_steps_in_order` (the mark before START_USER_RECOVERY, the target before END_USER_RECOVERY), `busy_is_retried_until_the_device_limit`, `another_recovery_failure_ends_the_device`, `a_failure_after_recovery_started_ends_the_device`, `resume_keeps_the_lifetime_tuple`, `resume_routes_under_a_new_generation`, `no_predecessor_buffer_or_completion_is_used`, `another_volume_never_takes_a_kept_device`, `an_old_token_keeps_the_device`, `a_gap_ends_the_device` (at the socket, and at the mark's write), `another_first_epoch_ends_the_device`, `a_second_attach_meanwhile_is_refused`, `a_crash_after_each_step_leaves_what_the_table_says` (a fake kernel that counts ready queues), `no_second_recovery_after_a_death_in_steps_3_to_5`, `a_try_that_gives_up_on_busy_turns_the_phase_back` | unit |
| S6 | `flaminio-node/src/engine.rs`: `a_second_engine_resumes_at_the_next_epoch` (a second engine on the store of one dropped without a detach: acknowledged data read back, writes in flight written again), `a_resume_at_the_old_epoch_is_refused`, `a_resume_of_another_first_epoch_starts_no_target`, `a_resume_of_another_size_fails`, `a_resume_marks_its_epoch_as_a_resume`, `a_resume_without_a_handoff_is_refused` (#83; by the second to land) | engine |
| S6 | `flaminio-repl/src/run_epochs.rs`: `resume_goes_only_at_the_next_epoch`, `resume_at_or_below_the_mark_is_stale`, `resume_over_a_gap_ends`, `resume_of_another_first_epoch_ends`, `resume_without_a_mark_ends` | unit |
| The invariant | `flaminio-repl/src/sim/tests.rs`: `a_kept_device_resumes_and_its_writes_are_issued_again`, `a_kept_device_ends_after_a_target_elsewhere`; the mutations `resume_over_gap` and, on the first scenario, #6's `restart_at_epoch` and #15's `volatile_run_mark` | simulator |
| S6, S8 | `control.rs`: `kept_and_resuming_in_list_status_and_device_info`, `detach_device_ends_a_kept_lifetime`, `detach_device_with_another_tuple_is_stale`, `detach_during_a_resume_ends_the_device`, `detach_by_name_ends_a_kept_entry`, `detach_by_name_ends_a_resuming_entry`, `resume_refusals` (each line), `longest_answers_fit_4096` | unit |
| S8 | `kept.rs`: `the_deadline_ends_a_kept_entry`, `the_deadline_skips_a_resume_under_way`, `a_clock_behind_the_stamp_expires`, `no_stamp_until_the_old_deadline_after_a_resume`, `the_stamp_comes_at_the_old_deadline_not_at_the_next_tick`, `an_exit_after_the_old_deadline_is_inside_a_new_bound`, `a_crash_loop_ends_at_the_first_exits_bound`; `cli.rs`: `cleanup_expired_ends_only_what_the_rows_end`, `cleanup_expired_keeps_a_helpers_device_inside_the_bound`, `cleanup_expired_ends_a_helpers_device_past_the_bound`, `cleanup_ends_every_recorded_device`, `cleanup_refuses_under_the_lock` (exit 3), `cleanup_prints_one_line_per_record` | unit |
| S7, S8 | `control/csi/resume_test.go`, a fake data plane and metadata service: `TestResumeWithoutKubelet`, `TestResumeWhileUnmountBlocks` (an `Unmount` blocked on a kept device under `n.mu`: the loop resumes it, and the unmount returns), `TestResumeSecondVolumeWhileUnmountBlocks`, `TestOneAttachPerBackoffStep` (counts the calls: 17 at most in 60 s), `TestTokenKeptUntilUsed`, `TestNotTargetEndsTheKeptLifetime` (`detach-device` with the tuple, never `detach VOLUME`), `TestStuckVolumeBesideAResumedOne`, `TestDeviceLeavesKeptVolumesToTheLoop`, `TestUnstageByNameAfterResume` (until #79), `TestUnstageAfterResume` and `TestDetachDeviceWithThePreRestartTuple` (#79's record and client; by the second of #48 and #79 to land), `TestKeptStateParsing` (an unknown state, a wrong arity, an answer over 4096 bytes: refused before any effect), `TestCleanupWhileTheSocketIsSilent` (first after 2 s, then every 10 s), `TestLockHeldIsLoggedNotOverruled`, `TestChallengeAndHandoff` (#83; by the second to land); `meta/service`: the existing test that `io` may not call `Attach` | unit (Go) |
| S9 | `ublk.rs`: `resume_line_has_six_times_in_order`, `first_is_taken_once_without_a_lock`; no time is taken from the attach's answer | unit |
| S10, S11 | `scripts/tools/node-scripts-test.sh`: `restart` sends the signal and starts the unit, with no `down`, `del` or `cleanup`; it dies on `FORMAT=1`, `ATTACH`, a changed `PRIVSEP`, a `SIGNAL` other than `TERM` or `KILL`; `node-restart` on a unit node resumes with the agent's identity only when no plugin pod is ready; on a pod node it deletes the pod or kills the process; a volume name with a space, `;` or `$(…)` is refused; `down` runs `cleanup` in both modes and keeps #74's `del`; `csi-image.sh` puts `flaminio-io` in the image | script |
| With #74's helper | With #74's helper, Tests | unit; kernel |
| S12 | `make lint`'s doc and help checks; the lab guide names `node-restart`, `PRIVSEP`, the downgrade rule | docs |

**The simulator** (`flaminio-repl/src/sim`; replication-protocol §9).
- **A kept restart,** a scripted step as #6's `DeviceFails` is, so that no seed's run changes:
  `Action::RestartKept(n)`. Node n's target ends as in a crash; its run mark stays (#15); its
  client's unanswered operations are held, not dropped.
- **At the node's next start** it calls `RunEpochs::resume` with the view's epoch, then `admit`
  as every start does. `Go`: the held operations are issued again to the new target, each
  still the operation it was in the history. `End`: they stay unanswered, as after a crash.
  `Stale`: nothing starts.
- **A new check:** the simulator records every target start (#6). A resume fails the run with
  `kept device resumed after epoch E ran elsewhere` if a target of the volume started at an
  epoch between the kept device's last one and the resume's. Linearizability cannot see this:
  a held write that lands after another target's acknowledged write is a legal order. What is
  broken is the device's one writer, so the check is its own, as #6's `two targets at epoch`.
- **Scenarios:** `a_kept_device_resumes_and_its_writes_are_issued_again` (overlapping
  unanswered writes; histories stay linearizable under the rule for unanswered writes), and
  `a_kept_device_ends_after_a_target_elsewhere` (the node exits at 5; another node attaches at
  6 and acknowledges a write; the first is attached again at 7: its start must end the kept
  device, and its held operations stay unanswered).
- **The injected bug** is `run_epochs::Bugs::resume_over_gap`: `resume` answers `Go` for any
  epoch above the mark. The second scenario must then fail with the new check's text. It turns
  off the comparison that ships, not a copy.
- **No second copy of #6's rule:** the draft's `resume_keeps_epoch` is gone. The first scenario
  also runs with #6's `restart_at_epoch` and #15's `volatile_run_mark`, each caught by
  `two targets at epoch`.
- **A volume made again** is `End(OtherVolume)`, tested in `run_epochs.rs` and in `ublk.rs`.
  The simulator has one incarnation of its volume, and a volume made again starts above its
  earlier epochs (#30), so its resume is also a gap.
- **A reissued request waits for its chunk:** the target's rule as it is, with the mutations
  §9 already lists. #48 adds none there.
- §9 gets the check, the two scenarios and the mutation's row.

Implementation runs `make lint test race hygiene contracts sim`, and the sanitizers' quick run
for the queues and buffers. The kernel tests need a disposable ublk device and root. No test
or lab action ran while drafting.

## Lab, documentation, risks and ADRs

**Lab** (R1-C10), on an exclusive lab, Linux 7.0. Never run unless asked (AGENTS.md).
`make node-restart-check` is new (`scripts/checks/node-restart.sh`; `CASE` is `restart`, the
default, `moved`, `back` or `gone`).

1. **Set up:** `make deploy`, `make node-up` (under the helper, #74's default), `make
   vol-create`, `DATAPLANE=unit make csi-deploy` (#12: a plain `csi-deploy` would replace the
   units with pods). Pass: each exits 0.
2. **The restart under a pod:** `make node-restart-check`. It starts a pod on fl-a with
   verified fio on an ext4 volume, runs `SIGNAL=TERM make node-restart`, then `SIGNAL=KILL make
   node-restart`, stops fio and verifies. Pass, for each restart:
   - fio reports no error and its verification passes; the kernel's log has no I/O error for
     the device;
   - `major:minor`, diskseq and the mount's source are those from before;
   - the volume's epoch is one higher;
   - the pause (S9) is under 2000 ms; the six times are printed.
3. **Direct mode:** `PRIVSEP=0 make node-up`, then step 2 again. Same pass.
4. **Zero copy:** `ZERO_COPY=1 make node-up` (direct mode), then step 2 again. Same pass. If
   `recovery` answers `off-flags`, the check fails and says so: S1's "with zero copy too" does
   not hold on that kernel, which goes back to the plan.
5. **A pod node** (#12; its pods run direct mode): `make csi-deploy`, then `DATAPLANE=pod make
   node-restart-check`. Pass as step 2, but the pause is reported, with no limit.
6. **Two volumes** (`PRIVSEP=0`, #15's `make multi-vol` set-up): fio on both, `make
   node-restart`. Pass: both resumed, each as step 2.
7. **Moved away:** `CASE=moved make node-restart-check`. With fio running in the pod on fl-a:
   `SIGNAL=KILL PHASE=stop make node-restart`; `make vol-attach VM=b`; `PHASE=start make
   node-restart`. Pass: fl-a's device is gone within 5 s of the start, the pod's fio reported
   errors, and the data verifies on fl-b.
8. **Moved away and back** (R1-S1): `CASE=back make node-restart-check`. `PHASE=stop`, as in
   step 7; `make vol-attach VM=b`, a verified write on fl-b's device, the volume detached
   there; the administrator's `metactl "$VM_A" "$VM_A" attach "$VOL" "$VM_A"`; `PHASE=start`.
   Pass: fl-a's answer or log has `resume: gap`, its device is gone, and a new attach on fl-a
   reads what fl-b wrote.
9. **No return:** `CASE=gone make node-restart-check`: `PHASE=stop make node-restart` with the
   plugin running. Pass: the device is gone between 59 s and 75 s after the signal, and the
   pod's fio reported errors.
    Steps 7 to 9 run under the helper, the lab's default; step 9 also with `PRIVSEP=0`.
10. **Unchanged checks:** `make chaos`, `make node-fault`, `make csi-failover`. Pass: each
    exits 0.

**Restore:** the check deletes its pod and claim on any exit; `make csi-undeploy`; `make
node-down`, which ends what a failed step left, once nothing is mounted; `make node-up` in the
default mode. Nothing reformats: `FORMAT` stays unset.

**Kernel check** (open, for the ADR; before implementation). Read in Linux 6.6 and 7.0, or
shown by `tests/recovery.rs` on each:
1. both flags are offered by `GET_FEATURES`, alone and with the zero-copy flags;
2. START_USER_RECOVERY's conditions: the character device closed, QUIESCED, and the test of
   `nr_queues_ready` before the queues are reset (`EINVAL`; 6.1.112 `:1918–1919`, 6.8.12
   `:2725–2726`), which an early 6.6.y may lack;
3. when a dead server's device turns QUIESCED, and whether an open character device (the
   helper's pin, until its exit) delays it;
4. END_USER_RECOVERY takes the pid of another process (the helper sends its child's);
5. a recovery changes neither `major:minor` nor diskseq;
6. STOP_DEV on a QUIESCED device fails its held requests;
7. what a successor that dies during a recovery leaves (R2-B5, R3-B1): fetches that are not
   released, or a queue that never was ready. It is the reason why no second
   START_USER_RECOVERY is sent; no rule here depends on the answer.

A "no" to 1 on 6.6 changes what the baseline promises: a superseding ADR (spec, Q5). A "no" to
any other changes this design, and comes back as a finding.

**Docs.** ARCHITECTURE §5.1, §11.1, §18; README; `docs/lab.md` (`node-restart`, the check, the
plugin first, detach before a downgrade, the quarantine and its `del`); `docs/followups.md`
(the lab's result, the kernel check); `docs/replication-protocol.md` §9. Protocol versions and
the disruption guard stay future.

**Risks,** each named, not accepted:
- **A gap that is no move.** A lost `Attach` reply, an agent that restarts holding an unused
  token, or (#83) a data plane that restarts between the `Attach` and the attach: the device is
  ended, and the pod sees I/O errors, as after a crash today. Proving continuity otherwise
  would need the metadata service to say since which epoch a node is the target, which S11
  leaves unchanged.
- **The kernel facts above** are read in 6.1.112 and 6.8.12 only. On a kernel that quiesces a
  dead server's device by a 5 s monitor, the pause passes 2 s whatever the data plane does.
- **A data plane stopped as a whole** (`SIGSTOP`, a stall) holds the lock and enforces nothing;
  the agent logs it and overrules nothing. Its kept devices wait.
- **A quarantined device** waits until the operator's `del`.
- **A slow crash loop:** a request that ends its server after the device's old deadline has
  passed is issued again at the next resume. The stamp's rule ends only loops whose server
  dies before that deadline.
- **A downgrade** to a build without #48 leaves kept devices waiting: detach first.
- **The CSI image carries `flaminio-io`,** a root path to ublk control in a pod that is
  privileged already. It runs only `cleanup --expired`, under the lock.
- **Under `--privsep` the note is the I/O user's,** in the trust class of its disk: a
  compromised I/O process can leave its successor a wrong volume for the device it served.
  Root's record, and the device identity the plugin checks, are not its to write.
- **Settled at approval, 2026-11-13,** and unchanged: the agent must run for the bound to hold
  with no data plane; an older agent resumes nothing; a mounted id may outlive the bound.
- A lost ISR member adds the takeover wait, and #83's guard where it applies; neither is
  shortened to meet 2 s. Pauses of the kernel or the hypervisor are not covered.

**Cross-plan items,** each made by whichever of the two plans lands second; none is edited
here:
- **#74:** `add`'s `ctrl_flags` and `fixed_flags`; outcome 4 (a running recoverable device is
  kept, not deleted); outcome 5 and Ownership (a new helper adopts by record; a kept device is
  owned without a pin); the three requests, their TEXT and the invariant's "nothing else"; the
  helper takes #15's `flock` and writes a record (its decisions R1-C8 say no files); the two
  libublk hunks; `node.sh restart`, and `down` running `cleanup`; `--records DIR`, which each
  test of `tests/kernel.rs` and `make privsep-check` pass, so that they run beside the live
  node (R2-B3), and which `--privsep` refuses; the refusals' TEXT; the table's `ending` state
  and `kept`'s `ok quarantined DEV`; the start's cleanup thread made after step 7, with
  `credentials_after_start` reading every thread (R3-B3 to R3-B5).
- **#15:** a revision of `device-lifetime-contract.md`: the states `kept` and `resuming`, and
  "Compare boot/instance against this daemon" read as the entry's lifetime, which for a
  resumed entry is the adding run's. In "With #74's helper": the records directory and its
  `flock` exist under `--privsep` once #48 is in. In S10: the start tries the `flock` for 5 s
  before it refuses; `--records DIR`; and its three decisions by `ENODEV`, which libublk's
  second try can hide on a kernel without legacy opcodes (R2-B6).
- **#79:** pins that revision. Its record and rules do not change. One case to state: an
  operation left unbound by a lost attach reply, met after a restart, must read `device-info`
  before it takes an epoch, since the volume may have a kept or resumed entry.
- **#83:** the loop and `node-restart` use its sequence; its plan does not change.
- **#12:** the CSI image gains `flaminio-io`; `should_exit`'s new case. Its manifests stay.
- **#6:** `RunEpochs` gains `resume` and its bug; `ADD_DEVICE_LIMIT` bounds the `EBUSY` wait.

**ADRs.** 0003's 6.6 baseline is consistent with the UAPI read; the kernel check decides the
rest. 0005 and 0006 (fencing, ISR) are unchanged. The audit's proposed 0007's stricter
readiness is kept. The [issue-local ADR](adr-proposal.md) was accepted with the plan,
2026-11-13; this round's changes to it are recorded in decisions.md.

**Parallelizable?** Yes, against Contracts: the Go loop; the Rust record, scan and resume; the
helper's requests. `RunEpochs::resume` lands first, as the engine, the socket and the simulator
use it.
