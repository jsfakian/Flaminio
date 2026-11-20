# Issue #45: Automatic replacement through existing moves — design

Approved 2026-11-13; not implemented. Changed after approval, in rounds 1 to 13 of the plan
review (2026-11-14; see decisions.md).
Implementation of existing [spec](spec.md) S1–S20 and [intent](intent.md).
Scope is node-level steps 1/2 only: **#45 stays open** for
many-to-many copying, minimum-ISR progress, adaptive scheduling and largest-domain reserve.
Proposed state/availability policy: [ADR proposal](adr-proposal.md).
Settled at approval, 2026-11-13: the ADR proposal is accepted with the plan (see decisions.md, Settled at approval).

The facts it starts from:
- Node records contain only address/labels; alive is a separate TTL key, draining another key
  (`meta/service/service.go:154–268`). Heartbeat currently performs Get then unconditional
  PutTTL (`:221`); this cannot atomically order recovery against a failed-state transition.
- `MoveReplica` records moving only, comparing chunk/header; it does not copy or alter ISR
  (`:866–916`). #18 adds domain checks; #78 adds the shared `placement-gate`, whose revision
  every admission compares.
- Drain counts replicas and chooses nodes, without capacity or node lifecycle
  (`control/drain/drain.go:46–163`). There is no rebuild package/controller or per-disk metadata.
- Target resync/move and joins remain `flaminio-repl/src/target.rs`; effective ISR/minimum,
  epoch and expected-version checks remain unchanged. Production ChangeIsr allows 64 changes;
  #81 fixes the confirmed larger-proposal defect. Do not replace that fix in this issue.
- #13, #17 (#15 under it), #18, #72 and #78 are assumed dependencies: approved plans, not code in
  this checkout. #76 is not needed: #78 works without it (#78's Q2). #44's new design measures
  existing resync only. #72's `MoveReplica` charges the destination through its ledger and
  refuses one without room or a capacity report (#72 S5); its heartbeat stores the node's
  capacity report as the value of `alive/<node>` (#72 design, Reports and ledgers).

## State record, transitions and heartbeat ordering (S1–S8, S14, S19)

Add `meta/domain/node_state.go` with Active/Missing/Failed and pure transitions. Separate etcd
`node-state/<id>` JSON v1 stores state, since_unix_ms, reason, actor, ever_heartbeat and
missing_credit_start_us. Absence means Active, never observed alive. Witnesses have no state.
Times are diagnostic service wall time; only credited service opportunities below govern wait.
Unknown/corrupt state refuses admission/automatic transition, never silently becomes Active.

On new registration retain state separately; no-op registration/heartbeat of Failed never
admits it. New nodes stay Active until their first recorded heartbeat or an admin declaration.
Legacy nodes first observed down cannot be distinguished from never-alive nodes: keep Active
until a heartbeat/admin action establishes history, and report that rollout limitation.

**The heartbeat** (S3, S6, S8). `NodeHeartbeat` is allowed to an administrator or the node's own
`io` identity only; the agent's is refused (only the data plane heartbeats, `engine.rs:726`; #72
S10 limits reports to `io` already). Extend `kv.Op` with an optional TTL for transactional puts,
in both Mem and Etcd. A heartbeat is one transaction comparing the node record and
`node-state/<id>` revisions: it puts `alive/<id>` with #72's value (the capacity report JSON when
valid, otherwise `1`; #72 S1) and, if Missing, Active with ever_heartbeat; Failed stays Failed.
Etcd keeps today's lease per key (`PutTTL`, `kv/etcd.go:120`): the transaction puts on the cached
lease; one refused because that lease expired grants a fresh lease and runs again; a committed one
then renews the lease (`KeepAliveOnce`). **The lease rule,** one for heartbeats and for the
ownership key below: a committed TTL put registers, under the key, its lease and the revision it
wrote (`e.leases`, `etcd.go:48`, which `PutTTL` fills today with the lease alone). `PutTTL`
registers the same pair, the revision being its put's. The registration is replaced under `e.mu`
only by a committed put, and only by one with a newer revision, so when puts to one key overlap
the later commit wins whatever order their answers come in. A transaction that does not apply
leaves the registration as it was and revokes a lease it granted for itself. A renew never
publishes an absent key: Missing and timeout Failed commit only with `alive/<id>` absent, and a
key gone with its lease does not come back by a renew. All TTL validation is bounded and tested;
no client outside `meta/kv` talks to etcd.

**Missing and timeout Failed** (S2, S4) are recorded by the observer only (next section): the one
instance holding `rebuild-observer`, in its 5 s pass, from one listing each of `nodes/`, `alive/`
and `node-state/`. An Active data node with `alive/` absent and ever_heartbeat becomes Missing,
whether or not the policy is on; a Missing node whose credited wait reaches the policy's becomes
Failed, only while the policy is on. Missing compares node/state and `alive` absence plus the
clock's revision; one state write records first observed time and the clock's credit as the wait's
origin. Timeout Failed compares the same evidence, the Missing state revision, the clock's
revision and the policy key's revision: a failure computed under one wait, or with the policy on,
does not commit after an administrator changed either. Heartbeat first invalidates those
compares; failure first makes subsequent heartbeats preserve Failed.

**Administrator transitions.** Fail/Admit compares the exact state revision read, records
certificate identity/reason and uses current alive evidence. Admit without alive yields Missing
with a fresh wait origin; live admit yields Active. Admit of a forgotten node is refused
(`FailedPrecondition`, `node N is forgotten`), its transaction comparing #13's mark.
**Failed for good** (Forget) is the only way to forget a node once #45 lands. It runs #13's
`ForgetNode`, whose mark transaction (its step 2) also reads and compares `node-state/<id>`, which
must be Failed (at `expected_state_revision` through FORGET, at the revision read otherwise), and
`alive/<id>`, which must be absent. So an admit or a heartbeat that committed first makes it fail
(`Aborted`), and the reread refuses it. Not Failed: `FailedPrecondition`, `node N is not failed`;
alive: `FailedPrecondition`, `node N is alive`. Then #13's steps 3 to 6, rerunnable. The check
sits in `ForgetNode` itself, so #13's RPC makes it too, and `metactl forget NODE REASON` sends
`SetNodeState` FORGET (a change to #13, its tests included, that whichever of #13 and #45 lands
second makes; decisions, rounds 2 and 3). The state stays Failed. Its writes (the mark, the drain marker, releases, fence holds,
tombstones) are #13's, under #13's tests and model. The automatic, reversible Failed never
forgets, and Failed, timeout or declared, is no evidence that the node is gone (runbook below).

**The placement gate** (#78). A write into Failed (timeout or Fail) or out of it (Admit) bumps
#78's `placement-gate` in the same transaction, comparing the gate read. Missing↔Active and
heartbeats do not: #78 does not bump it for heartbeats, and Missing gates no admission (creation
takes alive nodes; a move refuses only Failed). Do not create a second gate or alter any
chunk/epoch in these writes.

## Outage-aware wait (S2–S5, S17)

Settled 2026-11-13 (user's instruction; see decisions.md): S5's authority is the
observed-opportunity definition, and the wait counts only the credit clock's credited intervals.
Timeout failure runs on it while the policy is on. The policy is off by default; an administrator
turns it on after S19's rollout, and the implementation ships only once this clock has passed its
implementation review and tests (ship-spec's review loop).

A wall deadline from missing_since is forbidden: it would fail nodes immediately after a long
metadata outage. `rebuild-clock` stores v1 cumulative credited_us and observer sequence;
`rebuild-observer` is a 5 s TTL ownership key. Every `serve` runs the observer loop; one instance
acquires ownership by CAS, and only the owner acts: it credits the clock, records Missing and
timeout Failed (above) and runs the rebuild's step (below). **An acquisition** is the conditional
write that creates the key, or replaces another owner's, with a new nonce: a transactional TTL
put, whose lease the lease rule (The heartbeat, above) registers under the key with the
acquisition's revision. **A renewal** is a keepalive of that lease, which puts nothing, so the
key's revision and nonce are those of the acquisition until the next one. That revision and
nonce fence late old observers.

**`Renew`**, one new method of `kv.KV`: `Renew(ctx context.Context, key string, rev int64)
error`. It extends the key's TTL, as written, only while the key is still the entry written at
`rev`; otherwise it returns `kv.ErrLost`. It writes nothing and never grants a lease, so a lost
ownership never looks like a renewal. It is scoped to the acquisition, not to the key, the same
in both stores:
- **Etcd:** no lease registered under the key at `rev`: `ErrLost`. Else it first reads the key
  (a linearizable `Get`, no write) and requires `ModRevision == rev`: a key that is gone or at
  another revision is `ErrLost`, since an overwritten or deleted key leaves its old lease alive.
  Only then `KeepAliveOnce` on the registered lease; `rpctypes.ErrLeaseNotFound` is `ErrLost`,
  with no new grant (unlike `PutTTL`, `etcd.go:129-137`). Every other error of the read or the
  keepalive, the `retry` helper's exhausted `Unavailable` included (`etcd.go:26-41`), is returned
  as it is: not renewed, not lost. On `ErrLost` the registration is dropped, under `e.mu`, only
  if it is still the same lease and revision the call started with: a `Renew` that answers late,
  after the instance acquired again, leaves the newer registration. The dropped lease is left to
  expire, within its TTL, since nothing keeps it alive; it is not revoked, which would delete a
  key still attached to it. A keepalive after a passing check is harmless if the
  key changes in between.
- **Mem** (`expires`, by key only today, `mem.go:91-99`): the expiry is extended only if the
  entry is unexpired and its `Rev` equals `rev`; else `ErrLost`. So an old owner's renewal never
  prolongs a new owner's key.

**Cadence.** The owner renews at each 1 s tick, after the tick's read, in the tick's goroutine,
with a 1 s context: about five renewals in the 5 s TTL, never behind a slow pass. Each renewal's
send time is taken, by the monotonic clock, before `Renew` is called, so before its `Get`: the
earlier, conservative time. **The ownership is held from the first renewal that succeeds after
the acquisition commits,** sent at once after it, not from the acquisition: the acquisition may
put on a lease last extended long before, so only a keepalive that succeeded says the key lasts
5 s from a known send. Until then the instance holds nothing: it reads no B0 and credits, writes
and asks nothing. Its ticks from the acquisition until then credit zero but read as any tick:
they still advance the tick time, the newest revision and its rise. The tick in which a renewal
first succeeds is one of them: the renewal runs after the tick's read, so that tick has credited
zero already. The first credit comes at the next tick, for its own interval only: no later tick
credits the span before it, and none is dropped as more than 2 s late because of it. So the
first credit is delayed up to the next tick after the first renewal succeeds: that renewal's
round trip, at most its 1 s context, plus that one tick, plus one tick for each renewal that
failed; like a crash gap, it only delays a failure. Of the reviewer's two options this is the
simpler: it is the renewal that exists already, where a fresh lease for the acquisition would
add a second lease path to the TTL put. **The ownership ends at once**, in memory, in three
cases; from then on the instance credits nothing, writes nothing and asks nothing, drops B0 and
T, and may acquire again like any other instance:
- a renewal answers `ErrLost`;
- 4 s have passed, by the monotonic clock, since the send time of the last renewal that
  succeeded. The server extends the lease no earlier than that send, so the key cannot expire
  before send + 5 s: the old owner stops at least 1 s before the key can expire and be acquired
  again, however late the answer came. This bound is checked, under `clockMu`, before every
  credit, every use of B0 or T and every write or ask, not only when a renewal returns;
- a transaction that compares the ownership is refused, and the reread of the key that follows
  (a linearizable `Get`) shows another revision or none, or fails: fail closed. A reread showing
  the acquisition's own revision leaves the ownership, B0 and T as they are: another compare
  lost, so a clock write rereads the clock and goes again at the next pass, and a maintenance
  ask reports its own answer.

A lost lease is thus found by the next renewal, not only by the next compare.

The first owner creates `rebuild-clock` (v1, zero credit), comparing its absence, in the
transaction that takes ownership; only an owner records Missing, so a clock always exists to
read. The observer and its clock run whether or not the policy is on, so turning it on fails at
once a node already missing longer than the wait (S4).

**The probe reads; it writes nothing.** Each 1 s tick the owner reads, linearizably, the keys and
mod revisions of `alive/` without their values: a keys-only range read (a new `kv.ListRevs`,
etcd's `WithKeysOnly`), so #72's report JSON is not read. It counts only the keys of data nodes
Active at the last pass: a Failed node that heartbeats (S6) says nothing of whether other nodes
could; fewer heartbeats counted only delays a failure. It keeps the newest such `ModRevision`
it has read and, by its monotonic clock, when it last saw that value rise. Both start unset, and
are unset again after a failed read, a tick more than 2 s after the previous one or an ownership
change. A rise counts only when the owner saw it: a value newer than one it read before, so a
key read for the first time, which may be 10 s old, credits nothing. A rise is a heartbeat
committed, through some instance (each heartbeat puts its key, #72's report or `1`). A tick
succeeds when the last rise it saw is at most 5 s old (each node heartbeats every 3 s,
`node.rs:331`). Its interval, the monotonic time since the previous tick, is credited, capped at
1 s; a tick more than 2 s after the previous one (a paused or restarted owner), a failed read or
an ownership change credits zero. This is S5's opportunity itself: a heartbeat committed,
so etcd accepted writes and an instance served a node with a certificate the node accepts. So no
certificate check and no wall clock are needed. etcd's NOSPACE alarm or a lost quorum stops the
heartbeats, and the credit after at most 6 s of it: the interval of the tick that sees the last
rise, at most 1 s by the cap, plus the 5 s after that sighting, in which ticks credit only their
own intervals, however many they are. An owner whose certificate expired, or one started
`--insecure` (no certificate), credits from the heartbeats other instances take, since a node
tries the instances in turn (`flaminio-metaclient/src/lib.rs:6`); if every instance's
certificate expired, no heartbeat commits and nothing is credited. No owner-side condition stops
credit while another instance could credit: its view of `alive/` is etcd's, and an owner that
cannot reach etcd loses its 5 s ownership. With no other node heartbeating, nothing is credited:
a cluster whose every data node is down fails none by timeout (none could take the chunks).

**Two goroutines, one mutex.** The tick runs in the observer's goroutine; the 5 s pass (Missing
and timeout Failed, the clock write, the rebuild's step) runs in a second one, so the pass's calls
never make a tick late. They share the accrued credit, the last revision and rise, and the Active
set under one mutex, `clockMu` (`meta/service/rebuild_clock.go`), held for in-memory reads and
updates only, never across a call.

**Persisting the credit.** The owner adds credit, in memory, only while some node is Missing, and
writes it to `rebuild-clock` at the start of each 5 s pass while some node is Missing: one
conditional write a pass, before the pass decides any timeout Failed against the value written.
With no node Missing it writes nothing. **The write sets, never adds:** once it holds the
ownership (its acquisition committed and its first renewal succeeded), the owner reads the clock
(stored credit and sequence); the credit is its base B0. Every write compares the ownership,
so from the acquisition's commit on no older owner's write can commit: one in flight at the
takeover either committed before it, and B0 holds it, or is refused. T, the monotone total credited since, starts at zero with each acquisition, so an
owner that loses and regains the ownership rereads B0 and starts T again; across a renewal it
keeps both B0 and T. Until the B0 read succeeds, the owner credits nothing and writes nothing.
Each write puts B0 + T (T as snapshotted under `clockMu`), with sequence s+1 and the owner's
nonce, comparing sequence s and the
ownership. Whatever its outcome (committed, refused, or unknown after a lost reply or an ended
context), the pass rereads the whole clock (sequence and stored credit), and the next write goes
against that sequence; nothing is subtracted or kept aside. A write that commits late, after
that reread, stores an older total, which the next write replaces with a larger one; a write that
finds its sequence taken is refused. So each committed value is B0 plus credit earned, counted
once, and the stored credit never decreases. **The pass decides timeout Failed against the stored
clock** (the value its write committed, or the clock as reread), whose revision that transaction
compares, never against the in-memory B0 + T. A stored value that lags B0 + T (a write refused,
or a late commit pending) only delays a failure. So #45 adds at most one etcd revision every
5 s, and only during a node's outage (120 at most for a node failed by timeout at the default
wait), beside the heartbeats' one per node every 3 s; etcd's history is compacted as it must be
for heartbeats anyway (runbook). Missing stores the clock's value as its origin, so service
restarts do not restart earned wait; credit lost in a crash, at most one pass, only delays a failure. A new owner
preserves committed credit, adding none for the crash gap. No wall jump adds credit. A policy
change takes effect only through an administrator's transaction, which timeout Failed compares.

What the probe cannot see, as the observed-opportunity definition accepts (changed in round 3,
decisions): up to 6 s of each outage (the sighting tick's interval, at most 1 s, plus the 5 s
after the sighting, above), and
reachability from each node. By the invariant a wrong failure costs copies
only.

## Destination admission and rebuild loop (S7, S9–S18)

**Destinations** (S7). Extend Create/Move/#18's `choose` candidates to exclude Failed, and rebuild
candidates to require Active, alive, non-witness and not draining. Each attempt reads #78's
placement gate first, then lifecycle/domain/header/chunk data; the commit compares the gate and
the revisions #78 and #18 name. `nodes()` reads `node-state/` after `alive/` and before `drain/`
(`nodes/`, `alive/`, `node-state/`, `drain/`), so a hook on its `List` of `drain/` runs after the
attempt has read every node's state. No admission transaction compares a `node-state/` key: the
gate, bumped by every write into or out of Failed, covers them all, so the creation claim compares
one key for every candidate's state, not one per candidate. A fail committed first makes an old
admission's compare fail; an admission first is earlier, and completes under #78's original
claim. Move rejects a failed
destination for all callers before its no-op response: `FailedPrecondition`, its message starting
with the fixed `node failed: ` (`domain.ErrNodeFailed`; `node failed: destination "b"`), which is
neither #18's collision prefix nor #78's `is draining`, so #18's drain and the rebuild take that
destination out of the pass (#18 S14). Lifecycle never gates sessions, read sources, replica
acknowledgements or ChangeIsr; a wrong declaration only requests copies.

**The loop** (S17). Add `control/rebuild/{rebuild.go,rebuild_test.go}` with `Step(ctx, Port) ->
Report` and derived work, no durable work queue. `Port` uses the service's shared validated
move/maintenance paths. Every `flaminio-meta serve` runs it every 5 s in the observer loop, and it
acts only while that instance holds `rebuild-observer` and the policy is on: one loop acts at a
time. A trusted in-process adapter supplies fixed actor `rebuild/<instance>`, never
caller-supplied admin claims; its `AttachMaintenance` also compares the `rebuild-observer`
revision the loop holds, so an ask from an instance that lost the ownership fails and is not
retried. External RPCs still run Authorize. Each RPC has a 5 s context; one mutation is in flight
per loop, one chunk per MoveReplica, at most kv.MaxTxnOps=120 compares/ops. No node-count-sized
CAS, direct ISR edit or unbounded retry inside one step. A step whose node listing shows no Failed
node stops there.

**Lost replicas** (S9, S10). Read nodes/volumes; derive lost replicas on Failed nodes, whatever
their drain mark: #13's `ForgetNode` marks a node failed for good draining for good, and a stopped
drain leaves its mark (`drain.go:171-189`, #18 S13); `CheckMove` admits one move per chunk
(`domain.go:287`), so a running drain and the rebuild never both record one. Rank by active ISR
count, then volume/chunk/source name. A queued request is revalidated at service commit.

**The choice** (S11): #18's `choose`, `strict` and `bestEffort`, with #13's order, over the
rebuild's candidates less the source and the staying replicas:
1. a chunk at #13's bound (`MaxReleased`) waits, `holding 2 releases of chunk C`; nothing is asked;
2. eligible nodes exclude the chunk's `GetReleased()`;
3. #18's domain filter: `strict`, the nodes `domain.Strict` gives no reason for; `bestEffort`,
   the fall-back by level;
4. none left: if a released node would pass the filter, the chunk waits, `only nodes releasing it
   are left`; otherwise it is blocked, `no destination`, with #18's reason (`no eligible node`, or
   `domain.Strict`'s, such as `replica "x" has no zone label`).

Then the least loaded, counting pending incoming moves; a count rises only on Applied=true.
`MoveReplica` charges the destination through #72's ledger; the rebuild keeps no accounting.

**A refused proposal** (#18 S14, #72 S8):
- a collision (`FailedPrecondition`, message starting `failure domain collision: ` from
  `domain.ErrCollision`): the chunk is blocked for the pass, with that message; the destination
  stays a candidate;
- any other `FailedPrecondition`, or `ResourceExhausted` (#72's no room or no capacity report,
  `node failed: `, #78's `is draining`, which is also how a forgotten destination answers with
  #78): the destination leaves the pass's candidates, for every chunk, and `choose` runs again;
  none left: blocked, `no destination`, with the last refusal;
- `Applied=false` with #13's `reason` `node T is forgotten` (without #78): as the class above,
  the destination leaves the pass;
- `Applied=false` with #13's release `reason` (`node T releases chunk C`, `holding 2 releases of
  chunk C`: a release committed since the read): the chunk waits with that reason, asked again
  at a later pass (#13 S16); with no `reason` (a move recorded meanwhile): it has a move pending,
  reread at the next pass, no back-off;
- `NotFound`: the volume gone or the destination unregistered, the next volume; other errors end
  the step, reported.

A refused chunk is blocked and the pass goes on. **The back-off:** a chunk the service refused
is not asked again for 30 s, which includes the rest of the pass (S11's "for the pass"). It
starts on a collision, on a chunk left with no candidate after refusals, and on `Applied=false`
with a release `reason`; not on a destination refusal after which `choose` found another that
applied, nor on `Applied=false` without a `reason`, nor on `NotFound`. In memory: a takeover or a
restart forgets it, costing one more ask; progress only. Pending moves are never replaced or
cancelled.

**At the ISR minimum** (S15). Blocked when the lost replica is in an ISR no larger than the
minimum (its move is still asked; Q6), or when fewer ISR members than the minimum are on alive
nodes, under which a new target stays in `Fencing` (`target.rs:2252`). Of a volume not held, the
second case gets no move. **A volume not held with any pending move that cannot be carried out**
(below the minimum on alive nodes, or its destination lost) gets no maintenance attach: its
target would never end (#17 S11: work is any pending move of the volume), would not be asked
again while alive and started (#17 S17), and would keep one of its node's 2 slots (#17 S14). The
volume's other chunks wait, reported with that chunk's reason; other volumes go on.

**Maintenance** (S12): #17's S15–S17 and its drain design, the rebuild in the drain's place and
the volume's lost replicas as its progress.
- Held is read from `held`, never from `target`: `EndMaintenance` clears a finished maintenance
  record to unheld, still naming its node. A held volume keeps its client's target.
- Ask only while the volume is not held, a move is pending on it and every pending move can be
  carried out (read again after this pass's moves), and no maintenance attach stands. Any
  maintenance record stands while it names a node #17's S16 allows, whoever asked (a drain,
  `flaminio-meta maintain`, this loop before a takeover), a Failed one included: a started target
  is not deposed.
- The node of a new ask: #17's S16 (alive, not draining, not a witness, holding a replica of the
  volume or the destination of one of its moves) and also Active, ranked by most chunks of the
  volume, then name. A Failed node that is alive is never chosen: a target reads its local
  replica first (`target.rs:1649-1651`), and #17 S17 would then pin the copy to the disk declared
  failed. None: `no maintenance node`. #17's drain asks the same way: its new maintenance asks
  also exclude Failed nodes, as S7 already makes its destinations do (a change to #17's drain
  that whichever of #17 and #45 lands second makes, with a drain test; decisions, round 3). So no
  standing record on a Failed node is new: it predates the failure.
- The sighting (node, epoch, time) is set at the first sight of a maintenance record naming a
  node, and set again by the rebuild's own applied ask: its answer counts as a read of that node's
  maintenance record with a move pending, and resets the sighting's time and epoch. The node's own
  new epoch resets neither.
- Ask another node: at once when the node is no longer alive or allowed; at once after a failed
  start (at the last read, that node's maintenance record with a move pending; now unheld, still
  naming it, the move still pending); after 5 minutes since both the sighting's time and the lost
  replicas last went down, the record still at the sighting's epoch. Never again a node that is
  alive, allowed and took an epoch, however long its copy takes.
- Answers: `applied=false`, held now, not an error; `NotFound`, gone or the node unregistered;
  `FailedPrecondition`, the node forgotten (#13), the next candidate at the next pass;
  `Unimplemented`, `waiting`, `the metadata service is older`; the ownership compare failing, this
  instance asks nothing for the rest of the step, nor in any step it runs without the ownership;
  when it takes the ownership again (a new acquisition), it rebuilds its sightings from the records
  and acts.
- No failure timeout unholds a volume (`UnholdVolume`) or invokes failover. Standing asks and
  progress are rebuilt from the records after a restart; a lost in-memory sighting grants no
  authority.

**The report** (S15, S16). Classify S15's reasons: no_destination (with #18's message or the last
refusal), releases (with #13's reason), no_target, no_maintenance_node, isr_minimum,
destination_lost. Other chunks continue (but those of an unheld volume waiting on an uncarriable
move, above). Count a failed node's replicas and
moving_from/moving_to references until none remain. Log transition/request/new blocked reason;
`rebuild-status` derives counts by redundancy, pending moves and blocked volume/chunk/reason from
the same records. Deduplicate logs only in memory; restart may repeat a truthful report. A fully
reprotected node remains Failed until Admit.

## Capacity and the remainder

#72 is a dependency: `MoveReplica` charges the destination through #72's ledger, and refuses one
without room or a capacity report (`RESOURCE_EXHAUSTED`, `FAILED_PRECONDITION`, #72 S5). The
rebuild skips such a destination (above) and keeps no accounting of its own, so its destinations
are admitted as a drain's are. No reserve exists in this slice: #45 step 5 owns the
largest-failure-domain reserve, on #72's ledger. Steps 3/4 retain the new copier/live-write rule
and adaptive scheduling; current transfer passes through the target with its existing bulk limit.
No multi-disk placement or many-to-many throughput is delivered.

## Contracts and authorization

Enums follow buf STANDARD (`make lint`): the type's prefix on every value, `_UNSPECIFIED = 0`,
PascalCase names.
- `Node` proto gains `NodeState state = 6`, with `enum NodeState { NODE_STATE_UNSPECIFIED = 0;
  NODE_STATE_ACTIVE = 1; NODE_STATE_MISSING = 2; NODE_STATE_FAILED = 3; }`, int64
  state_since_unix_ms=7, string state_reason=8, string state_actor=9, uint64 state_revision=10.
  Witness reports UNSPECIFIED; legacy absent state on a new server reports ACTIVE. Incoming
  Register ignores these server-owned fields.
- New admin-only `SetNodeState`: request `{string node = 1; NodeStateAction action = 2; uint64
  expected_state_revision = 3; string reason = 4;}`, with `enum NodeStateAction {
  NODE_STATE_ACTION_UNSPECIFIED = 0; NODE_STATE_ACTION_FAIL = 1; NODE_STATE_ACTION_ADMIT = 2;
  NODE_STATE_ACTION_FORGET = 3; }`; response `{Node node = 1; ForgetNodeResponse forgotten = 2;}`
  (#13's message, set for FORGET only). UNSPECIFIED or a value this version does not know:
  `InvalidArgument`, nothing written; an unset action is refused, never a failure. Revision 0
  means absent. Validate IDs/reason ≤ 256 UTF-8 bytes; reject witnesses. Admin certificate
  identity is server-derived, never accepted from a field. Stale compare: Aborted. FORGET of a
  node not Failed or alive, and ADMIT of a forgotten node: `FailedPrecondition`.
- `MoveReplica` to a Failed node: `FailedPrecondition`, `node failed: destination "b"`.
- Admin-only `SetRebuildPolicy`: request `{uint64 expected_revision=1; uint32 wait_seconds=2;
  bool enabled=3;}`; response `{uint64 revision=1; uint32 wait_seconds=2; bool enabled=3;}`.
  Default 600 s and off, accepted range 1…86400; ListNodesResponse reports these policy fields
  at 2/3/4 as wait_seconds/policy_revision/enabled. **`enabled`** gates the automatic actions
  only: timeout failure and the rebuild's requests (moves and maintenance attaches). Not gated:
  Missing (an observation that requests nothing), the observer and its clock, an administrator's
  FAIL, ADMIT and FORGET, and S7's refusal of failed destinations. An absent policy reads as off.
- `NodeHeartbeat`: an administrator or the node's `io` only, the agent `PermissionDenied`; the
  value of `alive/<node>` stays #72's.
- CLI: `fail-node NODE REASON`, `admit-node NODE REASON`, `forget NODE REASON` (#13's command,
  sending `SetNodeState` FORGET: failed for good), `rebuild-policy WAIT_SECONDS on|off`, each reads revisions before CAS; `nodes` and
  `rebuild-status [NODE]` emit derived JSON. Auth tests deny agent/io/meta identities for both
  new mutations, and the agent's heartbeat. Observer transitions are private service operations,
  not unauthenticated RPCs. Audit committed transitions once.
- All new state/clock/policy/observer keys are under `/flaminio/v1/`; policy JSON v1 stores
  wait_seconds/enabled, CAS revision supplies generation. No replication wire, token or on-disk records change. New metadata JSON keys are v1; unknown
  versions fail closed. `make proto`, Rust Node/pb decoding and `make contracts` cover changes.

## Files and tests: every rule

| Rules | Files and concrete validation |
|---|---|
| S1–S4, S6 | `meta/domain/node_state.go`, service `node_state.go/service_test.go`: table incl. never-alive/witness/legacy/corrupt states; first observer; heartbeat/failure both interleavings; admin stale-revision/no-op/re-admit cases on Mem and real etcd. Policy off: Missing recorded, never Failed by timeout; turned on, a node missing past the wait fails. A timeout racing a policy change (`interleaveKV`) fails its compare. FORGET: refused unless Failed; refused for a Failed node that heartbeats (`node N is alive`); an admit or a heartbeat racing it (`Aborted`, then refused); then ADMIT refused. #13's `ForgetNode` of an Active node, and of a Failed node that heartbeats, refused. |
| S2–S5, S17 | `meta/kv/{kv.go,etcd.go,mem.go}` and the shared suite `meta/kv/kvtest/kvtest.go`, run on Mem and real etcd: `ListRevs` (keys and mod revisions, no values); `Renew`: it keeps the key and its revision and extends it past its first TTL; after expiry it answers `ErrLost` and the key stays absent (nothing granted); with the old revision after another owner's acquisition it answers `ErrLost` and the new key expires at its own time; the lease renewed is the one a transactional TTL put registered (no `PutTTL` before it); on real etcd too, the displaced owner: the key replaced by a second `KV`'s acquisition on the same etcd while the first's lease is alive, and the key deleted while its lease lives: `ErrLost` for the first, its registration dropped, the new key not extended; an etcd error other than `ErrLeaseNotFound` is returned, not `ErrLost`; a refused TTL transaction leaves the previous registration (a `Renew` at its revision still succeeds) and revokes the lease it granted; an `ErrLost` answered late for an old revision, after a new registration of the key, leaves the new one (a `Renew` at the new revision still succeeds); `PutTTL` registers its put's revision; of two overlapping puts to one key, the later commit's registration stays. `meta/service/rebuild_clock.go` tests: persisted credit; an old `alive/` key read by a new owner, with no rise: zero; the same after a late tick; a Failed node's heartbeat alone: zero; a lost reply after the write committed, and one without a commit: the next write stores B0 + T, credit counted once (regressions: round 4's rule passed them too); a write that times out, whose reread sees the old value, then commits late: the stored value after the late commit is the older total, and the next write replaces it with the true credit, counted once (fails under round 4's rule); a new owner's first write is exactly the previous owner's last stored credit plus its own T (fails if B0 is read before the ownership commits, or T is not reset); an old owner's write in flight at the takeover is refused or already in B0, and the new owner's stored credit is never below the old owner's last committed value; a regained ownership rereads B0 and starts T at zero; across renewals, with a node Missing: the owner credits x, renews, credits y, and its next write stores B0 + x + y, not B0 + y, with exactly one B0 read in all and no gap in the writes (it fails if a renewal, or any new key revision, resets T or rereads B0); a renewal answering `ErrLost`, and 4 s since the send of the last renewal that succeeded: the ownership ends at once, later ticks credit nothing and no write is sent; on a fake clock, a renewal answered at its 1 s context limit: the ownership ends 4 s after its send, taken before `Renew`'s `Get`, so 3 s after the answer; a tick falling between send + 4 s and the next renewal credits nothing, and a pass then writes nothing (the bound is checked before each credit and write); an acquisition whose first renewal fails: no B0 read, nothing credited or written, until a renewal succeeds; shaped so: on a fake clock with 1 s ticks, at least three renewals fail (so the failures span more than 2 s), a heartbeat rise is seen during them, then one succeeds; the tick of the success credits zero, and the first credited tick is the next one, adding exactly its own 1 s interval: not zero (the rise was advanced), not the span before it, and not dropped as late; a variant in which the successful renewal is answered at its 1 s context limit: the first credit is still at the first tick after the answer, adding exactly its own interval; a refused transaction with the key unchanged (another compare lost) leaves the ownership, B0 and T; one whose reread fails, or shows another revision or none, ends the ownership; renewals run every tick while a pass is blocked; no credit or write before the B0 read succeeds; timeout Failed decided against the stored clock, not the in-memory total; an outage beginning just after a heartbeat commits credits at most 6 s; all-service and quorum outages longer than the wait; heartbeats refused (a NOSPACE answer) for longer than the wait: credit stops within 6 s of the last heartbeat; every instance's certificate expired (no heartbeat commits): the same; the owner's certificate expired, or the owner `--insecure`, while other instances take heartbeats: credit goes on; a tick more than 2 s late (a paused owner) adds zero; a clock write that commits after its context ends stores an older total, replaced by the next; no clock write while no node is Missing; no other node heartbeating: nothing credited; restart/owner ABA, wall jumps/overflow; the first owner creates the clock. |
| S3, S7, S19 | `service.go/placement_test.go`, drain tests: failed exclusion everywhere; destination fail/admit/register races invalidate #78's gate; the gate bumped into and out of Failed only, not by Missing↔Active or heartbeats; a Fail racing a move (`interleaveKV`): the move is refused, `node failed: `; old state absent ACTIVE. Two data nodes, one Failed and back: an RF 2 creation refused, then accepted after ADMIT. |
| S3, S8 | Heartbeat on Mem and real etcd: one transaction; `alive/` holds #72's report value (or `1`); a live lease is renewed after the commit, an expired one replaced once; nothing republishes an absent key. |
| S8 | `auth.go/auth_test.go`, main CLI tests: two admin RPCs only, TLS caller audit, reasons/IDs bounded, expected revision required; an unset action refused, nothing written; the agent's heartbeat refused; node heartbeat affects only itself and never admits Failed. |
| Invariant, S18 | `flaminio-repl/src/{meta.rs,node_state.rs,sim/mod.rs,sim/tests.rs}`, `examples/meta_vectors.rs`, Go replay: lifecycle changes preserve all chunk/ISR versions/epochs; failure/admit chaos with moves/maintenance, existing linearizability checker; injected fail_removes_isr caught. Service, on Mem and real etcd: every `volumes/` and `chunks/` key keeps its revision across a heartbeat, Missing, timeout Failed, FAIL and ADMIT (FORGET is #13's). |
| S7 | `control/drain/drain.go` (with #17: `Drainer`'s new maintenance asks exclude Failed nodes, a change to #17; with #18: `choose`'s candidates exclude Failed) and `control/drain/drain_test.go`: a drain's new maintenance ask passes over a Failed, alive node holding the most chunks; a standing record on it stands. |
| S9–S12 | `control/rebuild/rebuild_test.go`: strict redundancy/order, active #18 destination ranking (`strict` and `bestEffort`), held/unheld/#17 races, pending moves; a failed source marked draining by a stopped drain, and one failed for good, both get moves; a releasing node beside the chunk is not chosen; no target takeover. Maintenance, one test each: a standing record of another asker stands; no ask without a pending move; a failed start asks elsewhere at once; the own applied ask resets the sighting; a started, alive node is not asked again past 5 minutes; an unheld record naming a node is not held; each answer (`applied=false`, `NotFound`, `FailedPrecondition`, `Unimplemented`). A Failed, alive node holding the most chunks is not chosen for a new ask, while a standing record on it stands. Two loops, a takeover between them: one maintenance epoch for one need; an owner that loses and regains the ownership acts again. |
| S13, S14 | Target/engine tests: source down/outside ISR completes; source at minimum with pending write remains blocked; returned same-disk node resync/drop and #13 reclamation; no lifecycle session filter. |
| S15–S17 | Rebuild tests: every blocked reason, other chunks progress: `holding 2 releases of chunk C`, `only nodes releasing it are left`, #18's S5 reason, a collision refusal (chunk blocked, destination kept), a destination failed mid-pass (left out, the next chosen), a full destination (#72, skipped, the next chosen); at the minimum, unheld, RF 3/min 2 with two failures and RF 1: no move, no maintenance, other volumes still get theirs; unheld, one carryable move beside another chunk's below the minimum, and a move whose destination is lost: no maintenance attach, the volume reported; the back-off: started by a collision, by no candidate left and by a release `reason`, not by a destination refusal followed by an applied move; no Failed node: no scan; restart between request/response/join/release, conditional lost replies, no remaining replica or move reference; Failed remains sticky. |
| S18, S19 | `flaminio-metaclient/src/pb.rs`, interop: decode new Node fields without affecting I/O, regenerated proto/vectors; an absent policy reads as off. Real >64 recovery is #81, not a permissive model fixture. |
| S20 | `scripts/checks/rebuild.sh`, `scripts/tools/rebuild-test.sh`, Makefile: fake services/VMs cover safe commands, all lab failure/cleanup branches (the policy off before the single-node checks included) and help; protocol/lab/followups identify implemented node states versus absent disk/reserve features. |

Implementation: `make lint test race contracts sim`; real etcd tests exercise transactional TTL,
and Rust-client/Go-service interop keeps production 64-change limits. No tests ran in drafting.

## Lab and docs

Later explicit authorization only, where #83's admission lets volumes attach: exclusive
fl-a/fl-b/fl-c, same disks, all metadata and the assumed dependencies upgraded, release deletion
on for every node (#13 S14). `make rebuild-test` (help: "Automatic rebuild: fl-b stopped, then
declared failed, under verified writes; each node alone then serves every byte") runs
`scripts/lab.sh rebuild-test` (`cmd_rebuild_test`); every `metactl` is an administrator's:
1. `metactl nodes` must show every data node Active and alive, else the run fails before any
   change (a node already missing would fail at once at 5 s). Save it (states and policy);
   `metactl rebuild-policy 5 on`. Two private 16 GiB,
   64-chunk RF 2/minISR 1 volumes: `vol1` attached on fl-a under verified writes
   (`write-load.sh`), `vol2` written, then detached.
2. `node down b`. Within 600 s, `metactl rebuild-status fl-b` shows no replica or move naming
   fl-b, and both volumes have two in-sync replicas, on fl-a and fl-c (`vol2`'s by a maintenance
   target), with no manual move; the load does not fail. Measured: failure commit to zero
   references, fl-c's rx bytes and rate, beside the measured link rate.
3. `node up b`: fl-b stays failed; within 60 s `metactl releases fl-b` is empty and fl-b's
   `node.sh list` of each volume shows only its fence-only record, no chunk (#13: a node that
   confirmed a release keeps the volume's fence). `metactl admit-node fl-b lab`; an RF 3 volume
   is created (it needs fl-b), then deleted.
4. `metactl drain fl-c`, `metactl undrain fl-c` (replicas back on fl-b). Under the load,
   `metactl fail-node fl-b lab`: no write fails; the result of step 2. `metactl admit-node fl-b
   lab`.
5. `metactl rebuild-policy <saved wait> off`, so no node stopped below fails; then each of fl-a
   and fl-c alone, the other's data plane stopped, a fresh attach, a `VERIFY_ONLY=1` load.

Cleanup (an `EXIT` trap) detaches/deletes only test volumes, restarts nodes, restores the saved
policy and states by conditional admin changes; never overwrites concurrent operator state. A
deleted volume's tombstone is confirmed by the nodes' sweeps (#13 S13), never by hand; pending
cleanup is reported truthfully. No disk format or VM suspend/rollback is permitted. §16.2's target
is labelled unachieved. Record benchmark/raw logs only after running.

Docs: protocol §7/§9, proto comments, `docs/lab.md` (the states, the commands, `make
rebuild-test`, S19's rollout rule as an operator's, and that a failed node's disk is not formatted
while a chunk names it), the runbook, followups and the implemented state table; leave
many-to-many/reserve/maintenance claims future. The runbook and `docs/lab.md` say:
- Failed, timeout or declared, is no evidence that a node is gone: a partition between the node
  and the metadata service gives the same Failed. A node is declared failed for good (`metactl
  forget`) only on evidence from outside the cluster bound to that machine (powered off, disk
  destroyed), by analogy with the standard #82 sets for node-wide recovery (its S1, S2; #82's
  evidence provider is not part of FORGET), once not alive and no chunk names it as a replica:
  that frees the chunks it releases (#13's S2 bound), and retires its id for good. Moves to it may
  remain (next item);
- a volume not held with a pending move whose destination is gone for good never gets a
  maintenance attach under #45 (S12): no move is withdrawn, and forgetting the destination does
  not clear it (#13). Its other chunks then keep their reduced redundancy until the operator acts:
  attach the volume (its client's target carries out the other moves), or `flaminio-meta
  maintain` it and, once its other moves are done, have root end the maintenance target (#17
  S14). The stuck chunk itself waits for the follow-up that withdraws such a move;
- etcd needs periodic compaction (`--auto-compaction-*`): heartbeats write a revision per node
  every 3 s, and #45 adds at most one every 5 s during an outage. The lab's `etcd.sh` sets none:
  a follow-up;
- where RF equals the number of data nodes, as on ADR 0006's 2-node edge site, a failed node's
  chunks have no destination, and once back it stays failed, so every creation of that RF is
  refused until an admit: keep the policy off, or the wait long, on such a site.

## Risks, ADRs and parallel work

Settled (see decisions.md): Q1 to Q8, each with its recommendation, at approval; S5's authority,
the observed-opportunity definition; VM clock qualification, left to #83's separately qualified
profile, lab checks running only where #83's admission lets volumes attach. Risks: a wrong
declaration must remain only a placement cost. Storage-disk replacement can still corrupt
authority until disk identity lands: prohibit formatting any named failed replica. Where RF
equals the number of data nodes, a failed node is not rebuilt and blocks creations of that RF
until admitted (runbook above). A maintenance target already started whose remaining work later
cannot be carried out (below the minimum by a later failure, or its move's destination lost)
keeps its slot until a node returns or root ends it (#17 S14): reported as `isr_minimum` or
`destination_lost`, not solved here. A volume not held with such a move gets no new attach; when
the destination is gone for good, that wait has no end under #45, and the volume's other chunks
keep reduced redundancy until the operator's step in the runbook. Neither is accepted: both are
named, reported, and left to the follow-up that withdraws a move.

[Issue-local ADR](adr-proposal.md) records the lifecycle/credit policy, accepted with the plan
(decisions.md), S5's item settled by the observed-opportunity definition.
Accepted 0001/0002 retain Go control, Rust model and dedicated-etcd transactions. No change to
accepted fencing/ISR ADRs; audit 0007 is Proposed and current strict takeover gates remain.
Parallel work: agree proto/records/gate/TTL contracts first; Go state service and rebuild loop,
then Rust model/interop can proceed separately. No runtime implementation is authorized here.
