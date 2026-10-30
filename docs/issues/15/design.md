# Issue #15: more than one volume per node — design

Spec rules S1–S13 ([spec](spec.md)); settled choices in [decisions](decisions.md). Paths are under
`dataplane/crates/` unless they say otherwise.

## The shape

**Today:**
- The main thread drives the engine (`flaminio-io/src/ublk.rs`, `serve_node`, `drive_until`).
- An attach moves the engine into one device's `Shared`, and blocks in `run_target`
  (`attach_and_serve`, `run_device`).

**After:**
- **`Node`** (new, `ublk.rs`) holds the engine, behind the same `Mutex`. With it:
  - the registry;
  - the router;
  - the instance;
  - the generation and op counters.
- **The engine thread** (new) alone owns the input receiver. Queue threads' `pump_with` no longer
  drains inputs: it only flushes, and hands the outputs to the router.
- **The main thread** takes attaches in turn (S1, below). Each attach that succeeds gets a device
  thread.
- **`Shared`** keeps only a device's own state: its queues' wakes, its staging, its generation.
- **Lock order:** the registry, then the engine. No thread takes the registry while it holds the
  engine. The arrivals counters live in the engine (`Engine::add_arrivals`, `remove_arrivals`),
  so `Engine::arrivals()` reads them under its own lock.

**The router** (`Node::route`), called by whichever thread took the outputs:
- `Done`, `LocalRead` and `Piece` go by op id to the waiting tag, through `Mutex<Routes>`: op →
  (generation, queue, channel), as `waiters` today. That lock is a leaf: it is never held while
  another lock is taken;
- `Attached`, `AttachFailed` and `Detached` go by `(volume, generation)` to the registry entry's
  waiter;
- `Deposed` goes to the entry's stop channel (S2);
- anything whose op or generation is unknown is dropped and logged.
- When `run_target` returns, the router purges every op of that generation. The engine's own
  per-volume state goes with the target (`Engine::detach`): its pending `local_reads` are
  dropped, as today.

## S1: limits, names, attaches

- **The limit:** `--max-volumes N` (`node.rs`, `NodeConfig`), default 8. The usage text gives a
  device's cost: depth × `--io-buf` per queue, 64 MiB at the defaults, virtual until touched.
- **The registry:** a `Mutex<Registry>`, holding entries by volume. `reserve(volume)` checks the
  limit and the volume, and inserts a starting entry, under that lock.
  - A late-started device (#6, S2) keeps its entry until its `run_target` returns, so it counts.
  - The count of running devices is kept under the same lock, and decides the asynchronous
    commits (S3).
- **Names:** `ValidID` (`meta/domain/domain.go`), checked in `control.rs` for every command, and
  in `control/csi/dataplane.go` before a line is built. `cli.rs`'s 16-byte rule stays `serve`'s.
- **Tokens before the slot:** the socket checks the token's signature, `claims.volume` and
  `claims.node`, then reserves. The `TokenKey` comes from `build` (`node.rs`, `token.pub`).
  - The startup `--attach` goes through the same check and reservation.
  - Tokens carry `issued_at` and no expiry, and none is checked here, as for sessions today.
- **Errors:** every error starts with its volume:
  - `error: limit: N volumes attached or attaching`;
  - `error: VOLUME is starting|running|stopping here`.
- **One attach at a time, on the main thread.** This is today's `attach_and_serve` metadata
  phase, without the engine in hand.
  1. It sends `Input::Attach { volume, token, generation }`, and waits on the entry's waiter.
     The wait ends at `METADATA_LIMIT`, or at `steps_by` if earlier.
  2. **`Attached`:** it spawns the device thread, which runs `add_device` and `run_target`.
     The main thread takes the next attach.
  3. **`AttachFailed`, or the wait timing out:**
     - it sends `Input::Detach { volume, generation }`, as #6's S3 does today;
     - it ends the entry, and answers the attach with the reason.
- **Detach, delete, status and list** are answered on the socket's command threads. They never
  queue behind an attach.

## S2: identity and the device thread

- **Ids:**
  - **`first_id`** is computed at start, before the cleanup (S10). It is the highest of:
    - every id under `/sys/class/ublk-char/ublkc*`;
    - every recorded id;
    - `--id`, less one;

    plus one, and at least 0. `--id -1`, the default, sets no floor.
  - **Later ids** only increase. An `EEXIST` from `ADD_DEV`, or a record that already exists
    (`O_EXCL`, S10), moves to the next.
  - `add_device`'s retry of its own id goes, and `--id`'s usage text says how ids are chosen.
- **Generations and the instance:** from `Node`'s counters. The instance is a `u64` from
  `ring::rand::SystemRandom`, and `ring` is added to `flaminio-io`'s dependencies.
- **The device thread's control loop is the ready callback.** `run_target` calls it on the
  caller's thread, after starting the device and before joining the queue threads (libublk,
  `run_target`). So the callback:
  1. waits for `/dev/ublkbN` (udev), within `START_LIMIT`;
  2. opens it `O_RDONLY`, without `O_EXCL`, and `fstat`s it. The entry keeps the fd and its
     `major:minor`;
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
    - **Without zero copy:** a failure there calls `dev.notify_buffer_registration_complete(true)`.
      `start_dev` then returns `EPERM`, and `run_target` returns `Err`.
    - **With zero copy,** the kernel waits for every tag's FETCH. A watchdog, armed for
      `START_LIMIT` before `run_target`, stops and deletes the device if the callback has not
      run by then. The zero-copy test on Linux 7.0 shows whether that ends `START_DEV`. If it
      does not, a queue failing before its loop panics, and the node aborts (S5).
  - **`run_target` returns `Err` before the callback.** Some queue threads may be in their loop,
    and are not joined.
    - The device thread sets a stop flag in `Shared`, and signals each queue's own eventfd.
      `is_done` is true once the flag is set.
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
- **Why mounting by path, then checking, is enough** (S9): while the node runs, ids are never
  reused, and the held fd keeps the gendisk and its minor alive. So between the check and the
  mount, the path can name only that device, or nothing.
- **Generations, end to end:**
  - `Input::Attach`, `Input::Detach` and `Input::Io` carry the generation, and `TargetRt` keeps
    it (`flaminio-node/src/engine.rs`);
  - the outputs echo it;
  - the fast-read path (`try_fast_read`, `plan_fast_read`, `finish_fast_read`) takes it, and
    refuses a mismatch;
  - `Detach` with no target now gives `Detached` (`Engine::detach`).
- **Stopping entries** are shown in `list`. One stopping for more than 5 minutes is logged once.

## S3: the engine thread

- **The wait:** `ppoll` on the node's eventfd. Its timeout is the earliest of:
  - the next turn;
  - the gather or hold deadline;
  - #10's arrivals limit (the window's end + `ARRIVALS_LIMIT`).

  `Engine::next_wakeup` returns all three, at nanosecond precision.
- **Arming:** a queue thread that arms a deadline (`take_gather_timer`) signals the eventfd. The
  per-queue gather timer tasks go.
- **Its loop:** under the lock:
  1. handle at most 64 inputs;
  2. `turn` if it is due, and `flush` otherwise;
  3. take the outputs, and let go of the lock;
  4. route the outputs.
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

- **Unchanged:** `EngineCtx::submit` and `pump_if_due`, under the lock.
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
  chunks of 256 MiB) ends within 50 s.
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

- **Why `attach` is unchanged:** an older plugin takes all of it after `ok` as the path
  (`dataplane.go`, `command`).
- **Why plain `status` names one volume:** an older plugin's `strings.Cut` would take a longer
  line's rest as the path.
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

- **`NodeGetInfo`:** from `list`. A usage line from the data plane gives 1. Kubelet reads it when
  the plugin registers, so the plugin is restarted after the limit changes.
- **`device`:** asks `status VOLUME`, and calls `Attach` when there is no entry. The plugin maps
  its failures (`node.go`, `device`), not to `Internal`, so kubelet retries or unstages:
  - an attach that times out: `DeadlineExceeded`;
  - starting: `Unavailable`;
  - stopping: `FailedPrecondition`;
  - an attach refused for its epoch (S11): `FailedPrecondition`. Kubelet's retry reuses the
    publish context, so only a new `ControllerPublishVolume` recovers.
- **The identity check** (`NodeStageVolume`, `NodePublishVolume`):
  1. `status VOLUME`;
  2. open the path, and `fstat` it: a block device, of that `major:minor`. The fd stays open
     through steps 3–5;
  3. `status VOLUME` again: the same instance and generation;
  4. `blkid` and `mkfs` by path;
  5. mount by path. The last `/proc/self/mountinfo` line for the target must then show the same
     `major:minor` (field 3), or the plugin unmounts.
- **A target already mounted** is checked against `status VOLUME`:
  - a filesystem: mountinfo's last matching line;
  - a raw block target: `stat(target).st_rdev`;
  - on a mismatch, the plugin unmounts and refuses.
- **Raw block across restarts:**
  - kubelet's `MapBlockVolume` is expected to attach a loop device that holds the device open.
    This is from memory of `volumepathhandler.AttachFileDevice`, not checked here; the lab test
    decides.
  - If it does not, the plugin holds an fd per raw block publish. At its own start, it reopens
    them for the publishes whose instance and generation still match.
- **Errors:** `limit:` and "one volume at a time" give `ResourceExhausted`.
- **An older data plane:** the plugin falls back to plain `status` on a usage line, as for #6, and
  logs that it skipped the identity check.
- **Locking:** the plugin's one node lock stays.

## S10: crash and restart

- **The record:** `/run/flaminio/ublk/<id>`.
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
  - `flaminio-io del` uses `del_dev_async`.
- **Scripts:**
  - `node.sh down` stops the unit, then deletes the recorded devices. A build without records,
    such as main in `bench-ab`, keeps today's `del --id` before the stop: `bench-ab` runs each
    revision's own `scripts/guest/node.sh` from its archive, and A keeps `--id`;
  - every script takes the path from the attach or `status VOLUME`.

## S11: tokens and epochs

- **The engine gets the `TokenKey`.** `Input::Attach` verifies the token, and keeps its claims.
  After `GetVolume` (`start_target`), `claims.epoch` must equal `volume.epoch`.
- **Every epoch refusal** answers `error: VOLUME: epoch: <reason>`. The reasons:
  - an epoch already run (`ran_epochs`);
  - at or below the fence, including `u64::MAX` after a delete;
  - `claims.epoch` is not the volume's epoch.

  The CSI node plugin matches the exact prefix `error: <requested volume>: epoch: `, not a
  substring, and maps it to `FailedPrecondition` (S9).
- **Refused epochs:**
  - one in `ran_epochs`, which is kept for the run;
  - one at or below the local replica's durable fence for the volume (`ReplicaCore`, recovered
    with each chunk).
- **The local `Hello`** uses the verified claims.
- **After a crash,** an attach with the old token is refused. There is no automatic path with a
  new token:
  - `node.sh up` with `ATTACH` and an old token fails;
  - the lab attaches again with `cmd_vol attach` (`lab.sh`), which takes a new token through
    `metactl`.
- **Earlier volumes of the same name** are #30's.

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
  - every error names its volume;
  - the attach's timeout sends `Detach` with its generation;
  - the startup `--attach` takes the same path. Its retry stops once that volume has an entry,
    or at an `epoch:` refusal, which is permanent; another volume's entry does not stop it.
- **S2:**
  - ids: above those present and recorded, and at or above `--id`. `EEXIST` is skipped, and no
    id comes twice;
  - **routing:** two devices with the same tag sequence, then a detach and a re-attach with ops
    in flight. Every completion reaches its own generation. Stale ones are logged, and the
    router holds nothing of the old generation afterwards;
  - **engine:** a stale `Deposed` after a re-attach is dropped. I/O and fast reads of an older
    generation are refused. `Detach` with no target gives `Detached`;
  - **lab VM:** a stopping entry's fd keeps its minor while another device starts; a detach, a
    deposition and a queue's end each reach the stop channel.
- **S3:**
  - a gather deadline and an arrivals deadline, armed from another thread, fire on time;
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
    refused, and a new delete completes (store crash images).
- **S8:** `status` with none, one, and three running and one starting; `status VOLUME`, `detach`
  and `list` in each state.
- **S9** (Go):
  - name validation; `list` and its fallback; `MaxVolumesPerNode` 1, or an error;
  - `ResourceExhausted` on `limit:`, `DeadlineExceeded` on an attach's timeout, and
    `FailedPrecondition` on each of the three epoch answers, and not on another volume's;
  - starting gives `Unavailable`, stopping `FailedPrecondition`;
  - an older plugin's parse of the new `status`;
  - **`HostMounter`** (Linux, root, loop devices): a mismatch at the open, at the second status,
    or in mountinfo is refused, with nothing mounted. An existing mount of the wrong device is
    unmounted and refused, for a filesystem and for a raw block bind.
- **S10** (lab VM):
  - a restart with two volumes: the recorded leftovers are deleted;
  - a live device of another ublk server is left;
  - a record left by a failed `ADD_DEV` is gone, and one whose device is gone (`ENODEV`) is
    removed;
  - **a forced failure before a queue's loop,** without and with zero copy, and a forced
    `set_params` failure: `run_target` returns, every queue's exit is counted, then the device is
    deleted. This runs on the lab's kernel, and on the synchronous-delete fallback;
  - a device thread's three failures (`add_device`, `start_dev`, `START_LIMIT`) each answer the
    attach, and end the entry;
  - **raw block:** a raw block pod's volume; then the data plane is killed, the CSI node plugin
    restarted, and another volume attached. The stale bind's `rdev` is not the new device's.
- **S11** (engine):
  - another volume's, another node's, an older or newer epoch, a bad signature, an empty token,
    a malformed one: each fails;
  - an epoch already run is refused;
  - **lab VM:** after a crash, `node.sh up` with the old token is refused at the fence, and
    `cmd_vol attach` then attaches.

## Measurement (S12)

- **A new target:** `make bench-ab A=<rev> B=<rev>`.
- **Building:**
  - each revision is built from `git archive <rev>`, into its own path outside `deploy`'s wipe;
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
