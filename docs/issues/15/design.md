# Issue #15: more than one volume per node — design

Spec rules S1–S13 ([spec](spec.md)); settled choices in [decisions](decisions.md). Paths are under
`dataplane/crates/` unless they say otherwise.

## The shape

**Today:**
- The main thread drives the engine (`flaminio-io/src/ublk.rs`, `serve_node`, `drive_until`).
- An attach moves the engine into one device's `Shared`, and blocks in `run_target`
  (`attach_and_serve`, `run_device`).

**After:**
- **`Node`** (new, `ublk.rs`) holds the engine and the I/O routes together, behind the engine's
  one `Mutex` (`Core { engine, routes }`). With it:
  - the registry;
  - the instance;
  - the generation and op counters.
- **Commit settings** (`--gather-us`, `--commit-target-mib`) are applied once, to the engine,
  when `Node` is built. Today `run_device` applies them at each attach (`ublk.rs:952–957`).
  Asynchronous commits follow the running count (S3).
- **The engine thread** (new) alone owns the input receiver. Queue threads' `pump_with` no longer
  drains inputs: it only flushes, and hands the outputs to the router.
- **The main thread** takes attaches in turn (S1, below). Each attach that succeeds gets a device
  thread.
- **`Shared`** keeps only a device's own state: its queues' wakes, its staging, its generation.
- **Lock order:** the registry, then the engine. No thread takes the registry while it holds the
  engine. The arrivals counters live in the engine (`Engine::add_arrivals`, `remove_arrivals`),
  so `Engine::arrivals()` reads them under its own lock.

**The router** (`Node::route`), called by whichever thread took the outputs:
- `Done`, `LocalRead` and `Piece` go by op id to the waiting tag, through the routes: op →
  (generation, queue, channel), as `waiters` today. Each thread routes these before it lets the
  engine's lock go (`try_send` does not block), so a request's outputs reach it in the order the
  engine produced them (S2; R1-C2). Routed after the unlock, a read split into a remote and a
  local piece could see its `Done`, routed by the queue thread that ended the local read, before
  the engine thread routed its `Piece`; the tag ends the read on `Done` (`ublk.rs:1341–1353`).
  The routes add no lock to the I/O path: submit and pump already take the engine's (R1-C10);
- `Attached`, `AttachFailed`, `MarkEpoch` (S11) and `Detached` go by `(volume, generation)` to
  the registry entry's waiter, after the engine's lock is let go (lock order, above);
- `Deposed` goes to the entry's stop channel (S2);
- anything whose op or generation is unknown is dropped and logged.
- When `run_target` returns, the router purges every op of that generation. The engine's own
  per-volume state goes with the target (`Engine::detach`): its pending `local_reads` are
  dropped, as today.

## S1: limits, names, attaches

- **The limit:** `--max-volumes N` (`node.rs`, `NodeConfig`), default 8. The usage text gives a
  device's cost: depth × `--io-buf` per queue, 64 MiB at the defaults, virtual until touched.
  - **Bounds:** 1 to 16, checked at parse (R1-S5). `list`'s longest answer, 16 entries of a
    128-byte name and an 8-byte state, is 2 220 bytes with its newline, within #74's 4096-byte
    answer cap (S6), which `NodeGetInfo` reads through.
  - **Under `--privsep`** (#74): the default is 1, and a larger value is refused at parse
    (`--max-volumes above 1 needs direct mode`) (R1-S1; With #74's helper, below).
  - **Before Linux 6.9, if the 6.8 gate fails** (R5-3; The lab's mechanics, The 6.8 gate): the
    default is 1, and a larger value is refused at start (`--max-volumes above 1 needs Linux 6.9
    or later here: a synchronous ublk delete blocks every add while it waits`). The release is
    read as for `DEL_DEV_ASYNC` (S10); one that does not parse counts as before 6.9. Direct mode
    on 6.9 and later is unchanged. Built, with its tests, only on that outcome (R6-2).
- **The registry:** a `Mutex<Registry>`, holding entries by volume. `reserve(volume)` checks the
  limit and the volume, and inserts a starting entry, under that lock.
  - A late-started device (#6, S2) keeps its entry until its `run_target` returns, so it counts.
  - The count of running devices is kept under the same lock, and decides the asynchronous
    commits (S3).
- **Names:** `ValidID` (`meta/domain/domain.go`), checked in `control.rs` for every command, and
  in `control/csi/dataplane.go` before a line is built. `cli.rs`'s 16-byte rule stays `serve`'s.
- **Tokens before the slot:** the socket checks the token's signature, `claims.volume`,
  `claims.node`, and `claims.epoch` against the volume's run mark (S11), then reserves. The
  `TokenKey` comes from `build` (`node.rs`, `token.pub`).
  - The startup `--attach` goes through the same check and reservation.
  - Tokens carry `issued_at` and no expiry, and none is checked here, as for sessions today.
- **Errors:** every error answer to a command that names a volume starts `error: VOLUME`, the
  socket's own (no answer in time, shutting down) included:
  - `error: VOLUME: limit: N volumes attached or attaching` (R1-C9, R1-S11). The CSI node plugin
    matches the exact prefix `error: <requested volume>: limit: `, as it does `epoch: ` (S11), so
    an answer about a volume named `limit` is never taken for the limit;
  - `error: VOLUME is starting|running|stopping here`.
- **One attach at a time, on the main thread.** This is today's `attach_and_serve` metadata
  phase, without the engine in hand.
  1. It sends `Input::Attach { volume, token, generation }`, and waits on the entry's waiter.
     The wait ends at `METADATA_LIMIT`, or at `steps_by` if earlier.
  2. **`MarkEpoch`:** it makes the volume's run mark durable, and sends `Input::Marked { volume,
     generation }` (S11).
  3. **`Attached`:** it spawns the device thread, which runs `add_device` and `run_target`.
     The main thread takes the next attach.
  4. **`AttachFailed`, a failed mark, or the wait timing out:**
     - it sends `Input::Detach { volume, generation }`, as #6's S3 does today;
     - it ends the entry, and answers the attach with the reason.
- **An attach never begun** (R1-C4): the reservation is a guard that the queued request owns.
  - The main thread skips a request whose `steps_by` has passed (`ublk.rs:775–781`). It answers
    `error: VOLUME: its caller's time ran out before it could begin`, and drops the request.
  - Dropping a request that was never begun, there or anywhere else (a main thread that ends),
    ends its starting entry and frees its slot.
  - Once begun, the steps above end it.
- **Detach, delete, status and list** are answered on the socket's command threads. They never
  queue behind an attach.

## S2: identity and the device thread

- **Ids:**
  - **`first_id`** is computed at start, before the cleanup (S10). It is the highest of:
    - every id under `/sys/class/ublk-char/ublkc*`;
    - every recorded id;
    - `--id`, less one;

    plus one, and at least 0. `--id -1`, the default, sets no floor.
  - **Later ids** only increase. A record that already exists (`O_EXCL`, S10) moves to the next id.
    A failed `ADD_DEV` at an id moves to the next id when the id is held: `GET_DEV_INFO` at it
    answers (R3-B1). That covers a live device, whose `ublkc<N>` exists, and a deleted one that is
    still open: `DEL_DEV` removes `ublkc<N>`, but the id is freed only at the last close (#74, Owned
    devices), and `GET_DEV_INFO` answers until then, as the cleanup already assumes. The next id is
    chosen straight after the failed add, before the deadline check and the wait (the loop's order,
    below; R7-L1). In the 6.1 source read here (`eve-kernel`, 6.1.112), `GET_DEV_INFO` finds the
    device by `idr_find` alone (`ublk_drv.c:1518–1531`), and the id leaves the idr only at the
    release (`:1443–1451`), when the disk's last close drops the reference `START_DEV` took
    (`:360–366`, `:1576`). A failed add at an id not held tries that same id again until the
    attach's deadline (R4-B2): the walk skips an unheld id only when the attach fails, as the
    counter moves past the last id tried. The errno is not used: libublk retries a failed command
    with its legacy opcode, which kernels without legacy opcodes (Ubuntu's) refuse, so an `EEXIST`
    comes back as `EOPNOTSUPP` (`ctrl.rs:1500–1520`; #74, Owned devices, Errors). So a failure
    unrelated to the id (`ublk_drv` not loaded, `ENOMEM`, `ublks_max` reached) burns at most one id
    per failed attach.
  - **Pace and bound** (R2-B6, R5-1): every `ADD_DEV` is paced, one per 250 ms, held id or not,
    as today (`ublk.rs:896–899`) and as #74's helper paces adds, until `add_device`'s deadline
    (`ADD_DEVICE_LIMIT`, 10 s). After each failed add, in this order (R6-1, R7-L1):
    1. the next id is chosen: the next one if the id is held, the same one if not;
    2. if it is above the ceiling (below), the attach is refused at once, with the ceiling's
       text, with no wait;
    3. if the deadline has passed, the attach fails with the last add's error;
    4. otherwise the 250 ms wait, then the add at that id.

    Steps 3 and 4 are today's loop (`ublk.rs:891–899`), so one attach tries at most 41 adds (40
    waits, the last add at about 10 s), and skips at most 41 ids, all held but the last. Step 2
    before step 3 means the ceiling's text wins when the held add at 1048575 is also the one
    that ends the deadline (R7-L2). Pacing the held skips too is the simpler of the two ways to
    make that bound true: one rule for every add, and one bound that the test can check; a held
    id costs 250 ms, and held ids above `first_id` are rare (only devices added or deleted since
    the start). The counter has moved past every id tried,
    so the next attach starts above the last id this one tried (R3-B1).
  - **The ceiling** (R4-B2, R5-2): no id above 1048575, the kernel's last (`UBLK_MAX_UBLKS`,
    2^20; #74's ID bound) and the records' grammar's, is tried. An attach whose next id would be
    above it is refused at once, without the pace's wait: step 2 above comes before the deadline
    check and the wait (R6-1, R7-L1):
    `error: VOLUME: limit: no ublk id left in this run; restart the node`,
    and so is every later one: ids are never reused in a run (S2), and a restart computes
    `first_id` afresh. That holds whether the walk gets there by a held skip at 1048575, by a
    `first_id` above 1048575 at start (the node then starts, and refuses every attach), or after
    a successful add at 1048575 (that attach succeeds; the next is refused). The refusal carries
    the limit's prefix, so the plugin maps it to `ResourceExhausted`, as it does the volume
    limit; it is never the last add's error, even when the deadline has also passed (R7-L2).
  - `add_device`'s retry of its own id goes, and `--id`'s usage text says how ids are chosen.
  - **A failed `ADD_DEV` deletes nothing** (R1-C1). In libublk 0.4.8 a failed add drops its
    handle, whose `Drop` sends `DEL_DEV` for the id it asked for (`ctrl.rs:1258`, `:1164–1167`,
    `:1085–1096`, `:1626–1635`). That deletes the device holding the id, and before 6.9 blocks
    the device thread if that device is open. #74's vendored libublk patch marks the handle
    deleted when the add fails (#74 design, Contracts, libublk patch). Whichever of #15 and #74
    lands first vendors libublk 0.4.8 (`dataplane/vendor/libublk`, `[patch.crates-io]`) with
    that hunk and #15's own (R1-C5, below); the other adds its own hunks.
- **Generations and the instance:** from `Node`'s counters. The instance is a `u64` from
  `ring::rand::SystemRandom`, and `ring` is added to `flaminio-io`'s dependencies.
- **The device thread's control loop is the ready callback.** `run_target` calls it on the
  caller's thread, after starting the device and before joining the queue threads (libublk,
  `run_target`). So the callback:
  1. waits for `/dev/ublkbN` (udev), within `START_LIMIT`;
  2. opens it `O_RDONLY|O_NOFOLLOW|O_CLOEXEC`, without `O_EXCL`. `fstat` must give a block
     device whose `major:minor` is `/sys/class/block/ublkbN/dev`'s, else the fd is closed and
     the attach fails as those below do (R1-S3). It reads the diskseq (`BLKGETDISKSEQ`). The
     entry keeps the fd, its `major:minor` and diskseq. The step takes the path and the sysfs
     root as parameters, for its test (R2-B4);
  3. turns the entry running, and answers the attach;
  4. blocks on the entry's stop channel, which three things feed. Its receiver is in a `Mutex`,
     as the callback must be `Send + Sync + 'static`, like `ready_reply` today:
     - a `detach`;
     - the routed `Deposed`;
     - any queue closure returning;
  5. then sends `Input::Detach { volume, generation }` and signals the node's eventfd, so that
     the target ends first and fails its pending ops (`NotAttached`), as today's detach does;
  6. then runs `kill_dev`, and returns, so that `run_target` joins the queues and stops the
     device. With the target gone, nothing in flight waits for it.
- **Failures:** in each case the device thread answers the attach, if it is still waiting,
  with the reason. It sends `Input::Detach { volume, generation }`, removes the record, and
  ends the entry.
  - **`add_device` fails:** there is no device.
  - **A queue that fails before its loop.** `start_dev` waits for every queue, so it must be
    told:
    - **Setup moves out:** per-queue setup that can fail and need not be on the queue's thread,
      such as the staging, is done before `run_target`. What stays is the task ring and
      `UblkQueue::new`.
    - **A failure there** calls `dev.notify_buffer_registration_complete(true)`. `start_dev`
      then returns `EPERM` before it sends `START_DEV`, and `run_target` returns `Err`.
    - **Zero copy too** (R1-C5). libublk 0.4.8 skips that wait under `UBLK_F_AUTO_BUF_REG`
      (`io.rs:1108–1115`), which zero copy sets, and the kernel would then wait in `START_DEV`
      for every tag's FETCH. The vendored libublk's second hunk waits under
      `UBLK_F_AUTO_BUF_REG` too, and a zero-copy queue calls
      `notify_buffer_registration_complete(false)` after its last fallible step. So a queue
      that fails before its loop ends its own attach in both modes: no watchdog, no abort (S5).
      The hunk is offered upstream with #74's.
  - **`run_target` returns `Err` before the callback.** Some queue threads may be in their loop,
    and are not joined.
    - The device thread sets a stop flag in `Shared`, signals each queue's own eventfd, and stops
      the device (`kill_dev`). `STOP_DEV` ends every pending FETCH, even on a device that never
      started (#74's reading of `ublk_drv.c:1663–1680`, `:1497–1512`).
    - `event_loop` breaks as soon as the stop flag is set, whatever `aborted` says (R1-C7). Today
      it breaks only on `(aborted || failed) && is_done()` (`ublk.rs:1670`), and a device that
      never started gets no `UBLK_IO_RES_ABORT` of its own.
    - Each queue closure counts its exit with a drop guard. The count means it left its loop: its
      ring and the device's character device go when the thread exits.
    - The thread waits until the count reaches the returned `dev_info.nr_hw_queues` × the I/O
      threads per queue, then deletes the device.
  - **Step 1's `START_LIMIT` runs out:** the callback stops the device itself, and returns.
    No fd was opened.
- **A late-started device** is one whose attach the socket had already answered as timed out
  (#6, S2) when it reached running. It stays running, counts against the limit, and the caller's
  retry finds it through `status VOLUME`.
- **After `run_target` returns,** in this order:
  1. the router purges the generation, and the arrivals counter is removed;
  2. the device is deleted (S10):
     - **on 6.9 and later,** with `DEL_DEV_ASYNC`. Once the command returns, the kernel holds
       it, and ends the device at its last close, whether or not the process lives;
     - **before 6.9,** with `DEL_DEV` on a thread of its own, which can block until the last
       close. Step 4 does not wait for that thread;
  3. the record goes when the delete returns `Ok` or `ENODEV`: at once with `DEL_DEV_ASYNC`, and
     when the thread's `DEL_DEV` returns before 6.9. Any other error keeps the record, logs it,
     and leaves the device to the startup cleanup, as does an exit before the delete returns;
  4. last, under the entry's lock, which also covers its state and its detach flag:
     - flag set: the thread closes the fd and ends the entry;
     - flag clear (a deposition, or a device that stopped on its own): it marks the entry
       stopped, and keeps the fd.
- **A detach, under the same lock:**
  - on a stopped entry, it closes the fd and ends the entry itself;
  - otherwise it sets the flag.

  Neither side can miss the other.
- **Node shutdown** ends with `process::exit`, as returning from `main` would wait for a device
  thread blocked in `kill_dev`.
  - The kernel stops such a device once the process has finished exiting, which that thread can
    delay.
  - The next start's cleanup works from the record, not from the device being gone (S10).
  - `process::exit` skips `Drop` and buffered output. This loses nothing: commits are durable
    before they are acknowledged, and the logs are flushed before the exit.
- **Why the plugin's tools may take the path** (S9): an open fd on a ublk disk keeps the disk,
  its minor, and the device's ublk id, which is freed only when the device is released (#74's
  reading of `ublk_drv.c:2150–2164`). So while the plugin holds its checked fd, `/dev/ublkb<N>`
  names that device or nothing, across restarts of the data plane too. The S2 lab test checks
  it. #79 moves the tools to the fd itself, with its image test (R1-C3).
- **Generations, end to end:**
  - `Input::Attach`, `Input::Marked`, `Input::Detach` and `Input::Io` carry the generation, and
    `TargetRt` keeps it (`flaminio-node/src/engine.rs`);
  - the outputs echo it;
  - the fast-read path (`try_fast_read`, `plan_fast_read`, `finish_fast_read`) takes it, and
    refuses a mismatch;
  - `Detach` with no target now gives `Detached` (`Engine::detach`).
- **Stopping entries** are shown in `list`. One stopping for more than 5 minutes is logged once.

## S3: the engine thread

- **The wait:** `ppoll` on the node's eventfd. Its timeout is the earliest of every engine
  deadline:
  - the next turn;
  - the gather or hold deadline;
  - #10's arrivals limit (the window's end + `ARRIVALS_LIMIT`);
  - #75's QoS release: its gates' earliest `due`, which `Engine::take_qos_timer` arms and
    `pump_with` sends on the gather timer's channel in #75's design (Release), a channel that
    goes here (R1-C8; R2-N1).

  `Engine::next_wakeup` returns the earliest, at nanosecond precision. Whichever of #15 and #75
  lands second folds the QoS deadline in, with its test.
- **Arming:** a thread that arms any deadline (`take_gather_timer`, `take_qos_timer`) signals
  the eventfd. The per-queue gather timer tasks go.
- **Its loop:** under the lock:
  1. handle at most 64 inputs;
  2. `turn` if it is due, and `flush` otherwise;
  3. take the outputs, and route the I/O ones (`Done`, `LocalRead`, `Piece`; R1-C2);
  4. let go of the lock;
  5. route the rest, and signal the queues it delivered to.
- **What it logs,** every 10 s and in each detach's commit report:
  - its longest wait for the lock;
  - its heartbeats' longest lateness: sent, less due.
- **If the 100 ms engine test fails:** a hand-off flag, with a condvar (a follow-up).
- **Commits:**
  - `set_async_commits(!c.sync_commits)` when the running count goes from 0 to 1, as on #10's
    branch;
  - `set_async_commits(false)` when it goes back to 0.
- **Device control** runs only on the device thread (S2). Today it runs on a one-off thread for a
  deposition (`pump_with`), and on the socket's thread for a detach (`control.rs`).

## S4: queue threads

- **Unchanged:** `EngineCtx::submit` and `pump_if_due`, under the lock. `submit` adds its route
  under the same lock; `pump_with` routes the I/O outputs before it lets the lock go, as the
  engine thread does (S3).
- **Gone from queue 0:** the 10 ms ticker, and the wait on the node's eventfd (`serve_with`).
  Each queue keeps its own eventfd.
- **#10's arrivals:** each device registers its `Arc<AtomicUsize>` with
  `Engine::add_arrivals` when it starts, and `remove_arrivals` after `run_target`.
  `Engine::arrivals()` sums them.

## S5 and S6: isolation, settling, the shared store

- **Isolation:** no mechanism beyond S2. Each detach, deposition or stopped device acts on one
  generation.
- **Settling:** `settle_local` holds the lock until `committing` and `local` are empty. It is
  bounded by what was queued when it began: queue threads cannot add while the lock is held.
  - It runs on `NetEvent::Closed`, on `InspectLocal`, on each delete turn, and on #10's `Hello`.
- **Space:** `OutOfSpace` (`flaminio-store/src/store.rs`) closes the writing session
  (`replica/mod.rs`, `fill`). The engine logs the pool's name.
- **Panics:**
  - `[profile.release] panic = "abort"` in `dataplane/Cargo.toml`, which has none today;
  - a `std::panic::set_hook` that aborts, installed first thing in `main`.

## S7: deletes

- **Refused while the volume has any registry entry,** at the socket. The engine's own refusal
  while it has a target or a pending attach (`Engine::handle`, `DeleteReplicas`) is kept.
  - A starting entry has nothing in the engine yet, so the socket's check is what covers it.
- **`ReplicaService::delete_volume`** splits in two.
  - **`begin_delete`:**
    1. makes `Store::fence(uuid, u64::MAX)` durable: one `Fence` record;
    2. then lists the chunks;
    3. then forgets the volume in `vols` and the core.
  - **`delete_step(k)`:** deletes `k` chunks through a new `Store::delete_chunks(keys)`, as one
    `Plan` of `k` `ChunkDelete` records and one commit. Today `delete_chunk` runs one `Plan` per
    chunk.
- **Each engine turn** runs a step, after `settle_local`. `k` is sized so that 4 TiB (16 384
  chunks of 256 MiB) ends within 50 s; the S7 lab test measures it.
- **`pending_delete`:** while a volume is in it, the engine refuses its `Hello` and its `Attach`.
  It is cleared only when a delete finishes.
  - A store error stops the delete, and leaves the volume in the set. Errors that do not poison
    the store, such as `OutOfSpace` on the delete's commit, are among them.
  - After a restart, the fence at `u64::MAX` refuses the same.
- **`deleting`** is tracked apart from `pending_delete`: it is the volume whose steps are running.
  - A `delete` while `deleting` holds the volume answers `error: VOLUME: a delete is under way`.
  - A `delete` of a pending volume with no running step, as after a store error, restarts
    `begin_delete`.
  - The socket's 50 s ends the answer, not the delete.
- **Crash ordering:**
  - **The fence comes first.** Its record is durable before any chunk is forgotten or deleted.
    So a restart in the middle finds every remaining chunk fenced at `u64::MAX`.
  - **What that refuses:** every `Hello` and attach, at any epoch below `u64::MAX`. A volume
    made again at a newer epoch (#30) cannot adopt these chunks.
  - **A later `delete`** ends the work.
  - **Today:** `delete_chunk` passes `u64::MAX` only to its epoch check, and writes no `Fence`.
    `delete_volume` forgets the volume first.

## S8: the control socket

```text
attach VOLUME TOKEN-BASE64 [MS]  ->  ok /dev/ublkbN                        (unchanged)
detach VOLUME                    ->  ok
delete VOLUME                    ->  ok N replicas deleted
status                           ->  ok none | ok VOLUME /dev/ublkbN        (first running, by name)
status VOLUME                    ->  ok none | ok VOLUME STATE [/dev/ublkbN MAJ:MIN INSTANCE GEN]
list                             ->  ok limit N [VOLUME STATE ...]
```

Additive `device-info`/`detach-device` and their strict grammar, server locks and tests are in
[device-lifetime-contract.md](device-lifetime-contract.md); #79 depends on this unimplemented profile.

- **Why `attach` is unchanged:** an older plugin takes all of it after `ok` as the path
  (`dataplane.go`, `command`).
- **Why plain `status` names one volume:** an older plugin's `strings.Cut` would take a longer
  line's rest as the path.
- **Every answer fits #74's 4096-byte cap:** `list` holds at most 16 entries (S1), and
  `device-info`'s longest answer is under 400 bytes.
- **An attach refused for its epoch** answers `error: VOLUME: epoch: <reason>` (S11).
- **`detach`, by state:**
  - starting: `error: VOLUME is attaching`;
  - running or stopping: it takes the entry's generation, acts under the entry's lock (S2), and
    feeds the stop channel if the device is still running (the device thread ends the target
    first);
  - it waits up to 30 s, as today, for that generation's entry to end, then answers `ok`. It
    never waits on a newer entry of the volume. Past 30 s it answers `error: VOLUME: the device
    did not stop within 30 s`, and the device thread still ends the entry later;
  - no entry: `error: VOLUME is not attached here`, today's wording.

## S9: the CSI node plugin (`control/csi`)

**What #15 ships here, and what #79 ships** (R1-C3):
- **#15:** names; `NodeGetInfo`; `device()`'s states and errors, without the second-volume
  refusal; the device check below, with the contract's `device-info`; the check after mounting and
  of a mount already in place; the legacy path for an older data plane. Unstage keeps `detach
  VOLUME`.
- **#79:** durable ownership and its profiles; mount inspection by topology, which replaces
  #15's check of a mount in place; tools on the inherited fd, and their image test; the
  conditional-detach client in unstage; the end of the legacy path for lifecycle effects (its
  approved amendment, decisions.md). "#79 enabled" means the plugin build that includes #79:
  there is no flag or probe.
- **#74's `checkUblkDevice(path)` and 4096-byte answer cap** (#74 S6), as #74 defines them:
  whichever of #15 and #74 lands first adds them. #15 calls `checkUblkDevice` unchanged, in
  `device()` and on a raw block publish, where #74 places it, and adds its own comparison with
  the reply beside it (step 3 below; R2-B3).

- **`NodeGetInfo`:** from `list`. A usage line from the data plane gives 1. Kubelet reads it when
  the plugin registers, so the plugin is restarted after the limit changes.
- **`device`:** asks `device-info VOLUME`, and calls `Attach` when there is none. The plugin maps
  its attach's failures (`node.go`, `device`), not to `Internal`, so kubelet retries or
  unstages (the device check's own refusals are `Internal`, below):
  - an attach that times out: `DeadlineExceeded`;
  - starting: `Unavailable`;
  - stopping: `FailedPrecondition`;
  - an attach refused for its epoch (S11): `FailedPrecondition`. Kubelet's retry reuses the
    publish context, so only a new `ControllerPublishVolume` recovers.
- **The device check** ([contract](device-lifetime-contract.md)), before any probe, format, mount
  or bind:
  1. `device-info VOLUME` gives the expected lifetime: FirstEpoch, boot, instance, generation,
     path, `major:minor`, diskseq;
  2. `checkUblkDevice(path)`, #74's: the path matches `^/dev/ublkb(0|[1-9][0-9]{0,6})$`,
     `Lstat` finds a block device, and `/sys/class/block/ublkb<N>/dev` holds its rdev;
  3. #15's comparison with the reply (R1-S3, R2-B3): open the path with
     `O_RDONLY|O_NOFOLLOW|O_CLOEXEC`; the reply's `major:minor` must equal both
     `/sys/class/block/ublkb<N>/dev` and the fd's rdev (`fstat`, a block device), and
     `BLKGETDISKSEQ` the reply's diskseq. The comparison takes the device path and the sysfs
     root as parameters, for its test (R3-C2);
  4. `device-info VOLUME` again gives the same tuple.

  A failure of step 2 or 3 is `codes.Internal`, with #74's message form (`data plane answered
  %q: <reason>`): one code for a device that is not the ublk disk named, or does not match it.
  A second reply that differs at step 4 is a state change, not a device the kernel contradicts:
  it gets the state's code: stopping `FailedPrecondition`; none, starting or another lifetime
  `Unavailable`, so that kubelet starts again at step 1 (R3-N2, R4-N1).
  The fd stays open through `blkid`, `mkfs`, the mount or bind, and the check after it. The tools
  take the path, which the fd keeps naming that device or nothing (S2).
- **After mounting:** the staging path's `major:minor` in mountinfo is the fd's; otherwise
  `codes.Internal`, as above.
- **A target already in place:** checked against the fd's `major:minor`: a filesystem stage by
  mountinfo, a raw block target by `stat`. A mismatch refuses (`FailedPrecondition`: the target
  is another volume's), and unmounts nothing. #79 replaces this with its ownership records.
- **Raw block across restarts:**
  - kubelet's `MapBlockVolume` is expected to attach a loop device that holds the device open.
    This is from memory of `volumepathhandler.AttachFileDevice`, not checked here; the lab test
    decides.
  - If it does not, the plugin holds an fd per raw block publish. At its own start, it reopens
    them for the publishes whose instance and generation still match.
- **Errors:** `error: <requested volume>: limit: ` and "one volume at a time" give
  `ResourceExhausted`.
- **An older data plane** answers `device-info` with its usage line. Until #79, the plugin then
  takes today's path: plain `status` and `attach`, one volume, with `checkUblkDevice` on the
  path. So the two can be upgraded in either order (intent 4). #79's plugin refuses lifecycle
  effects there instead (fail closed); `NodeGetInfo`'s fallback stays.
- **Locking:** the plugin's one node lock stays.

## S10: crash and restart

- **The record:** `/run/flaminio/ublk/<id>`. Root acts on records at start, so their trust is
  fixed (R1-S6):
  - **The directory** is created root-owned, mode 0700. At start `lstat` must find a directory,
    not a link, owned by root with that mode, or the node refuses to start.
  - **One owner at a time** (R2-B6), in direct mode only, as are the records (R3-N1; under
    `--privsep` the I/O process cannot open this directory): the node takes an exclusive `flock`
    on the directory, non-blocking, at start, and holds it for its life; held, the node refuses
    to start.
    `flaminio-io cleanup` takes it the same way, and refuses while it is held (`the node is
    running`). Without it, a cleanup beside a live node could delete a device mid-attach: ublk
    reports `UBLK_S_DEV_DEAD` between `ADD_DEV` and `START_DEV`, and the record already holds
    the returned `dev_info`.
  - **Names** are `^(0|[1-9][0-9]{0,6})$`, #74's grammar for the same ids, and at most 1048575;
    any other name, `01` among them, is logged and left (R2-N3).
  - **Opens** use `O_NOFOLLOW`; writes of a record and of `.<id>.tmp` add `O_CREAT|O_EXCL`, so
    a link put in their place is never followed. A leftover `.tmp` is unlinked first.
  - **A record** is at most 4096 bytes, of a fixed grammar (the fields below, decimal); a longer
    or malformed one is logged and left, and its id still counts for `first_id`.
  - **Before `ADD_DEV`:** it is written with `O_EXCL`, holding the volume, the instance and the
    generation. If it already exists, that id is skipped.
  - **After `ADD_DEV`:** it is replaced atomically, by `.<id>.tmp` in the same directory and a
    rename, with the
    `dev_info` that `ADD_DEV` returned. The kernel rewrites the flags, caps `nr_hw_queues` and
    `queue_depth`, and sets the owner, so only the returned `dev_info` can be compared later.
  - It is removed on any `ADD_DEV` failure, and after the device's delete. The `first_id` scan
    ignores `.tmp` files, and the cleanup removes them.
  - The CSI pod mounts `/run/flaminio` (#12's plan).
  - libublk's `/run/ublksrvd/<id>.json` goes with `run_target`'s own `stop_dev`. Only a crash
    leaves it, and the cleanup removes it.
- **At start:** for each record that parses, the node checks with `GET_DEV_INFO`:
  - the state is `UBLK_S_DEV_DEAD`;
  - there is no `UBLK_F_USER_RECOVERY`;
  - these recorded `dev_info` fields match: id, `nr_hw_queues`, `queue_depth`,
    `max_io_buf_bytes`, flags and owner. The flags alone do not identify Flaminio, as they are 0
    without zero copy. The state and `ublksrv_pid` are not compared: the kernel rewrites them at
    `START_DEV`, and nothing is decided by pid.

  - A record whose `GET_DEV_INFO` gives `ENODEV` is removed.
  - A record without `dev_info`, from a crash between `ADD_DEV` and the rename, is logged and
    left alone.

  Then it deletes the device as a detach does (step 2 above: `DEL_DEV_ASYNC` on 6.9 and later,
  `DEL_DEV` on a thread before), and removes the record on the same `Ok` or `ENODEV` rule.
  It deletes only a device that `GET_DEV_INFO` reports dead. The node starts without waiting
  for a `DEL_DEV` thread; a thread still blocked at exit leaves its record for the next start.
  `flaminio-io cleanup` waits up to 5 s in all for its `DEL_DEV` threads before 6.9, then logs
  each device still held open and exits, keeping its record. `DEL_DEV` removes the device before
  it waits for the last close (#74's reading of `ublk_drv.c:2543–2546`); once that close comes,
  the next start or cleanup finds `ENODEV` and removes the record (R2-B6).
  - **A recorded device that is not yet dead** (`GET_DEV_INFO`'s state is not `UBLK_S_DEV_DEAD`),
    as after an exit with a device thread blocked in `kill_dev`, is checked again for up to 5 s.
    - Then it is logged as an error, and its record is kept for the next start.
    - The node starts anyway: its ids begin above the recorded one.
  - Anything else is logged and left alone.
- **`DEL_DEV_ASYNC`** is in Linux 6.9 and later. Centaur's 6.8 headers lack it, and the lab runs
  7.0.
  - libublk's control handle deletes when dropped (`UblkCtrlInner::drop`). The node parses the
    major and minor of the kernel's release from `uname` at start. A parse failure takes the
    disowned path. There is no feature bit for the command, and a backport only takes the safe
    path.
    - On 6.9 and later the handle is built with `UBLK_DEV_F_DEL_DEV_ASYNC`.
    - On an older kernel, which would only trace the failed command, the handle is disowned, and
      the node deletes the device itself, with `DEL_DEV` on a thread of its own.
    - Centaur's 6.8 runs the disowned path in a test.
  - `flaminio-io del` takes the same rule: `DEL_DEV_ASYNC` on 6.9 and later, `DEL_DEV` before or
    when the release does not parse (R1-C11). Always asynchronous, it would fail on the
    supported 6.6–6.8 (ADR 0003), Ubuntu 24.04's 6.8 among them.
- **Scripts:**
  - `node.sh down` keeps today's mount guard (`node.sh:22–25`: deleting a mounted device would
    wait for ever), for `$dev` and for `/dev/ublkb<id>` of every record whose name passes the
    grammar above; a mounted one stops `down` with today's message (R2-B6).
  - `node.sh down` then stops the unit, and deletes by mode (R4-B1). It stops before it deletes
    in both modes, as #74's `down` does (R3-C1):
    - **direct mode:** `flaminio-io cleanup`, the startup cleanup alone, which deletes only
      recorded, dead and matching devices (R1-S6). This replaces #74's direct-mode `del` of
      `/dev/ublkc$id`, since direct mode no longer has one fixed id;
    - **`PRIVSEP=1`:** #74's `del` of `/dev/ublkc<ID>`, the helper's one id, as #74 states (its
      outcome 5). There are no records to clean up in that mode.

    It has no branch for a build without records: `bench-ab` runs each revision's own
    `scripts/guest/node.sh` from its archive, and main's keeps its own `del --id`;
  - every script takes the path from the attach or `status VOLUME`.

## S11: tokens and epochs

- **What main has** (#30, R1-S9): `start_target` decodes the token with `decode_unverified`, and
  checks its volume, node and epoch against `GetVolume` (`engine.rs:1610–1621`, "Its signature
  is not checked here (#15)"). `flaminio-node` enables `flaminio-net`'s `unverified` feature for
  it (`Cargo.toml:16`).
- **The engine gets the `TokenKey`.** `Input::Attach` verifies the token, signature included,
  and keeps its claims; the target's `first_epoch` is the verified `claims.first_epoch`. After
  `GetVolume` (`start_target`), `claims.epoch` must equal `volume.epoch`, as on main.
  `decode_unverified`, its test and the `unverified` feature go: nothing else uses them.
- **The run mark** (R1-S2): `--state-dir DIR` (default `/var/lib/flaminio`); `DIR/epochs/<volume>`
  holds one decimal epoch.
  - **Loaded** at start into the registry. The socket refuses a token at or below its volume's
    mark before it reserves (S1).
  - **Written before any `Hello` at that epoch.** Once `GetVolume`'s checks pass, the engine
    holds the target unstarted and emits `Output::MarkEpoch { volume, generation, epoch }`. The
    main thread checks the mark again, unlinks a leftover `.<volume>.tmp`, writes it anew,
    `fsync`s it, renames it over the mark, `fsync`s the directory, updates the registry, and
    sends `Input::Marked { volume, generation }`. Only then does the engine start the target,
    whose `Hello`s, local and remote, follow; a `Detach` of that generation drops a target still
    held. So a target never
    runs at an epoch that is not durable here, whether or not the node holds a chunk of the
    volume: `Store::fence` writes nothing for a volume without one here (`store.rs:1307–1333`),
    and `ReplicaCore::fenced` is 0 for it.
  - **Not before `GetVolume` succeeds,** so a startup retry after the metadata service was out
    keeps its token. A write that fails answers `error: VOLUME: run mark: <reason>`, unlinks its
    `.tmp`, and ends the attach as a failed one. The fsync is on the main thread, which waits for
    this attach anyway, not under the engine's lock.
  - **A leftover `.tmp`** (R2-B1), from a crash before the rename or a write that failed, never
    became the mark, so removing it is safe. It is unlinked before each exclusive create, as the
    records' is (S10), and every stray `.tmp` is removed at start. Otherwise its `EEXIST` would
    fail every later attach of the volume on the node.
  - **The file steps** go through a small trait (`MarkFs`), so that a test can drive and see each
    of them (R2-B2, R3-N3): a directory's creation and its parent's `fsync`; the unlink of a
    leftover `.tmp`; the exclusive create; the write; the file's `fsync`; the rename; the
    directory's `fsync`. The test sees the last five happen, in order, before `Input::Marked`
    is sent.
  - **Its directory** is created 0700, owned by the process's user, and checked by `lstat` at
    start, as the records' (S10). Each directory it creates (`DIR`, `DIR/epochs`) is `fsync`ed in
    its parent, so a power loss cannot lose the marks' directory (R2-B2); kubelet keeps a CSI
    publish context, and with it the old token, across a reboot. Files are opened `O_NOFOLLOW`,
    the `.tmp` with `O_CREAT|O_EXCL`. A malformed mark refuses its volume's attaches, logged.
    Marks are never removed: a volume made again under its name starts above them (#30).
  - **It replaces `ran_epochs`,** which lasted one run, and whose key was left unsaid: by epoch
    alone, one volume's run would refuse another's attach (R1-S4). The mark is per volume.
  - **The local fence** still refuses an epoch at or below it: `u64::MAX` after a delete, or an
    epoch a newer target fenced here. The mark, not the fence, carries the bar across a crash, so
    no order between the local and remote `Hello`s is needed for it.
- **Every epoch refusal** answers `error: VOLUME: epoch: <reason>`. The reasons:
  - at or below the volume's run mark;
  - at or below the fence, including `u64::MAX` after a delete;
  - `claims.epoch` is not the volume's epoch.

  The CSI node plugin matches the exact prefix `error: <requested volume>: epoch: `, not a
  substring, and maps it to `FailedPrecondition` (S9).
- **The local `Hello`** uses the verified claims.
- **After a crash,** an attach with the old token is refused. There is no automatic path with a
  new token:
  - `node.sh up` with `ATTACH` and an old token fails;
  - the lab attaches again with `cmd_vol attach` (`lab.sh`), which takes a new token through
    `metactl`.
- **Earlier volumes of the same name** are #30's.

## With #74's helper (`--privsep`)

#74 (approved, not implemented) runs `flaminio-io node --privsep` unprivileged under a root
helper. The helper adds devices only at the id its own command line names, so it owns one; the
I/O process opens no ublk path and writes nothing under `/run/flaminio` (#74 S2, S3; R1-S1). #15
lands in either order with it:
- **Through the helper,** under `--privsep`: `add` (the helper's id), `start`, `stop` in place
  of the device thread's `kill_dev`, and `delete` in place of `DEL_DEV` and `DEL_DEV_ASYNC`
  (#74, The I/O process under `--privsep`).
- **Direct mode's only:** `--max-volumes` above 1 (S1), the id choice and `--id` (S2), the held
  fd (S2), the records and the startup cleanup (S10: the helper's pin and `node.sh down` cover
  its one device, #74 outcome 5), and zero copy (#74 S7). Under `--privsep` ids repeat across
  attaches; instance, generation and diskseq still tell devices apart.
- **`device-info` under `--privsep`:** the I/O process reads `major:minor` and diskseq from
  `/sys/class/block/ublkb<DEV>/dev` and `/diskseq`, after `start` and before running. The
  helper's pin keeps the id its device's until `delete`. The plugin's own checks against the
  kernel make the identity root's (S9; R1-S3).
- **The run mark's directory** (S11) belongs to the I/O user under `--privsep`.
- **The device thread under `--privsep`** (R2-B8). #74's I/O process has no `run_target`
  (#74, The I/O process under `--privsep`, Each attach), so the device thread runs #74's steps in
  place of `run_target` and its ready callback:
  1. `add`, then `UblkDev::from_cdev` with the reply's values;
  2. the queue threads, as libublk's `create_queue_handlers`;
  3. `start`, once `wait_for_buffer_registration` returns. A `start` with no reply within
     `START_LIMIT` ends the I/O process (#74's approved rule): S5 lists it as an exception;
  4. each queue thread's affinity, as #74 sets it;
  5. the wait for `/dev/ublkb<DEV>` (udev), within `START_LIMIT`, as direct mode's step 1, by
     `stat` alone, which opens nothing (R3-C3). Then the sysfs read above; then the entry turns
     running and the attach is answered;
  6. the stop channel, `Input::Detach`, then `stop`, as in direct mode's steps 4–6;
  7. the queue threads joined and the `UblkDev` dropped; then, as direct mode does after
     `run_target`: the router purges the generation and the arrivals counter is removed; under
     the entry's lock, the entry ends (detach flag set) or is marked stopped (R3-C3);
  8. last, `delete` through the helper, so that a deposed, mounted volume already shows
     stopped. Its wait is bounded only by the device's last close, the mount or whatever holds
     it, which #74 accepts (outcome 3, Risks, Availability). Nothing of #15's waits for it but
     the helper's next request: with one volume under `--privsep`, the next `add` comes only
     after the CSI's detach, and may wait behind it, as #74 states.

  A failure after `add` takes #74's outcome 2 (`stop`, join, `delete`) in step 7 and 8's order
  (R4-B3): `stop`, join the queue threads and drop the `UblkDev`; then answer the attach with
  the first error, send `Input::Detach`, purge the router, remove the arrivals counter and end
  the entry, as S2's failures say; `delete` comes last. So the answer never waits for a
  `delete` that something holds with no bound. The registry, the generations, the router and
  the stop channel are the same in both modes; the order of steps 7 and 8 is this one.
- **Order:** whichever of #15 and #74 lands second adds:
  - the parse refusal and its test;
  - the device thread's `--privsep` variant above, with its tests, the sysfs `device-info`
    among them (Tests, With #74's helper);
  - in `node.sh`'s `PRIVSEP=1` branch, the run mark's directory for the I/O user;
  - in `node-scripts-test.sh`, #74's direct-mode assertion of `del` turned into `flaminio-io
    cleanup`, and a `PRIVSEP=1` case for `del` kept (R4-B1).

  This is as #74 does for #71's telemetry. In direct mode, `node.sh down` runs `flaminio-io
  cleanup` (S10) where #74 deletes `/dev/ublkc$id`; under `PRIVSEP=1` it keeps #74's `del`.
- **Later:** one owned id per volume in the helper, with the held fd, the records and the cleanup
  there, is #74's next step (its `docs/followups.md` item 5, which changes its ADR). It lifts the
  limit under `--privsep`.
- **The lab** runs with `PRIVSEP=0` what needs several volumes or `ATTACH`: `multi-vol`,
  `node-fault` with `OTHER`, `node-fault-both`, `bench-ab` (its zero-copy reads need direct mode
  anyway), and every check that starts the node with `ATTACH`, S1's startup attach and S11's lab
  test among them (`node.sh up` refuses `ATTACH` under `PRIVSEP=1`, #74 R2-S7). Before #74,
  `PRIVSEP` is unused.

## The lab's mechanics

- **`make multi-vol`** (new, `lab.sh`):
  1. creates `VOL2` beside `VOL`;
  2. attaches both on fl-a;
  3. runs `write-load.sh` on both, with `DEV`, a seed and a 1 s fio log each.
     - `write-load.sh` takes `SEED`, `SIZE` and `LOG`, and writes its JSON next to `LOG`. `LOG`
       defaults to a name built from `DEV`, so two runs never share an output, and `node-fault`
       passes one for each volume.
     - A `VERIFY_ONLY` run passes the same `SEED` and `SIZE` as the write.
     - Its length is set by `SIZE`, not by a runtime, so that the verify phase reads back
       everything written;
  4. checks that neither log has an empty second;
  5. greps the node's log for S3's maxima, and records each volume's p99.
- **Depose-one,** in the same target: `metactl attach VOL fl-b` while `VOL2`'s fio runs on
  fl-a.
  - `VOL`'s fl-a entry turns stopping, and `VOL2`'s fio must be clean.
  - The check then detaches `VOL` on fl-a itself, not through `cmd_vol attach`, which first
    detaches elsewhere.
- **`make node-fault`** takes `VOL`.
  - With `OTHER=VOL2`, that volume is attached on fl-a, and under its own `write-load.sh`, through
    fl-b's stop and restart. Its load must end, and verify, before the failover's `cmd_node down
    a`, which ends its device too.
  - So `OTHER` is checked through fl-b's restart only.
  - `make node-fault-both` runs it twice, the volumes swapped, so that each one also goes through
    the failover.
- **The scripts with `/dev/ublkb1`** take the path from the attach: `node-fault`, `drain-test`,
  `chaos`, `partition`, `bench-reads`, `bench-writes`.
- **The 6.8 gate** (R5-3), before #15 ships: S2's lab test "the same on 6.8", on Centaur's 6.8,
  the lab's only kernel before 6.9. Its result goes in the issue's report.
  - **Pass** (the attach takes N + 1 within its deadline while the test's synchronous `DEL_DEV`
    waits): 6.8 does not hold `ublk_ctl_mutex` across that wait, and nothing changes. 6.6 and 6.7
    stay unchecked, and the risk stays named for them (Risks).
  - **Fail** (the attach's `ADD_DEV` waits for the test's close): S1's kernel-conditional limit is
    built, with the release check, the refusal and their test, so that one device's deletion
    never blocks another volume's attach there. With one volume, a re-attach's add may still
    wait behind its own volume's delete, as today's drop does.
  - #74's reading of 6.8.12, a `DEL_DEV` wait that an exit can cut short, does not settle this:
    an interruptible wait for the id says nothing of whether `ublk_ctl_mutex` is held during it,
    and `ADD_DEV` waits on that mutex either way (`mutex_lock_killable`, 6.1's `:1673`).
- **New values reaching remote commands** (R1-S8). `ENV_VARS` is interpolated raw by `run_on_vm`
  and `run_detached` (`scripts/lib.sh:157`, `:175`), and `node.sh` runs `FLAMINIO_IO` with
  `sudo`. So:
  - `bench-ab` resolves `A` and `B` with `git rev-parse --verify "$rev^{commit}"`, and names each
    build's path by the full hash;
  - `DEV`, taken from the data plane's answer, must match `^/dev/ublkb[0-9]+$`;
  - `VOL2` and `OTHER` must match the volume grammar (`ValidID`);
  - each goes into `ENV_VARS` quoted with `printf %q`;
  - `node.sh` refuses a `FLAMINIO_IO` that does not resolve under the bench root.

## Tests, by rule

- **S1** (`control.rs`, `ublk.rs`):
  - N + 1 concurrent reservations give N entries;
  - a late-started device counts;
  - the same volume twice at once gives one entry;
  - names: `pvc-<uuid>` is taken; whitespace and 129 bytes are refused;
  - tokens: another volume's or node's token, a bad signature, a volume claim of 255 bytes, or a
    token at the decoder's `MAX_LEN`, takes no slot;
  - with an attach that never answers, status, list and another volume's detach answer within
    1 s;
  - every error starts `error: VOLUME`, the limit's included (`error: VOLUME: limit: `); a
    volume named `limit` refused for its epoch is not taken for the limit by the plugin;
  - `--max-volumes` 0 and 17 are refused at parse; with `--privsep`, 2 is (once #74 is in);
    on the 6.8 gate's failure only (R5-3): before 6.9 the default is 1 and 2 is refused at
    start, an unparsable release counts as before 6.9, and 6.9 keeps the default of 8;
    `list` with 16 stopping entries of 128-byte names fits 4096 bytes with its newline;
  - an attach queued behind one that never answers, whose deadline passes before its turn, is
    answered, frees its slot and leaves no entry (R1-C4);
  - a token at or below its volume's run mark takes no slot;
  - the attach's timeout sends `Detach` with its generation;
  - the startup `--attach` takes the same path. Its retry stops once that volume has an entry,
    or at an `epoch:` refusal, which is permanent; another volume's entry does not stop it.
- **S2:**
  - ids, on a fake kernel: above those present and recorded, and at or above `--id`; no id comes
    twice. Adds are 250 ms apart, held id or not, and one attach tries at most 41, the last at
    about 10 s (R5-1, R6-1). A failed add at an id `GET_DEV_INFO` answers for moves to the next
    id straight after the failed add (R7-L1, R8-L1); a failed add at an id not held tries it
    again at each attempt until the deadline, and the next
    attach starts above it, so a failure unrelated to the id burns one id per failed attach
    (R2-B6, R3-B1, R4-B2);
  - **the ceiling** (R4-B2, R5-2, fake kernel), each refusal `error: VOLUME: limit: no ublk id
    left in this run; restart the node`, never the last add's error, and 1048576 never tried:
    - the counter at 1048575 and that add failing: the next attach is refused, tries no add, and
      so is every later one;
    - a held id at 1048575: the walk's next id would be 1048576, so that same attach is refused
      with the ceiling's text; on a fake clock, no wait passes between the held add and the
      refusal (R7-L1);
    - the held add at 1048575 is also the 41st, past the deadline: the refusal is the ceiling's
      text, not the add's error (R7-L2);
    - a present or recorded id 1048575 at start: `first_id` is 1048576, the node starts, and
      every attach is refused;
    - the add at 1048575 succeeding: that attach succeeds, and the next is refused;
    - the plugin maps the refusal to `ResourceExhausted`;
  - **routing:** two devices with the same tag sequence, then a detach and a re-attach with ops
    in flight. Every completion reaches its own generation. Stale ones are logged, and the
    router holds nothing of the old generation afterwards;
  - **routing order** (R1-C2, R2-B5): a direct read split into a remote piece and a local piece.
    A test hook pauses the engine thread, holding the remote reply's batch, just before it routes
    it, and releases it after a fixed 200 ms. Meanwhile the queue thread is started on ending
    the local read (`local_read_done`); the test never waits for it before the release. The tag
    must get the `Piece` before the `Done`, and the read's bytes must be whole. With routing under
    the lock, the queue thread waits for the release, and nothing deadlocks; with routing after
    the unlock, it routes `Done` during the pause, and the test fails;
  - **stopping entries:** with a fake clock, one stopping for 5 minutes is logged once, and not
    before (R1-C6);
  - **engine:** a stale `Deposed` after a re-attach is dropped. I/O and fast reads of an older
    generation are refused. `Detach` with no target gives `Detached`;
  - **lab VM:** a stopping entry's fd keeps its minor, and its ublk id (an `ADD_DEV` at that id
    fails), while another device starts; a detach, a deposition and a queue's end each reach the
    stop channel;
  - **lab VM, failed add** (R1-C1): a device the test adds as root at the node's next id after
    the node started. The attach skips it and takes the following id; the device survives; on
    Centaur's 6.8 (the disowned path) the attach answers within its deadline;
  - **lab VM, a deleted but open device** (R3-B1, the lab's 7.0): the test adds a device at the
    node's next id N, holds `/dev/ublkb<N>` open, and deletes it (`DEL_DEV_ASYNC`). The attach
    takes N + 1 within its deadline;
  - **the same on 6.8** (R4-B4, Centaur's 6.8, the disowned path): the test's synchronous
    `DEL_DEV` runs on its own thread while it holds `/dev/ublkb<N>` open; the attach takes N + 1
    within its deadline. It is the 6.8 gate (The lab's mechanics), with an outcome stated for
    each result (R5-3);
  - **the device thread's open** (R2-B4, `flaminio-io`, Linux, root): the open step, with the path
    and the sysfs root injected. A symlink at the path (to a real block node), a character device
    (`/dev/null`), and a block node (a loop device) under a sysfs `dev` naming another
    `major:minor` each fail the attach, and no fd is kept.
- **S3:**
  - a gather deadline and an arrivals deadline, armed from another thread, fire on time; so does
    a QoS release deadline, once #75 is in (R1-C8);
  - asynchronous commits turn on with the first device and off after the last, and stay off
    under `--sync-commits`;
  - **engine:** `Detach` with ops pending, on a target whose ISR is unreachable, fails them with
    `NotAttached`;
  - **`control.rs`, with a fake `kill_dev` that blocks:**
    - the detach answers at 30 s, and the entry is stopping;
    - when `kill_dev` returns, the device thread ends the entry, closes the fd, and frees the
      slot;
    - a second detach meanwhile only waits;
    - **the interleaving:** a detach that takes the lock just before, and just after, the device
      thread's last step. Either way, the entry ends once;
    - a detach waiting on generation G answers once G ends, even when a new entry of the volume
      starts meanwhile;
    - a recorded device that is not dead: it is checked again, then kept and logged, and the
      node still starts;
    - a delete that fails other than with `ENODEV` keeps the record and logs it; `Ok` and
      `ENODEV` remove it. Before 6.9, the record goes only when the `DEL_DEV` thread returns;
  - with 10 000 inputs queued and two threads taking the lock in a loop, the lock wait and the
    heartbeat lateness stay under 100 ms;
  - queue threads leave the inputs to the engine thread.
- **S4:** two devices' arrivals are summed, and a stopped one's are removed.
- **S5 and S6** (engine, two targets):
  - writes and direct reads on both, sharing a commit;
  - a detach and a deposition of one, with I/O on both;
  - #6's failed and timed-out attaches beside a running volume;
  - a store error fails both volumes' writes;
  - **space** (R1-C6): over a small store, one volume's writes fill it; the other's next write
    there fails with `OutOfSpace`, and the log line names the pool that ran out;
  - batches are durable in order across both;
  - the panic hook, in a child process, ends it by abort.
- **S7:**
  - a delete with any registry entry, or with an attach pending, is refused;
  - many chunks while two volumes are attached: their writes wait at most a turn and a commit;
  - a `Hello` and an `Attach` between turns are refused;
  - a second delete gets its answer;
  - an `OutOfSpace` on a delete's commit, in the run, leaves the volume refused, and a later
    delete restarts `begin_delete` and finishes it;
  - **a crash after `begin_delete`:** after the restart, a `Hello` and an `Attach` at E + 1 are
    refused, and a new delete completes (store crash images);
  - **the bound** (R1-C6): `Store::delete_chunks` over 16 384 chunks takes 16 384 / `k` commits
    (store test); on fl-b's disk, a volume with 16 384 chunks, one block written in each, is
    deleted within 50 s while another volume runs fio (lab VM).
- **S8:** `status` with none, one, and three running and one starting; `status VOLUME`, `detach`
  and `list` in each state.
- **S9** (Go):
  - name validation; `list` and its fallback; `MaxVolumesPerNode` 1, or an error;
  - `ResourceExhausted` on `limit:`, `DeadlineExceeded` on an attach's timeout, and
    `FailedPrecondition` on each of the three epoch answers, and not on another volume's;
  - starting gives `Unavailable`, stopping `FailedPrecondition`;
  - an older plugin's parse of the new `status`;
  - **an older data plane** (a fake answering `device-info` with its usage line): staging takes
    the plain `status` and `attach` path, one volume, and still refuses a foreign path (R1-C3);
  - **the device check** (R1-S3, R2-B3): a `device-info` reply naming another block device, such
    as a loop device, with that device's own `major:minor` and diskseq, is refused by #74's
    `checkUblkDevice` with `codes.Internal`. A mount in place on another device gives
    `FailedPrecondition`; a second reply that differs gives its state's code (stopping
    `FailedPrecondition`, another generation `Unavailable`), not `Internal` (R3-N2);
  - **#15's comparison** (R3-C2; Linux, root): with the device path and the sysfs root injected,
    the test makes a block node named `ublkb<N>` with a loop device's rdev, and a fake
    `ublkb<N>/dev`, and varies one field at a time: the reply's `major:minor` differs from the
    fake sysfs (the node matching the reply); it equals the fake sysfs but differs from the
    node's rdev; its diskseq differs from the loop device's. Each is refused with
    `codes.Internal`, and no probe, format, mount or bind runs. With the sysfs comparison removed,
    its case is taken, and the test fails;
  - **`HostMounter`** (Linux, root, loop devices): wrong expected diskseq at open, changed second
    device-info or mount evidence refuses before formatting; existing foreign mounts remain.
    Cover V -> path reuse by W and V -> newer V. Shared contract tests cover every compared field.
    (The conditional-detach client and its replacement test are #79's.)
- **S10** (lab VM):
  - a restart with two volumes: the recorded leftovers are deleted;
  - a live device of another ublk server is left;
  - **the matching branch** (R1-S7): a dead device at a recorded id whose `dev_info` differs (in
    owner, flags or queues) is left, and so is a dead device with no record;
  - a record left by a failed `ADD_DEV` is gone, and one whose device is gone (`ENODEV`) is
    removed;
  - **records' trust** (R1-S6, host test): a `.tmp` that is a symlink to another file leaves that
    file unchanged; a record name outside `^(0|[1-9][0-9]{0,6})$` (`01` among them), or a record
    over 4096 bytes, is logged and left; a records directory that is a link, not root's, or not
    0700 stops the start; `flaminio-io cleanup` deletes what the startup cleanup would, and
    nothing else;
  - **one owner** (R2-B6, host test): with the records directory's `flock` held, as by a running
    node, `flaminio-io cleanup` refuses and deletes nothing, and a second node refuses to start;
  - **cleanup before 6.9** (R2-B6, Centaur's 6.8): with a recorded dead device held open by the
    test, `flaminio-io cleanup` returns within 6 s, keeps its record and logs it; once the test
    closes it, the next cleanup leaves neither the device nor its record;
  - **a forced failure before a queue's loop,** without and with zero copy, and a forced
    `set_params` failure: `run_target` returns, every queue's exit is counted, then the device is
    deleted, and the node and the other volume go on (no abort, R1-C5). With zero copy, no
    `START_DEV` is sent. This runs on the lab's kernel, and on the synchronous-delete fallback;
  - a device thread's three failures (`add_device`, `start_dev`, `START_LIMIT`) each answer the
    attach, and end the entry;
  - **`flaminio-io del`** deletes a device on Centaur's 6.8 (`DEL_DEV`) and on the lab's 7.0
    (`DEL_DEV_ASYNC`) (R1-C11);
  - **raw block:** a raw block pod's volume; then the data plane is killed, the CSI node plugin
    restarted, and another volume attached. The stale bind's `rdev` is not the new device's, and
    is no non-ublk disk's in `/sys/class/block` (R1-S10).
- **S11** (engine):
  - another volume's, another node's, an older or newer epoch, a bad signature, an empty token,
    a malformed one: each fails; `flaminio-node` builds without `flaminio-net`'s `unverified`
    feature;
  - **the run mark, in the engine** (R1-S2): on a node that holds no chunk of the volume, the
    engine holds the target after `GetVolume` and emits `MarkEpoch`; a fake network sees no
    `Hello` at E before `Input::Marked`; a `Detach` of that generation instead drops the target
    with no `Hello`; a metadata failure before `MarkEpoch` emits none.
- **S11** (`flaminio-io`: the registry, the socket and the main thread, where marks are read and
  written; R2-B2):
  - **across a restart:** an attach at E on a node that holds no chunk of the volume; a new node
    over the same store and state directory loads the mark, and its socket refuses E before any
    slot, and takes E + 1. A metadata failure before `MarkEpoch` writes no mark, and the retry at
    E is taken;
  - **its key** (R1-S4): volume B is taken at an epoch A has run; A at that epoch again is
    refused;
  - **durability:** with a recording `MarkFs`, the exclusive create, the write, the file's
    `fsync`, the rename and the directory's `fsync` all happen, in that order, before
    `Input::Marked` is sent; an implementation that drops or reorders either `fsync` fails it.
    Creating `epochs/` `fsync`s its parent; a leftover `.tmp` is unlinked before the create;
  - **a failed write** (a `MarkFs` that fails the create, the write or an `fsync`, and a
    read-only state directory): the answer
    `error: VOLUME: run mark: …`, `Input::Detach` with the generation, the entry ended, the
    `.tmp` gone, and no `Hello`. Once the directory is writable again, the next attach at a
    higher epoch works (R2-B1);
  - **a leftover `.tmp`** (R2-B1), as a crash before the rename leaves it: the next attach at a
    higher epoch works, and a start removes any stray `.tmp`;
  - **a malformed mark** refuses that volume's attaches, logged, and not another volume's; a state
    directory that is a link, another user's, or not 0700 stops the start;
  - **the simulator** (`flaminio-repl/src/sim`): each node keeps a run mark per volume across its
    crashes; `start_target` refuses a view at or below it, and raises it before the target's
    first output. A chaos event sends a node, after its restart, the last `Start` it took (an old
    token). The run fails if two target incarnations ever ran at one epoch. Injected bug
    `sim::Bugs::volatile_run_mark` (the mark is lost in a crash): the simulator must catch it, as
    it does the other injected bugs (`make sim`);
  - **lab VM:** after a crash, `node.sh up` with the old token is refused at the run mark, and
    `cmd_vol attach` then attaches (`PRIVSEP=0`).
- **With #74's helper** (R2-B8; by the second of #15 and #74 to land):
  - the device thread's `--privsep` variant attaches, detaches and is deposed through the
    helper's requests, on #74's fake helper; a failure after `add` sends `stop`, joins the queue
    threads, sends `delete`, and ends the entry;
  - its attach is answered only once `/dev/ublkb<DEV>` exists (a fake node path that appears
    late); with a fake helper whose `delete` blocks, a deposed entry is already stopped, the
    router holds nothing of its generation, and the socket still answers (R3-C3); and on a
    failure after `add` (a queue thread that fails), the attach is answered with that error and
    the entry ended while that `delete` still blocks (R4-B3);
  - `device-info`'s `major:minor` and diskseq are what `/sys/class/block/ublkb<DEV>/dev` and
    `/diskseq` hold: a unit test with an injected sysfs root, and on the lab VM, under the
    helper, against the kernel's own values (`make privsep-check`);
  - `--max-volumes 2` with `--privsep` is refused at parse (S1's test).
- **Scripts** (R1-S8, `scripts/tools`): `bench-ab` refuses a revision that `git rev-parse` does
  not resolve, and names its path by the hash; a `DEV`, `VOL2` or `OTHER` with a space, `;` or
  `$(…)` is refused before any remote command; `node.sh` refuses a `FLAMINIO_IO` outside the
  bench root.
- **`node.sh down`** (R2-B6, `scripts/tools/node-scripts-test.sh`, which #74 also adds;
  whichever lands first creates it): in each mode, `down` stops with today's message while
  `$dev` or a recorded device is mounted. Otherwise it stops the unit first, then deletes by
  mode, and the test asserts each order (R3-C1, R4-B1):
  - in direct mode, the stop, then `flaminio-io cleanup` (this replaces #74's direct-mode `del`
    case);
  - under `PRIVSEP=1`, the stop, then `del` of `/dev/ublkc<ID>`, #74's own case kept.

## Measurement (S12)

- **A new target:** `make bench-ab A=<rev> B=<rev>`.
- **Building:**
  - each revision is resolved to its commit hash, and built from `git archive <hash>`, into its
    own path, named by the hash, outside `deploy`'s wipe (R1-S8);
  - both run with `PRIVSEP=0` (#74 S7: zero copy is direct mode's);
  - `node.sh` takes the binary's path from `FLAMINIO_IO`, passed through `node_env`;
  - the RF 2 peers, fl-a and fl-c, switch with fl-b.
- **Order:**
  - reads first, on `vol1`, as the write campaign reformats fl-b;
  - then writes, on `wbench`, reset each run (W1–W2);
  - in each repetition: raw, then A and B, with A and B swapping places every repetition.
- **Repetitions:** 3. One more when 2 × spread exceeds the bound. If it still does, the job is
  inconclusive.
- **What is compared:** each side's spread is (max − min) / median. B's median share of raw is
  compared with A's.
- **Jobs:**
  - reads: 4K QD1, 4K QD32 and 1M QD8;
  - writes: 4K QD1 at RF 1 and RF 2, with one raw run for both; and 1M QD8 at RF 1.
- **RF 2 topology:** the 2026-10-17 campaign's. The target is on fl-b, with a replica of each
  chunk on fl-b and one on fl-a or fl-c. Only fl-b is reset.
- **Time:** reads under an hour; writes, 24 runs of about 25 minutes, about 10 hours. Wear: in
  decisions.md.

## Risks

- **The lock:** two polled volumes contend with the engine thread. S3's log measures it, and the
  hand-off flag is the fallback.
- **The extra wake-up** per commit and per remote reply: RF 2 4K QD1 measures it.
- **Kubelet's loop device** may not hold the device. The fallback is in S9; the plugin's own
  restart is the gap left.
- **`DEL_DEV_ASYNC`** needs Linux 6.9 or later.
- **#10 is still moving:** this design follows its branch as of `692de13`.
- **The vendored libublk** (R1-C1, R1-C5): two hunks, #74's failed-add hunk and the
  registration wait under `UBLK_F_AUTO_BUF_REG`, maintained here until upstream takes them
  (#74's settled Q2). `make hygiene` checks the copy.
- **Under #74's `--privsep`,** the node takes one volume until #74's per-volume step, and holds
  no device fd, so a deposed raw block volume's minor is free once its device goes (Accepted
  risks, the raw block gap).
- **The run mark** adds a durable file per volume ever targeted on the node, and one `fsync`
  pair to each attach, off the engine's lock.
- **A synchronous `DEL_DEV` on 6.6–6.8** (R4-B4). In the 6.1 source read here, `DEL_DEV` holds
  `ublk_ctl_mutex` while it waits, without interruption, for the last close
  (`ublk_drv.c:1758–1781`), and `ADD_DEV` takes the same mutex (`:1673`); `GET_DEV_INFO` takes
  neither, and answers until the release. Whether 6.6–6.8 still hold the mutex across that wait
  is not checked here; #74's reading of 6.8.12 does not settle it (The 6.8 gate). If they do,
  every add on the host, an attach of another volume included, waits behind the node's own
  `DEL_DEV` thread, or another server's, until the held device closes; the device thread then
  waits inside `ADD_DEV`, where neither the deadline nor the pace runs. Named until the 6.8 gate
  runs: a failing gate closes it on those kernels with S1's limit of 1 (R5-3); a passing one
  leaves it named for 6.6 and 6.7. The lab runs 7.0, which deletes asynchronously.
