# Issue #72 plan review, round 5 (incremental), design-code and design-security

Scope: `git diff 8439357..dee77a1 -- docs/issues/72/ ':!docs/issues/72/report.md'` in
`/tmp/flaminio-plans-2026-11-12/72`. Checked against that worktree's `meta/kv` (`kv.go:31–49`,
`etcd.go:90–91`), `meta/service/service.go` (`ConfirmDeletion`, `:627–671`), `auth.go:75–78`,
`meta/cmd/flaminio-meta/main.go:69–81`, `scripts/lab.sh:509–535`, and #13's approved plan
(`/home/jsfakian/Documents/src/flaminio-13/docs/issues/13/`, 7defc59) only where the diff relies
on it.

## Earlier findings

- R4-1 (blocked node marked for good): fixed for a node whose mark keeps `prev`; the same
  revision opens R5-B1 for a mark without `prev`.
- R4-2 (#13 change not fully stated): fixed; S11 lists the change and the places (Nit 1).
- R4-3 (Parallelizable overstated): fixed.
- R4-C Nit 1 (confirmer, lab step 5 sweep): fixed.
- R4-S1 (118 vs #13's 119): fixed as a stated prerequisite (S11); the 120-operation totals hold.
- R4-S2 (blocked node, "run it again" useless): fixed (`prev`, per-node exit-2 lines); the
  exit-0 case of a restored blocked node is R5-B4.
- R4-S3 (hand deletion frees nothing while release deletion is off): fixed.
- R4-S4 (fence-only slots uncharged): fixed in the rules; the initialization test asked for is
  missing (R5-B5).
- R4-S5 (stale, replayed, non-releasing `ConfirmRelease`): fixed.
- R4-S Nits 1–2: fixed.

## Findings

### R5-B1 — Medium

**A release on a mark without `prev` writes nothing, so an initialization that read its
snapshot before that release writes a charge the node has already released. The release is
lost (a permanent over-charge until another `--all`).**

- design.md:79–80, the release rule: "Absent or unreadable, or a mark keeping no value: the
  revision compared, no write." Round 4's rule rewrote every mark ("Being initialized: the mark
  rewritten, so a running initialization's write fails"); this revision narrowed it.
- design.md:117–119: a non-blocked target's write compares this call's own mark revision, and
  relies on a release having rewritten the mark to fail.
- Sequence (`kv.Cmp` compares revisions only, `kv.go:18–22`): node X has no ledger (every node
  in an upgraded cluster, or an unreadable ledger, or a mark whose `prev` an underflow dropped).
  `InitCapacity` marks X `{"init":true}` at revision R; its snapshot sees tombstone T, written by
  a new instance with `charges`, pending on X; X's sweep calls `ConfirmDeletion` (or
  `ConfirmRelease`): the rule compares R and writes no ledger, T records X confirmed; Init's write
  compares R, which still holds, and writes the recorded charge, T's included. T will never
  release X again.
- This contradicts spec S6 and its tests ("a `ConfirmDeletion` or `ConfirmRelease` racing it
  leaves a node marked for a rerun that is exact", spec.md:157–159), the invariant's "more only
  where S4 says" (spec.md:32–33) and the ADR's "a rerun or a restore never loses a release"
  (adr-proposal.md:32–33). The upgrade (capacity-contracts.md "Upgrade order") is exactly when
  absent ledgers are initialized while sweeps confirm every 5 s.
- `TestReleaseRuleByLedgerState` ("marked … without `prev`") would assert the faulty no-write,
  and `TestInitRacingConfirm`'s normal node does not say its ledger was absent before the call,
  so no test fails.

Fix: on any mark, the release rule writes the mark: `prev` less the charge when it keeps one,
otherwise the same `{"schema":1,"init":true}` again, which moves its revision. Test:
`TestReleaseRuleByLedgerState` asserts the revision moves for a mark without `prev`;
`TestInitRacingConfirm` adds a node with no ledger before the call and a release between snapshot
and write: left marked, its rerun exact.

### R5-B2 — Medium

**The blocked restore compares "the revision just read", not this call's mark. That contradicts
the normative contracts, the ADR and decisions, and lets one call overwrite another call's
mark.**

- design.md:119–121: "a mark with `prev` is replaced by `prev`, comparing the revision just
  read".
- capacity-contracts.md:40: "`InitCapacity` write, per node | the ledger at this call's mark".
  adr-proposal.md:31–32: "writing only where its own mark is still in place". decisions.md:99:
  "Initialization writes only under its own mark". An implementer who follows the contracts
  compares this call's mark. A release between mark and restore then fails the restore, so
  `TestInitRacingConfirm`'s "a blocked one restored from `prev` less the release" fails.
- With the design's compare: call 1 sees X blocked, call 2 (`--all`) does not. Call 2 re-marks X,
  keeping `prev`. Call 1 reads call 2's mark and restores `prev`. Call 2's write at its own mark
  fails, so its row says `marked` and the CLI prints `X still marked: run it again`. But X is now
  readable, so a rerun without `--all` does not target it. X keeps its pre-call value, which
  Q3's procedure runs `--all` to correct. `TestConcurrentInit`'s "the second call's marks leave
  the first's nodes marked" is false for the first call's blocked nodes.

Fix: choose one rule and state it in the design, contracts row, ADR and decisions. For example,
the mark carries a call id, which a release keeps when it rewrites the mark. The restore writes
only when the mark it just read carries this call's id, comparing that read's revision.
Alternatively, state that a restore may replace any mark that keeps `prev`, and correct the
other three texts and `TestConcurrentInit`.

### R5-B3 — Low

**The diff makes `ConfirmDeletion` "by the node's data plane", which needs #13's S12. S11 depends
only on #13's S1–S5, and the lab step still names today's CSI sweep.**

- spec.md:67 (S4), design.md:82–83 ("#13's S12") and the S4/S11 test row ("by the node's data
  plane") need S12. Today `auth.go:77–78` allows only `admin` or the node's `agent`, so the `io`
  identity gets `PERMISSION_DENIED`.
- spec.md:132–133 (S11) lists #13's S1–S5 as the dependency. design.md:194–195 (lab step 5) says
  "the CSI node plugins' today, #13's data-plane sweep after it". So #72 may land before S12, and
  `TestDeleteReleasesOnConfirm` then fails.

Fix: add #13's S12 to S11's dependency and drop "today's" CSI sweep from lab step 5. Otherwise,
write S4 and the test as "by the node's identity that `auth.go` allows (its agent, its data plane
after #13's S12) or an administrator".

### R5-B4 — Low

**A blocked node restored from `prev` makes the CLI exit 0 with no line, although its ledger was
not recomputed and Q3's procedure needs another `--all` once its blocker clears.**

- capacity-contracts.md:130–133: exit 0 when no row is `marked`, and lines only for marked rows.
  A restored node has `marked` false and `blocked_by` set (design.md:119–121).
- spec.md:181–186 (Q3): nodes pending on a tombstone an older binary rewrote "stay blocked, so
  run `--all` again once those tombstones are gone". In that case `prev` can still miss an older
  binary's placements. That is the under-charge `--all` exists to find.
- The operator and `meta up` see exit 0 and nothing on stderr, so the advice is wrong in that
  case.

Fix: print a line for every row with `blocked_by`, for example `capacity-init: <node> blocked by
<id>: kept its earlier ledger; run it again with --all once <id> clears`. Decide whether it
exits 2. Add the case to the CLI test in the S10 row.

### R5-B5 — Low

**No initialization test has a fence holder.** R4-S4's fix asked for "a deletion and an
initialization with one". Only the deletion and `ConfirmRelease` tests have one (design.md:167).
`TestInitCapacity`'s fixture (design.md:170) has no `Fenced` entry and no tombstone naming a
fence holder. `domain/capacity_test.go` tests only `Room`. An Init that skipped `h.Fenced`, or a
fence holder's tombstone slot, would pass "exact rows".

Fix: add to `TestInitCapacity`'s fixture a volume with a fence holder that holds no other copy,
and a tombstone pending on one. Expect one slot and zero bytes each.

## Nits

1. spec.md:144 lists #13's `spec.md:105` (S17's read bound) among the places to change. The
   bullet above says that bound "already counts" the ledger read. Say what changes there, or
   drop it.
2. design.md:120–121: the restore rereads "up to 32 times", and the outcome after that is not
   stated. Presumably the node stays marked with `blocked_by`. The CLI line then says to wait for
   the blocker, though a plain rerun would restore the node.
3. capacity-contracts.md:80–82: `had_ledger` and `previous_*` do not say whether a mark's `prev`
   counts as the ledger before the call. If it does not, a rerun after a raced `--all` cannot
   name the placements the ledger did not charge (Q3's purpose).
4. design.md:170, `TestInitBlockedWithoutPrev`: "until the tombstone clears". The node is
   blocked only while it is pending on the tombstone (S6), and it needs a rerun afterwards. The
   test should confirm the node, rerun, and assert exact.

Reviewed commit: 7aae38e9b5400cbe744aabf5e7ce7014c292a59b (plan documents as of dee77a1)
