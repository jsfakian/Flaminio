# Issue #30: a volume created again starts above every earlier epoch — intent

[Issue #30](https://github.com/jsfakian/Flaminio/issues/30). Found while planning #13, #14 and
#15; they build on this plan and drop their own versions of it.

## Why

A node knows a volume by its name alone, and a volume created again with the same name starts
where the first one did.

1. **The node's identity of a volume is its name.**
   - The store keys chunk replicas by a volume UUID that is a SHA-256 of the name
     (`volume_uuid`, `flaminio-node`).
   - A replica's `Hello` creates only the placed chunks it lacks, and keeps those it has.
   - It refuses outright a volume whose size or chunk size differs from what it holds.
2. **Every volume starts at epoch 0.** `NewVolume` leaves the epoch at 0, and each attach adds
   one (`Header.Attach`). A volume deleted after 7 attaches, and created again, gets epoch 1 at
   its first attach.
3. **So an earlier volume of the name looks like the new one.** On a node that still holds the
   earlier volume's replicas:
   - **its fences:** replicas fenced above the new epoch answer `Stale`, and depose the new
     target;
   - **its data:** replicas fenced at or below the new epoch are kept, and the new volume serves
     the old volume's blocks;
   - **its tokens:** capability tokens name only the volume, the epoch and the node, and never
     expire. An old token whose epoch the new volume reaches again opens a session on it.
4. **Today only tombstones (#8) stop this.** A name stays taken until every node named on its
   tombstone has deleted its replicas. Nodes the tombstone does not name (#13: a replica moved
   away and kept), and nodes outside Kubernetes, which never confirm (#14), are not covered.

**Raising the epoch alone is not enough.** A first epoch above every earlier one ends the
deposition and the token reuse. But it puts every leftover replica at or below the new epoch,
which is the case that is adopted. The node must also tell where the new volume begins.

## What we want

1. **A volume's first epoch is above every epoch any earlier volume of its name reached.** It
   is the store's revision of the claim that creates the volume: every attach is one header
   write, so a volume's epoch never passes the revision of its last header write.
2. **A replica drops what an earlier volume of the name left.** The capability token carries
   the volume's first epoch, signed. A node whose record of the name is fenced below it holds
   only an earlier volume's chunks, and deletes them before it opens the session.
3. **What this closes and what is left,** stated in the spec.

Done when a volume deleted and created again with the same name attaches on a node that still
holds the old volume's replicas or fences, neither adopts their data nor is deposed by them,
and an old capability token is refused.

## Out of scope

- **Tokens that never expire.** An old token stays valid on a node the new volume has not
  reached, against the old volume's leftovers only. Expiry or revocation is #15's and later.
- **Leftover replicas on nodes the new volume never reaches.** They are harmless to it, but
  hold space until a sweep deletes them (#13).
- **Re-creating a name before its tombstone is confirmed.** Tombstones keep blocking it. #14
  could relax that only once the sweep is bounded by epoch: today it deletes every replica of
  the name, past any fence, and would wipe the new volume's.
- **The token key on every node.** Root on any node can sign tokens for any epoch, as before.
- **A new etcd key** (a per-name high-water mark, or a global counter): the revision does the
  job.
- **Changing the on-disk volume UUID.** The name keeps identifying the volume on a node; the
  first epoch tells its incarnations apart.
