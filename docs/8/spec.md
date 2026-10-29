# Issue #8: DeleteVolume reads the chunks outside the tombstone's transaction — specification

## Behaviour

**S1. Which nodes are on a tombstone.** These are the nodes that the volume's chunk records name
when the deletion lists them:
- each chunk's replicas;
- while a chunk is moving, the move's source and destination. A destination may hold a partial
  copy before it becomes a replica.

This is as today. A completed move's source is no longer named by any record. It is not on the
tombstone, and its copy is issue #13's (intent, out of scope).

**S2. A deletion in two steps.**
1. **Mark.** `DeleteVolume` marks the volume's header as deleting, in a transaction conditional
   on the header it read. From then on the volume does not exist for any other operation: it
   reads as `NotFound` and is not listed.
2. **List and delete.** It then lists the chunks. In one transaction, conditional on the marked
   header, it deletes the header and writes the tombstone. It deletes the chunk records after,
   as now.

**S3. No chunk changes after the mark.** Moves (`MoveReplica`) and ISR changes (`ChangeIsr`)
compare the header's revision in their transactions.
- **Fenced by the mark:** one that read the header before the mark fails its compare. On its
  retry it finds the volume gone (`NotFound`). One that starts after the mark finds it gone at
  once. So the listing in S2 is final, and nothing can retry against it.
- **Recorded before the mark:** a move or ISR change committed before the mark is in that
  listing.
- **ISR changes add no unlisted node:** an ISR change can only add a chunk's moving destination
  to its replicas, or drop its moving source (`validMove`). Both are named while the chunk is
  moving.

**S4. A deletion interrupted between its two steps** leaves the header marked. The volume stays
gone for every other operation, and its id cannot be reused. The next `DeleteVolume` of it finds
the mark, and does step 2.

**S5. A move needs the header it read.** A move's transaction compares the header's revision, so
that it is applied only to a volume that exists, unchanged since the move read it. An attach in
between makes the move read again. A deletion in between fails it with `NotFound` (S3).

**S6. The drain skips a deleted volume.** A drain step whose move of a volume's chunk gets
`NotFound` (the volume was deleted after the step listed it) goes on to the next volume, instead
of failing the step.

**S7. Unchanged.**
- **Deleting during a move:** a volume can still be deleted whatever its chunks are doing.
- **Contention:** the mark conflicts only with other writes of the header (attaches). A drain's
  moves no longer delay a deletion.
- **The tombstone:** the volume's id is not reused while it remains.

## Tests

`meta/service/service_test.go`, each against the in-memory store and against etcd:
- **A move before the mark.** A test store runs a move of chunk 0 to a node that holds no
  replica of it, right after `DeleteVolume` reads the header, before it marks it. The tombstone
  lists that node.
- **Moves after the mark.** A move run when `DeleteVolume` lists the chunks (after the mark) gets
  `NotFound`. The tombstone lists the replicas, and not the refused move's destination. Without
  the mark, the move would be applied after the listing, and the test fails.
- **Moving chunk.** A volume deleted while a chunk is moving: the tombstone lists the move's
  destination.
- **Attach in between a move's read and write.** The move writes twice, the first refused on the
  header, and is applied. The volume ends at the attach's epoch and target.
- **Deletion in between a move's read and write.** The header is deleted directly, as by a
  deletion. The move returns `NotFound`, and the chunk is not moving.
- **Interrupted deletion.** With the header marked directly:
  - `GetVolume` and `MoveReplica` return `NotFound`, `ListVolumes` leaves the volume out, and a
    create with its id is refused;
  - `DeleteVolume` then finishes: no header, and a tombstone for the replicas.
