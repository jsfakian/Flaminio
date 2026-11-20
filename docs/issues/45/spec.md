# Issue #45: Many-to-many rebuild — specification

Steps 1 and 2 of the issue, at their smallest ([intent](intent.md)). Decisions and design come
after approval. Assumes #13, #17 (and #15 under it), #18, #72 and #78 as planned. Rules marked
(Q*n*) follow the recommendation of an open question.
Changed after approval, in round 1 of the plan review (2026-11-14; see decisions.md): the Terms,
the invariant, S4, S6, S8, S9, S11, S12, S15, S17, S19, the Tests and Q5; in round 2: S4, S11,
S12, S15, S20 and the Tests; in round 3: S7, S20 and the Tests; in rounds 4 to 7: S5.

## Terms

- **State:** a data node is active, missing or failed. Witnesses have none.
- **Missing:** its liveness was seen lapsed; the record says since when. **The wait:**
  `replicaReplenishmentWait`, 10 minutes by default.
- **Failed:** missing longer than the wait, or declared by an administrator. **Admit:** an
  administrator's record that a failed node is active again.
- **Lost replica:** a chunk's replica on a failed node. **Redundancy** of a chunk: its ISR
  members on active nodes, as the records say.
- **The rebuild:** what asks for new replicas. **Rebuild move:** a move (protocol §7) off a
  lost replica that the rebuild requested.
- **Held, maintenance attach, unhold:** as #17. **Eligible, collides:** as #18. **Released,
  forgotten:** as #13.
- **Failed for good:** an administrator's declaration that a failed node will not return: #13's
  forgetting of it (S19 of #13).
- **Blocked:** a lost replica that the rebuild cannot give a move, or whose move cannot go on.

## The invariant

A node's state says where replicas should be, never which data is good. No ISR, ISR version,
minimum, replica set or fence changes because of it, and no client's target is deposed. All the
rebuild writes are move requests and maintenance attaches: each is safe at any moment, for any
node, alive or not. Sessions, reads, acknowledgements and leases never read a state. So a wrong
declaration costs copies, never an acknowledged write, and never allows a stale read. The one
declaration that writes more is failed for good: it is #13's forgetting, which takes the node
out of releases, fence-holder lists and tombstones, and whose safety is #13's (its S19).

## Behaviour

### Node states

**S1. Three states.** Every registered data node has one, and `flaminio-meta nodes` shows it,
with since when and why. A node not yet alive since it registered is active.

**S2. Missing.** An active node whose liveness is seen lapsed becomes missing, in one
conditional write that records the time (Q3). The first observer wins.

**S3. Back in time.** A missing node that heartbeats before it is failed is active again.
Nothing was asked for it. Its replicas rejoin by the ordinary resync, as today.

**S4. Failed.** A node missing longer than the wait becomes failed, in one conditional write:
a heartbeat that committed first wins. An administrator may declare any data node failed at
once, and a failed node failed for good. A node that is not failed, or that is alive, is not
declared failed for good, and nothing else forgets a node: #13's own forgetting makes the same
checks. Failed is no evidence that a node is gone (a partition gives it too). The record says
when, why and by whom. The wait is one setting of the cluster.

**S5. An outage of the metadata service fails no node.** The wait counts only time in which
the service could have received the node's heartbeat: by the observed-opportunity definition
(decisions), time in which the rebuild saw other active nodes' heartbeats commit. So up to 6 s
of each outage counts: the interval of the tick that sees the last heartbeat before it, at most
1 s, plus the 5 s after that sighting.

**S6. A failed node stays failed (Q2).** Its heartbeats and registrations are accepted as today
and change no state. Only an admit makes it active; one that is not alive is then missing. A node
failed for good is never admitted.

**S7. A failed node takes no new replica.** Creation does not place on it. The drain and the
rebuild do not choose it, as a destination or for a new maintenance attach (one standing stands,
as #17 says). A move to it is refused, for every caller, with the reason.

**S8. Callers.** Declaring, admitting and declaring failed for good: an administrator, logged
with the caller and the node. A node changes its own state only by S3, and only through its data
plane's heartbeat: its agent's identity may not heartbeat.

### The rebuild

**S9. What it asks.** For each lost replica whose chunk has no move pending, a move off the
failed node to a destination (S11), whether or not the node is marked draining: a node failed for
good is marked draining for good (#13), a stopped drain leaves its mark, and a chunk has one move
pending at most, so a drain and the rebuild never both record one. It asks nothing for an active
or missing node. A chunk with another move pending waits for that move. It asks no move #13
refuses (#13's S16): a chunk at #13's bound, or with only nodes releasing it left, waits (S15). Of
a volume not held, it asks no move for a chunk whose ISR has fewer members on alive nodes than
the minimum (S15): no new target could carry it out.

**S10. Lowest redundancy first.** In each pass, moves and maintenance attaches are asked in
order of redundancy, lowest first, then volume and chunk. This orders the asking only; each
target carries its moves out at its own pace, as in a drain (Q7).

**S11. The destination** is the one #18's `choose` gives, under `strict` and `bestEffort` and
with #13's order (the chunk's released nodes are not eligible), among nodes that are also
active. The staying replicas are the chunk's replicas other than the lost one. A proposal the
service refuses is handled as #18's S14 says: a collision blocks the chunk for the pass, and the
destination stays a candidate; any other refusal (#72's room or report, a destination failed,
draining or forgotten since the pass read it) takes the destination out of the pass, and `choose`
runs again. A refused chunk is blocked, for the pass and the back-off after it (S15); the pass
goes on.

**S12. Who carries it out.** A held volume: its client's target, as today. A volume not held,
with a move pending, every pending move being one a new target can carry out (none at the
minimum by S15's second case, none whose destination is lost): the rebuild asks for a
maintenance attach unless one stands, whoever asked for it. A standing one stands while it names
a node #17's S16 allows; a new ask goes to a node #17's S16 allows that is also active, and asks
again only as #17's S17 says. A refusal because the volume became held is not an error.

**S13. Carrying out is unchanged.** The target adds the destination, resyncs it from an ISR
member and drops the lost replica, by `ChangeIsr` at its epoch. The drop releases the source
(#13).

**S14. A failed node that returns** holds what the records place on it, like any node.
Targets open sessions with it by the existing rules: outside an ISR it is resynced, inside one
it answers again, and its pending moves go on and drop it. No move is withdrawn. Copies no
record names any more are #13's. Whether its store is the one that earned its replicas is not
this change's to check: that is disk identity (Q1), which this rule neither needs nor replaces.

**S15. Blocked chunks do not stop the rest.** Each is considered again at every pass; one whose
move the service refused is asked again after a back-off. Each has one of:
- **no destination:** no eligible active node; or under `strict` every one is refused by #18's
  S5, with `domain.Strict`'s reason (`replica "x" has no zone label`); or every candidate was
  refused by the service in this pass (S11), with the last refusal (#72's no room or no capacity
  report among them);
- **releases** (#13; the chunk waits): `holding 2 releases of chunk C`; `only nodes releasing it
  are left`;
- **no target:** held, and the target's node is not alive; this includes the failed node
  itself (Q5);
- **no maintenance node** (#17);
- **at the ISR minimum:** the lost replica is in the ISR, which is no larger than the minimum:
  its move is asked, and may wait for the node while a client's write waits (Q6); or fewer of
  the ISR's members than the minimum are on alive nodes, so no new target gets past fencing: of
  a volume not held, no move is asked for it (S9);
- **destination lost:** the pending move's destination is missing or failed.

A volume not held with a pending move of either of the last two kinds gets no maintenance
attach (S12), since its target could never end and would keep one of #17's slots: its other
chunks wait too, reported with that reason. Blocked chunks of one volume do not stop the
others'.

**S16. The report.** A failed node is re-protected when no chunk names it as a replica or in a
move; it stays failed (S6). Until then, per failed node: lost replicas left, by redundancy;
moves pending; blocked chunks by reason, with their volumes. Each change of state, each request
and each newly blocked chunk is logged.

**S17. It keeps nothing but the records.** It runs in every metadata instance and acts in one at
a time: the one that holds the rebuild's ownership. Stopped and started, or taken over by another
instance, it asks the same moves: a second request for a chunk is refused by the first, and an
instance that lost the ownership asks no maintenance attach.

### Unchanged, rollout and docs

**S18. Unchanged.** `ChangeIsr`; `MoveReplica` and the drain, but for S7; the target, the
replica and their messages; the on-disk format; the failover controller; what liveness means.

**S19. Rollout.** A node without a state record is active. Every metadata instance runs the new
version before the rebuild is on: an older one knows no state and may place on a failed node.
This is an operator's rule, as #18's Q5: nothing checks it.

**S20. Docs.** Protocol §7 (moves off a failed node, what blocks them) and §9; `meta.proto`;
`docs/lab.md` (states, the commands, the check, and: a failed node's disk is not formatted
while a chunk names it, until Q1 lands; a node is declared failed for good only on evidence from
outside the cluster that it is gone, by analogy with the standard #82 sets for node-wide
recovery; etcd's compaction); follow-ups (withdrawing a move whose destination is gone for
good; compaction for the lab's etcd); which states of ARCHITECTURE §10.1 exist.

## Tests

- **S1–S8, S19 (unit and service, in memory and on etcd; `auth_test.go`):** each transition and
  its refusals; a heartbeat racing the failing, in both orders; a service down for longer than
  the wait fails nothing; a failed node is neither placed on, chosen nor accepted as a
  destination; a node without a record; failed for good: refused unless failed and not alive,
  through FORGET and #13's `ForgetNode` alike, then never admitted; an agent's heartbeat refused; the log. Each transition but failed for good leaves
  every volume and chunk record as it was.
- **The invariant (simulator):** chaos declares nodes failed at random moments, alive,
  partitioned or down, admits them, and asks moves off them, among crashes, failovers and
  maintenance attaches. Linearizability and replica agreement hold as before. New check: a
  failure or an admit leaves every chunk record and epoch as it was (failed for good is #13's
  forgetting, which #13's model checks). Injected bug, caught in some run: failing a node removes
  it from its chunks' ISRs.
- **S13, S14 (target tests in `flaminio-repl`; engine):** a move off a source that is down and
  outside the ISR completes. One off a down source at the ISR minimum, with a write waiting,
  does not: the test names today's limit (intent, Why 6). A failed node that is back is
  resynced, then dropped by its move; its session opens whatever its state.
- **S9–S12, S15–S17 (the rebuild, against an in-memory service):** only lost replicas get
  moves; the order; #18's choice with failed nodes left out; a failed node marked draining (by a
  stopped drain, or failed for good) still gets moves; a releasing node beside it is not chosen;
  held and not held volumes; a failed, alive node is not chosen for a new maintenance attach,
  by the rebuild or a drain, while a standing one on it stands; a volume with a pending move that cannot be carried out
  gets none; each reason of S15, and the rest still asked; each refusal class of
  S11; two rebuilds at once never ask two maintenance attaches for one need; a restart mid-way;
  the report.
- **Lab, a later step** (script tests for its command in `make lint`):
  - fl-b's node stopped under a verified write load on an RF 2, minimum 1 volume, the wait
    shortened: every chunk ends with two in-sync replicas, on fl-a and fl-c, with no command,
    and each node alone serves every byte. A volume written then detached does the same.
  - fl-b declared failed while it runs, under the load: no write fails; the same result.
  - fl-b back with its disk, release deletion on: no replica or move names it, and after its
    sweep no release does, and it holds no chunk of the volumes, only each one's fence (#13).
    Admitted, it takes new volumes.
  - **Measured:** the time from failed to re-protected, the bytes fl-c received and the rate,
    beside the link's rate and §16.2's target, in `docs/benchmarks/`.

## Open questions

- **Q1. Disk identity** (intent, Why 9; confirmed and widened by #61's intent). (a) #61's first
  step; (b) an issue of its own, which #61 and this issue's later steps depend on. S14 holds
  either way, and this step needs neither. **Recommended: (b),** before any failed node's disk
  is replaced. A node on a new store may then be failed at once, without the wait: a later step.
  Settled at approval, 2026-11-13: (b), disk identity is an issue of its own, before any failed node's disk is replaced (see decisions.md, Settled at approval).
- **Q2. A failed node that is alive again** (S6). (a) Stays failed until admitted, as
  ARCHITECTURE §10.1 draws it: a partition of the metadata service from every node, longer than
  the wait, then needs every node admitted. (b) Active by itself: a flapping node is rebuilt
  and reused, again and again. **Recommended: (a).**
  Settled at approval, 2026-11-13: (a), a failed node stays failed until admitted (see decisions.md, Settled at approval).
- **Q3. Where "missing since" lives** (S2). (a) A record in etcd, by the metadata service's
  clock: skew only shifts when copying starts. (b) The rebuild's memory: a restart restarts
  every wait. **Recommended: (a).**
  Settled at approval, 2026-11-13: (a), a record in etcd, by the metadata service's clock (see decisions.md, Settled at approval).
- **Q4. Where the rebuild runs** (S17). (a) A loop in every `flaminio-meta serve`. (b) A
  command, as the drain. (c) The CSI controller, which fl-c and clusters without Kubernetes
  lack. **Recommended: (a).**
  Settled at approval, 2026-11-13: (a), a loop in every `flaminio-meta serve` (see decisions.md, Settled at approval).
- **Q5. Volumes held by a target whose node is lost** (S15). (a) Reported: they wait for a
  failover. (b) Unheld by the rebuild: a maintenance attach then deposes a target that may be
  alive, cut off from the metadata service only. It needs an ADR. **Recommended: (a).**
  Settled at approval, 2026-11-13: (a), reported: they wait for a failover (see decisions.md, Settled at approval).
- **Q6. Chunks at their ISR minimum** (S15). (a) Reported; rebuilt with step 3, whose copier
  needs the same new rule (writes in flight reach the joiner). The least redundant chunks are
  then the ones not rebuilt, and the check uses minimum 1. (b) That rule now.
  **Recommended: (a).**
  Settled at approval, 2026-11-13: (a), reported, and rebuilt with step 3; the check uses minimum 1 (see decisions.md, Settled at approval).
- **Q7. A bound on moves asked at once** (S10). (a) None, as the drain. (b) A cap, per
  destination or volume: a number with no measurement behind it. **Recommended: (a);** the cap
  comes with step 4, from this step's numbers.
  Settled at approval, 2026-11-13: (a), no bound; the cap comes with step 4 (see decisions.md, Settled at approval).
- **Q8. The states in the Rust model and its vectors.** (a) S2 to S6 in both models, since the
  invariant is about them; the rebuild's choices in Go only, as #18's placement. (b) Go only.
  **Recommended: (a).**
  Settled at approval, 2026-11-13: (a), S2 to S6 in both models; the rebuild's choices in Go only (see decisions.md, Settled at approval).
- **Q9. Numbers, dependencies, ADRs.** The wait: 10 minutes by default, and its value in the
  check. Of #44 this step needs the measurement only: drop it from the dependencies. No ADR is
  contradicted; Q5 (b), and removing a lost member below the minimum, would each need one.
  Settled at approval, 2026-11-13: the wait is 10 minutes by default; of #44 only its measurement is a dependency; no ADR is contradicted (see decisions.md, Settled at approval).
