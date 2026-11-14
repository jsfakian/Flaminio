# Issue #15: more than one volume per node — intent

[Issue #15](https://github.com/jsfakian/Flaminio/issues/15), follow-ups §5, the
[performance plan](../../performance-plan.md) (R3, R4).
Settled choices and accepted risks: [decisions](decisions.md). Depends on #10 and #30 (a
volume created again starts above every earlier epoch of its name).

## Why

A node serves one attached volume at a time. Follow-ups §5 gives the reason as the engine living
on the attached device's queue thread, for the zero-copy local read path. The code says more.

1. **Only one device can run, and the node says so everywhere.**
   - **The main thread** drives the engine while no volume is attached (`serve_node` in
     `flaminio-io/src/ublk.rs`, `drive_until` in 50 ms slices), and takes control commands
     between slices. An attach (`attach_and_serve`, `run_device`) moves the engine out of the
     main thread's `Slot` into the device's `Shared`, and then blocks in libublk's `run_target`
     until the device stops. No other attach is taken meanwhile.
   - **The control socket** (`control.rs`, `serve`) refuses an attach while one volume is
     attached: "one volume at a time". `Attached` holds one `(volume, id)`, and `status`
     answers one.
   - **The device id:** `--id` is one id, used by every device (`add_device`, which retries
     while a deposed device with that id goes away). The lab starts nodes with `--id 1`, and
     `node.sh down` deletes only device 1 (`scripts/guest/node.sh`).
   - **The scripts hard-code `/dev/ublkb1`:** `node-fault`, `drain-test`, `chaos`,
     `partition`, `bench-reads` and `bench-writes` (`scripts/lab.sh`, lines 739, 759, 790,
     1251, 1267, 1411, 1434, 1513–1560, 1665–1674 and 1808).
   - **The CSI node plugin** (`control/csi/node.go`): `NodeGetInfo` reports
     `MaxVolumesPerNode: 1`, `device` refuses a second volume with `ResourceExhausted`, and one
     mutex serialises every operation on the node. It formats, mounts and binds the path the
     data plane named, without checking which device that is (`node.go`, 91–125 and 180–197).
2. **The engine already holds several targets; the frontend does not.**
   - `Engine` (`flaminio-node/src/engine.rs`) keeps its targets in a map, and refuses an attach
     only of a volume it already has. Its tests already run targets with no device
     (`tests/engine.rs`, `start_node` and `engine.run`).
   - **One store per node:** one `ReplicaService` over it (`node.rs`, `build`). Its local
     commits already gather requests from every target and from other nodes' targets into one
     group commit (`flush_local`, `committing`). Its data clusters and metadata page pool are
     node-wide (`store.rs`, `OutOfSpace`).
   - **Completions are routed per device.** `EngineCtx::pump_with` takes all the engine's
     outputs, but `Shared.waiters` knows only its own device's requests. Op ids come from a
     per-device counter that starts at 1 (`Shared.next_op`), and `Output::Done` carries no
     volume. A second device's completions would be dropped, or delivered to another device's
     request with the same op id.
   - **`Deposed`** stops the device only if it names the pumping device's volume. Any other
     volume's is logged and dropped.
   - **The engine's periodic work runs on device threads:**
     - the 10 ms ticker (timeouts, heartbeats, leases) runs on queue 0 (`serve_with`);
     - queue 0 also waits on the node's eventfd, which the network and metadata threads
       signal, and which the store's commit ring signals on each completion
       (`notify_commits`);
     - every queue has its own gather timer.

     Without a device, the main thread does this work, with synchronous commits:
     `run_device` turns asynchronous commits off when its device stops.
   - **No engine test** runs two targets at once.
3. **What the engine on the queue thread buys.** The engine is behind a lock in `Shared`, and each
   queue thread calls it directly (results of
   [R1–R6](../../benchmarks/2026-10-10-reads-r1-r6.md), R4).
   - **Writes:** `EngineCtx::submit` hands a write to the engine on the queue thread, and the
     pump at the end of each event-loop round sends the round's writes to the store together:
     one group commit, with no thread crossed. A commit's completion wakes queue 0, which pumps.
   - **Fast reads (R3):** `Engine::try_fast_read` admits the read, by the target's rules
     (`Target::try_fast_read`) and the local replica's (`ReplicaService::admit_read`). Then
     `plan_fast_read` plans it, the queue's ring reads it, and `finish_fast_read` ends it. All
     of it runs on the queue thread, under the engine's lock. R3 took 4K QD32 from 88% to 95% of
     raw.
   - **The "per-queue admission" of item 1.2 was not built as planned.** The plan was a
     lock-free check per queue, against state that each chunk publishes. R4's queues instead
     share the engine under its lock, held only between awaits. So each queue thread already
     admits its own local reads (the issue's step 2), but by taking the engine's lock.
   - **The lock stalls everything while it is held.**
     - `settle_local` holds it while it waits for the commits in flight to land
       (`finish_commit(wait)`, engine.rs 1082–1087). It runs when a peer's session closes, as in
       the node fault test, and on `InspectLocal` and `DeleteReplicas`. On #10's branch, every
       `Hello` settles too, including a local attach's.
     - `delete_volume` deletes every chunk of a volume in one call.
     - The standard mutex is not fair.
     - The inputs are drained without limit on Linux (`pump_with`, `drive_until`).
4. **What moving the engine's work off the queue thread costs has not been measured.**
   - **The nearest figure** ([issue #9's results](../../benchmarks/2026-10-29-reads-qd1-queues.md)):
     serving a read on another CPU than the submitter's costs about 2 µs more than on the same
     CPU (3.1–3.5 µs, less 1.2 µs for the I/O scheduler).
   - **Against a read:** a 4K QD1 read takes about 76 µs on one queue.
   - **Against a write:** a 4K QD1 write with O_DSYNC takes about 2.5 ms on RF 1 (396–409 IOPS,
     performance plan, W4.1–W4.2).
   - **Benchmark noise is of the same order as the differences to find.** Between campaigns, raw
     4K QD32 O_DSYNC was 8 823, 8 344 and 7 927 IOPS (performance plan). Raw 1M across the disk
     was about 984 MiB/s in #10's profile, and about 600 on another day of #10's campaign. A
     raw reference that moved between regions once gave a 35.9% spread
     (`2026-10-29-reads-qd1-queues.md`, line 28).
5. **Issue #10 makes more state per device.** Its design is on the #10 branch only
   (`issue/10-1m-writes`, `docs/issues/10/design.md`).
   - **Per device:** an arrivals counter, which `run_device` gives the engine
     (`Engine::set_arrivals`).
   - **Commit settings** are applied in `run_device`: asynchronous commits, the gather, and
     holding by default.
   - **The 8 ms limit on holding for arrivals** (`ARRIVALS_LIMIT`) is armed by no timer:
     `next_wakeup` leaves it out, and queue 0's 10 ms ticker is its backstop.

   The group commit is per node, so with several devices all of these become per node.
6. **Memory and CPU per device.**
   - **Buffers:** each queue has its own, depth × the largest I/O: 64 MiB at the defaults (128 ×
     512 KiB, issue #9). In copy mode they are the tags' buffers. With zero copy, each queue has a
     staging memfd of the same size (`serve_with`). Both are virtual until touched.
   - **CPU:** each device has its own queue threads. With `--poll-us`, each busy queue spins a
     core.
7. **An attach does not check its token's signature** (a gap that predates this issue). On main,
   after #30, `start_target` decodes the token with `decode_unverified`, and checks its volume,
   node and epoch against the metadata service's answer; the local replica takes its first epoch
   from it (engine.rs 1610–1621: "Its signature is not checked here (#15)"). `flaminio-node`
   enables `flaminio-net`'s `unverified` feature for this alone. The engine has no token key:
   only the network thread does (`ServerSide`, net.rs). With one volume per node, the control
   socket's root-only mode was the only guard. With several, a token that was not signed for a
   volume must not start a target for it. (Restated against main after approval, R1-S9.)
8. **Issue #17 needs targets without devices.** A drain moves only attached volumes, because a
   volume's target carries out its moves (follow-ups §11). Today a target's turns come from its
   device's queue 0, or from the main thread while it holds the engine.

## What we want

1. **Several volumes attached on one node,** each on its own ublk device, up to a limit the node
   states.
2. **Each volume's faults stay its own:** a detach, a deposition, a failed attach, or a stopped
   device ends that volume only. A device is never mistaken for another, by the node or by the
   CSI node plugin.
3. **An engine thread** that runs the engine's own work, devices or not. Each device's queue
   threads still submit their writes, and serve their local reads, themselves, as today.
4. **The control socket and the CSI node plugin** serve several volumes per node, and can be
   upgraded in either order.
5. **One volume's performance unchanged,** within bounds that the noise above allows, measured
   A/B.
6. **Done when:** two volumes are attached on one node, under fio on both, and the node fault
   test (`make node-fault`) passes for each.

## Out of scope

- **#17's maintenance target:** an attach without a device on the control socket, and the
  drain's flow. With the engine thread, a target's turns no longer need a device (spec S10).
- **A lock-free admission check,** with per-chunk state published to the queue threads (R3's
  original plan). The fast-read rules stay as built: the target's and the local replica's
  checks, under the engine's lock. If the bounds of the spec are not met, this is the next step.
  It changes who decides, so it goes into the simulator first.
- **Asynchronous commits on nodes that serve only replicas.** They keep synchronous commits
  here, as today. Turning them on is a follow-up, once the full RF 2 set (4K QD1, 4K QD32, the
  70/30 mix, 1M) is measured with it.
- **Attaches whose metadata phases overlap.** Attaches stay one at a time (spec S7). With 8
  stages at once, the last may pass kubelet's deadline and be retried. After a restart, later
  attaches may expire. Overlapping the metadata phase is a follow-up.
- **Fairness, QoS and space reservations between volumes.** Writes of several volumes share the
  node's group commits, and its disk space. One volume's large writes can delay another's small
  ones by a commit. A thin volume that fills the disk fails other volumes' writes to it (as
  today for replicas). The spec states the behaviour and tests it, and nothing more.
- **The default queue count, depth and I/O size:** unchanged. So is placing queue threads on CPUs
  across devices.
- **Surviving a data-plane restart with the devices alive** (`UBLK_F_USER_RECOVERY`). A restart
  ends every device, as it ends the one today.
- **More than one Flaminio disk per node** (the node agent claims one, `control/agent/disks.go`).
- **Attaching several volumes at startup:** `--attach` still takes one.
- **One lock per volume in the CSI node plugin.** Its one lock stays. Attaches are taken one at a
  time by the data plane anyway.
- **Who may delete a volume's replicas** (a gap that predates this issue): an accepted risk,
  left to the node agent's work (#12) (decisions.md).
- **Hardening the control socket** (a gap that predates this issue): a read timeout, and a cap
  on connections. The socket is root-only, and #12 moves it.
