# Issue #15: more than one volume per node — specification

This issue is built after #10 lands. S11 measures it against main with #10.

Terms:
- a **device** is a ublk device that the node serves a volume on (`/dev/ublkbN`), with its queue
  threads, rings and buffers;
- the **instance** is a random number of 64 bits or more, drawn from `getrandom` once per run of
  the node;
- a **generation** numbers the node's attaches, from 1 in each run;
- the **registry** is the node's list of entries `(volume, id, generation, state)`. The state is
  one of:
  - starting: from the attach's reservation until the device is ready;
  - running;
  - stopping: from a detach, a deposition or the device's end, until the entry goes (S5);
- the **engine thread** owns the engine's inputs (S3);
- **node-wide** state is shared by every volume: the store and its space, the group commit, and
  #10's gather, holding and arrivals.

## Behaviour

**S1. Several attaches, up to a limit.**
- **The limit:** `--max-volumes N`. The default is to be approved; 8 is proposed.
  - 8 devices with one queue each reserve up to 512 MiB of buffers at the default depth and I/O
    size. That is address space, backed only once touched.
  - The usage text gives a device's cost: depth × the largest I/O, per queue.
- **Names:**
  - every socket command checks its volume name by the metadata service's rule
    (`ValidID`, `meta/domain/domain.go`): 1–128 of `[A-Za-z0-9._-]`, starting with a letter or
    digit;
  - the CSI node plugin checks it too, before it builds a command line (`dataplane.go`);
  - the 16-byte rule (`cli.rs`) stays `serve`'s only.
- **Before a slot is reserved,** the socket checks the token: its signature, its volume and its
  node (S12).
- **Then, in one step under the registry's lock,** the attach takes an entry, or is refused.
  Every entry counts against the limit until it goes, whatever its state. So does a device that
  starts after its attach timed out (#6, S2).
- **Refused at once,** before the metadata service is asked:
  - at the limit: `error: limit: N volumes attached or attaching`. Callers match the prefix
    `limit:`;
  - the volume has an entry: the answer names its state.
- **Every error names its volume.** `attach`'s success answer stays as today (S7).

**S2. Device identity.**
- **Ids only increase.**
  - At start, the first id is above the highest ublk id present on the host, and at or above
    `--id` if that is given.
  - Each later device takes the next free id. An id the kernel refuses as taken is skipped.
  - `add_device`'s retry of its own id goes. The usage text of `--id` says how ids are chosen.
- **The marks of a device,** and the reuse each one rules out:
  - **`major:minor`, read with `fstat` on an open fd:** the fd is the device the node named, not
    a file or another device at that path;
  - **the instance:** the answer came from this run of the node. Ids and generations start again
    after a restart;
  - **the generation:** the device is this attach's, not an earlier attach's of the same volume.
- **The device's node in `/dev`:**
  - **Waiting for it:** udev creates `/dev/ublkbN` after libublk's ready callback. The node
    waits for it, within `START_LIMIT`, before it answers the attach.
  - **The held fd:** the node then opens it `O_RDONLY`, without `O_EXCL`, and holds that fd until
    the volume's `detach`, the CSI node plugin's call. A deposition does not close it: the entry
    stays stopping until the detach.
  - So, while the node runs, no later device gets that `major:minor` while a mount may still use
    it.
  - **Stopping entries:** each holds a slot, an fd and a thread. One older than 5 minutes is
    logged, and logged again every 5 minutes.
- **Across a restart,** the held fds go with the process.
  - **A filesystem mount** holds its device open, so the device's id stays taken until the
    unmount.
  - **A raw block publish:** the bind mount of a device node does not hold it open.
    - What should: kubelet's block-volume mapper (`MapBlockVolume`) attaches a loop device to
      the published device file. This is from memory of kubelet's code
      (`volumepathhandler.AttachFileDevice`), and is not checked here: the lab test (Tests)
      settles it.
    - If kubelet does not hold it open, the CSI node plugin holds an fd per raw block publish,
      until the unpublish. That still leaves a gap: while the plugin itself restarts, nothing
      holds the device.
- **No deletion is synchronous.**
  - **What deletes:** every deletion of a device is `DEL_DEV_ASYNC`: the node's, and
    `flaminio-io del`'s.
  - **libublk's own:** its control handle deletes the device synchronously when dropped
    (`UblkCtrlInner::drop`). It is created with `UBLK_DEV_F_DEL_DEV_ASYNC`, or disowned, so that
    it never does.
  - **The kernel:** `DEL_DEV_ASYNC` is in Linux 6.9 and later; 6.8's headers lack it. The lab runs
    7.0.
  - **On an older kernel:** the synchronous `DEL_DEV`, which waits for the device's last close,
    runs on a thread of its own, outside every lock. Nothing waits for it.
- **Leftovers at start.**
  - **Before `ADD_DEV`,** the node records the device's id under `/run/flaminio`, and removes the
    record after the delete.
  - **At start, before any attach,** it deletes, asynchronously, every recorded device that the
    kernel says is dead.
  - **An open dead device,** such as one still mounted after a crash, is deleted at its last
    close.
  - **Unrecorded devices** are never touched, nor is anything decided by a server's pid, which a
    pid namespace hides. #12's plan follows the same rule.
- **The lab's scripts:**
  - they take a device's path from the attach's answer or from `status VOLUME`, never
    `/dev/ublkb1`;
  - `node.sh down` stops the unit first, which closes the fds, then deletes its leftovers
    asynchronously.

**S3. The engine thread.** It runs from the node's start to its end, devices or not.
- **It alone owns the engine's input channel.** The network and metadata threads, the control
  socket and the frontend all send there.
- **It waits on the node's eventfd.** Every sender signals it, and so does the store's commit
  ring on each completion.
  - **Its timeout** is the earliest of three: the next 10 ms turn, the gather or hold deadline,
    and #10's arrivals limit (the window's end + 8 ms). These go into `next_wakeup`, at
    nanosecond precision: `ppoll`, a timerfd or a ring, as the design picks.
  - **A thread that arms a deadline** signals the eventfd.
- **Device control never runs on it,** nor under the engine's or the registry's lock. Stopping a
  device, deleting it, and waiting for either run on that device's own thread.
  - Today, a deposition spawns a one-off thread (`ublk.rs`, `pump_with`), and a detach stops the
    device on the socket's thread (`control.rs`, `detach`).
  - A device has stopped once `run_target` has returned. A detach waits for that, never for the
    deletion.
- **Lock holds:**
  - the engine thread handles at most 64 inputs per hold of the engine's lock. Today's drains
    have no limit (`pump_with`, `drive_until`);
  - **hand-off:** while the engine thread waits for the lock, a flag says so. A queue thread that
    sees it at the end of its round yields the lock, and waits on a condvar until the engine
    thread is done. There is no new dependency;
  - **the bound:** with two busy volumes, the engine thread's longest wait for the lock, and the
    lateness of its heartbeats, each stay under 100 ms. The lease is 5 s, renewed every 1 s
    (`Timing::default`). The bound is to be approved;
  - **measured** by the engine thread itself. It logs both maxima every 10 s, and at the commit
    report of each detach. A heartbeat's lateness is the time it was sent less the time it was
    due.
- **Commits** are asynchronous from when the first device starts until the last one stops, and
  synchronous on a node with no device, as today.

**S4. Queue threads, as today.**
- **Writes, and reads the fast path refuses:** submitted, and pumped at the end of each round,
  under the engine's lock (`EngineCtx::submit`, `pump_if_due`).
- **Fast reads:** admitted, read and ended on the queue thread that holds the request. Their
  rules, and the simulator's model, are unchanged.
- **The engine thread now delivers** commit completions and remote replies: RF 2 acknowledgements
  and remote read pieces. Each costs one wake-up more than today, when queue 0 served the eventfd
  itself. S11 bounds the cost.
- **Routing:**
  - op ids are unique node-wide, and never reused in a run;
  - `Input::Io` carries the generation, and I/O whose generation is not its target's is dropped;
  - every completion, and every piece of a direct read, reaches the generation and the tag that
    hold its request, whichever thread pumped it;
  - an output whose op or generation the registry does not know is dropped and logged.
- **#10's arrivals:** each device registers a counter when it starts, and removes it when it
  stops. The wait holds on their sum.

**S5. One volume's faults stay its own.**
- **Generations, end to end:**
  - `Input::Attach` and `Input::Detach` carry the generation;
  - `Attached`, `AttachFailed`, `Deposed` and `Detached` echo it;
  - an attach's outputs reach the attach that waits for them, by `(volume, generation)`;
  - a stale output is dropped and logged;
  - **`Detach` always gives `Detached`,** even with no target left. Today it gives nothing then
    (engine.rs 1578).
- **Each of these ends only its own volume:**
  - **detach:** the entry turns stopping; the target ends; the device stops; the held fd is
    closed; the entry goes;
  - **deposition:** a newer target takes over, the device stops, and the entry stays stopping
    until the detach;
  - **a failed or timed-out attach:** #6's S3, per volume;
  - **a device that stops otherwise,** such as a queue thread's error: its target ends with it.
- **The socket's answers for each state:**

  | Command | starting | running | stopping | no entry |
  |---|---|---|---|---|
  | `detach` | `error: VOLUME is attaching` | as today | waits for the stop, closes the fd: `ok` | `error: VOLUME is not attached here`, today's wording |
  | `status VOLUME` | S7 | S7 | S7 | `ok none` |
- **Deleting a volume's replicas.**
  - **The engine refuses it** while the volume has a target or a pending attach. The socket's
    check alone would race an attach queued before the delete.
  - **At the first step,** the volume is forgotten, with its sessions, and goes into a set of
    pending deletes. While it is there, a `Hello` or an `Attach` for it is refused.
  - **Each engine turn** deletes at most `k` chunks, with `k` set by the design so that a turn
    stays within its 10 ms. Other volumes' writes, heartbeats and leases go on between turns.
    - **Why:** today `delete_volume` deletes every chunk in one call. A volume of 1 TiB has
      4 096 chunks, and at about 2.5 ms each, its delete would outlast the 5 s lease.
  - **The volume leaves the set** when its delete ends, or on a store error.
  - **Who may delete** stays unchecked: an accepted risk, left to #12 (intent).
- **What one volume can still make others wait for:**
  - **Settling** (`settle_local`, #10's `launch_held`) holds the engine's lock until the commits
    in flight land, and those of what was queued or held when it began.
    - **When:** a peer's session closing (at fl-b's restart in the node fault test),
      `InspectLocal`, each delete turn, and, on #10, every `Hello`, another volume's attach
      included.
    - **The bound:** those commits.
    - The lab records the other volume's longest read then.
  - **The disk's space,** which is node-wide, as today. A volume that fills it makes the store
    refuse other volumes' writes there (`OutOfSpace`), and their targets drop the replica, or
    fail the write. The node logs which pool ran out.
- **Panics end the node:**
  - `panic = "abort"` in a new `[profile.release]`; `dataplane/Cargo.toml` has none today;
  - an aborting panic hook in every build, installed in `main` before `build` spawns a thread.

  A poisoned engine would otherwise leave devices up and serving nothing.

**S6. One store, shared.**
- **The group commit** may hold several volumes' writes, as it holds other nodes' targets' today.
  A write is acknowledged once durable on the whole ISR, and batches stay in order.
- **#10's gather and holding** are node-wide. #10's S1.4 still lets a small write queued behind a
  held group start it.
- **There is no fairness between volumes.**
- **A store error** fails every volume's writes on the node.

**S7. The control socket.**

```text
attach VOLUME TOKEN-BASE64 [MS]  ->  ok /dev/ublkbN                        (or: error ...)
detach VOLUME                    ->  ok
delete VOLUME                    ->  ok N replicas deleted
status                           ->  ok none | ok VOLUME /dev/ublkbN
status VOLUME                    ->  ok none | ok VOLUME STATE [/dev/ublkbN MAJ:MIN INSTANCE GEN]
list                             ->  ok limit N [VOLUME STATE ...]
```

- **`attach`'s answer is unchanged.** An older CSI node plugin takes all of it after `ok` as the
  path (`dataplane.go`, `command`).
- **`status` keeps today's format.** It gives the first running volume in name order. So an older
  plugin sees one volume, and refuses any other as today.
- **`status VOLUME` and `list` are new.** The device's fields come once it exists.
- **Attaches are taken one at a time,** under #6's deadlines. Detach, delete, status and list
  never wait for an attach.
- **#6's limits stay:** 50 s for an attach or a delete, 30 s for a detach.

**S8. The CSI node plugin** (`control/csi`).
- **`NodeGetInfo`:** `MaxVolumesPerNode` comes from `list`.
  - On an older data plane, which answers `list` with its usage line, it is 1.
  - With no answer, `NodeGetInfo` fails, and kubelet retries.
  - Kubelet reads it when the plugin registers: the plugin is restarted after the data plane's
    limit changes.
- **Staging:**
  - `device` asks `status VOLUME`, and attaches if there is no entry;
  - starting gives `Unavailable`, and stopping `FailedPrecondition`;
  - another attached volume is no longer a refusal.
- **Which device** (`NodeStageVolume`, and a raw block `NodePublishVolume`: `node.go`, 91–125 and
  180–197):
  1. **`status VOLUME`.**
  2. **Open the path once,** and `fstat` the fd: a block device, of that `major:minor`.
  3. **`status VOLUME` again:** the same instance, generation and `major:minor`.
  4. **`blkid` and `mkfs`** read and write the device through that fd, passed as `/dev/fd/3`
     (`ExtraFiles`).
  5. **The mount** is by path. Then the new mount's `major:minor`, field 3 of
     `/proc/self/mountinfo`, must equal the `fstat`'s, or the plugin unmounts.
  6. **A raw block bind** checks the bound file's `major:minor` the same way.

  On any mismatch, the plugin refuses with `FailedPrecondition`, and leaves nothing mounted.
- **Refusals:** the `limit:` prefix, or an older data plane's "one volume at a time", becomes
  `ResourceExhausted`. Other failures stay `Internal`. `Detach` still treats "is not attached
  here" as done.
- **Compatibility:**
  - **A newer plugin on an older data plane:** `status VOLUME` gets the usage line. The plugin
    falls back to `status`, refuses while another volume is attached, and skips the identity
    check, which it logs. This is how it falls back for #6's time limit.
  - **An older plugin on a newer data plane** keeps `MaxVolumesPerNode: 1`.

**S9. A crash or a restart.**
- **Every device ends with the process,** and every volume's I/O fails, as today. The next start
  deletes the recorded leftovers (S2).
- **Each volume is attached again** under a new epoch, or fails over.
- **`--attach` at startup** still takes one volume.
  - It is tried again while that volume is not attached. Today: while no volume is.
  - It stops at S12's epoch check, which no retry with the same token file can pass.
- **The store's recovery is unchanged.**

**S10. A target does not need a device.** This follows from S3: a target's turns run on the
engine thread. The engine's tests already run targets with no device. An attach without a
device is #17's.

**S11. One volume's performance, against main** (bounds to be approved).

The method:
- **A new target:** `make bench-ab A=<rev> B=<rev> REPS=3 JOBS=…`, on fl-b, one volume, default
  settings. A is main, with #10.
- **The builds:**
  - **Installing:** `deploy` builds A and B into separate paths, and `node.sh` takes the binary's
    path from a variable.
  - **The RF 2 peers** are switched with the target.
  - **Volumes:** reads run on `vol1`. Writes run on `wbench`, reset before each run (performance
    plan, W1–W2).
- **One repetition:** a job runs raw, then A, then B.
- **Spread:** per side, (max − min) / median.
- **What is compared:** B's median share of raw against A's.
- **When the result is unclear:** a job whose 2 × spread exceeds its bound runs once more. If it
  still does, it is reported as inconclusive, for the owner to decide.

The jobs:
- **Reads,** zero copy, 1 queue: 4K QD1, 4K QD32 and 1M QD8, each at most 3% below A. About 15
  minutes of jobs, under an hour with cache drops and restarts.
- **Writes:**
  - **4K QD1,** at RF 1 and at RF 2: each at most 5% below A;
  - **1M QD8,** RF 1: at most 5% below A, and #10's rule (steady at 80% or more of raw across the
    disk, that day). The gather and arrivals deadlines move to the engine thread.
  - **RF 2** runs as in the 2026-10-17 campaign: the target on fl-b, every chunk with a replica on
    fl-b and one on fl-a or fl-c. Only fl-b's disk is reset.
  - **Time:** 3 jobs × 3 repetitions × 3 sides is 27 runs, of about 25 minutes each, or about
    11 hours.
  - **Wear:** about 0.6 TB per 1M run, and at least the whole-disk write and the preconditioning
    (about 0.13 TiB) per 4K run. About 8 TB in all.
- **Integrity:** a verify-mode read run with 0 checksum failures. Every run's end-to-end check
  passes.

**S12. Attach tokens are checked** (a gap that predates this issue).
- **The key:** the engine is given the node's token key, which `build` loads (node.rs).
- **At the socket, before a slot is taken:** the signature, the volume, and the node
  (`cfg.node`).
- **After `GetVolume`:** the epoch is the volume's epoch.
- **The local session** takes the verified claims, not made-up ones (engine.rs 1654–1666).
- **No epoch runs twice.** The engine refuses an attach at an epoch it has already run a target
  at in this run.
  - Across a restart, the replicas' fences do not stop it: they refuse only lower epochs
    (`flaminio-repl/src/replica.rs`, `m.epoch < v.fenced`).
  - So root can still start a target at the epoch of one that died with the node, through the
    socket. The CSI node plugin always takes a new epoch.
  - This is an accepted risk, left to #12.
- **A failure** gives `AttachFailed`, which names the check, and starts nothing.

**S13. Unchanged:**
- fencing, epochs and leases;
- the fast-read rules and the replication protocol;
- the metadata service;
- the store's format and crash safety;
- #6's limits;
- the node's defaults.

Any new `unsafe` goes in `flaminio-uring`. The workspace forbids it elsewhere.

## Tests

- **Rust, socket and registry** (`flaminio-io`):
  - **`status` and `list`:** `status` with none, one, and three running and one starting: `ok
    none`, today's line, then the first running volume. `status VOLUME` and `detach` in each
    state, and `list`;
  - **names:** a `pvc-<uuid>` name is taken; a name with whitespace, or past 128 bytes, is
    refused;
  - **the limit:** N + 1 concurrent attaches give N entries, and the last is refused with
    `limit:` and sends no request. A device that starts after its attach timed out counts;
  - **the same volume attached twice at once:** one goes on, the other is refused. So is an
    attach of a stopping volume;
  - **tokens:** another volume's or node's token, or a bad signature, is refused before any slot
    is taken;
  - **no waiting:** with an attach that never answers, `status`, `list` and another volume's
    detach answer within a second;
  - **ids:** above the highest present, and at or above `--id`. An id the kernel refuses is
    skipped. None is given twice;
  - **a stopping entry keeps its minor:** its fd keeps its `major:minor` taken while another
    device starts;
  - **routing:** two devices whose requests use the same sequence of tags, with ops in flight on
    both. Then one is detached and attached again, with ops in flight. Every completion reaches
    its own generation and tag; stale ones are dropped and logged;
  - **panics:** the hook, installed in a child process that panics a thread, ends it by abort.
- **Rust, the engine thread:**
  - a gather deadline, and the arrivals limit, armed from a queue thread, fire on time, not at
    the next turn;
  - commits are asynchronous with one device, and synchronous after the last stops;
  - two devices' arrivals are summed, and a stopped device's removed;
  - **with 10 000 inputs queued** and two threads looping on the lock, the engine thread's lock
    wait, and its heartbeats' lateness, stay under 100 ms. A queue thread waits at most one
    batch of 64.
- **Engine** (`flaminio-node/tests/engine.rs`, two targets on one engine):
  - writes and direct reads on both. Their writes share a commit, acknowledged once durable;
  - a detach of one, with I/O on both: only its ops fail;
  - **a stale `Deposed`,** of a generation before a detach and a re-attach, is dropped;
  - I/O of an older generation is dropped;
  - a `Detach` with no target gives `Detached`;
  - #6's failed and timed-out attaches, while another volume's I/O completes;
  - **tokens:**
    - another volume's, another node's, an older or newer epoch than `GetVolume`'s, a bad
      signature, an empty token, a malformed token: each fails and starts nothing;
    - an attach at an epoch that already ran here fails;
  - **deletes:**
    - a delete while the volume's attach is pending is refused;
    - a delete of many chunks, while two volumes are attached: no write of theirs waits more than
      one turn and a commit;
    - a `Hello` or an `Attach` of the deleted volume, between turns, is refused.
- **Go** (`control/csi`):
  - name validation;
  - `status VOLUME` and `list` parsed, with their fallbacks;
  - `MaxVolumesPerNode` from `list`, 1 on an older data plane, an error with no answer;
  - `limit:` and "one volume at a time" give `ResourceExhausted`;
  - staging: starting gives `Unavailable`, stopping `FailedPrecondition`;
  - **an older plugin's parse** of the new `status`, with two volumes attached, gives the first
    volume and its path;
  - **identity, at the `HostMounter` level** (Linux, root, loop devices; skipped elsewhere): a
    different `major:minor`, instance or generation, at the open, at the second `status
    VOLUME`, or in the new mount's `mountinfo`, is refused with nothing left mounted. `mkfs`
    writes through `/dev/fd/3`.
- **Node, in a lab VM** (a check script, Linux 7.0, ublk):
  - two volumes on distinct ids, with fio on both, each with its own `randseed` and
    `verify_pattern`: 0 errors;
  - **a detach of one** while fio runs on the other;
  - **a deposition,** with writes in flight on both and a filesystem mounted on the deposed
    volume. Only its device stops, and it stays stopping, with its `major:minor`, until its
    detach. The other's fio has no error, and no lock is held while the device is deleted;
  - **a restart with both attached:**
    - the recorded leftovers are deleted asynchronously at the next start;
    - a live device of another ublk server is untouched;
    - the startup `--attach` is retried while another volume is attached through the socket,
      and stops at the epoch check once the volume has a newer epoch;
  - **a raw block publish across a restart:** a raw block pod's volume, then the data plane is
    killed and another volume attached. The stale bind's `rdev` is not the new device's.
- **Lab, later steps:**
  - **Two volumes under fio** on fl-a, verified writes and reads on both for 10 minutes:
    - 0 errors;
    - no second without completions on either volume;
    - each volume's read p99 at most twice its p99 alone, without polling and with
      `--poll-us 100` (to be approved);
    - S3's lock-wait and heartbeat maxima, from the log.
  - **`make node-fault` for each of two volumes** (`VOLS`), each device's path from its attach:
    - both loads end without a failed write;
    - both ISRs are full again;
    - both fail over to fl-b with every byte;
    - each volume's longest read at fl-b's restart is recorded.
  - **Isolation:**
    - while fio runs on volume 1, volume 2 is attached on fl-b through `metactl` and `node.sh`,
      not `cmd_vol attach`, which first detaches it elsewhere (`lab.sh`, 700–704);
    - that deposes it on fl-a. Volume 1's fio has no error;
    - the check then detaches volume 2's stopping entry on fl-a itself.
  - **One volume:** `make bench-ab` (S11), and `scripts/checks/reconcile.sh` with zero copy.
