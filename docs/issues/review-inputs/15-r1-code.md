# Issue #15 plan review, round 1, focus: design-code

Scope: docs/issues/15/ (intent, spec, design, decisions, device-lifetime-contract), checked against
the code in /home/jsfakian/Documents/src/flaminio-15 and libublk 0.4.8 (cargo registry).

### R1-C1 — High
**A failed ADD_DEV at a chosen id deletes whatever device holds that id; the id plan depends on it.**
The design always picks the id itself (`first_id`, then increasing) and treats `EEXIST` from
ADD_DEV as "move to the next id". In libublk 0.4.8 a failed ADD_DEV runs the handle's `Drop`, which
sends DEL_DEV for the id that was asked for. `EEXIST` means another device holds that id, so the
`Drop` deletes it. That breaks S10's "Others are never touched". Before 6.9 the `Drop`'s DEL_DEV
is synchronous: if the foreign device is open, the device thread blocks inside `add_device`.
The attach is then never answered and its starting entry is never ended (a leaked slot). The
design's disown and DEL_DEV_ASYNC handling only applies to a handle that `build()` returned, and a
failed add returns none. This is cross-plan item 2: #74 found the same path (`ublk.rs:902–934` on
its branch is `add_device`, here `ublk.rs:875–907`) and fixes it with a vendored libublk patch.
#15 does not mention that patch.
Evidence: libublk `ctrl.rs:1258` (`dev.handle_device_lifecycle(config.id)?` drops `dev` on
error), `:1164–1167` (`self.add()?`), `:1085–1096` (`Drop`: `for_add_dev() && !is_disowned()` →
`del()`), `:1626–1635` (DEL_DEV or DEL_DEV_ASYNC for `dev_info.dev_id`, the requested id).
design.md:79–80 and :427 (`EEXIST` is skipped); design.md:342–351 (disowning only a built
handle); spec.md:134–136.
Fix: make a failed ADD_DEV send no delete. Either depend on #74's libublk patch, which marks the
handle deleted when the lifecycle step fails, or add ADD_DEV with id -1 and record the id the
kernel returns, and restate S10's crash argument for that order. Add a test: a foreign device
sits at the next id; the attach skips it and takes the following id; the foreign device survives;
on the disowned (before 6.9) path, nothing blocks.

### R1-C2 — High
**Outputs routed by two threads after the engine lock is released can deliver a read's `Done`
before its `Piece`, so the read returns wrong data.**
The engine thread and the queue threads each take an output batch under the engine lock, release
the lock, then route the batch (design S3 loop steps 3–4, S4 `pump_with`). Nothing keeps batches
in the order they were produced. Take a direct read split into a remote piece and a local piece
(a read across a chunk boundary, with the local replica out of the ISR for one chunk, as during a
rebuild):
1. The engine thread handles the remote reply. `sub_done` pushes `Piece`. The engine thread takes
   the batch, releases the lock, and has not routed it yet.
2. The tag's local read ends on its queue thread. `local_read_done` → `sub_done` pushes `Done`, and
   that thread's pump routes `Done` first.
3. The tag task breaks out of its loop on `Done(Ok)` and completes the read with the piece missing.
   The late `Piece` is then "dropped and logged".

Today the default `--queues 1` routes every batch on queue 0's one thread, in order. The design
introduces this race at the defaults. S2's "reaches its generation or is dropped and logged"
does not rule it out, and the routing test (two devices, re-attach) would pass with it.
Evidence: design.md:28–35, :176–180, :194; ublk.rs:557–606 (route after unlock); engine.rs:1612–1630
(`Piece` at each sub's end, `Done` when `pending` reaches 0); ublk.rs:1184–1194 and :1341–1353 (the
tag breaks on `Done` whatever pieces arrived).
Fix: deliver I/O outputs in production order. For example, take `Routes` before releasing the
engine lock (engine → routes; routes stays a leaf), or route the I/O outputs under the engine lock,
since `try_send` does not block. Add an engine/router test: hold the engine thread between unlock
and route, produce `Done` from a queue thread's `local_read_done`, and assert the tag sees the
`Piece` first.

### R1-C3 — Medium
**The plugin side of S9 is split between #15 and #79 with no clear boundary, and it depends on #79 in a cycle.**
- spec.md:127–129 conditions behaviour on "With #79 enabled". No flag, probe or default defines it.
- design.md:292 makes #15's rule for an already-mounted target ("checked against the volume's
  status", spec.md:120) depend on "#79 durable ownership". #79 depends on #15's contract
  (#79 design.md:30, :142–144: the identity client is #79's, the shared lifetime is #15's). So #15,
  which lands first, cannot build its own S9 rule as designed.
- design.md:288–291 and the S9 `HostMounter` tests (design.md:483–486) have #15 build the
  device-info/open/device-info client that #79's design lists as its own.
- decisions.md:120–124 (Q5/U5) still says mkfs runs by path ("`/dev/fd/3` for mkfs was cut") and
  that existing mounts are checked against `status`. device-lifetime-contract.md:30–33 says tools
  must operate on the held object.
- intent.md:128–129 ("upgraded in either order") is met only if a new plugin keeps a legacy path on
  an older daemon. design.md:281 always sends `status VOLUME`, which an older daemon answers with
  its usage line, and no fallback is given.

Fix: state which CSI changes #15 ships and which #79 ships. Define "#79 enabled" (a plugin flag or a
capability probe). Give #15's own rule for existing mounts and its legacy `status VOLUME` fallback.
Mark Q5/U5 as superseded by the contract.

### R1-C4 — Medium
**A reserved attach that is never begun has no step that ends its entry.**
The socket reserves a starting entry and then queues the request for the main thread. The main
thread skips a request whose `steps_by` has passed (today's ublk.rs:775–781). The socket's own
wait ends at the caller's deadline while the request is still queued (control.rs:137–144). The
design ends the entry only on `AttachFailed` or on the metadata wait timing out (design.md:58–66).
A skipped request would leave a starting entry for good: the volume stays "starting" (the CSI maps
that to `Unavailable` and retries forever) and holds a slot. intent.md:146–148 expects exactly this
case with 8 attaches staged at once.
Fix: end the entry, and answer, on the skip path too, and on any path where the reply channel is
dropped. Add a test: an attach queued behind a slow one that passes its deadline frees its slot
and leaves no entry.

### R1-C5 — Medium
**The zero-copy fallback aborts the node on one volume's failed attach, which S5 forbids.**
If the watchdog cannot end START_DEV, "a queue failing before its loop panics, and the node
aborts". That turns one volume's failed attach into a failover of every volume. spec.md:72–73
lists a failed attach among the faults that end only their own volume. decisions.md does not list
this abort as an accepted risk or an exception.
Evidence: design.md:111–114; spec.md:72–73, :81.
Fix: list the abort as an S5 exception and an accepted risk, or specify a way that does not abort
the node. For example, cancel or interrupt the pending START_DEV; its wait for FETCH is
interruptible.

### R1-C6 — Medium
**Spec rules without a named test.**
- S5 space: a volume that fills the disk fails other volumes' writes, and the node logs which pool
  ran out (spec.md:79–80, design.md:208–209). The S5/S6 tests (design.md:459–465) have no
  `OutOfSpace` case and no check of the log line.
- S2: stopping entries are logged while they last, once past 5 minutes (spec.md:47,
  design.md:164). No test.
- S7: a 4 TiB delete ends within 50 s, which is how `k` is sized (spec.md:92–93,
  design.md:227–228). The S7 tests (design.md:466–474) check fairness between turns, not this
  bound.

Fix: name a test for each, for example an engine test with two targets over a small store, and a
store-level `delete_chunks` timing test at 16 384 chunks.

### R1-C7 — Low
**Setting the stop flag does not end a queue's event loop.**
After `run_target` returns `Err`, the design ends the queue loops by a stop flag that makes
`is_done` true, plus each queue's eventfd. `event_loop` breaks only on
`(aborted || failed) && is_done()`. `aborted` comes only from `UBLK_IO_RES_ABORT`, and a device
that never started never gets one. The eventfd's completion reaps normally. So the loop never
exits, and the device thread's wait for every exit hangs. The S10 lab test would hang rather than
pass.
Evidence: design.md:115–122; ublk.rs:1670; libublk `uring_async.rs:414–420`.
Fix: make the stop flag break `event_loop` directly. State that the pending FETCH commands are
cancelled when the ring is torn down at thread exit.

### R1-C8 — Low
**The engine thread's wake-up list leaves out #75's QoS release timer (cross-plan item 1).**
#75 keeps one gate per volume in `TargetRt`, before `Target::submit` and `try_fast_read`, which #15
keeps intact. But #75 arms its release timer through `Engine::take_qos_timer()`, which
`EngineCtx::pump_with` sends on the gather timer's channel. #15 removes that channel, and its
`next_wakeup` returns "all three": the turn, gather/hold, and arrivals. Whichever lands second
leaves held requests waiting for the 10 ms turn. #15 never mentions #75.
Evidence: design.md:168–175; #75 design.md:123–128.
Fix: say that `next_wakeup` covers every engine deadline, #75's gate `due` included, and that
arming any deadline signals the node's eventfd.

### R1-C9 — Low
**The limit error contradicts "every error starts with its volume".**
design.md:55 says every error starts with its volume, then gives `error: limit: N volumes attached
or attaching`, which does not. Spec S1 (spec.md:33) and the S1 test "every error names its volume"
(design.md:422) would fail on it. The CSI parses this prefix (design.md:301).
Fix: fix the wire form, for example `error: VOLUME: limit: N ...`, and say whether the CSI matches
it as a prefix, as it does for `epoch:`.

### R1-C10 — Low
**One `Mutex<Routes>` for the whole node on every submit and completion.**
Today each device has its own `waiters` map (ublk.rs:517). The design replaces it with one node-wide
`Mutex<Routes>` that every device's queue threads and the engine thread take, on each submit and
each routed output. That adds contention between volumes on the steady-state path (AGENTS.md: "takes
no locks"), and S12 measures one volume only.
Evidence: design.md:29–31.
Fix: keep one route map per device, and find the device from the op id (for example a device slot in
its high bits), so that devices share no lock on the I/O path.

### R1-C11 — Low
**`flaminio-io del` always uses DEL_DEV_ASYNC, which fails on the supported 6.6–6.8 kernels.**
The node chooses DEL_DEV_ASYNC by kernel release and falls back to DEL_DEV before 6.9. But
`flaminio-io del` always uses `del_dev_async`. ADR 0003 sets the baseline at 6.6 (Ubuntu 24.04
runs 6.8). There `node.sh down`'s deletion of the recorded devices fails and leaves them behind.
Evidence: design.md:342–352; docs/adr/0003-kernel-baseline.md.
Fix: apply the same release rule in `del`, and test it on Centaur's 6.8.

### Nits
- spec.md:69: "S11 bounds it" should say S12. S11 is tokens and epochs; S12 holds the performance
  bounds.
- decisions.md:83 still says "8 by default (to be approved)", although "Settled at approval" fixes
  it.
- The design never says where `--gather-us` and `--commit-target-mib` are applied once
  `run_device` no longer takes the engine. Today they are set there (ublk.rs:952–957).

Reviewed commit: 976e7d54e7f7e7af712c1979507a73748903a519
