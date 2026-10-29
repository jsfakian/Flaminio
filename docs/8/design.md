# Issue #8: DeleteVolume reads the chunks outside the tombstone's transaction — design

## The change

**The header** (`meta/domain`) gets `Deleting bool`, next to `Creating`. `readHeader` treats a
header with either as absent (`NotFound`), and `ListVolumes` leaves it out, as for `Creating`.
So every operation that reads the volume through `readHeader` sees it gone once a deletion has
marked it. That covers attaches, ISR changes, moves and reads.

**`DeleteVolume`** (`meta/service/service.go`). In the existing retry loop:
1. **Read** the header (`kv.Get`, not `readHeader`, so that it sees a marked one).
2. **Mark.** If the header is not marked, write it with `Deleting` set, conditional on its
   revision. On a conflict, read again. Then read it once more, for the marked header's
   revision.
3. **List** the chunks and compute the tombstone's nodes (`replicaNodes`), as now.
4. **Commit.** In one transaction, conditional on the marked header's revision, delete it and
   write the tombstone. Then delete the chunk records, as now.

A deletion that finds the header already marked (an interrupted one, S4) starts at step 3.

**`MoveReplica`.** Its transaction compares the chunk's revision, as now, and the header's
revision from `readHeader`. That is what fences it off a deletion (S3). Its doc comment says so.

**`ChangeIsr`** compares the header's revision already. No change, except that a chunk record
missing because the volume is gone is `NotFound` (`chunkMissing`, which `MoveReplica` uses too).

**`createVolume`.** After claiming the id with its `Creating` header, it reads that header's
revision once. Every chunk batch, and the final write that clears `Creating`, compare it. Before,
the batches had no compare, and the final write compared a fresh read. A deletion's mark in
between now stops the creation. A claim that fails because a deleting header holds the id
answers `FailedPrecondition`.

**The drain** (`control/drain/drain.go`). Each volume's moves are in `stepVolume`, whose progress
the step adds up only for volumes still there. `NotFound` from `GetVolume` skips the volume. On
`NotFound` from `MoveReplica`, it reads the volume: if that is `NotFound` too, the volume was
deleted and is skipped, uncounted. Otherwise, as for an unregistered destination, the error
fails the step.

**The package doc's key list** gains `drain/<node>`, which it was missing.

## Why this, and not the alternatives

- **A per-volume move record** (the first design): `MoveReplica` bumps
  `/flaminio/v1/moves/<id>`, and the deletion compares it. It works, but during a drain of the
  volume a move lands every few milliseconds. A deletion lists every chunk on each attempt, so
  each attempt can be invalidated, and all 32 lost: `Aborted` for as long as the drain runs. It
  also adds a key family whose lifecycle must follow the header's. With the mark, a deletion
  conflicts only with attaches, and once marked, no move can commit.
- **Rewriting the volume header on each move.** This has the same starvation. Every attach and
  ISR change of the volume also conflicts with every move.
- **Compare every chunk's revision in `DeleteVolume`.** A transaction may hold at most
  `kv.MaxTxnOps` (120) compares, so this fails for volumes over 119 chunks (about 29.75 GiB at
  256 MiB a chunk).

## Costs

- **A deletion is two transactions,** and an extra read of the header.
- **Moves and attaches** conflict through the header's compare: an attach during a move makes the
  move read again. Attaches are rare.
- **A crash between the two steps** leaves a marked header, which hides the volume until the
  deletion is retried. The CSI provisioner retries a deletion that did not return success.

## Tests

`meta/service/service_test.go`. `interleaveKV` runs a hook after the first `Get` of a key, or
`List` of a prefix. `interleaveStores` runs each test against a wrapped in-memory store and a
wrapped etcd.
- **A move before the mark:** hook on `Get` of the header (the deletion's first read). The move
  of chunk 0 from a to c is applied, and c is on the tombstone.
- **Moves after the mark:** hook on `List` of the chunks. The move returns `NotFound`; a and b are
  on the tombstone, c is not.
- **Moving chunk:** move, then delete: c is on the tombstone.
- **Attach in between:** hook on `Get` of chunk 0's key (`MoveReplica`'s read after
  `readHeader`), attaching to b. Two writes of the chunk outside the hook; epoch 2, target b,
  chunk 0 moving to c.
- **Deletion in between:** same hook point, deleting the header directly. `MoveReplica` returns
  `NotFound`; chunk 0 has no move.
- **Interrupted deletion:** the header written marked directly. `GetVolume` and `MoveReplica`
  return `NotFound`, `ListVolumes` is empty, and `CreateVolume` gets `AlreadyExists`. Then
  `DeleteVolume` leaves no header, and a and b on the tombstone.
