# Issue #72 plan review, round 4 (incremental), design-security

Scope: `git diff bbdb51d..8439357 -- docs/issues/72/ ':!docs/issues/72/report.md'` in
`/tmp/flaminio-plans-2026-11-12/72`, checked against that worktree's code, and against #13's
approved plan (`/home/jsfakian/Documents/src/flaminio-13/docs/issues/13/`, at 7defc59) only where
the revision relies on it.

## Earlier findings

- R3-S1 (left copies strand; two models of one copy): fixed in this plan. `left`, `CompletedMove`
  and the free move back are gone; the copy is #13's `released`, a move to it is refused by #13's
  `CheckMove`, and `ConfirmRelease` carries the release rule. Two things remain. #13's approved plan
  still says 119 chunks (R4-S1). The stranding while release deletion is off is stated in design
  Risks but not in spec S11 (R4-S3).
- R3-S2 (`--all` while a rewritten tombstone is pending): fixed. The two-run procedure is stated in
  S6, Q3, the contracts' upgrade order and the ADR, and `TestInitAfterOldBinary` confirms the
  remaining nodes before it asserts exactness.
- R3-S3 (confirmation between mark and snapshot lost on a blocked node): fixed. Every write compares
  this call's own mark revision (`TxnRev`, `meta/kv/kv.go:45–49`). `TestInitRacingConfirm` covers a
  blocked node and an absent ledger. Its side effect is in R4-S2.
- R3-S4 (marked nodes, CLI exit 0): fixed. `NodeCharge.marked`, the CLI exits 1, and Risks states
  the pause under `all`. The blocked case is in R4-S2.
- R3-S5 (misreports): fixed. Risks states both effects and their bound.
- R3-S6 (move back skips the physical-use guard): fixed. No move charges nothing any more, and a
  move to a released node is refused before any charge.
- R3-S7 (nil room map fails open): fixed. Nil has no room, the tests use `domain.Unlimited`, and
  `TestPlaceRoom` has the nil case. `Place`'s only caller is `service.go:404`.
- R3-S nits 1–3: fixed.

### R4-S1 — Medium

**The 118-chunk bound conflicts with #13's approved plan. Implementing #13 as approved strands
the charges of any volume with 119 or more released chunks on a node.**

- #72 states, as a fact about #13, that "#13's sweep confirms in batches of 118" (design.md:81–83).
  The contracts row gives `n ≤ 118` (capacity-contracts.md:34).
- #13's approved plan says 119 everywhere:
  - `ConfirmRelease` takes "1 to 119 chunks" (`flaminio-13/.../design.md:72`, `:628`);
  - the sweep sends `ConfirmRelease(node, volume, <= 119 chunks)` (`:142`);
  - its test makes 120 the refused case (`:674`);
  - decisions.md:248 says "One `ConfirmRelease` still takes at most 119 chunks". #72's own fact
    list repeats 119 (design.md:18).
- The sweep is #13's S6, a node rule. #72's S11 depends only on #13's S1–S5 (spec.md:127–129).
  "Parallelizable?" says `ConfirmRelease`'s part waits for #13's, so #13 may land first, as
  approved.
- #72's Files touched names no node-side file (`flaminio-node` sweep or its batch constant), and no
  test checks the node's batch against the service's bound. The change #13 needs is recorded only
  in the batch notes (`docs/issues/batch-2026-11-12.md`, commit 2a2bdc2), "#13 not edited".

What happens:
- **The service refuses 119.** A node that keeps #13's 119-chunk batch gets `InvalidArgument` for
  every first batch of a volume with 119 or more deleted released chunks. #13's sweep "skips that
  volume or batch" and the next sweep repeats the same batch (`flaminio-13/.../design.md:150–152`).
  The release is never confirmed, and the charge is never released.
- **The service keeps 119 and adds the ledger.** That is 121 compares. etcd rejects it
  (`meta/kv/etcd.go:91`), but the in-memory store does not, so tests on Mem pass.

The case is realistic: draining a node that holds a volume of 30 GiB or more at 256 MiB chunks.
Once full, that node refuses every create and every move to it, rebuild moves included, until the
volume is deleted.

Fix:
- Have #13's plan amended to 118 (service bound, sweep batch, `TestConfirmRelease`'s limit cases)
  before either plan is implemented. Alternatively, put those #13 files and tests into #72's
  Files touched and its S11 dependency, #13's S6 batch included.
- Add an engine test: a volume with 119 released chunks is confirmed in two batches against a
  service that takes 118. Run `TestConfirmReleaseReleasesCharge` on both stores, with etcd
  enforcing 120.
- Correct design.md:18 or :83 so that the plan does not state two bounds for #13.

### R4-S2 — Low

**A blocked node without a readable ledger stays marked, and the CLI says "run it again", which
cannot help while the blocker stands. A concurrent release turns a blocked node with a valid
ledger into such a node.**

- S6 and design step 4: a blocked node gets back its earlier readable ledger "or else stays
  marked" (spec.md:104–110, design.md:114–117). The CLI then exits 1 with `capacity-init: still
  marked: <nodes>; run it again` (capacity-contracts.md:127–128). `marked` reads "run again"
  (:83).
- A rerun finds the same tombstone without charges, or the same unparseable header, and leaves the
  node marked again. A whole-cluster `meta up`, where that failure is fatal (design.md "Files
  touched", scripts/lab.sh), then fails on every run until the blocker clears. In an upgraded
  cluster every node starts with no ledger, so one legacy tombstone pending on a node is enough.
- With the R3-S3 fix, any release on a blocked node between its mark and the write fails the
  restore. The release rule only rewrote the mark (design.md:77–78), so the earlier ledger is lost.
  Every later run sees a mark, not a readable ledger, and leaves the node marked. A healthy node
  then takes no copy until the blocker clears. For an unparseable header, that lasts until an
  administrator repairs or deletes the volume.

Fix:
- Report blocked rows apart from marked ones. The CLI names the blocker
  (`blocked by <tombstone|volume>: wait for its confirmations or repair it, then run again`).
- Keep the pre-call ledger in the mark (`{"init":true,"prev":{…}}`). The release rule subtracts
  its charge from `prev` when it rewrites a mark, and a blocked node is restored from the current
  mark's `prev`.
- Test: a blocked node, a release between mark and write, gets its ledger back less that release.
  A CLI test covers a blocked row.

### R4-S3 — Low

**Spec S11 says #13's sweep releases a hand-deleted or replaced-disk copy, but #13's sweep does so
only with release deletion on. While it is off, nothing can release that charge.**

- spec.md:132–134: "Until #13's release deletion is on …, released copies stay charged, as they
  stay on their disks; a copy deleted by hand or lost with a replaced disk is released by #13's
  sweep, which confirms it absent."
- #13 confirms absent releases only in its S6 step 2, "when release deletion is on". Release
  deletion is off by default, and off before any downgrade (`flaminio-13/.../spec.md:48–49,
  84–90`).
- Only the node may call `ConfirmRelease`. Administrators "only see releases"
  (`flaminio-13/.../spec.md:77–79`, `design.md:102`). `capacity-init --all` counts `Released`
  again (design.md:57–58, 111).
- So with deletion off, the documented cleanup (`node.sh delete`, docs/followups.md:248–249) frees
  physical space but no charge. A replaced disk keeps the charges of every released copy until each
  volume is deleted. Its node refuses creates and moves of other chunks meanwhile.
- design.md:220–221 states this correctly ("with release deletion on"). The spec, decisions.md
  (round 3, first entry) and the planned followups text (design.md:201–204) do not.

Fix: in S11, decisions and the followups text, say that while release deletion is off a
hand-deleted or lost released copy stays charged until its volume is deleted, and that hand
deletion no longer frees capacity. If that is not acceptable, give an administrator path.

### R4-S4 — Low

**#13's fence-only slots take a slot that no ledger charges. The plan counts them in "slots used"
but nowhere else.**

- The revision adds fence-only slots to slots used (spec.md:13–14, design.md:40–41).
- #13's `ConfirmRelease` turns the last released copy of a volume on a node into a fence holder:
  `Header.Fenced`, and a state-4 slot "taken everywhere" by the allocator until a tombstone or
  #30's drop (`flaminio-13/.../spec.md:31–33, 68–69`, `design.md:59, 285–289`).
- #72's release rule subtracts n slots, and `VolumeCharges` and Init count replicas, `Moving.To`
  and `Released`, never `Fenced` (design.md:57–58, 81–83). So L under-counts slots by the number
  of fence-only slots. That contradicts the invariant's "L bounds its physical use plus its
  unwritten commitments" (spec.md:35–36).
- The over-commit is the smaller of the fence-only slots and the unwritten commitments. The 2×
  slot sizing (`flaminio-format/src/consts.rs:47–51`) makes it an edge case: it takes over 1,600
  such slots on a 100 GiB store. Risks does not mention it.
- The contracts say a tombstone's `charges` has "one entry per node in `nodes`"
  (capacity-contracts.md:21–23). #13's tombstones also name fence holders, for which
  `VolumeCharges` gives no entry. No test has a fence holder.

Fix: charge one slot and zero bytes per `Fenced` entry. `ConfirmRelease` then subtracts n − 1
slots when it adds the node to `Fenced`, and `VolumeCharges` and Init count it. Alternatively,
state the gap and its bound in the invariant and Risks. Either way, define the tombstone entry of
a fence holder, and test a deletion and an initialization with one.

### R4-S5 — Low

**`ConfirmRelease` is the new release path, but no #72 test covers a stale or replayed
confirmation, or one on a marked ledger.**

- `TestConfirmReleaseReleasesCharge` lists only "118 chunks released at once, 119 refused, a
  racing create rereads" (design.md, S4/S11 row).
- `TestReleaseRuleByLedgerState` and `TestInitRacingConfirm` name "a confirmation" in general.
  #13's `TestConfirmRelease` asserts records, not ledgers.

Nothing checks the following:
- a `ConfirmRelease` that is stale (chunk revision moved, node no longer in `released`) or
  replayed after it applied leaves the ledger unchanged;
- one that names a chunk that does not release the node (`InvalidArgument`) writes nothing;
- one that lands between `InitCapacity`'s mark and write leaves the node marked, and the rerun
  is exact.

Fix: add these cases to `TestConfirmReleaseReleasesCharge`, and make `TestInitRacingConfirm` run
with both `ConfirmDeletion` and `ConfirmRelease`.

## Nits

1. design.md:227–228: "An agent or administrator confirming early releases that node's charge".
   After #13's S12, which this plan requires, the agent loses `ConfirmDeletion`. The party that
   can confirm early is the node's own `io` identity, through `ConfirmDeletion` and
   `ConfirmRelease`.
2. design.md:220: "Replicas still named on a replaced disk stay charged until moved off". Moving
   one off makes it released, still charged, until #13's sweep (with release deletion on) confirms
   it absent. The text reads as if the move freed it.

Reviewed commit: b13ee9cbcf5b2de4decfdade8ccb971b78befec3 (plan documents as of 8439357)
