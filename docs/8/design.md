# Issue #8: DeleteVolume reads the chunks outside the tombstone's transaction — design

## The change

`meta/service/service.go`, with a new key `moveKey(id) = /flaminio/v1/moves/<id>`. Its value
does not matter, only its revision: it changes whenever a move of the volume is recorded.

**`MoveReplica`.** It reads the header with `readHeader` already, which returns its revision.
The transaction compares the chunk's and the header's revisions, then writes the chunk and puts
the move record:

```go
h, hrev, err := s.readHeader(ctx, r.GetVolume())
...
ok, err = s.kv.Txn(ctx,
	[]kv.Cmp{{Key: e.Key, Rev: e.Rev}, {Key: volumeKey(h.ID), Rev: hrev}},
	[]kv.Op{{Key: e.Key, Value: b}, {Key: moveKey(h.ID), Value: []byte("1")}})
```

When the compare fails, the move reads again, as it does for a chunk conflict. If the volume is
gone, `readHeader` returns `NotFound`.

**`DeleteVolume`.** It reads the move record, along with the header, before it lists the chunks.
Its transaction compares both revisions and deletes both:
- **The read:** `Get(moveKey)`. Its revision is 0 when the volume has never had a move.
- **Compares:** the header's revision and the move record's. A `Cmp` with `Rev 0` requires the
  key to be absent (`meta/kv/kv.go`).
- **Ops:** delete the header, delete the move record, put the tombstone, as now.

**Why every write changes the revision.** etcd gives every put a new `ModRevision`, and `kv.Mem`
gives every op the store's next revision, even when the value is the same. So `MoveReplica`'s put
always invalidates a deletion that read the move record before it.

**No record outlives its volume.** The move record is written only in a transaction that
compares the header, so only while the volume exists. It is deleted in the transaction that
deletes the header. A volume recreated later with the same id, once its tombstone has gone,
starts without one.

## Alternatives

- **Rewrite the volume header on each move.** It has the same effect on deletions, but every
  attach and every ISR change of the volume compares the header too. A drain records moves back
  to back, one per chunk replica on the node: about 2,700 for a 1 TiB volume at 256 MiB a chunk.
  The target's ISR changes read the header and up to 64 chunk records before they write. They
  could then lose all 32 attempts and return `Aborted`. The target sends again only after its
  metadata timeout (2 s), and client writes waiting on an ISR removal would wait with it.
- **Compare every chunk's revision in `DeleteVolume`.** A transaction may hold at most
  `kv.MaxTxnOps` (120) compares, and as many operations. With the header's compare, that leaves
  119 chunks: about 29.75 GiB at 256 MiB a chunk. Larger volumes would need another path.
- **Refuse the deletion while a chunk is moving.** This closes nothing by itself: a move could
  still start after the check. It would also make deletion wait on a move (intent, 2).

## Costs

- **Moves of different chunks** do not conflict: a move compares its chunk and the header, and
  only writes the move record. Moves of the same chunk conflict through the chunk, as today.
- **Moves and attaches** conflict through the header's compare: an attach during a move makes
  the move read again. Attaches are rare.
- **Deletions and moves** conflict by design (S2). During a drain's burst of moves of the same
  volume, a deletion, whose window spans its listing of every chunk, can lose all 32 attempts
  and return `Aborted`. The CSI provisioner retries a failed deletion.

## Tests

`meta/service/service_test.go`:
- **The store wrapper.** `interleaveKV` gains a hook on `List` of a prefix, next to its hook on
  `Get` of a key.
- **Racing move.** A volume on nodes a and b, with node c registered. When `DeleteVolume` lists
  the volume's chunks, the hook moves chunk 0 from a to c. `ListTombstones` for c then lists the
  volume.
- **Moving chunk.** Move chunk 0 to c, then delete: c's tombstones list the volume.
- **Attach in between.** Attach the volume to a. The hook sits on `Get` of chunk 0's key, which is
  `MoveReplica`'s read after `readHeader`, and attaches the volume to b. The wrapper counts the
  writes to chunk 0's key outside the hook, reset after setup. There must be 2: one that fails
  its compare on the header, and one that is applied. Without the compare there would be 1.
  `GetVolume` then shows epoch 2 with target b, and chunk 0 moving to c.
- **Deletion in between.** The same hook point. The hook deletes the header key directly
  (`store.KV.Txn`), as a `DeleteVolume` interrupted before it deletes the chunk records. A full
  `DeleteVolume` in the hook would not test this: deleting the chunk records would fail the
  move's chunk compare on its own. `MoveReplica` returns `NotFound`, and `Get(moveKey)` finds
  nothing.
- **The move record.** Across a `MoveReplica`, the move record's revision rises and the header's
  does not. After `DeleteVolume`, the move record is gone.
