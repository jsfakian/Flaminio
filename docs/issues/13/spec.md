# Issue #13: Replicas left behind by moves — specification

[Intent](intent.md), [decisions](decisions.md), [design](design.md). Builds on issue #30 (done):
a volume created again starts above every earlier epoch, and drops what earlier ones left.

## Terms

- **Copy:** a chunk replica in a node's store.
- **Released:** the chunk's record names the node as having a copy to delete.
- **Fence holder:** a node a volume's record names as having confirmed a release of it, which
  keeps its fence once it has no chunk (S3, S10).
- **`NotHeld`:** a replica's reply for a chunk it does not hold: the chunk's record as read
  from the metadata service (the volume's epoch, the chunk's ISR version, replicas and ISR).
- **Forgotten:** a node an administrator marked as gone for good (S19).

## The invariant

A node deletes a copy only once released there, while no local target of the volume or a later
one places it there; a target still asking for it learns of the drop from the node, accepting
only an open proposal of its own that drops the sender (S17, S18). Only the node confirms, after
deleting; a tombstone names every node a record names, until that node is forgotten (S19).

## Behaviour

### The metadata service

**S1. A drop releases the source.** The change that drops a move's source from a chunk's
replicas releases the source's copy, atomically with the drop. Nothing else releases a copy.

**S2. No move back while released.** A move of a chunk to a node it releases is refused, as
other refused moves are. A release and a move of the same chunk never both commit. **Its bound,**
within #80's budget: a chunk holds two releases at most, one at most beside a move; a move of a
chunk holding two is refused, with the reason `holding 2 releases of chunk C`, until one is
confirmed or its node, gone for good, forgotten (S19).

**S3. Confirming a release,** only by the node itself (S12), names releases as listed; if a record
changed since, nothing is confirmed. It makes the node a fence holder (16 at most a volume; a
later node is confirmed and not named, its fence listed by S8), bumps no ISR version, and is
refused for a volume deleted or being deleted (its tombstone covers the node).

**S4. Listing releases.** A node lists its releases by volume, at most 1024 a call, the rest in
later sweeps; not those of volumes being created or deleted, nor records a deletion left.

**S5. A volume's deletion.** Its tombstone also names every released node and every fence
holder. Each deletes all it holds of the volume, fence included, and confirms, or is forgotten
(S19); until then the id stays taken. Releases and fence holders cannot change once the deletion
has begun.

### The node

**S6. The sweep,** `flaminio-io node`'s loop, every 5 s, as the node's identity (S12), on its
engine, one metadata call outstanding at most, reading per volume:
1. **Tombstones:** the volume's copies here are deleted, refused while it is attached here, then
   confirmed, as by the CSI node plugin today.
2. **Releases,** when release deletion is on (S14): those listed are deleted (S7), but for a
   volume step 1 handled; those deleted or absent are confirmed at their listed revisions.

**S7. Deleting released chunks.** The node deletes a listed chunk's copy only if:
- **in the volume's read for this deletion** (one covers its listed chunks), the chunk's record
  releases this node, and neither has it as a replica nor in a move;
- **at that chunk's deletion:** the read is at most 5 s old (else the next sweep reads again);
  no target on this node places it here; the volume's fence here is at or below the epoch read;
  nothing deleted the volume's chunks or fence here since the read.

One volume at a time, confirmed once done, keeping the fence and other chunks, even while
attached here. Each chunk stalls I/O by a settle (a held group too, #10's S5) plus its commit.

**S8. Listing copies.** The control socket lists the node's copies and fences by volume hash,
for root on the node and the CSI node plugin's pod, which shares the socket's directory.

**S9. A session creates chunks only when it raises the fence,** every missing one named, with the
fence (a failure leaves the old state; #30's drop stays); a target's first session does. Else only a
resync creates one (S17). An accepted `Hello` sets the shape, kept with none held. Its reply has a
row per chunk held: a named chunk without one is not held there, and gets nothing from the reply;
the session opens.

**S10. A fence outlives the last chunk,** across restarts, until a tombstone or #30's drop: an
older target stays refused. One per volume and node; older binaries then refuse the store (S14).

### Sweepers, rollout, failure

**S11. One sweeper per node:** its data plane's; only one opens a block device (a file-backed
store, tests only, has no such guard). The CSI sweep goes; an old one is refused. No tombstone is
confirmed during a release deletion of its volume. A node gone for good keeps its ids taken, and
its releases count toward S2's bound, until an administrator forgets it (S19).

**S12. Callers.** Only a node's data plane identity lists and confirms its node's releases and
tombstones (the CSI pod holds no other); administrators only see releases, keep today's
tombstone rights, and alone forget a node (S19).

**S13. Issue #14's step 2 goes** (`vol-confirm`, `flaminio-meta confirm`, the stop for fl-c).
`delete_volume` waits for the nodes' sweeps, up to a limit; a timeout names the pending nodes.

**S14. Rollout and rollback.** Metadata, then CSI without its sweep, then the data plane (no
sweeps, so no lab deletions, between). **Release deletion is off unless started on,** until every
metadata instance and node is new, and before any downgrade: an old instance drops releases it
rewrites, so a move back can commit (no S2) and, at RF 1, the copy deleted may be the only one.
Likewise no node is forgotten (S19) until every metadata instance is new: an old one ignores the
mark. With release deletion off, a moved chunk keeps its source's release: a chunk moved twice
moves again only once one of those nodes runs with release deletion on and deletes, or, gone
for good, is forgotten (S2's bound), and never back to one of them (S2); a drain then waits,
saying why. A live node is cured by release deletion, not by forgetting.
Only the operator enforces this; changing it restarts the node's data plane. `NotHeld` is sent
whatever the option: an old target fails its session on it at once, not at a dropped request's
timeout. **No rollback** once a release, fence holder or fence-only record exists.

**S15. Failure and retry.** A deletion refused or failed is not confirmed, and is retried; a
confirmation refused or lost is redone by the next sweep. Without the service, nothing is done.

**S16. Ordering.** A slow or cut-off proposer learns of its drop by S17-S18. The drain never asks
a move S2 refuses: a chunk at S2's bound, or with only nodes that release it left to move to,
waits, counted, with its reason; a move a late release refused is asked again.

### The replication protocol

**S17. A replica answers `NotHeld` for a chunk it lacks.** A request at its current session for a
chunk in range that the node lacks (but a resync's start, which creates it) is answered from a read
of the volume's record: `NotHeld` if the record does not name the node as a replica; else, if the
read fails, or if the volume is gone, dropped, as today; out of range, dropped unread. Nothing
deleted is remembered, so a restart loses nothing.
- **Bounded:** one read per node in flight besides the sweep's (S6), so a `ChangeIsr` waits behind
  two at most; waiters join their volume's read, holding no payload, within bounds per volume and
  node, volumes in turn (a gone one not for 3 s); the rest drop. **In time:** a read fits the 3 s
  request timeout; if not, the session fails, and the resend on reconnect (0.5 s on) is answered
  from the last read under 3 s old (only requested chunks not naming the node).

**S18. A target adopts a `NotHeld` record only if it drops the sender, keeps every other ISR member
and equals an open proposal,** epoch and version included (authentication is always on). Open: a
change it sent for the chunk at its view's version, an identical resend kept once, in its first
place, at most 4 distinct per chunk (the oldest evicted); a timeout closes none. A seen reply closes
its request's proposal for each chunk the request named; the view passing that version closes all
the chunk's; a dropped change (a move cancelled) closes its own. Only the target at its epoch
commits ISR changes, so it has committed (or, the service reading header and chunks apart, a later
target's identical one, as a refresh could: fencing and leases cover both). A `NotHeld` from a
sender the view, as now held, no longer names fails the request on that member, session kept: reads
and digests requeued, resync rounds aborted, writes per #11 S11. Any other fails the session.

### Forgetting a node

**S19. An administrator forgets a node gone for good.** First the node is marked forgotten, for
good. From then on no move goes to it, no volume is placed on it, it does not register, its data
plane lists and confirms no release, a drop no longer releases it, and no tombstone written
names it. Then it leaves every release, fence-holder list and tombstone; a tombstone left with
no node pending goes, freeing the id. A volume being deleted is left to its tombstone, which
does not name it; a volume being created keeps it until a later run, and the call names such
volumes. Its copies stay where they are, and its replicas are moved off as any other. Only for
a node gone for good: forgetting retires its id for good.

## Tests

- **S1–S5, S12** (service, in memory and on etcd; `auth_test.go`; the model's vectors, `make
  contracts`): each rule's cases, stale and late confirmations, the limits, S2's bound and its
  reason.
- **S19** (service, in memory and on etcd; `auth_test.go`; the vectors): after the mark, each
  refusal (a move to it, placement, registration, its data plane's listing and confirmation, a
  drop's release, a later tombstone naming it); the clearing, in batches, a tombstone completed,
  a volume being deleted left to its tombstone; a move back, a confirmation, a placement and a
  deletion racing the forgetting, each in the interleaving its compare closes.
- **S6, S11, S14, S15** (engine, fake metadata client): release deletion off by default, failures
  not confirmed, tombstones ordered against releases, nothing deleted without the service; an old
  CSI sweep refused; a stalled client holds a `ChangeIsr` behind two calls; `NotHeld` sent anyway.
- **S7–S10** (store, format, replica service, engine): each refusal of S7 (a read over 5 s old
  defers); deletion across re-creation and under I/O; sessions at and above the fence, failed and
  crashed; the fence-only record across restarts, replay and deletion.
- **S17, S18** (replica service, target, engine): `NotHeld` only when the record does not name the
  node; joined, bounded (one volume cannot fill it), round-robin, no payload held; dropped on a
  failed read or a gone volume (not reread for 3 s); a resend answered in time from the cache, of
  chunks not naming the node only; a stale `Hello` sets no shape; a reconnect naming a deleted
  chunk opens its session, that chunk given nothing. Refused: a rejected or dropped exact
  proposal, the fifth distinct, after the view moved on, a record naming the sender, a drop with a
  pending removal; a stale reply ignored. Accepted: a drop, its release confirmed, after
  `meta_timeout`, rebuilt after a lost first one, the first distinct after five identical resends.
  Reads, digests, `Mapped` probes answered `NotHeld`: requeued or aborted, session kept, also after
  the view learned the drop first. **Cut-off proposer:** drop committed, answer lost, target cut
  off, chunk idle, copy deleted: its next write gets `NotHeld` and goes on, no acknowledged write
  lost; also after the source restarts.
- **Simulator:** a drop releases the source, whose sweep reads the model later and deletes
  later still, as S7's decision (shared with the node's sweep) allows, then confirms; a `Hello`
  creates only as S9 allows; restarts keep fences; relayed reads split header and chunks; one
  node forges records (version+1 with another ISR, another epoch, a closed exact proposal, an
  open one naming it or removing another member), never an open one without them. An adopted
  record was committed at that version (at the target's epoch, or later with it deposed). Each
  injected bug (epoch, sender or other members unchecked, any record at view+1, proposals left
  open after a reply or a drop, a `Hello` creating at the fence, a fence lost with the last chunk,
  a move while released, a node's releases cleared without the mark) breaks it, or
  linearizability, in some run.
- **S13** (`make lint`): `lab-deletion-test.sh` (no stop for fl-c; the wait; a timeout names
  the pending nodes); `node-scripts-test.sh` loses its `confirm.sh` cases. **S16** (drain): no
  move S2 refuses, no released destination; a chunk at the bound, or left only released nodes,
  waits, its reason in the progress; the step counts releases.
- **The lab** (Done when; S11, S14; waits of 60 s at most; release deletion on, without which the
  drain's move back waits), after rollout and hand cleanup:
  draining fl-b to fl-c and back under verified writes leaves no copy of `vol1` there, nor a
  release; item 3's check (RF 1, 16 GiB, fl-a to fl-b) leaves fl-a only its fence-only slot; with
  fl-b's release deletion off, a move back is refused, then taken (fio verifies), and a deleted
  volume's tombstone names fl-b until it sweeps, then created again reads zeros; no CSI sweeps.

## Open questions

None.
