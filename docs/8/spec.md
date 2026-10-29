# Issue #8: DeleteVolume reads the chunks outside the tombstone's transaction — specification

## Behaviour

**S1. Which nodes are on a tombstone.** These are the nodes that the volume's chunk records name
when the deletion commits:
- each chunk's replicas;
- while a chunk is moving, the move's source and destination. A destination may hold a partial
  copy before it becomes a replica.

This is as today. A completed move's source is no longer named by any record. It is not on the
tombstone, and its copy is issue #13's (intent, out of scope).

**S2. A move after the list is seen.** Recording a move (`MoveReplica`) changes the volume's move
record, `/flaminio/v1/moves/<volume>`, in the same transaction that marks the chunk as moving. A
deletion compares that record's revision, along with the header's, and deletes both atomically.
So a deletion that listed the chunks before the move fails its compare, lists them again, and
includes the move's destination.

**S3. A move needs the header it read.** A move's transaction compares the volume header's
revision. It is applied only if the volume still exists, unchanged since the move read it.
- **A deletion in between** fails the move with `NotFound`, the volume being gone.
- **An attach in between** makes the move read again.

Without this compare, a move could be recorded for a volume deleted meanwhile. The move record
would then outlive the header: it would never be deleted, and would block nothing.

**S4. Other chunk changes need nothing.**
- **ISR changes are covered.** An ISR change can only add a chunk's moving destination to its
  replicas, or drop its moving source (`validMove`). Both were on the tombstone while the chunk
  was moving.
- **A source dropped before the deletion lists the chunks is not on it** (S1).
- **After the deletion,** an ISR change fails, because the header is gone.

**S5. Unchanged.**
- **Deleting during a move:** a volume can still be deleted whatever its chunks are doing.
- **The rest of `DeleteVolume`:** deleting the chunk records after the transaction, and keeping
  the volume's id from being reused while the tombstone remains.
- **Attaches and ISR changes** neither read nor write the move record, so recording a move never
  makes them fail. A move still retries after an attach (S3), and conflicts with an ISR change of
  the same chunk through the chunk's compare, as today.

## Tests

`meta/service/service_test.go`:
- **Racing move.** A test store runs a `MoveReplica` of chunk 0, to a node that holds no replica
  of it, right after `DeleteVolume` lists the chunks and before it writes. The tombstone lists
  that node. Without S2, it does not, and the test fails.
- **Moving chunk.** A volume deleted while a chunk is moving: the tombstone lists the move's
  destination. This holds today, and the test keeps it so.
- **Attach in between.** The test store runs an `Attach` to another node right after
  `MoveReplica` reads the chunk, before it writes. The move's first write fails its compare on
  the header and the second is applied: two writes to the chunk, where there would be one
  without S3's compare. The volume ends at the attach's epoch and target.
- **Deletion in between.** At the same point, the test store deletes the volume's header
  directly, as a `DeleteVolume` would before it deletes the chunk records. `MoveReplica` returns
  `NotFound`, and no move record is left.
- **The move record.** `MoveReplica` changes the move record's revision and leaves the header's
  revision as it was. `DeleteVolume` removes the move record.
