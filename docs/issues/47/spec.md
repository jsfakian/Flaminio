# Issue #47: Gray-failure detection — specification

Step 1 and the first half of step 2 of the issue ([intent](intent.md)): measure, judge, prefer
for reads. Settled choices and named risks: [decisions](decisions.md). How:
[design](design.md).
Changed after approval, in round 1 of the plan review (2026-11-17; see decisions.md): the
Terms, S1 to S6, S12 to S15 and the Tests; in round 2: the Terms, S7 and the Tests.

## Terms

- **Node:** a node holding replicas of the volume's chunks, the target's own included. A node
  has one disk today, so a node's judgement is its disk's, or its link's: the target cannot
  tell them apart. Nor can it tell its own slow link from slow peers: its local replica is
  served in process, so with the target's link slow every other node is late against the local
  one (S12).
- **Usable member** (of a chunk): an ISR member a read may go to today: its session open, its
  removal not in flight.
- **Sample:** one write started while its chunk has two or more ISR members. Its members are
  those ISR members, fewer as members leave (S1). A member's time runs from the write's start
  to that member's acknowledgement, on the target's clock.
- **Behind** (a member, on a sample): its time is above the bar: the floor, or `k` times the
  median of the other members' times on that sample, whichever is larger. With one other
  member, that member's time.
- **Late** (a member, on a sample): behind, from the moment its time passes the bar until it
  acknowledges. A node's **late time** in a window is the part of the window during which it
  was late on some sample. It is counted as a lower bound: where late writes are
  acknowledged out of order it may be undercounted, never overcounted.
- **Window:** the target's clock cut into periods of one length.
- **Over, under, unjudged** (a node, in a window):
  - over: it has the least number of samples and is behind on more than the upper share of
    them; or it is late for more than the upper share of the window;
  - under: it has the least number of samples, is behind on fewer than the lower share of
    them, and is late for less than the lower share of the window;
  - unjudged: it has fewer samples than the least number and is not over.
- **Slow:** the target's judgement of a node (S3). Its **hold** is the number of windows a
  slow node stays slow at least.

## The invariant

The measurement changes one thing: the order in which a chunk's usable members are tried for a
read. Which members are usable, the lease, the ISR, every metadata request, every timeout and
every write are what they would be without it.

## Behaviour

### Measuring

**S1. Samples.** The target records a sample for every write it starts on a chunk with two or
more ISR members: client writes and reconciliation's repairs alike. The local replica is a
member like the others; a joining replica is not one.
- A member's time runs from the write's start, whatever happens to its session: a write sent
  again on a new session is not timed from the second send.
- A member that leaves the chunk's ISR or its replica list (a committed shrink, a move's drop)
  leaves the sample, acknowledged or not. A sample left with one member is dropped: with two
  replicas, the whole sample.
- A sample is complete once every member it still has has acknowledged. It is then counted
  once, in the window open at that moment, even if its write still waits for a joining replica.
- A write to one member, a resync copy, a read and a heartbeat are not samples.

(Follows Q1 and Q3.)

**S2. Unacknowledged writes.** At a window's end, a sample that is not complete, and that a
member has acknowledged, counts in that window with each unacknowledged member at its age so
far; and again in each later window, until it is complete. So a member that stops answering is
counted before its request times out, in every window that ends before its removal commits.
Once the removal commits it has left the sample (S1): with the default window (5 s) and
request timeout (3 s), a member that stalls early in a window is removed before the window
ends, and that stall leaves no sample of it. It is then no usable member either, so no read
is preferred to it.

**S3. The judgement, with hysteresis.** At each window's end every node is over, under, neither
or unjudged, by that window's samples alone. A node not slow becomes slow when it is over. A
slow node stops being slow when it is under and has been slow for its hold. Otherwise it stays
as it is. So a node changes at most once per window, and a window between the two shares
changes nothing.

The hold is one window when a node becomes slow for the first time, or later than the cap's
length after it last stopped being slow. When it becomes slow again within that length, its
hold is twice its last one, up to the cap. So a node whose latency follows the reads (slow
while it serves them, quick once they have left) changes less and less often: each time in a
row it stays slow at least twice as long, and once its hold is at the cap it changes at most
twice in any span of the cap's length.

**S4. Lapse.** A slow node unjudged for the lapse (a set number of windows in a row) stops being
slow, whatever its hold: without writes there is no evidence either way, and reads return to
today's order. (Follows Q3.)

**S5. Scope.** The judgement is one target's, of one node, in memory.
- A new target (an attach, a failover) starts with no node slow and no hold. A session's
  failure or reopening does not change it.
- A node that holds none of the volume's chunks any more is forgotten, with its judgement and
  its hold. If it holds one again, it starts as on a new target.
- When the target's timer runs more than one window after it last ran (the target was not
  run, or its clock jumped), the open window and every window passed since are unjudged, and
  the samples in flight are dropped: a jump changes no judgement, short of the lapse.

**S6. Cost.** A write's send and acknowledgement record its sample and do nothing else: no
lock, no metadata request, no message, no memory that grows with the I/O rate. Judgements are
taken on the target's timer, never per request. The timer is due at each window's end, and
the ages of S2 are taken at the window's end, whenever the timer runs. With no node slow,
reads and writes perform as before, within the benchmarks' spread.

### Reads

**S7. Preference.** A read goes to the first of: the chunk's usable members that are not slow,
in today's order (the local node, then the lowest id); then, if there is none, its usable
members in today's order. The same choice serves client reads and the reads of repairs and
resync copies, as today, and the member a resync asks for its digest and its `Mapped` probe.
(Follows Q5.)

**S8. A read never waits or fails for a judgement.** A read that would start today starts. A
read that would end as unreadable does. A read that failed on a member avoids it as today
(docs/issues/5); the preference orders what is left.

**S9. The local replica.** When the local node is slow and the chunk has another usable member
that is not, the chunk's reads go to that member through the queue, and no fast read of the
chunk is admitted. A chunk whose only usable member is the local one keeps its fast reads.
(Follows Q2.)

**S10. Reads under way.** A change of judgement redirects new reads only. A read in flight is
not sent again, and a fast read under way ends by today's conditions: it is not redone because
the judgement changed.

**S11. Unchanged.**
- A slow node stays in the ISR, receives every write, and is waited for by every write.
- A session fails after the request timeout (3 s), and a failed member is removed as today.
- A replica out of the ISR is resynced as today, slow or not.
- The replication protocol's messages, the metadata API, the on-disk format, the MVP path.

### Seeing it

**S12. Log.** Each change of judgement is logged once: the volume, the node, the new judgement
and its hold, and the window's figures behind it (samples, how many behind, the late time, and
estimates of the node's and the others' median times). An estimate is never below the true
median and less than a quarter above it. With the target's own link slow the log names the
peers (Terms): with two replicas and a local one, the healthy remote node; with three, nobody.

**S13. Report on request.** For each target it runs, the node reports, for each of the
target's nodes: slow or not and for how long, and the last whole window's samples, how many
were behind, and estimates (as in S12) of the median and 99th percentile of its times.
- The report leaves the daemon the way #71's S3 lets observations leave it: in the `observe`
  snapshot on the telemetry socket, under #71's bounds, never on the control socket. Asking
  changes nothing and holds up no I/O.
- A target with more than 32 nodes lists 32, the slow ones first, and says how many it has
  and how many are slow.
- Until #71's socket exists, and on a node without telemetry (off, or under #74's
  `--privsep`), there is no report: S12's log is what an operator has.
- Issue #62's bytes per node belong on the same path, not on a report of their own.

**S14. Thresholds** (the floor, `k`, the two shares, the least number, the window's length, the
lapse, the hold's cap). The daemon's defaults, the same for every volume; tests set them, and
may turn the judgement off. Their values are the design's, within Q4.

**S15. Docs.** `docs/lab.md`: the report and how to read it, the lab check, and what a slow
link at the target's own node looks like (S12). ARCHITECTURE §7.2 and §11.3: a line each on
what this step does (compared with the chunk's other replicas; reads only).

## Tests

- **S1, S2 (unit, the target on a scripted clock):** which requests are samples; behind with
  two and with three members, at the floor and at `k`; a peer acknowledging in 0 us does not
  make a member at the floor behind; a write sent again on a new session is timed from its
  start; an unacknowledged write counted at its age in each window until it completes; a
  complete sample counted once though its write still waits for a joiner.
  - **The departed member.** Three replicas, a 5 s window: C stops answering 1 s into a window
    with 8 writes in flight that A and B acknowledge, and its removal commits before the
    window's end: A and B have 8 samples each, none behind, and C has none. The same stall 3 s
    into a window: at its end C has 8 samples, all behind, and is late for the 2 s less the
    bar; once the removal commits they are not counted for C again. Two replicas: the removal
    drops the 8 samples whole. A move's drop does the same.
  - **Late time.** A member frozen for 2.8 s of each window (the request timeout set above
    it), behind on under 1% of its samples, is over. Frozen for 0.4 s, it is under. Two
    writes late over the same span count that span once.
- **S3–S5 (unit, scripted clock):** a fault of four windows gives exactly two changes (slow,
  then not); times alternating within a window, or between the two shares, give none; too few
  samples give none; the lapse, which ends a hold; a new target starts clean.
  - **The hold.** Two members, the one that serves the reads late: over 64 windows with a cap
    of 8, each node changes at most 18 times, and at most twice in any 8 windows once its hold
    is at the cap. Without the hold it changes every window.
  - A node that leaves the volume and returns starts clean.
  - A clock jump of two windows with writes in flight changes no judgement.
  - The target's own link slow: with two replicas and a local one the remote node becomes
    slow; with three, no node does.
- **S6:** unit: a judgement produces no message and no metadata request; with no other timer
  due, the target's next wake-up is the window's end, and ages are taken at it; with the
  judgement off, the target's outputs and wake-ups are today's. Lab, later: the read and write
  benchmarks with no fault, by `docs/performance-plan.md`'s measurement rules.
- **S7–S10 (unit):** the order with none, one and every usable member slow; a member being
  removed or without a session is never chosen, slow or not; the retry rule with a slow member;
  a slow local node with and without another member; a judgement changing under a fast read
  and under a queued read.
- **S9 (engine):** a slow local device: reads of its chunks leave the fast path and are served
  by the other node; they return after the fault.
- **The invariant, S9 and S10 (simulator):** a campaign of its own, with the judgement on,
  thresholds low enough for its slow links to make nodes slow, fast reads on, and a delay on a
  node's own replica among its faults. The existing campaigns run with the judgement off and
  keep their histories.
  - Its report counts redirected reads, fast reads refused because the local node is slow, and
    fast reads completed across a change of the local node's judgement: each above zero.
  - It checks, at each choice, that the source (a queued read's member, the member a resync
    asks, a fast read's local replica) is a usable member of its chunk, and the history as
    before. The check runs only with the judgement on.
  - **Injected bug:** the preference chooses among the chunk's replicas with an open session,
    not among its usable members. The simulator must catch it on a queued read and on a fast
    read: one check for each, passing only on its own failure.
- **S11 (unit):** a member answering every write after 2.9 s is judged slow; no ISR change is
  proposed, no session fails, and every write completes only after its acknowledgement.
- **S12 (engine):** the log line, once per change, with its figures.
- **S13:** unit: the report's fields and ages, while I/O runs; the 32-node bound, the slow
  nodes first, and its totals; an estimate against the true order statistic. The telemetry
  line: its golden line, the lines the decoder refuses, and its length at every bound.
- **S15 (script test):** the lab check killed during the fault leaves no netem qdisc after the
  margin.
- **The lab, a later step:** a target with chunks that have no local replica, a mixed read and
  write load, and `chaos.sh slow on 100ms`, for several windows, on the node their reads go to.
  - The load's read and write latency with the fault, before this change and after it.
  - Those reads move to the chunks' other member within two windows of the fault's start, and
    return once, within two windows of its end; the log shows two changes for the node. Where
    the reads go is read from the bytes each replica node sends.
  - No chunk's ISR changes, and writes stay as slow as before: that is the later step.
  - `make chaos` passes.

## Open questions

Each rule above follows the recommendation.

- **Q1. What a replica is compared against.** (a) The other ISR members of the same chunk, on
  the same write, with an absolute floor (S1–S3): it needs no device class and works with two
  replicas, but departs from §11.3's wording (p99 against a class median). (b) An absolute
  threshold only. (c) Wait for disks and device classes in the metadata service. *Recommended:
  (a);* (c) becomes the cluster-wide check of a later step.
  Settled at approval, 2026-11-13: (a), with (c) as the cluster-wide check of a later step (see
  decisions.md, Settled at approval).
- **Q2. May the local replica lose its reads?** Yes (S9): a slow local disk is where reads
  suffer most, at the price of that chunk's fast reads while it is slow. Or no: only other
  nodes are demoted. *Recommended: yes.* The lab cannot slow a disk: simulator and engine only.
  Settled at approval, 2026-11-13: yes (see decisions.md, Settled at approval).
- **Q3. A load without writes.** Not judged, and a slow judgement lapses (S1, S4). Or judge on
  reads and heartbeats too: not like for like, and heartbeats do not touch the disk.
  *Recommended: not judged in this step.*
  Settled at approval, 2026-11-13: not judged in this step (see decisions.md, Settled at
  approval).
- **Q4. The thresholds.** The design fixes them; none is per StorageClass yet. *Recommended:* a
  window of about 5 s, so that `make chaos`'s 20 s fault is judged and recovered from within
  the run; `k` and the floor from the lab's healthy RF 2 write times, which need measuring.
  Settled at approval, 2026-11-13: the design's defaults (a 5 s window); `k` and the floor
  provisional until the healthy RF 2 write times are measured (see decisions.md, Settled at
  approval).
- **Q5. Bulk reads.** Repairs and resync copies follow the preference (S7), as they follow the
  same choice today. Or client reads only. *Recommended: the same.*
  Settled at approval, 2026-11-13: the same preference for bulk reads (see decisions.md,
  Settled at approval).
- **Q6. The lab fault.** A slow link (`chaos.sh slow`) stands for a slow disk in this step; a
  `dm-delay` fault is its own change. *Recommended: accept.*
  Settled at approval, 2026-11-13: accepted (see decisions.md, Settled at approval).
