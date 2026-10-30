# Issue #15: more than one volume per node — specification

Settled choices and accepted risks: [decisions](decisions.md). How: [design](design.md).
Depends on #10 (built after it) and #30 (epochs across volumes of one name).

Terms:
- a **device** is a ublk device that the node serves one volume on;
- an **entry** is the node's record of one attach. It has the volume, the device's id, the
  attach's **generation** (numbered from 1 in each run) and a state:
  - starting, from its reservation until the device is ready;
  - running;
  - stopping, from a detach, a deposition or the device's end, until the entry goes;
- the **instance** is a random number of 64 bits or more, drawn once per run of the node;
- the **engine thread** is the thread that owns the engine's inputs;
- **node-wide** state is shared by every volume: the store and its space, the group commit, and
  #10's gather, holding and arrivals.

## Behaviour

**S1. Several attaches, up to a limit.**
- **The limit:** the node attaches volumes up to a limit the operator sets. The default is 8 (to
  be approved).
  - **What counts:** every entry, whatever its state, and a device that starts after its attach
    timed out.
  - **Atomic:** taking a slot and checking the limit happen in one step, so N + 1 concurrent
    attaches give N entries at most.
- **Refused before a slot is taken,** and before the metadata service is asked:
  - a name the metadata service would not accept;
  - a token whose signature, volume or node is wrong;
  - the node at its limit, with a stable error prefix;
  - a volume that already has an entry, with the entry's state.
- **Every error names its volume.**
- **Attaches are taken one at a time,** each within #6's deadlines. Detach, delete and status
  never wait for an attach.

**S2. Every device is told apart from every other.**
- **Ids are never reused while the node runs.** The first id is above every ublk id present when
  it starts.
- **A device's marks:**
  - `major:minor`: the device itself, not a file or another device at its path;
  - the instance: this run of the node;
  - the generation: this attach, not an earlier one of the same volume.
- **The node holds every device open,** from ready until the CSI node plugin detaches the volume,
  even after a deposition. So no later device gets its `major:minor` while a mount may still use
  it.
- **Stopping entries** are listed, and logged while they last. Root's detach ends one.
- **Every completion, and every output of the engine,** reaches the generation that it belongs
  to, or is dropped and logged. Op ids are never reused in a run.

**S3. The engine thread.**
- **It runs** from the node's start to its end, devices or not.
- **What it does:** the turn every 10 ms, every input, commit completions, the gather, hold and
  arrivals deadlines (each on time, not at the next turn), and the engine's part of attaches,
  detaches and deletes.
- **What it never does,** nor any thread holding the engine's or the registry's lock: stop,
  delete or wait for a device.
- **Holds are short:** it holds the engine's lock for a bounded batch of inputs. Its longest wait
  for the lock, and its heartbeats' lateness, stay under 100 ms with two busy volumes (to be
  approved), and are logged.
- **Commits** are asynchronous while any device runs, and synchronous on a node with none, as
  today.

**S4. Queue threads, as today.**
- **Writes, and reads the fast path refuses,** are submitted from the queue thread.
- **Fast reads** are admitted, read and ended there, by unchanged rules.
- **What the engine thread now delivers:** commit completions and remote replies. Each costs one
  more wake-up; S11 bounds it.

**S5. One volume's faults stay its own.**
- **What ends only its own volume,** with no other volume's I/O failing: a detach, a deposition,
  a failed or timed-out attach, or a device that stops.
- **A deposed volume's entry** stays stopping until its detach.
- **What one volume can still make others wait for:**
  - **settling:** the commits in flight and queued when it begins. It runs when a peer's session
    closes, and, with #10, on every session's start, another volume's attach included;
  - **its writes in a shared commit** (no fairness);
  - **the disk's space:** a volume that fills it makes other volumes' writes there fail on this
    replica, as today. The node logs which pool ran out.
- **A panic ends the node,** and every volume fails over as on a crash.

**S6. One store, shared.**
- **One group commit** may hold several volumes' writes.
- **A write** is acknowledged once durable on the whole ISR.
- **Batches** stay durable in order.
- **#10's gather, holding and arrivals** are node-wide.
- **A store error** fails every volume's writes on the node.

**S7. Deleting a volume's replicas.**
- **Refused** while the volume has any entry here, a target, or an attach pending.
- **It deletes in turns.** Each turn takes one commit, and another volume's I/O, heartbeats and
  leases go on between turns. A delete of 4 TiB ends within #6's 50 s.
- **While a delete runs,** a session or an attach of that volume is refused. A second delete
  answers that one is under way. The delete goes on past the socket's deadline.
- **A crash during a delete** leaves the remaining chunks fenced against every epoch: no
  session or attach, nor a volume made again under the same name, can use them. A later delete
  ends the work.

**S8. The control socket.**
- **An older CSI node plugin** works unchanged with one volume: the attach answer and the plain
  status keep their format. With several volumes, the plain status names one of them.
- **New:**
  - a per-volume status: the state, and once the device exists its path, `major:minor`,
    instance and generation;
  - a list: the limit and every entry.
- **#6's time limits are unchanged.**

**S9. The CSI node plugin.**
- **`MaxVolumesPerNode`** is the data plane's limit. It is 1 on an older data plane. When the
  data plane does not answer, the call fails.
- **Staging:**
  - starting is `Unavailable`, stopping is `FailedPrecondition`;
  - another attached volume is no longer a refusal.
- **Before it formats, mounts or binds,** the plugin checks that the device it opened has the
  `major:minor` the node gave, and that a second status gives the same instance and generation.
- **After mounting,** it checks the mount's device.
- **A mount it finds already in place** is checked against the volume's status.
- **Any mismatch:** the plugin unmounts, refuses (`FailedPrecondition`), and formats nothing.
- **A raw block publish** keeps its device across restarts of the data plane and of the plugin:
  its `major:minor` never serves another volume while published.
- **The limit refusal** is `ResourceExhausted`, as is an older data plane's "one volume at a
  time".
- **Either can be upgraded first.** A newer plugin on an older data plane falls back to today's
  status, and logs that it skips the identity check.

**S10. Crash and restart.**
- **Every device ends with the process.** Each volume is attached again under a new epoch, or
  fails over.
- **At start, the node deletes only its own leftovers:** recorded, dead, and matching what it
  recorded.
  Others are never touched.
- **The attach at startup** takes one volume, and retries while that volume is not attached.
- **The store's recovery is unchanged.**

**S11. Tokens and epochs.**
- **An attach's token** must be signed, and name the volume, this node and the volume's current
  epoch. This fixes a gap that predates this issue.
- **An attach is refused at an epoch** that already ran a target here in this run, or that is at
  or below the local replica's durable fence for the volume.
  - So after a crash, an attach with the old token fails. Attaching again takes a new token
    from the metadata service.
  - So does a startup retry after its device failed.

**S12. One volume's performance, against main** (bounds to be approved). Measured A/B on fl-b
against main with #10, as each build's share of raw in the same session:
- reads, zero copy: 4K QD1, 4K QD32 and 1M QD8, each at most 3% lower;
- writes: 4K QD1 at RF 1 and RF 2, at most 5% lower;
- 1M QD8 at RF 1: at most 5% lower, and #10's rule (80% of raw across the disk);
- 0 checksum failures, and every end-to-end check clean.

A job whose spread is too large for its bound is repeated once, then reported as inconclusive.

**S13. Unchanged:** fencing, epochs and leases; the fast-read rules and the replication
protocol; the metadata service; the store's format and crash safety; the node's defaults.

## Done when

- Two volumes are attached on one node, under verified fio on both, with distinct data, for 10
  minutes:
  - 0 errors;
  - no second without completions on either volume;
  - S3's maxima logged;
  - each volume's p99 recorded.
- `make node-fault` passes for each of two volumes, failover included.
- Deposing one volume while fio runs on the other leaves the other's fio clean.
- S12's bounds hold, or the owner accepts an inconclusive job.
