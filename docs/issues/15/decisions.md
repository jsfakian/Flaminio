# Issue #15: more than one volume per node — decisions

Settled in plan reviews 1–4. Reviewers take these as given. Each line gives the choice, the
reason, and where it came from: rounds 1–4, prefixed F, M, Q and U; "author" for the first draft.

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
  earliest of the turn and the gather, hold and arrivals deadlines. A thread that arms a deadline
  signals it. (F1; Q-round Low: `ARRIVALS_LIMIT` was never armed)
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
  skipped. `add_device`'s own-id retry and a "highest id" rule were cut. (F4, M3)
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
- A per-node volume limit, 8 by default (to be approved), reserved atomically. It counts every
  entry and late-started devices. (author, F9)
- Attaches are taken one at a time. Overlapping metadata phases are a follow-up. (F9)
- The token's signature, volume and node are checked before a slot is reserved, and the epoch
  after `GetVolume`. (M8)
- Volume names are checked by the metadata service's rule everywhere, in the CSI node plugin
  too. (Q1)
- `attach`'s answer and plain `status` keep today's format, so an older CSI node plugin works on
  one volume. A per-volume status and a list are new. (F11, author)
- A stable `limit:` prefix marks the limit refusal. (F11)
- The control socket's hardening (a read timeout, a connection cap) is not done here: the socket
  is root-only, and #12 moves it. (M-round Low)

**Tokens and epochs**
- Attach tokens are verified: signature, volume, node and epoch. This fixes a gap that predates
  this issue. (F6)
- An attach is refused at an epoch already run in this run, or at or below the local replica's
  durable fence for the volume. After a crash, the restart path gets a new token. (Q6, U1)

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
  cover it. (Q5, U5)
- Existing mounts are checked against the volume's status, and refused on mismatch. Filesystem
  mounts are checked by mountinfo, and raw block by `stat` of the target. (U2)
- Raw block across restarts: kubelet's loop device is expected to pin the minor; a lab test
  decides. If it does not, the plugin re-opens its raw publishes' fds at its start. (Q4, U3)
- One lock per node stays: attaches are serialised anyway. (M-round cut)

**Performance**
- `bench-ab`: 3 repetitions, A and B alternating between repetitions, reads before writes.
  Jobs: reads QD1, QD32, 1M; writes 4K QD1 at RF 1 and RF 2 (one raw run for both), and 1M QD8.
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
  (Q4)
- **The lab's 1M write campaign** wears the drive by about 7.5 TB. (Q8)

## Added in the full review

- **The device thread's control loop is libublk's ready callback.** `run_target` runs that
  callback before it joins the queues, so the loop can `kill_dev` from there.
- **One router in `Node` delivers every output:**
  - I/O by op id;
  - attach results by `(volume, generation)`;
  - `Deposed` to the device's stop channel.

  Queue threads no longer drain inputs.
- **The main thread waits for an attach's result,** and on a timeout sends `Detach` with the
  generation.
- **The router purges a generation's ops** when its `run_target` returns.
- **No automatic re-attach after a crash.** The old token is refused. The lab attaches again with
  `cmd_vol attach`, which takes a new token.
- **The startup `--attach`** takes the socket's reserve-and-token path.
- **The startup record:**
  - written `O_EXCL`, so an id whose record exists is skipped; removed on any `ADD_DEV` failure,
    and when `GET_DEV_INFO` gives `ENODEV`;
  - written before `ADD_DEV` with the volume, instance and generation, then replaced atomically
    with the `dev_info` that `ADD_DEV` returned, which the kernel rewrites. All of it is compared
    at cleanup. A record without `dev_info` is logged and left, unless its device is gone;
  - `first_id` counts the recorded ids, is at least 0, and `--id -1` sets no floor.
- **Lock order:** the registry, then the engine. The arrivals counters live in the engine.
- **The device thread's failures** (`add_device`, `start_dev`, `START_LIMIT`) each answer the
  attach, send `Detach`, remove the record, and end the entry. After a `start_dev` failure, a
  stop flag and each queue's eventfd make the queue threads drop their rings, and the device is
  deleted once all have exited.
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
    through `node_env`.
- **No token expiry is checked.** Tokens carry `issued_at` only, as for sessions today.
