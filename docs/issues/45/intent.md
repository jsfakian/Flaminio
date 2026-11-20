# Issue #45: Many-to-many rebuild — intent

[Issue #45](https://github.com/jsfakian/Flaminio/issues/45), from the README roadmap (phase 2)
and ARCHITECTURE §9, §10.1, §10.5, §11.1, §11.4 and §16.2. This change is the issue's steps 1 and
2 in their smallest form: a node gone too long is failed, and its chunks get new replicas with no
operator, by the moves that exist. It is not yet many-to-many. Steps 3 to 5 follow: issue #45
stays open. Assumes #13, #17 (and #15 under it), #18, #72 and #78 as planned. Of #44 it needs
only the measurement: a returning node is resynced as today. Changed after approval, in round 1
of the plan review (2026-11-14; see decisions.md): the dependencies, Why 6, outcome 6 and two
items of Out of scope.

## Why

ARCHITECTURE describes disk states, a rebuild controller and a copier. The code has none of them.

1. **Nothing re-protects data on its own.** `control/` has the drain, which an operator runs
   (`flaminio-meta drain`), and the failover of targets, which marks a Kubernetes node out of
   service and moves no replica (`control/failover/failover.go`).
2. **A lost node is a lease that ran out.** The data plane heartbeats every 3 s
   (`flaminio-io/src/node.rs:331`); `alive/<id>` expires after 10 s (`NodeHeartbeat`,
   `meta/service/service.go:221`). `ListNodes` answers alive or not, never since when. The only
   other node state is `drain/<node>`. There is no `Missing`, no `Failed` and no
   `replicaReplenishmentWait`.
3. **The metadata service knows no disk.** A node's record is an address and labels
   (`nodeRecord`, `service.go:154`). A node has one data disk (`Pick`,
   `control/agent/disks.go:158`); the agent's states are claim verdicts, printed and never
   registered. A disk that fails under a running node poisons the store and closes the sessions
   (`poison_on`, `flaminio-store/src/store.rs:743`; `fill`, `flaminio-node/src/replica/mod.rs`).
   Nothing else reacts: the node keeps heartbeating, and stays alive (static trace).
4. **A chunk with a lost member cannot be told from its record.** The node stays in `replicas`.
   - **With a target:** it leaves the ISR within the request timeout (3 s) if the ISR stays at
     the minimum (`member_failed`, `flaminio-repl/src/target.rs`). A restarting node looks the
     same (`vol_degraded`, `scripts/lab.sh`).
   - **ISR at its minimum:** it stays a member, and writes wait for it (protocol §4).
   - **No target:** nothing changes. The record shows a full ISR on a dead node.
5. **A drain of a dead node already re-protects, by hand** (static trace; never run in the lab).
   `drain` needs the node registered, not alive; `MoveReplica` checks the destination only
   (`service.go:870`). The target adds the destination, resyncs it from an ISR member, and drops
   the source, which need be neither in the ISR nor reachable (`drive_move`, `drive_resync`,
   `send_meta`; `validMove`, `meta/domain/domain.go:259`). The simulator's chaos asks moves of
   crashed replicas too (`request_move`, `sim/mod.rs:1261`); no test names the case. This is
   §10.5's "drain with the source gone". Missing: someone to ask, and an order.
   - *Lab, phase 1 step 6:* fl-b drained into fl-c under a write load, 64 chunks, in about 115 s;
     back, 16 GiB over 1 GbE in 157 s, about the link's rate.
6. **Where that stops** (traced, not tested):
   - **A volume without a running target** does not move. #17's maintenance attach serves it,
     and leaves the asking to this issue. A volume held by the lost node waits for its failover.
   - **A chunk at its ISR minimum cannot take a new member while a write waits.** Writes in
     flight before `BeginResync` are never sent to the joiner (`start_write`; `retransmit` covers
     `required()` only), and it joins only once it has acknowledged every write in flight
     (`drive_resync`). Those writes wait for the lost member for good. A new target stays in
     `Fencing` while fewer members than the minimum answer (`advance_chunk`): it neither resyncs
     nor moves. So RF 2 with minimum 2, or a second loss at RF 3, cannot be rebuilt.
   - **A destination lost mid-move** keeps the move pending: `CheckMove` refuses another, and no
     request cancels one.
   - **No capacity is accounted.** `Place` and the drain count replicas per node. A rebuild can
     fill a destination. #72 adds the accounting: a move to a node without room is refused.
7. **It is not many-to-many.** Every byte of a move passes through the volume's target: read
   from one ISR member, written to the joiner (protocol §7), with at most 4 pieces of 1 MiB in
   flight per target (`DEFAULT_MAX_BULK`, `max_io`). The source is the local replica, else the
   lowest id (`source`). §16.2 wants a 4 TiB disk rebuilt in 30 minutes on 10 nodes at 25 GbE,
   about 2.3 GiB/s. The lab has three data nodes, 100 GiB disks and 1 GbE: it can measure a
   rebuild against its link, not against that target.
8. **Safety does not come from the declaration, and must not.** A move changes a replica set
   or an ISR only through the target's `ChangeIsr`, at its epoch and the expected versions;
   `MoveReplica` changes neither (`service.go:860`). So a node declared failed while alive and
   partitioned costs copies, not data. Cut off from the metadata service only, it stays in its
   ISRs, and its move is an ordinary one. Cut off from the targets, it left the ISRs by the
   rules of protocol §4 and §6, and returns outside them, or holding copies no record names
   (#13 deletes those).
9. **A node back with another disk is taken for the same node** (found in this trace; so
   today). `Hello` creates, empty, every chunk the target's view places on a node whose store
   lacks it, and reports nothing uncertain (`ReplicaService::hello`, `replica/mod.rs:236`;
   `ReplicaCore::add_chunk`): right at a volume's first attach. A node whose disk was replaced
   or reformatted while the records had it in an ISR (no target ran, or the ISR was at its
   minimum) returns as an in-sync member of empty chunks. No digest is compared, and reads from
   it return zeros. Nothing reads `disk_uuid` but `flaminio-disk inspect`. `make node-up
   FORMAT=1` does this in the lab. #13's "a `Hello` at the node's own fence creates no chunk"
   does not cover it: a new disk has no fence.

## What we want

1. **A node is active, missing or failed, and the metadata service says since when.** Missing:
   its liveness lapsed. Failed: missing longer than `replicaReplenishmentWait` (default 10
   min), or declared by an administrator. The record is durable: a restart of whatever decides
   neither forgets nor restarts the wait. `flaminio-meta nodes` shows it.
2. **A missing node that returns in time is only resynced,** as today. No move is asked for it.
3. **A failed node's chunks get new replicas with no operator action.** Each chunk with a
   replica there gets a move to a node that may take it (#18's rules), carried out by the
   volume's target, or by a maintenance target when no client holds the volume (#17). Lowest
   redundancy first: the fewest in-sync replicas on active nodes.
4. **A failed node takes no new replica, and does not come back by itself.** If it returns, the
   replicas still placed on it are resynced or dropped by their moves, and #13 deletes the
   rest. An administrator admits it again, whatever still names it.
5. **The declaration decides when to copy, never what is safe.** No ISR, minimum or replica
   set changes because of a node's state, and no client's target is deposed: the only new epochs
   are #17's maintenance attaches, of volumes no client holds. A node declared failed while
   alive loses no acknowledged write and serves no stale read. The simulator checks it, with an
   injected bug.
6. **The rebuild says what it does.** Per failed node: the chunks left, by redundancy, and
   those it cannot re-protect, with the reason (no destination; #13's releases; held by a target
   that does not run; at the ISR minimum; destination lost).
7. **A lab check, and a number.** fl-b's node is stopped under a verified write load on an RF 2
   volume, with the wait shortened. Every chunk then has two in-sync replicas, on fl-a and
   fl-c, with no command, and each node alone serves every byte. The time and the rate go in
   `docs/benchmarks/`, beside the link's rate and §16.2's target.
8. **Docs:** the protocol (moves off a failed node), `docs/lab.md`, follow-ups, and which states
   of ARCHITECTURE §10.1 exist.

## Out of scope

- **Step 3: a copier on the destination, pulling from a source.** This is what makes a rebuild
  many-to-many (Why 7). Pull or push is a protocol decision, with a new rule: writes in flight
  must reach the joiner (protocol §7). Decided with this change's number.
- **Chunks at their ISR minimum** (Why 6). The same rule frees them. Here they are reported.
- **Spreading sources,** and destinations beyond #18's choice.
- **Step 4: adaptive rate and background priority.** A rebuild shares the target's limit on
  bulk work, as a drain does.
- **Step 5: the rebuild reserve.** It builds on #72's capacity accounting (Why 6).
- **Disk identity** (Why 9): #61's intent took it as its first step, and may move it to an
  issue of its own. It is fixed before a failed node's disk is replaced. A node's state gates no
  session, so nothing here closes it or widens it: until then the check brings fl-b back with
  its disk, and no failed node's disk is formatted while a chunk names it.
- **States per disk, and `Failed` on I/O errors, device removal or SMART** (Why 3). A node has
  one disk. A node that heartbeats on a dead disk is failed by an administrator.
- **A move whose destination is lost:** cancelling or re-aiming it is a new transition. Reported.
- **Volumes held by the failed node** wait for a failover or a client's attach. Unholding one
  (#17's `UnholdVolume`) on a timeout would depose a target that may be alive (open question 5).
- **Maintenance mode** (#44's step 3): it builds on outcome 1's states. And **#44's record**.
- **Force-promotion** when every ISR member is lost (§11.1).
- **The operator, CRDs and `kubectl flaminio`.**
- **§16.2's target itself:** out of the lab's reach (Why 7).
- **ARCHITECTURE §9 to §11** keep describing what is wanted.
