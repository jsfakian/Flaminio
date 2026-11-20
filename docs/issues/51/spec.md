# Issue #51: Snapshots and clones — specification

Steps 1, 2 and 7 of the issue ([intent](intent.md)). The choices are under
[Open questions](#open-questions), each settled at approval, 2026-11-13, as recommended
(decisions.md, Settled at approval); a rule that follows one names it (Q*n*). Changed after
approval in plan review round 1 (decisions.md): S2, S4, S6, S12, S19, S22 and the Tests; in
round 2: S2 and the Tests.

## Terms

- **Generation:** a volume's counter in the metadata service, 0 at creation. Only a cut moves it.
- **Cut:** the move from generation *g − 1* to *g*. It makes the **snapshot at *g***: the
  volume's content at that instant.
- **Snapshot:** a record of a volume: a name and, once cut, its generation. **Pending** before.
- **Layer of generation *h*:** on a chunk replica, what was written at *h*. The snapshot at *g*
  is, per chunk, the layers below *g*. A chunk not written at *h* has no layer of *h*.
- **Top generation:** that of a chunk replica's newest layer.
- **Held:** a client write that is queued, neither sent nor failed.
- **Catch-up:** what a resync becomes (protocol §7): a replica outside the ISR is completed
  generation by generation.
- **Replica:** a whole copy of a chunk. #64 restates these rules for fragments.

## The invariant

Every in-sync replica of a chunk holds every snapshot, with the same content: the chunk's at
the cut, every write completed before it and none sent after. Three things keep it: a cut
happens only with every chunk reconciled and no write in flight; nothing lands under a cut
afterwards but catch-up copies of what the ISR holds there; a replica joins the ISR only once
it holds every snapshot.

## Behaviour

### The metadata service

**S1. Generation and snapshots.** A volume has a generation and a list of snapshots. Both
come with every answer that carries the volume, `Attach`'s included. Generations only grow.

**S2. Requesting a snapshot.** An administrator names a volume and a snapshot. The request is
recorded as pending, with the volume's snapshot count, and charges the space the snapshot may
retain (below); it changes nothing else. A name the volume already has answers that
snapshot. Refused, before any write: an invalid name, a volume unknown, being created or being
deleted, a volume that already has 63 snapshots, pending ones counted (Q6), an encrypted volume
(#50's descriptor not `none`: no snapshot until its keys are pinned, as #50 requires), and a
volume whose replies with a full catalog would not fit the 4 MiB bound (design, Budget). A
later state that forbids snapshots (#68: still hydrating) is refused here too.

The charge: a volume with *k* snapshots, pending ones counted, is charged (1 + *k*) times what
#72's capacity ledger and #73's quota charge it without, since a layer holds at most a chunk
size and takes at most one slot's page tables. Each request adds one chunk size and one slot per
named copy and, for a tenant volume, its size to its quota. It is refused, before any write,
where the quota has none or a node holding a copy has no room at #72's soft watermark or no
valid capacity report (a down node's copy, a released copy awaiting `ForgetNode` included). The
nodes are charged after the request is recorded, in as many transactions as the volume needs:
until that is complete the snapshot is not cut (it retains nothing yet), and a charge that a
racing admission leaves without room, a node that stops reporting or a capacity initialization
blocks, or a crash interrupts, is completed by a later request of the volume that is not refused
(the same name's included). A volume with snapshots that grows (#70) stays within the 4 MiB
bound and charges its new copies alike; its assignment to a tenant (#73) charges (1 + *k*) times
its size.

**S3. The cut.** One conditional transaction: the generation moves by one and the oldest
pending snapshot gets the new generation. It is applied only for the data plane of the volume's
current target, at the volume's current epoch, from the generation the caller names, with a
snapshot pending (Q1). Otherwise nothing changes, and the answer carries the current epoch and
generation. It is ordered with `Attach` as `ChangeIsr` is. The Rust model and the Go service
implement the same table.

**S4. Who may call.** Requesting: an administrator; once #73's tenancy is active, `cluster-admin`
(#73's roles). Listing: any identity, one of #73's open reads, as `GetVolume`. The cut: the
data plane, and S3's caller only; another node's data plane gets S3's answer.

**S5. Commands.** `flaminio-meta` requests a snapshot, can wait until it is cut, and lists a
volume's snapshots with their state and generation.

**S6. A deleted volume** takes its snapshots with it (Q6), and its deletion releases their charge
(S2). A deletion interrupted at any point leaves no snapshot record for good: the next deletion
or creation of the name clears it. A volume created again under the name starts at generation 0
with none, and no read lists an earlier volume's; its replicas' layers went with the earlier
volume's chunks (docs/issues/30).

### The target: the cut

**S7. When.** The target learns of pending snapshots at its poll, and cuts them one at a time,
in the order requested. A volume without a target keeps them pending until its next attach, as
a requested move waits (Q2).

**S8. The drain.** A cut goes in this order:
1. it waits until every chunk is reconciled, holding nothing;
2. it holds every client write submitted from then on, and waits until every write in flight,
   repairs included, has completed;
3. it proposes S3, and sends no write until the answer;
4. applied: held writes go, in their order, at the new generation.

Reads are served throughout. No client operation fails because of a cut.

**S9. No write ahead of the metadata service.** A target sends a write of generation *g* only
once the metadata service has answered that the volume is at *g*. A cut refused leaves the
target at the answer's generation, or deposed if the epoch moved. A cut without an answer
keeps the writes held until one comes (Q8).

**S10. A target that dies** before its cut is applied leaves the snapshot pending for the next
target. After, the snapshot is cut, and the next target starts at its generation (S1).

### Writes and layers

**S11. Writes carry a generation:** client writes, repairs and catch-up copies. Nodes of
different versions do not mix (Q7).

**S12. The replica.** For a write or copy of generation *h* on a chunk whose top is *t*:
- *h* > *t*: it pushes the layer of *h* first; the push is durable no later than the write;
- *h* = *t*: as today;
- *h* < *t*: refused, with no effect: the replica ends the session, and the target takes the
  replica as failed, as on a store failure.

A write at the top generation costs no commit more than today.

**S13. A layer that cannot be pushed** (the page pool is full) fails the write as a write
without space does today.

**S14. Reconciliation** compares and repairs the current content, at the target's generation,
as today. S8 is why nothing below a cut needs it.

### Catch-up

**S15. Oldest first.** A replica outside the ISR is completed one generation at a time, from
the oldest it may lack to the target's. At generation *h* it gets, from an ISR member, the
blocks changed at *h* that it may lack, as copies of generation *h*. It gets live writes at the
target's generation only, once it holds everything below. No data goes under a layer the
replica has: no format change (Q3).

**S16. What it may lack.** For a replica this target removed: at each generation since, the
blocks of the writes in flight to it at the removal and those written while it was out.
Otherwise every block of every generation. Unwritten ranges and blocks that already match are
skipped per generation, as they are today for the current content.

**S17. A cut during a catch-up** does not wait for it. The replica gets no write of the new
generation until it holds everything below (Q4).

**S18. Joining.** A replica is proposed for the ISR only once it holds every snapshot and
protocol §7's conditions hold. A move's destination is caught up in full; the drop follows the
join, as today.

### Reading a snapshot

**S19. At a generation.** `Read`, `Digest` and `Mapped` take a generation and answer from the
layers below it; without one, the current content, as today. They change nothing (Q5). A chunk
the volume gained (#70) while at generation *g* is all zeros in every snapshot at or below *g*.

**S20. What changed.** A replica answers which ranges of a chunk may differ between the
snapshot at *a* and the one at *b*, or the current content, from the layers of *a* up to *b*.
Every block whose content differs is in the answer; blocks that do not differ may be. Two
replicas' answers may differ in those only. This request is this step's: catch-up uses it,
and #52 and #58 read through it.

**S21. Offline.** A stopped node's store gives a snapshot's per-chunk digests and the data of
a range, as `flaminio-io digest` gives the current content's.

**S22. What a reader may rely on.** An in-sync replica answers S19 and S20 rightly for every
snapshot. A replica does not know whether it is in sync: the asker does. In this step the asker
is the target, on its session. A reader that is not the target, and its right, are #52's. What
this step gives such a reader is its completeness evidence: a replica holds the snapshot at *g*
in full when a consistent volume view (#77) has it committed and names the replica in the
chunk's ISR (contracts, Completeness evidence).

### Unchanged, and docs

**S23. Unchanged.** The on-disk format. Epochs, fencing, sessions, leases, ISR changes and
moves' metadata steps. Fast reads. Client discards (answered without acting). The I/O path
makes no metadata round trip outside a cut.

**S24. Docs.** `docs/replication-protocol.md`: generations, the cut, catch-up, the new checks
and their catches (§9). ARCHITECTURE §13 as built. `README.md` and `docs/lab.md`: the commands,
the 63-snapshot limit, the charge, the lab check. `docs/followups.md`: audit events for snapshot
operations (#73's S7 leaves them to this issue), not in this step.

## Tests

- **S1–S6 (unit, Go; contracts):** every refusal and the idempotent request, an encrypted volume's
  included; the cut's conditions, one by one, and its order with `Attach`, a request's too; the
  caller rules, before and once #73 is active (an `audit-reader` or `deletion-override` caller
  refused a request; another node's data plane answered, not refused, a cut); the charge, its
  refusals (slots alone, a node without a report) and its release, across several transactions:
  with a move racing it, a node lost or a capacity initialization between them, a stop right
  after the request is recorded, each completed by a later request; a deletion while charging; an
  assignment after snapshots, one racing the request too; no cut before the charge is complete;
  growth of a volume with snapshots past the bound refused; a deletion stopped between each pair
  of its steps, then finished, and a re-creation; the commands against an in-memory service;
  transition vectors from the Rust model, replayed by the Go service.
- **S7–S10, S14 (unit, target):** scripted runs: the order of S8, reads served meanwhile, a cut
  refused, a cut without an answer, pending snapshots cut in order, a target started at a
  later generation.
- **S11–S13 (unit, replica; engine; fuzz):** the three cases of S12 against a store, the layer
  durable across a reopen at every crash point, a write below the top ending the session with
  the store unchanged; a full pool; the frames' round trip, with regression inputs.
- **S15–S18 (unit, target; engine):** a replica out across one and several cuts, a full
  catch-up into an empty chunk, a cut during each stage, a session lost mid-stage.
- **S19–S21 (store; engine; script test):** reads, digests and mapped ranges at each generation
  of a chunk with sparse layers, and of a chunk added at a later generation; S20 against a
  model, also across different cluster sizes; the offline reader refusing an earlier volume's
  chunk.
- **Simulator.** Its replicas keep layers. Snapshots are requested during chaos, in a share of
  the runs, in both campaigns. Four checks join protocol §9's:
  1. **the cut:** every block of every snapshot holds a value a read at the instant the cut was
     applied may return;
  2. **agreement:** at the end, every replica of every chunk holds identical blocks in every
     snapshot;
  3. **changes:** on every replica, S20's answer for consecutive snapshots covers every block
     whose value differs;
  4. **publication:** the catalog shows a snapshot committed only once its cut was applied, at
     the generation the cut applied, and every committed snapshot is listed until the volume
     goes.

  Each injected bug below must be caught by one of them, on a seed that passes clean:

  | Broken rule | Check |
  |---|---|
  | A cut with writes in flight (S8) | 1 or 2 |
  | A cut with a chunk not reconciled (S8) | 2 |
  | The metadata service applies a cut of an old epoch (S3) | 1 |
  | A replica writes a newer generation into its top layer (S12) | 1 |
  | Catch-up copies the current content only (S15) | 2 |
  | A replica not removed by this target is caught up at the last generation only (S16) | 2 |
  | A joiner keeps getting live writes across a cut (S17) | 2 |
  | A join proposed without rechecking the frontier after a new cut (S18) | 2 |
  | A reply of an older stage or session advances the catch-up frontier (S15, S18) | 2 |
  | S20 answers from the newest layer only (S20) | 3 |
  | A pending snapshot is shown committed before its cut is applied (S3) | 4 |

  S9 broken alone stalls writes (S12 refuses them): a target unit test, not a mutation.
- **The lab, a later step:** a volume under a load that records, per block, the last write
  acknowledged before the request and the first submitted after the cut. For each case, every
  replica's snapshot is identical and within those bounds (S21): a cut under load; a failover
  after it; a replica stopped across two cuts, then resynced; a move of a chunk with snapshots.
  **Measured** (performance plan): how long writes are held by a cut under load; write latency
  with no snapshot against main; the first small write to a cluster after a snapshot.

## Open questions

1. **Who makes the cut.** §13 reads as a bump by the metadata service. *Recommended:* the
   target, by S3, after an administrator's request (S3, S7–S10). A bump while writes are in
   flight or a chunk is unreconciled leaves snapshots that differ between replicas.
   Settled at approval, 2026-11-13: the target, by S3, on an administrator's request (see decisions.md, Settled at approval).
2. **A volume that is not attached.** *Recommended:* the snapshot waits for the next attach
   (S7). A bump by the metadata service alone is unsafe when the last target died with writes
   in flight. A target without a device (follow-ups §11) lifts the wait later.
   Settled at approval, 2026-11-13: the snapshot waits for the next attach (see decisions.md, Settled at approval).
3. **Catch-up without a format change.** *Recommended:* S15's order. The other way is a journal
   record that writes under a layer: an incompatible format feature.
   Settled at approval, 2026-11-13: S15's order (see decisions.md, Settled at approval).
4. **A cut during a catch-up.** *Recommended:* it does not wait (S17); a long move would
   otherwise delay every snapshot. The cost: the catch-up gains a stage.
   Settled at approval, 2026-11-13: it does not wait (see decisions.md, Settled at approval).
5. **How a snapshot is read in this step.** *Recommended:* per replica only (S19–S21). A
   read-only device comes with clones (step 4).
   Settled at approval, 2026-11-13: per replica only (see decisions.md, Settled at approval).
6. **Limits before deletion exists.** *Recommended:* 63 snapshots in a volume's life (S2), and
   a snapshot goes with its volume (S6). #52 and #58 meet this limit until step 3.
   Settled at approval, 2026-11-13: 63 snapshots in a volume's life; a snapshot goes with its volume (see decisions.md, Settled at approval).
7. **Upgrades.** The write and copy frames change, and the wire has no version. *Recommended:*
   nodes are upgraded together (S11), as the lab does.
   Settled at approval, 2026-11-13: nodes are upgraded together (see decisions.md, Settled at approval).
8. **A metadata outage during a cut.** Writes stay held until the answer (S9): safety over
   availability, as writes waiting on an ISR removal already do. *Recommended:* accept it. The
   other way, giving up a cut whose answer is lost, cannot tell an applied cut from a lost one.
   Settled at approval, 2026-11-13: accepted: writes stay held until the answer (see decisions.md, Settled at approval).
