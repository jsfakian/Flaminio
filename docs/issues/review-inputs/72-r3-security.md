# Issue #72 plan review, round 3 (incremental), design-security

Scope: `git diff 629bc32..bbdb51d -- docs/issues/72/ ':!docs/issues/72/report.md'` in
`/tmp/flaminio-plans-2026-11-12/72` (the round-2 fix 167ee08 and the approval bbdb51d), checked
against that worktree's code. #13's approved plan (`/home/jsfakian/Documents/src/flaminio-13`) is
read only where this plan relies on it ("#13 would delete them sooner").

## Earlier findings

- R2-S1 (completion released on ISR-only changes): fixed. `ChangeIsr` writes no ledger, and
  `CompletedMove` is `ApplyIsrChange`'s own condition (`domain.go:276–279`). `TestCompletedMoveOnly`
  covers the join, an ISR-only change, a removal and join plus drop.
- R2-S2 (interrupted creation leaks its claim): fixed. `capacity-claims/<volume>` is moved into the
  tombstone, and the mark keeps it stable (`service.go:539–551`, `:438`).
  `TestDeleteInterruptedCreationReleasesAll` covers it.
- R2-S3 (drain then undrain over-commits): fixed for copies recorded after this change. Left copies
  stay charged, and the second example and `TestDrainUndrainCreates` prove it. The fix opens R3-S1
  and R3-S6.
- R2-S4 (older binaries erase `charges`): partly fixed. Q3 and Compatibility now state the loss,
  but the repair they claim fails while the rewritten tombstone is pending (R3-S2).
- R2-S5 (unreadable ledger): fixed. Initialization targets unreadable ledgers, and `ConfirmDeletion`
  is specified by ledger state (`TestConfirmByLedgerState`).
- R2-S6 (administrators typed totals): fixed. A report is accepted only from the node's `io`
  identity, and the S10 matrix covers every identity.
- R2-S7 (full zone falls back): fixed, as settled. A side effect for misreports is in R3-S5.
- R2-S nit (a warning on every heartbeat): fixed.

### R3-S1 — High

**A left copy has no release except its volume's deletion. The remedy the plan names, #13, is
an approved plan that contradicts this one on the same record.**

1. **Charges strand when the copy disappears.** `MoveReplica` does not check that `from` is
   alive (`service.go:870–903`). So a rebuild or drain off a failed disk records that node in
   `left` for every chunk it held, and charges stay on its ledger (spec S3). When the disk is
   replaced under the same node id (`RegisterNode` keeps the existing record, design.md:43–45),
   the store is empty, yet L still holds the old disk's contents. The same happens to copies
   deleted by hand with `node.sh delete`, which followups §13 documents as the current practice.
   - `capacity-init --all` recomputes the same charge, because `VolumeCharges` counts `Left`
     (design.md:52, 105).
   - No RPC drops a `left` entry. A move to the node only turns `left` into `Moving.To`.
   - The node therefore refuses creates, and moves of every other chunk, rebuild moves that
     restore redundancy included, until each affected volume is deleted.
   - The plan does not state this case. design.md:191 (followups item 13) and Risks give only
     "stay charged until their volume is deleted".
2. **#13's plan names the same copies `Chunk.Released` (`released`).** It adds them inside
   `ApplyIsrChange` (`flaminio-13/docs/issues/13/design.md:58–64`), which this plan says is
   unchanged (design.md:55–56). #13 also refuses a move to a released node (S2, design.md:65–66).
   This plan admits that move for free and makes it a test (`TestMoveBackToALeftCopy`, spec S3).
   #13's `ConfirmRelease` removes the node in a transaction that is already at 120 compares
   (`design.md:71–78`) and subtracts no charge. Implemented as approved, each copy #13 deletes
   would stay charged permanently, and there would be two fields for one fact.

Fix:
- Reconcile with #13 now:
  - one field and name for the copy;
  - one rule for a move back;
  - #13's confirmation subtracts the charge in its own transaction, which means at most 118
    chunks so that the ledger compare and write fit.
- Until then, state the stranding in Risks and in followups §13, including that a hand deletion
  no longer frees capacity. Also give an administrator path, for example an `InitCapacity` option
  that drops a named node from `left` after a disk replacement, with its ledger in the same
  transactions.
- Add a test: a replaced disk with left copies, then a move of another chunk to it.

### R3-S2 — Medium

**`capacity-init --all` does not repair an older binary's tombstone rewrite while that tombstone
is pending. `TestInitAllRepairs` contradicts S6.**
An old `ConfirmDeletion` rewrites the tombstone without `charges` (`service.go:72–75, 635–661`).
S6 blocks every node pending on a tombstone without charges (spec.md:101–103). Each blocked node
gets back its ledger from before the call (design.md:108–109), and that ledger still holds the
charge. Its remaining confirmations release nothing, because `t.Charges[node]` is absent
(design.md:75). The ledger is exact only after a later `--all`, once the tombstone is gone.
- design.md:156 (`TestInitAllRepairs`: "`all` makes the ledgers exact"), design.md:213–214,
  decisions.md:76–78 and Q3 (spec.md:146–149) all claim an immediate repair.
- An old instance that drops `left` from a chunk has the opposite effect. `--all` then lowers L
  below a copy that still exists. That is the "copies no record names" risk, not a repair.

Fix: keep the charges in a key that older binaries never rewrite (`tombstone-charges/<id>`,
written with the tombstone, deleted at the last confirmation, ignored when no tombstone exists),
so a rewritten tombstone stays computable. Alternatively, state the two-phase procedure ("after
every tombstone rewritten by an older binary clears, run `--all` again") and make the test confirm
the remaining nodes before it asserts exactness. Correct design.md:214 for dropped `left`.

### R3-S3 — Low

**A confirmation that lands between the mark and the snapshot is lost on a blocked node, so
`--all` over-counts it permanently.**
- `ConfirmDeletion` on a marked ledger only rewrites the mark (design.md:78–79).
- A blocked target gets back "its readable ledger from before the call", comparing "the mark as
  the snapshot holds it" (design.md:108–110, contracts:40).
- A rewrite before the snapshot is therefore in the snapshot. The compare holds, and the restored
  ledger still includes a charge whose node has confirmed.

This breaks the invariant ("more only where S4 says", spec.md:29–30) and the ADR's "so the result
is exact". `TestInitRacingConfirm` uses only a node that is not blocked.

Fix: restore a blocked node only if its mark is still at the revision of Init's own mark write.
Any rewrite then leaves it marked. Add the blocked case to `TestInitRacingConfirm`.

### R3-S4 — Low

**A failed or raced initialization keeps nodes out of placement, rebuild included, and the CLI
still exits 0.**
- With `all`, step 1 marks every node (design.md:99–101), and a marked node takes no new copy
  (:114). An interrupted call (a cancelled client, a crashed instance) or one `ABORTED` after some
  batches leaves those nodes marked. Until an administrator reruns it, every create and every
  `MoveReplica` to them fails, including rebuild and drain moves.
- A run whose write lost a race answers `OK` with `initialized` false (:110–111). The CLI exits 0
  (contracts:124–125), so `scripts/lab.sh meta up` and an operator's script do not notice.
- Risks (design.md:201–203) says only that "the next run replaces" the marks.

Fix: on a failure, restore the earlier ledger of each node the call marked (compare its own mark
revision). Have the CLI exit non-zero when any target ends up marked or without a ledger. In
Risks, state that `--all` pauses every create and move until its writes land. Add a test that
cancels the call after the marks.

### R3-S5 — Low

**A node's data plane can misreport to attract or repel placement, nothing bounds the figures,
and Risks understates the effect.**
Only `T > 0`, `U ≤ T` and `slots used ≤ S` are checked (spec S1).
- **Attract:** an inflated T removes the node's cap. Once the honest nodes have no room, it is the
  only candidate with room. It then takes a copy of every create (all of an RF-1 volume) and every
  move that would otherwise be refused with `RESOURCE_EXHAUSTED`, and those copies then fail to
  allocate.
- **Repel:** a node that is its zone's only member and reports no room is still counted for the
  domain (R2-S7's rule, design.md:87–89). With zones = RF, it makes every zone-spread create in
  the cluster fail, `bestEffort` included. Going silent instead lets `bestEffort` fall back.

Risks (design.md:208–210) says only "fills its own store … or keeps copies off it". No test
covers either path.

Fix: state both effects and their bound (one compromised `io` identity, availability only, never
an acknowledgement). Optionally bound T by a figure the node cannot rewrite on its own, such as
the agent's device discovery.

### R3-S6 — Low

**A move back to a left copy skips S5's physical-use guard.**
design.md:63–66 checks `Room` only when `to` is not in `Left`. The contracts row "`MoveReplica` to
a left copy" compares only chunk and header. If the copy is gone (R3-S1's cases), the resync
allocates a whole chunk on a node that may be past its hard budget. A full store fails every
replica on it that needs to allocate, not only this one. `TestMoveBackToALeftCopy` checks only the
charge.

Fix: for a left copy, still require `U ≤ budget at hard` (no added charge, since a present copy is
already in U). Test a node past hard.

### R3-S7 — Low

**A nil room map means "no limit", a fail-open default in the one function every placement path
must use.**
design.md:85–86 makes a nil map unlimited "for the domain tests only". The adr-proposal requires
#70 and #76 to place through this code (adr-proposal.md:55–57). A future caller that builds no map
silently skips every capacity check, and no test would catch it.

Fix: make nil mean no room (fail closed), and give the domain tests an explicit unlimited map or
helper. Add a case where `Place` with a nil map refuses.

## Nits

1. design.md:78–79: "so a running initialization rereads" contradicts :110–111 ("leaves it
   marked, `initialized` false: run again"). It does not reread.
2. design.md:4 and :222 still say the ADR is "Proposed", and :215 says "Accepted: none. Open: the
   spec's Q1, Q3–Q5". adr-proposal.md:3 says "Accepted with the plan", and decisions.md:13–29
   settles Q1, Q3–Q5.
3. design.md:64–65: "it leaves `Left` in the chunk write" reads as "keeps `Left` unchanged". Say
   that `to` is removed from `Left` (spec S3: "it stops being a left copy").

Reviewed commit: 4aacc8e34c72106dbf566c4952fa6241fab82377 (plan documents as of bbdb51d)
