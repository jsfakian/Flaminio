# Issue #72 plan review, round 6 (incremental), design-code and design-security

Scope: `git diff dee77a1..8c718f4 -- docs/issues/72/ ':!docs/issues/72/report.md'` in
`/tmp/flaminio-plans-2026-11-12/72` (HEAD 559eef3 differs from 8c718f4 only in report.md).
Checked against `meta/kv/kv.go:18-49`, `meta/kv/mem.go:70-89` and `etcd.go:90-115` (a put of
the same value moves the revision on both stores), `meta/service/service.go` `ConfirmDeletion`
(`:620-671`), `auth.go:77-78`, #13's approved plan (7defc59: design.md:58-104, spec.md S12, S17)
and #70's design ("Metadata growth", "Charges", `:41-91`, `:208`).

Interleavings walked (InitCapacity mark M / snapshot / write, against `ConfirmDeletion` and
#13's `ConfirmRelease`), for a node with a readable ledger, with none, blocked with `prev` and
blocked without: a release before the mark is in `prev` and the snapshot; one between mark and
snapshot or snapshot and write rewrites the mark, so the write or restore at M fails and the node
stays marked with the release once in `prev`; a release racing the revision-0 mark fails one of
the two compares and the release retries onto the mark. No release is lost or counted twice in
any case; two concurrent calls cannot overwrite each other's mark. Every changed transaction is
conditional and within 120 (`ConfirmDeletion` 2/2, `ConfirmRelease` 120/120, restore 1/1, mark
batches 120/120). The defects left are in the CLI's advice and in the growth rule's design.

## Earlier findings

- R5-B1 (release on a mark without `prev` wrote nothing): fixed (contracts rule 3, design:77-78,
  `TestReleaseRuleByLedgerState`, `TestInitRacingConfirm`'s node with none).
- R5-B2 (restore compared the revision just read): fixed (contracts rule 2, design:116-118; ADR,
  decisions consistent).
- R5-B3 (`ConfirmDeletion` by the data plane needed #13's S12): fixed (S4, design:81-83, Risks,
  test row; S11 stays S1-S5; `auth.go:77-78` agent or admin today).
- R5-B4 (restored blocked node exited 0 silently): fixed (contracts:137-143, S10 test); the line
  choice for a raced blocked node is R6-B1.
- R5-B5 (no initialization fence-holder fixture): fixed (design:167).
- R5 Nit 1 (#13 spec.md:105): fixed. Nit 2 (restore's 32nd try): fixed, no retry. Nit 3
  (`had_ledger` with `prev`): fixed. Nit 4 (`TestInitBlockedWithoutPrev` rerun): fixed.

## Findings

### R6-B1 — Low

**A blocked node whose restore lost its compare to a release gets the "wait for its
confirmations or repair it" line, though a plain rerun restores it at once.**

- Contracts rule 2 (capacity-contracts.md:48-49) and design step 4 (design.md:116-118): the
  restore compares this call's mark, with no retry, so a release between mark and restore leaves
  the node `marked` with `blocked_by` set.
- The CLI maps every `marked` row with `blocked_by` to `blocked by <id>: wait for its
  confirmations or repair it, then run it again` (capacity-contracts.md:140-141; the proto comment
  on `marked`, :93, says the same). A plain rerun targets the marked node, re-marks it with the
  same `prev` and restores it (S6), with no need to wait.
- While marked, the node takes no copy, rebuild and drain moves included (design Risks). When the
  blocker is an unparseable header, repair can take an operator a long time. This case is
  realistic during Q3's upgrade, when `--all` runs while sweeps confirm.

Fix: choose the line from `had_ledger`. If the row is marked, blocked and `had_ledger`, print, for
example, `capacity-init: <node> blocked by <id>: raced a release: run it again`. If it is marked,
blocked and not `had_ledger`, keep today's line. Add the raced blocked node to the S10 CLI test.

### R6-B2 — Low

**The exit code now depends on `blocked_by`, but no text says whether rows the call did not
target carry it.**

- capacity-contracts.md:137: "Exit 0 when no row is `marked` or has `blocked_by`". `:92`: "the
  tombstone or volume that blocked it". spec.md:110-117 defines "blocked" per node, from the
  snapshot, and reports "its blocker" per node. design.md:119 gives one row per registered node.
- A readable node that a plain run does not target can still be pending on a tombstone without
  charges. If the implementation fills `blocked_by` for it, the run exits 2 and prints `kept its
  earlier ledger`, although the node was not touched. That case affects lab step 1, which needs
  `capacity-init` to exit 0 (design.md:182), and `meta up`'s warning (design.md:143-145). Before
  this diff, such a row had no effect on the exit code.

Fix: state the scope. Either `blocked_by` is set only for this call's targets, or it is set for
every row, and then lab step 1 and the docs say that exit 2 can name untouched nodes. Add a
non-target row to the CLI or `TestInitCapacity` case.

### R6-B3 — Low

**S3's new growth rule has no approach, file or test in this design. If #72 lands after #70,
its implementer has nothing in #72's plan to build from.**

- spec.md:60-61: "Whichever of #70 and #72 lands second adds that charge and its test". The design
  does not mention growth. Charging and release (design.md:53-84) has no growth bullet, Files
  touched (:124-149) has no `meta/service/expand.go`, and the S3 test row (:163) has no growth
  test. The spec's Tests (spec.md:168-177) do not cover growth either.
- #70's design (`:41-42`, `:74-77`, `:208`) says the ledger's lander also removes `ExpandVolume`'s
  step-1 refusal and adds `TestExpandChargesLedger`. #72 states neither.
- intent.md:50 still lists Growth under "What stays open in #72". capacity-contracts.md:12-14
  (Writers) and the Transactions table leave out growth's ledger write.

Fix: add a design bullet that applies when #70 has landed: `ExpandVolume`'s step charges per
#70's "Charges", and the change removes its step-1 refusal. Add `meta/service/expand.go` to Files
touched, and #70's `TestExpandChargesLedger` (with `TestExpandRefusals` updated) to the S3 row.
List growth among the contracts' writers. Move the intent line out of "stays open", or reword it.

## Nits

1. design.md:143-145 ("nodes still marked (exit 2) a warning naming them") and design.md:200-202
   ("`capacity-init` exits 2 on a node left marked") are stale: exit 2 now also covers a restored,
   unmarked blocked node.
2. design.md:167: `TestInit121Nodes` dropped "both stores", but it is the one test of the
   120-node mark batch. capacity-contracts.md:43-44 says tests that fill a row run on both stores,
   with etcd enforcing 120.
3. design.md:167 ("leaves the node marked with `prev` less the release") and spec.md:163-164
   ("with the release in its kept value") also cover "a node with none", which has no `prev`. For
   that node, assert that the mark's revision moved and that the rerun is exact.

Reviewed commit: 559eef3311a396ed286a4c28b1fe664c90cd2d89 (plan documents as of 8c718f4)
