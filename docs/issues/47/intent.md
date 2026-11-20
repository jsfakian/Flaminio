# Issue #47: Gray-failure detection — intent

[Issue #47](https://github.com/jsfakian/Flaminio/issues/47), from the README roadmap (phase 2)
and ARCHITECTURE §11.1, §11.3 and §17.1. This change is the issue's step 1 and the first half of
its step 2: the target measures each replica, judges it against the others, and stops preferring
a slow one for reads. It removes nothing from the ISR. Issue #47 stays open after this change.

## Why

A replica that answers slowly, but answers, is treated as healthy.

1. **Only a failed session removes a member.** A request without a reply for 3 s fails its
   session (`tick_inner`, `flaminio-repl/src/target.rs:811`; `request_timeout`,
   `types.rs:60`; the daemon runs `Timing::default()`, `flaminio-io/src/node.rs:329`), and
   `member_failed` (`target.rs:1300`) then asks for the removal, if the ISR stays at or above
   the minimum. The timeout is fixed, not adaptive as ARCHITECTURE §11.1 says. A reply after
   2.9 s counts as a good one.
2. **Every write waits for the slowest member.** A write completes once every ISR member, and a
   joining replica, has acknowledged it (`check_write`, `target.rs:1421`).
3. **Reads go to a replica chosen without regard to latency.** `source` (`target.rs:1644`)
   returns this node if its replica is a usable ISR member, else the usable member with the
   lowest node id. ARCHITECTURE §7.2's "fewest outstanding I/Os and lowest recent latency" does
   not exist. A chunk without a local replica (after a failover, or with locality off) reads
   from the lowest id for as long as its session holds, however slow it is. A slow local disk
   keeps every read: fast reads are admitted only when `source` is the local node
   (`try_fast_read_inner`, `target.rs:670`).
4. **Nothing measures a replica.** The target holds each request's send time
   (`Session::inflight`, `target.rs:359`) and gets the time of its reply (`on_reply_inner`,
   `target.rs:899`), but uses the pair only for the timeout and the lease. A fast read carries
   no admission time (`FastRead`, `target.rs:121`). There is no histogram anywhere in the data
   plane, and no latency in any status: the control socket's `status` answers the attached
   volume and its device (`flaminio-io/src/control.rs:104`), and the target reports one kind
   of event to log, a failed session (`Event`, `target.rs:132–135`). The `Health` of
   `flaminio-io/src/remote.rs` belongs to the MVP's own volume (`volume.rs`, `cli.rs`), not to
   the node engine.
5. **The lab injects the fault and checks only survival.** `make chaos` delays one node's
   packets for 20 s (`chaos_fault`, `scripts/lab.sh:1411`; `chaos.sh slow on 100ms`: netem on
   `wg0`, so what the node sends). A reply 100 ms late is far from 3 s: the member stays, every
   write to its chunks takes at least 100 ms, and so does every read `source` sends there. The
   run asks only that the load completes without errors (`lab.sh:1446`). No result in
   `docs/benchmarks/` records a load's latency under this fault: it needs measuring, before and
   after.
6. **The fault is a slow link, not a slow disk.** The lab has no disk delay (ARCHITECTURE §19
   names `dm-delay`; no script uses it). A target cannot tell the two apart for another node's
   replica: it sees the request's round trip. For its own local replica, served in process, only
   a slow disk shows, and the lab cannot produce one.
7. **There is no class to compare with.** ARCHITECTURE §11.3 compares a disk with "peers of the
   same device class". The metadata service has no disk record and no device class
   (`api/proto/flaminio/meta/v1/meta.proto`, `meta/domain`): it knows nodes (alive, draining,
   labels). The agent discovers and claims devices (`control/agent/disks.go`), no more. The lab
   has three data nodes and chunks of two replicas: a median of two says nothing.
8. **A removal on latency would be undone at once.** `drive_resync` (`target.rs:2340`) starts a
   resync for any replica out of the ISR whose session is open, and a slow replica's session is
   open. Removed, it would be copied to, joined, judged slow, and removed again. That is why
   the removal is a later step, with its own rule and simulator check.
9. **The simulator already makes links slow** (`slow_links`, `flaminio-repl/src/sim/mod.rs:343`
   and `1169`: 1 to 800 ms more, for 1 to 20 s), so a rule on latency can be checked there.
   It cannot yet make a node's own replica slow: a node's messages to itself take 5 to 50 us
   whatever the fault (`sim/mod.rs:659–660`).

## What we want

1. **The target measures each replica node:** the latency of the writes it sends it, window by
   window, from the times it already has. Nothing is added to the steady-state path
   but the recording of a sample: no lock, no metadata round trip, no extra request.
2. **A replica is compared with the other replicas of the same chunks, on the same writes:**
   every write goes to every ISR member, so the samples are like for like. A replica is slow
   when it stays well above the others and above an absolute floor; with two replicas, "the
   others" is the other one. No device class, no value from outside the target. A chunk with
   one replica is not judged. (Recommended; the spec lists the choice.)
3. **Hysteresis:** a replica becomes slow, and stops being slow, only after the condition held
   for a whole window, with a lower bar to recover than to be judged slow. One slow request, or
   a 20 s fault ending, does not make it flap. Nor does a latency that follows the reads: a
   replica that turns slow again soon after it recovered stays slow longer each time, up to a
   cap.
4. **Reads move away from a slow replica** within two windows, the local one included, when the
   chunk has another usable ISR member that is not slow. Otherwise they go where they go today:
   a read never waits or fails because of a measurement.
5. **Latency changes who is preferred, never who may serve.** The set a read may go to is
   today's: ISR members with an open session whose removal is not in flight, under the lease.
   No ISR change, no metadata request and no timeout depends on the measurement. The simulator
   checks it, with an injected bug (the preference choosing among replicas rather than among
   usable ISR members) that it must catch.
6. **An operator can see it:** each change of judgement is logged with the figures behind it,
   and each replica's latency and judgement can be read from the node on request, the way
   #71 lets observations leave the daemon (its telemetry socket), once #71 has landed.
7. **Checked in the lab:** with one node slowed, the reads of a chunk that went to it move to
   another member within two windows, come back once after the fault ends, and no chunk's ISR
   changes. The load's latency is recorded with the fault, before and after this change.

## Out of scope

- **Removing a slow replica from the ISR** (the rest of step 2). Writes keep waiting for it
  until then: "writes recover once it leaves the ISR" is not met by this change. It needs a rule
  that keeps a removed replica out while it is still slow (Why, 8), a way to judge a replica no
  write goes to, and a simulator check with its injected bug (never below the minimum ISR; no
  flapping that starves resync).
- **Marking a disk `Degraded`** (step 2). There is no disk record or disk state to mark; it
  needs disks and device classes in the metadata service, and the judgement reported there.
- **The class median** across the cluster (ARCHITECTURE §11.3). Same dependency.
- **Auto-drain** (step 3). `control/drain` drains a node on request; no policy starts it.
- **CSI `VolumeCondition`** (step 4). The driver declares no health capability.
- **A read-only load.** With no writes there is nothing like for like to compare: a slow replica
  is then not judged. Probe reads, or spreading reads by outstanding I/Os (ARCHITECTURE §7.2),
  come later.
- **A disk-delay fault in the lab** (`dm-delay` under a node's store). Until then a slow local
  replica is checked in the simulator and the engine's tests only; the simulator gains a delay
  on a node's own replica for it (Why, 9).
- **An adaptive request timeout** (ARCHITECTURE §11.1). The 3 s stay.
- **Metrics export** (Prometheus histograms, ARCHITECTURE §18) and per-StorageClass thresholds:
  the thresholds are the daemon's defaults in this step.
- **The MVP volume path** (`flaminio-io/src/volume.rs`, `remote.rs`).
