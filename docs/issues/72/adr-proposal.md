# Placement charges a per-node capacity ledger in its own transaction

- **Status:** Accepted with the plan, 2026-11-13; binding for this plan's implementation from
  the approval on; numbered and moved into docs/adr/ only with the implementation.
- **Date:** 2026-11-12, revised 2026-11-13 (design review, rounds 1 to 5; rounds 3 to 5 after
  the approval)
- **Scope:** Issue-local; no global ADR number is assigned until the implementation.

## Context

[Issue #72](https://github.com/jsfakian/Flaminio/issues/72) requires that concurrent creates,
growth and moves never over-admit capacity, with ARCHITECTURE §11.6's watermarks. Placement
counts replicas, not bytes, and no record holds a node's committed space. This first step keeps
one store per node, the current create, move and deletion paths, and the replication protocol
as they are. Later paths (#45's rebuild, #70's growth, #76's recoverable creation, #78's
admission gate) will place replicas too, and need one rule to follow.

## Decision

- **A ledger per node** in etcd, `capacity/<node>`: the charge of every copy the records name
  on it, written or not, one chunk size and one slot per copy. A copy a move leaves on its
  source stays named and charged as #13's released copy (the chunk's `released`); a creation's
  claim names its placements (`capacity-claims/<volume>`) until it completes.
- **Every placement that names a copy on a node** adds its charge in the same conditional
  transaction, comparing the ledger revision it read. A charge leaves only in the transaction
  that confirms the node deleted the copies: a tombstone's confirmation, or #13's
  `ConfirmRelease`, which therefore carries the ledger too.
- **Admission** requires both the ledger and the store's reported physical use, plus the
  charge, to fit a watermark's share of the store: soft for creates, hard for moves. The report
  rides the data plane's liveness heartbeat and expires with it.
- **Initialization** recomputes ledgers from one snapshot of the records, writing only where
  its own mark is still in place. The mark keeps the value it replaced; a concurrent release
  subtracts from it and always rewrites the mark, so no write by a call that read before the
  release lands, and a rerun or a restore never loses a release (the contracts' one rule).
- **No lock, lease or long-lived operation.** A conflict is a failed compare, reread and retried
  like every other metadata write. Nothing changes on the data plane's I/O path.

## Options

- **Per-node ledger in the placement transaction (chosen):** exact under concurrency, cheap per
  move; costs one compare and one write per touched node, so a create may touch at most 116
  nodes, and needs a one-time initialization of existing nodes.
- **Ledger derived by scanning the records at each admission:** nothing to drift or initialize,
  but a full scan per move, and no charge for a creation whose chunk records are not written
  yet.
- **Release a move's source when the move completes** (round 1): the copy stays on the disk, so
  a drain then undrain let creates be promised on top of it, up to 140 % of a store. Rejected.
- **A record of left copies of #72's own** (round 2): a second model of #13's released copies,
  with a move back #13 refuses. Rejected for #13's.
- **A class-wide cooperative operation with grants installed on the nodes** (the first draft):
  it held a class lock across acknowledgements that arrived by polling, so one slow or dead
  node stalled every create, move and rebuild of its class. Rejected for this step.
- **Per-device escrow:** for several stores per node and per-class policy; not needed with one
  store per node.
- **Telemetry only:** no protection against two creators reading the same free space.

## Consequences and open choices

Released copies stay charged until their node confirms deleting them through #13; this plan is
implemented with or after #13's metadata rules and a change to #13's approved plan (spec S11):
`ConfirmRelease` takes 118 chunks at most and writes the ledger. Until #13's sweep confirms
releases, a released copy deleted by hand or lost with a disk stays charged. Copies no record
names, from before this change, count only through physical use. Old metadata-service
instances charge nothing and drop the charges of tombstones
they rewrite: an upgrade replaces every instance, then reinitializes, twice if such a tombstone
was pending. Watermarks are fixed in this step. Future placement paths must charge through the
same ledger, in their own transaction: #45's rebuild already does through `MoveReplica`; #70 and
#76 must, and #76 must keep the claim's charges. The [spec](spec.md)'s questions were settled at
approval, 2026-11-13 (see decisions.md, Settled at approval).
Accepted ADRs 0001, 0002, 0004, 0005 and 0006 remain consistent; none is superseded.

## Confirmation

[Design](design.md) and [contracts](capacity-contracts.md): room arithmetic at every boundary;
interleaved creates, moves and confirmations on both stores that fail without the ledger
compares; a drain, undrain and creates that stay within the budget; release once per
confirmation and per `ConfirmRelease`; initialization never wrong against racing releases; the
Rust report read back through the Go service; and `make capacity-check` in the lab.
