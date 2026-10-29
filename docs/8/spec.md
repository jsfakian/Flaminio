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
gone for every other operation. The next `DeleteVolume` of it finds the mark, and does step 2.
A create with its id does the same, since the deletion's caller may never return, and then
answers `Aborted`: the id is free once the tombstone is confirmed.

**S4a. A deletion during a creation.** A creation writes its chunk records, and finally clears its
own mark, each conditional on the header it claimed. A deletion's mark changes that header, so
the creation stops there with an error. It adds no chunk after the deletion's listing, and does
not bring the header back.

**S4c. A header that does not parse** is deleted without a mark: no move or ISR change can
commit against it, as `readHeader` fails on it.

**S4d. Chunk records are deleted only while they are the deleted volume's.** After a deletion,
its chunk records are deleted conditional on the header still being absent. A creation's
cleanup of leftovers is conditional on its own claim. Neither can delete a newer creation's
records.

**S4b. A chunk missing because the volume is gone.** A move or an ISR change that read the header
before a deletion, and finds a chunk record gone, answers `NotFound`, as for any operation on a
deleted volume, not `Internal`.

**S5. A move needs the header it read.** A move's transaction compares the header's revision, so
that it is applied only to a volume that exists, unchanged since the move read it. An attach in
between makes the move read again. A deletion in between fails it with `NotFound` (S3).

**S6. The drain skips a deleted volume.** A volume deleted after a drain step listed it, found
at its read (`GetVolume`) or at a move of its chunk, is left out of the step: the step goes on to
the next volume, and its progress does not count the deleted one. A move's `NotFound` for another
reason, such as an unregistered destination, still fails the step.

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
    create with its id gets `Aborted`;
  - `DeleteVolume` then finishes: no header, and a tombstone for the replicas.
- **Deletion during a creation.** A 130-chunk creation, with a deletion run after its first batch
  of 120 chunk records commits: the creation fails with `Aborted`, and the tombstone names the
  first batch's nodes.
- **A deletion's retry** after chunk records were left behind clears them, and answers
  `NotFound`.
- **Volume deleted mid-move.** A deletion run when `MoveReplica` reads the header: the move gets
  `NotFound`.
- **Damaged header.** A header overwritten with bytes that are not JSON: `DeleteVolume` removes
  it and tombstones the replicas.

`meta/kv/kv_test.go`, against both stores: `TxnRev` gives the revision of the keys it wrote,
a compare at it holds, and a failed compare gives 0.

`control/drain/drain_test.go`: a volume deleted before the step's move of it, and one deleted
before the step reads it, are both skipped, and the step counts only the other volume. A move
refused with another `NotFound` fails the step.
