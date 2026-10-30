# Issue #13: Replicas left behind by moves — specification

Depends on issue #30: a volume created again starts above every earlier epoch of its name, and
a node drops what an earlier volume of the name left. The mechanisms are in the
[design](design.md); settled choices and accepted risks in [decisions](decisions.md).

## Terms

- **Copy:** a chunk replica in a node's store.
- **Released:** the chunk's record names the node as having a copy to delete.
- **Fence holder:** a node the volume's record names as keeping the volume's fence without a
  chunk of it (S10).
- **Unnamed copy:** a copy no record names: the node is not a replica of the chunk, not in its
  move, not released, and the volume is on no tombstone pending for the node.
- **Grace:** 10 s: longer than a drop's proposer can take to learn that its drop committed, and
  for its earlier requests to time out (7 s).

## Behaviour

### The metadata service

**S1. A drop releases the source.** The change that drops a move's source from a chunk's
replicas releases the source's copy, atomically with the drop. Nothing else releases a copy.

**S2. No move back while released.** A move of a chunk to a node it releases is refused, as
other refused moves are. A release and a move of the same chunk never both commit.

**S3. Confirming a release.** Only the node's own agent confirms its releases.
- A confirmation names the releases as listed. If a chunk's record changed since, nothing is
  confirmed; the agent lists again next sweep. A late confirmation never removes a newer release.
- Confirming makes the node a fence holder of the volume, if it is not one already.
- A confirmation for a volume deleted or being deleted is refused: its tombstone covers the node.

**S4. Listing releases.** A node's agent lists its releases, by volume, at most 1024 per call;
the rest come in later sweeps. Volumes being created or deleted, and records left by a deletion,
are not listed.

**S5. A volume's deletion.** Its tombstone also names every released node and every fence
holder. Each deletes all it holds of the volume, fence included, and confirms; until then the
id stays taken, so no new volume of the name adopts a released copy. Releases and fence holders
cannot change after the deletion has begun.

### The node agent

**S6. The sweep.** The node agent sweeps every 5 s, one agent per node, as an identity of its
own (S12). A sweep, step by step:
1. **Tombstones:** delete the volume's copies on the node, then confirm, as the CSI node plugin
   does today.
2. **Releases:** a release is deleted once the agent has listed it continuously for the grace;
   one that a sweep does not list, that was confirmed, or whose volume a tombstone step handled,
   starts again. Then the agent confirms those deleted, or already absent.

It deletes only what that sweep listed; without the metadata service, nothing.

### The data plane

**S7. Deleting released chunks.** The node deletes a chunk's copy on request only if, read from
the metadata service for this request:
- the chunk's record releases this node, and neither has it as a replica nor in a move;

and, at the moment of each chunk's deletion:
- no target on this node places the chunk here;
- the volume's fence on this node has not risen since that read, and the volume was not deleted
  on this node since;
- the request's deadline has not passed. A request not begun by its deadline never runs.

One request per volume at a time: a second, while one is pending, is refused. So a request
cannot outlive the agent that sent it and act after a later confirmation.

The deletion keeps the volume's fence and its other chunks. A later move to the node creates the
chunk afresh, empty. A chunk already absent counts as deleted. It may run while the volume is
attached on the node, this node its target included, and does not stall its I/O. The answer
names the chunks deleted and those refused.

**S8. Listing copies.** The node lists its copies and fences, by volume hash. Only root on the
node, and the CSI node plugin's pod, which shares the socket's directory, can ask. The hash
confirms a guessed name.

**S9. Opening a session creates chunks only when it raises the fence.** A session opened at the
epoch the node is already fenced at creates no chunk: the chunks it names that the node lacks
are not held, and its requests for them fail. Otherwise chunks are created only by a resync.
- A session that raises the fence creates every missing chunk it names. A crash leaves all of
  them and the fence, or none and the old fence. A failure leaves nothing: fence, chunks, session.
- Every target start takes a new epoch, so a new target's first session to a node raises the
  fence. A reconnect, and one after the node restarts, finds its chunks.
- The node records the volume's shape from a session even when it holds no chunk of the volume,
  so that a resync can create a chunk after a restart.

**S10. A fence outlives the last chunk.** When a node deletes its last chunk of a volume, it
keeps the volume's fence, across restarts. A target of an older epoch is refused, after a
restart too. One fence per volume and node, until the volume's tombstone deletes it, or #30's
drop of an earlier volume's leftovers. The store then refuses the binaries from before this
change (S14).

### Sweepers, rollout, failure

**S11. One sweeper per node.** Only the node's agent confirms releases and tombstones for its
node: the CSI node plugin's sweep goes, and an old one still running is refused. A tombstone's
confirmation frees the id; the agent never gives one while a release deletion of that volume
runs. A node gone for good has no confirmer: its volumes' ids stay taken (decisions).

**S12. Callers.**
- **The sweep identity,** a node's agent's alone: lists and confirms its node's releases and
  tombstones. The CSI node plugin's identity no longer may.
- **Administrators** see releases in a volume's record, and confirm none. The service still
  accepts an administrator's tombstone confirmation; no tool sends one.
- **The data plane** reads volumes, as it may today.
- Anyone else, any other method: refused, as now.

**S13. Issue #14's harness** (it lands first) loses its hand confirmation and its stop for fl-c.

**S14. Rollout and rollback.** Metadata service, then data plane, then the sweep identities,
the CSI driver without its sweep, and the agents, back to back.
- **From the metadata service's rollout until the agents run, no node sweeps:** no volume
  deletion in the lab then, or its tombstone wait times out.
- **Until every metadata instance is new,** an old one may erase releases and fence holders it
  rewrites: those copies become unnamed, those fences unlisted. Harmless with #30; listed by S8.
- **Rollback is not supported** once a release or fence holder exists (metadata) or a fence
  without a chunk has been written (data plane). The agents stop first in any case.

**S15. Failure and retry.**
- A deletion refused, failed or timed out is not confirmed; a later sweep tries again.
- A confirmation refused or lost: the next sweep lists, deletes (nothing there), confirms.
- The data plane down: nothing deleted or confirmed.
- A node that never comes back: its releases stay. They keep the chunk off that node only, and
  show in the drain's progress.

**S16. Ordering.** The other cases are argued in the design.
- **The drop's proposer, before it learns of it:** the grace outlasts it. Should it reconnect
  anyway, with its stale view, the chunk is not created again (S9); its requests fail, and a read
  goes to another in-sync replica. No client read returns zeros.
- **The drain:** a node a chunk releases is not a destination for it; with no other, the chunk
  waits. A release that lands after the drain's read refuses the move; a later step retries.

**S17. Unchanged:** what the intent's out of scope lists.
## Acceptance (the lab, after the rollout)

Each wait polls every 5 s, for at most 60 s.
- **Hand cleanup, once,** recorded in the PR: the unnamed copies on every node deleted (S8),
  only where no record names the node for the volume.
- **Drain:** `drain fl-b` into fl-c under the verified write load, fio verified. Then fl-b holds
  no copy of `vol1`, and `vol1` no release. The same for fl-c after the drain back.
- **Item 3's check again:** an RF 1, 16 GiB volume with 64 MiB written per chunk, its target on
  fl-a, moved chunk by chunk to fl-b. Then fl-a holds no copy of it, only its fence-only slot,
  counted apart from chunk slots; its chunk slots and clusters in use are as before the volume.
- **Move back:** with fl-b's agent stopped, a chunk moved off fl-b and back: refused. The agent
  started: the move is accepted, and fio verifies.
- **Delete and create again:** with fl-b's agent stopped and a release held, delete the volume:
  the tombstone names fl-b, and creating the id is refused until fl-b's agent runs. The new
  volume reads zeros wherever the old one held data.
- **One sweeper:** no CSI node pod sweeps; a second agent on a node exits. Issue #14's
  `bench-writes` and `vol delete` with `RF=2` stop for no tombstone.
