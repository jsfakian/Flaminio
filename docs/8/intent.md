# Issue #8: DeleteVolume reads the chunks outside the tombstone's transaction — intent

[Issue #8](https://github.com/jsfakian/Flaminio/issues/8), follow-ups §8.

## Why

Deleting a volume leaves a tombstone: the list of nodes that held its chunks. Each of those
nodes deletes its copies and confirms, and the tombstone goes once all have. A node missing from
the tombstone keeps its copy forever.

`DeleteVolume` lists the volume's chunks to find those nodes, then writes the tombstone, and
deletes the volume, in a transaction that checks only the volume header. A replica move recorded
between the list and the transaction is not in the tombstone. Its destination, which the
volume's target may already be copying to, keeps that copy.

Today a deleted volume is never attached, and only attached volumes move, so this cannot happen
yet. Draining volumes that are not attached (issue #17) will make it possible.

## What we want

1. Every node that the volume's chunk records name, as a replica or in a pending move, when the
   deletion takes them is in its tombstone, however a move and the deletion interleave.
2. A volume can still be deleted while one of its chunks is moving.
3. A deletion is not held up by moves of the volume, such as a drain's.

## Out of scope

- **Copies the records no longer name.** A completed move's source keeps its copy, whether or
  not the volume is deleted later, and no record names it any more, so no tombstone can. A
  move's destination that fails without a deletion is the same. Both are issue #13, replicas
  left by moves.
