# Plan review #72, round 4, design-code (incremental: bbdb51d..8439357)

## Round-3 findings
- R3-C1 (blocked node restored against the snapshot's mark): the compare is fixed (own mark, `TxnRev`),
  but the fix opens R4-1 below.
- R3-C2 (charge with no release path), R3-C3 (drained or retired node): fixed by #13's `released` and
  `ConfirmRelease`; the administrator's confirmation is tested (`TestDeleteReleasesOnConfirm`).
- R3-C4 (limit tests): fixed. `backends` (service_test.go:47) runs a real etcd (`kvtest.Etcd`), which
  `etcd.go:91` limits to 120; Mem does not, so only the etcd run can fail. Tests added.
- R3-C5 (`meta up`): fixed. `cmd_meta` loops `vms_of "${2:-cluster}"`; chaos passes `w`, so "no VM argument"
  is the whole-cluster case.
- R3-C6 (status lines): fixed.
- 118-chunk arithmetic: holds against `kv.MaxTxnOps` 120 (header + 118 chunks + ledger = 120 compares; 118
  chunk puts + header + ledger = 120 writes). The 116-node claim (2 + k = 118) is unchanged.

## Findings

### R4-1 — Medium: a blocked node can stay marked for good; a rerun cannot restore it
design.md "Initialization" step 4: a blocked node gets "its readable ledger from before the call ... else
it stays marked", written only while the ledger holds this call's own mark. Any release in the window
(`ConfirmDeletion`, or #13's `ConfirmRelease`, which the sweep issues every 5 s) rewrites the mark, so
the write fails and the node stays marked. The old value is gone: the rerun finds a marked ledger, which
is not readable, so "else stays marked" applies again, and the node is blocked again for as long as its
blocker lasts. An unparseable-header volume or a legacy tombstone with a gone node never clears. The node
takes no copy (drain and rebuild moves included); the CLI exits 1 each run and the plan's remedy ("run
again") cannot work. `TestInitRacingConfirm` says "a rerun exact" for the blocked case, which cannot hold.
Fix: have the mark carry the previous value (`{"init":true,"prev":{..}}`), let the release rule subtract
from `prev`, and restore from it; or restore the pre-call value less the releases that landed. Test: a
blocked node, a confirmation between mark and write, then a rerun that restores it.

### R4-2 — Low: the `ConfirmRelease` change is not fully stated, and the plans disagree meanwhile
- #13's design.md:71, :142 and :628 (and its tests) say 1 to 119 chunks; #72 needs 118. #72's
  `TestConfirmReleaseReleasesCharge` "119 refused" contradicts #13's accepted plan until #13 is amended.
  Say in S11 that #13's plan must change those places (1–118, sweep batch 118, the new compare and
  write), and record it on #13.
- #13's step 5 handles only a moved header or chunk. A ledger-only conflict (every create or move on the
  node rewrites it) falls into "every chunk at its listed revision: only the header moved, retry" by
  accident; with #13's "nothing is retried within a sweep" a busy node's releases wait a sweep each time.
  State the rule: a failed ledger compare retries as a header move does. The release rule's read of the
  ledger is also an extra read per attempt that #13's "one read per node in flight" bound (design.md:105)
  does not count.

### R4-3 — Low: "Parallelizable? Yes" overstates
Only `ConfirmRelease`'s part is said to wait for #13. `VolumeCharges` reads `Chunk.Released`,
`DeleteVolume` and `InitCapacity` count released nodes, and `TestCompletedMoveKeepsSourceCharged`,
`TestMoveToReleasedNodeChargesNothing`, `TestInitCapacity`'s fixture and the lab step 4 all need #13's
field and `CheckMove` rule. The Go domain and service units do not build without them. Say they follow
#13's metadata rules (S11 already says "with or after").

## Nits
1. design.md "Reports and ledgers" says `ConfirmDeletion` "adds the release rule" and the S4 row says "by
   that node or an administrator"; the lab step 5 relies on the sweep for fl-b's released copy, which is
   #13's tombstone delete. Say so once.
