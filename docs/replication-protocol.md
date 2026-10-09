# Replication protocol (phase 1)

The rules that keep a replicated volume consistent through replica failures, target failover,
partitions and metadata outages. This refines ARCHITECTURE §8 into something precise enough to
implement and to check by simulation. The reference implementation is the crate
`dataplane/crates/flaminio-repl`. It has no I/O, and its simulator checks every rule below
([§9](#9-how-it-is-checked)).

## 1. Actors and state

| Actor | Durable state | Volatile state |
|---|---|---|
| **Metadata service** (linearizable; etcd) | Per volume: `epoch`, target node, `minInSyncReplicas`. Per chunk: replica nodes, `isr`, `isr_ver` | — |
| **Replica** (one per node that holds chunk replicas) | Per volume: `fenced_epoch`. Per chunk: block contents | Per volume: the current session and the resync filters (§7). Per chunk: the **uncertain set** (§6) |
| **Target** (one per attached volume) | None | Epoch, cached placement and ISR, sessions, in-flight writes, lease confirmations |

Write versions are `(epoch, seq)`. `seq` restarts at 0 in each epoch and increases with every
write the target sends.

Every operation covers an **extent**: a run of blocks within one chunk. A request that spans
chunks is split at chunk boundaries. Locks, uncertain sets, dirty sets and resync filters are
sets of extents. Rules below that speak of "a block" apply to every block of an extent.

## 2. Metadata transitions

There are only two kinds of transition, and both are atomic.

- **`Attach(volume, node)`:** `epoch += 1`, target := node. Returns the new epoch, placement and
  ISR. Called by the failover controller, or for a first attach.
- **`ChangeIsr(volume, epoch, [(chunk, expected_isr_ver, new_isr)])`:** applied only if
  - `epoch` equals the volume's current epoch,
  - every `expected_isr_ver` matches,
  - every `new_isr` is a subset of the chunk's replicas, and
  - every `new_isr` has at least `minInSyncReplicas` members.

  Each changed chunk gets `isr_ver += 1`. Otherwise nothing changes, and the reply carries the
  current epoch and chunk state. A target that sees a higher epoch has been **deposed**.

A retried `ChangeIsr` whose earlier attempt was applied gets a version mismatch. The target
then compares the ISR in the reply with the one it proposed.

## 3. Sessions and fencing

A target talks to each replica node over a **session**: one ordered connection, opened by
`Hello{volume, epoch, session}`. Session numbers increase within an epoch.

- **On `Hello`**, the replica refuses with `Stale` if `epoch < fenced_epoch`. Otherwise it
  durably sets `fenced_epoch := epoch`, makes `(epoch, session)` the volume's current session,
  clears its resync filters, and replies with its uncertain set for every chunk of the volume it
  holds.
- **Every other request** carries `(epoch, session)`:
  - `epoch < fenced_epoch` gets `Stale`;
  - any other session that is not the current one is **dropped** without effect.

  So a message delayed on an old connection can never act after a new session has started.
- **On timeout**, the target treats a request as failed, closes that session, and opens a new
  one with a new number later. Replies on a closed session are ignored.

## 4. Writes

1. A write locks its block (range lock). Reads of the block wait too: two ISR members can
   briefly disagree while a write is in flight.
2. The target assigns `seq` and sends the write to the **write set**: the ISR plus the replicas
   being resynced (§7). Each write carries `acked_below`: every write with a lower `seq` is
   complete.
3. The write **completes** when every member of the target's *current* ISR has acknowledged it.
   Then the client gets its answer and the lock is released.
4. **If an ISR member fails** (timeout, closed session):
   - **if removing it keeps the ISR at or above the minimum,** the target proposes
     `ChangeIsr(ISR − r)`. Writes waiting for `r` complete only **after the change is committed**.
     This is what stops a stale replica from ever being chosen as a source after a failover;
   - **otherwise** the writes block. The target reopens a session to `r` and **resends every
     in-flight write that `r` has not acknowledged**. No write completed while `r` was away, so
     `r` is then complete again.
5. **`Stale` from any replica, or a higher epoch in a metadata reply,** deposes the target. It
   fails all its pending operations and stops.

## 5. Leases: who may serve reads

A target may return a read for a chunk only while **every member of the chunk's lease set**
has confirmed the target's epoch within the last `L`:

- **Lease set:** `ISR ∪ {replicas whose ISR expansion is in flight}`.
- **Confirmation:** a heartbeat or I/O reply to a request sent at local time `t` confirms until
  `t + L`.
- **Read check:** the lease is checked when the reply arrives.

Expansions count from the moment they are proposed. Shrinks count only once they are committed.
So the lease set always contains the ISR that the metadata service holds.

**Read sources.** Reads go only to ISR members **whose removal is not in flight**. A member
being removed may already be out of the metadata service's ISR, so a new target would not fence
it (§6). Under this rule, an old target only ever reads from members of the metadata service's
current ISR.

## 6. Failover: fencing and reconciliation

A new target starts from `Attach`, which bumps the epoch. For each chunk:

1. **Fence.** Send `Hello` to every replica. Each reply fences that replica and returns its
   **uncertain set**: blocks written in the replica's recent window, meaning writes that might
   not have completed. The window is defined below.
2. **Wait for the ISR to be fenced.** The chunk may serve I/O only when **every ISR member is
   fenced**. If some ISR members do not answer, the target waits `L × (1 + ρ)` from the first
   fence of an ISR member, then removes the silent ones with `ChangeIsr` (if the ISR stays at
   or above the minimum; otherwise it keeps waiting). Why the wait is needed:
   - Until every ISR member is fenced, the old target may still hold a valid lease. It can then
     read from an unfenced member.
   - If the new target served reads earlier, the two could return different outcomes for the
     same unfinished write.
   - After the wait, the old lease has expired, because it needed the fenced member's
     confirmation.
3. **Reconcile.** Pick one ISR member as the **authority**. For every block in the union of the
   ISR members' uncertain sets, read the authority's value and write it to the whole ISR as an
   ordinary write of the new epoch. When those writes complete, every ISR member agrees on every
   block. Blocks outside the union already agreed, because their last write completed.
4. **Serve.** The chunk is ready.

**When no wait is needed.** If every ISR member answers, the chunk serves as soon as it is
reconciled. An older target can then neither read nor write:
- it reads only from members of the metadata service's ISR (§5), and every one of them now
  refuses it;
- it cannot complete writes, because every write needs the whole ISR to acknowledge.

**The uncertain set on a replica:**
- **Membership:** a write's extent enters the set, with the write's version `(e, s)`, when the
  write is applied.
- **Pruning:** entries are pruned using the `acked_below` carried by writes. A write with
  `prune = (e, a)` removes entries older than `(e, a)`.
- **Reconciliation writes** carry no `prune`, so a target that dies mid-reconciliation leaves
  the old entries for the next one.
- **Settling.** Once a chunk is reconciled, the target sends every ISR member
  `Settle{below}`, with `below` the version of the chunk's oldest write in flight (nothing is in
  flight at that moment, so `below` is the next version). The replicas drop the entries below
  it. A replica joining the ISR gets the same.
  - The repairs themselves are entries too.
  - Without settling, the next target would reconcile everything again, until client writes
    happened to prune it.
- **The set is volatile.** After a restart, a replica fenced at epoch `f` reports **every block
  of its chunks** as uncertain:
  - it cannot tell which of its recent writes completed;
  - every write it applied had an epoch of at most `f`.

  That lasts until a write prunes, or a `Settle` arrives, with an epoch above `f`. Such a target
  has reconciled the chunk, so everything older is settled. The cost: a failover soon after a replica restart
  reconciles whole chunks. The data plane makes that cheaper by comparing digests first.

## 7. Resync and rejoin

A replica outside the ISR rejoins in four steps:

1. **`BeginResync{chunk}`** on the current session. From then on, the replica remembers which
   blocks of the chunk it received live writes for (the *resync filter*), and the target adds it
   to the write set.
2. **Copy.** For each block to copy, read it from an ISR member and send `Copy{block, value}`.
   The replica **skips a copy for any block in its filter**, because the live write is newer.
   Copies take **no lock**, so a resync never stalls client I/O. The filter is what keeps a copy
   read before a live write from landing after it.
3. **Propose.** Propose `ChangeIsr(ISR + r)` only when:
   - every copy is acknowledged;
   - **`r` has acknowledged every live write sent to it**, including writes that have already
     completed on the ISR without `r`;
   - every write in flight on the chunk has reached `r`.

   `r` joins the lease set and the write quorum at once.
4. **Join.** On commit, `r` is an ISR member.

**Writes in flight at `BeginResync`** are not sent to `r`:
- **No copy can miss them:** a copy reads its source over the same ordered session that
  carried the write, so the read sees it.
- **`r` cannot join early:** it may only join once those writes have completed.

The real data plane's copier pulls from the source directly (ARCHITECTURE §11.4), which breaks
the ordered-session argument. It must send those writes to `r` as well.

**Which blocks to copy:**
- **If this target removed `r` itself:** the blocks of writes in flight to `r` at the removal,
  plus every block written since then.
- **Otherwise:** every block of the chunk. In the real data plane this becomes a digest
  comparison, as in the MVP.

**Bulk work is bounded.** Repairs (§6) and resync copies share each replica's ordered session
with client writes and heartbeats. A target therefore keeps at most a few of them in flight (4
by default, each at most `max_io` blocks). Unbounded, a resync queues so much data ahead of
the heartbeats that their requests time out. The session then fails, the resync aborts, and it
starts over: a livelock. That happened in the lab, not in the first simulator (§10).

A failure of `r` during resync aborts it, with no metadata change. The target keeps its dirty
set and tries again later.

## 8. Timing assumptions

- **Delays:** none are bounded. Messages may be delayed arbitrarily or lost with their
  connection.
- **Clocks:** only the *rate* of local monotonic clocks is bounded: any two run at speeds within
  a factor `1 + ρ`. No clock synchronization.
- **Liveness:** every node eventually comes back and partitions heal. Then all ISRs become full
  again.

## 9. How it is checked

The simulator (`flaminio-repl/src/sim/`) runs the three state machines on one thread, in event
order, from a seed. A seed reproduces a run exactly. Each run has two phases:

- **Chaos.** Each run picks:
  - its topology: 3–4 nodes, RF 2–3, `minInSyncReplicas` from 1 to RF, 1–2 chunks of 2–6
    blocks;
  - operations of 1–3 blocks, and repairs and copies of 1–3 blocks;
  - its timing: lease 2–6 s, clock rates up to 10% apart;
  - its client pace, and a read share of 50, 80 or 95%.

  Faults then arrive every few seconds:
  - crashes and restarts;
  - single-link partitions;
  - isolation of a whole node, half the time the target's node, so that the old target keeps
    serving its client;
  - random multi-link cuts;
  - slow links;
  - message delay spikes;
  - metadata outages.

  The failover controller attaches the volume elsewhere when its target is gone or cut off
  from the metadata service, after a detection delay of 0.1–10 s. It also fails over at random
  moments with the old target alive.
- **Settle and check.** Heal everything, wait until the current target has full ISRs and
  nothing in flight, read every block, and settle again.

**What each run checks:**

1. **Linearizability**, per block (Wing & Gong search). A write that never got an answer may
   take effect or not, but only before the first operation of a later epoch on that block
   completes. So an old, unacknowledged write can never resurface over newer data.
2. **Convergence:** every replica of every chunk holds identical blocks.
3. **Liveness:** every ISR is full, and every operation of the final target has completed.

**Scripted scenarios** reproduce the two split-brain cases exactly, instead of waiting for a
seed to stumble on them:
- an old target cut off from everything but its local replica, still serving its client;
- a failover while an ISR removal is committed but its reply has not reached the old target.

**Mutation checks:** each rule has a test-only switch that breaks it, and a test asserts that
the simulator then fails:

| Broken rule | Caught by (first failing seed) |
|---|---|
| Acknowledge once a removal is proposed, not committed (§4) | seed 27 |
| Accept requests from an old session (§3) | seed 0 |
| No reconciliation after failover (§6) | seed 1 |
| Reconciliation writes prune uncertain sets (§6) | seed 778 |
| Settling before the repairs are done (§6) | seed 71 |
| No lease check on reads (§5) | seed 216, split-brain scenario |
| No takeover wait (§6) | seed 81, split-brain scenario |
| Reads from a member being removed (§5) | removal-in-flight scenario |
| Joiner counts for leases and writes only once committed (§5, §7) | seed 154 |
| Removed member's dirty set omits writes in flight (§7) | seed 14 |
| No resync filter on the replica (§7) | seed 14 |

The seed numbers move whenever the simulator changes, so the tests search up to 20 000 seeds,
and 60 000 for reconciliation pruning. That bug is rare by nature: the target must die between
two repairs that are usually sent together.

**Commands:**
- `make test` runs the unit tests, the scenarios and a 200-seed campaign;
- `make sim` runs 20 000 seeds plus every mutation check, in release, in about a minute;
- `cargo run --release -p flaminio-repl --example sim -- <first> <count>` runs any range of
  seeds;
- `FLAMINIO_SIM_TRACE=chunk,block` prints every message, ISR change and failover touching
  one block.

## 10. What the simulator found

The first draft of the protocol had these flaws, each found by the simulator or by working out
a scenario for it:

1. **A rejoining replica could join the ISR without a write it had missed.**
   - A live write sent to it was lost when its connection broke.
   - The write had already completed on the ISR, so the target's "everything acknowledged"
     check, which looked only at writes in flight, passed.
   - Result: diverging replicas, and a stale read after a later failover.
   - Fix: track every live write sent to the joiner until it acknowledges it (§7).
2. **Old reads during failover** (found while designing the scenarios).
   - An old target with a valid lease could read from a member whose removal was already
     committed, so the new target would not fence it.
   - Fix: never read from members being removed (§5). With that rule, the earlier "fence every
     replica, not only the ISR" condition is unnecessary and was dropped.
3. **Resync copies held block locks**, which made the replica's resync filter dead code that no
   test could exercise.
   - Copies are now lock-free, as ARCHITECTURE §11.4 describes, and the filter is checked.
4. **Liveness:** reconciliation work could queue behind client operations that were waiting for
   it. Only client operations are now ordered per block.
5. **Livelock under real bandwidth** (found in the lab, step 4e). A resync issued all its
   copies at once.
   - Over 1 GbE, gigabytes queued ahead of the heartbeats on the replica's session.
   - The heartbeats timed out, the session failed, the resync aborted, and it started again,
     every 8 s.
   - The simulator's links have no bandwidth, so it could not see this.
   - Fix: repairs and copies in flight are bounded (§7). The simulator now runs with bounds
     of 1–4, which also made the reconciliation-pruning mutation easier to catch.
6. **Reconciling again and again** (found in the lab, step 4d). After a replica restart, every
   later failover reconciled the whole volume (16 GiB, 150–260 s before the first read).
   - The restart marker, and the repairs' own entries, cleared only through client writes.
   - The simulator's chunks are a few blocks, so a needless reconciliation costs it nothing:
     a performance bug cannot show there.
   - Fix: `Settle` (§6). A move between running nodes now serves its first read in 15 ms.
     Its mutation (settling before the repairs are done) is caught.

**Results:** 320 000 random seeds pass (on average 4 failovers, 2.5 crashes and 7 partitions per
run), all scenarios pass, and every mutation above is caught.
