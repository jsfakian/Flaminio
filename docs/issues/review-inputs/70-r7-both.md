# Issue #70, plan review round 7 (incremental), design-code and design-security

Scope: `git diff 942d1eb..2511b40 -- docs/issues/70/ ':!docs/issues/70/report.md'` in
`/tmp/flaminio-plans-2026-11-12/70` (HEAD 24bd911 adds only report.md). Line numbers are at HEAD.
Dependency plans read at: #72 db06456, #73 ac399a6, #75 597b861 and 06d161a.

## Earlier findings

- R6-B1: fixed. Step 1's refusal is gone; #72 is an ordering prerequisite (design.md:14–15, 76–81). With it, the tests, the CLI test, `make csi-expand`, the advertising and the Done-when hold. #72's text still names the removed refusal (R7-C2).
- R6-B2: fixed. The flag is checked first (design.md:44–45).
- R6-S1: fixed once tenancy is active (design.md:86–90; spec S2). The activation window is still open, and the new "absent" compare cannot be reached (R7-S1).
- R6-S2: fixed in form. The row is in the Contracts (design.md:200–203), with `auth_test.go` cases (:219) and the batch log entry. The handler's owner-kind check has no place in the steps (R7-C1).
- R6-C1: fixed. Per-run ids are checked with `valid_id`, which allows 128 characters (`scripts/lab.sh:1633`). The trap rules are used, and the delete after a failed check is tested (:223). The cited commit is stale (Nit R7-C3).
- R6-C2: fixed (design.md:50–52).
- R6-C3: fixed (decisions.md:141, 161–165).

The 120 bound holds. A tenant step has 1 + k + D + 2 operations (header, chunks, ledgers, owner record and quota usage) and 2 + D + 3 compares (header, gate, ledgers, owner record, policy and quota key). k shrinks to fit, and one chunk fits, within `kv.MaxTxnOps` (`meta/kv/kv.go:33`, enforced at `meta/kv/etcd.go:91`). #70's Ledger bullet says the same as #72's growth bullet and transaction row: soft watermark, the room map, each used node's ledger compared and written, the longest prefix that fits, deletion by the release rule (#72 design.md:85–89, capacity-contracts.md:34). The only exception is R7-C2.

## Findings

### R7-S1 — Medium

**A step that reads the store inactive compares none of #73's keys. If tenancy is activated mid-call, the growth can commit uncharged on an assigned tenant volume. The "absent" compare the fix added can never be reached.**
- **What the design says:** step 3 reads `owners/<id>` only "once #73's tenancy is active" (design.md:50–52). The Quota bullet says "the step compares `owners/<id>` absent or at its revision", then "Before activation, nothing" (:86–90).
- **The "absent" compare is dead code:**
  - Once active, a missing owner record is refused before any transaction (`unassigned`, :88–89).
  - Before activation, the design compares nothing.
- **The test cannot pass:** `TestExpandChargesQuota`'s "an owner record assigned between the read and the commit makes it read again" (:217) does not hold as written.
- **The bypass:**
  1. A step reads the store inactive.
  2. `ActivateTenancy` runs, then `AssignOwner` of this legacy volume. AssignOwner charges the header's current size and writes `owners/<id>` (#73 design.md:161–166). It does not write the header.
  3. The step commits: its header, gate and ledger compares still hold.
  4. Result: k chunks of a tenant volume that its quota never charged. Deletion later releases only the owner record's bytes.
- **Not stated:** the design does not say whether "active" is decided once, by Authorize's `inactiveRules` mark, or on each attempt. If it is decided once, every later step of a long request also commits uncharged after the assignment.
- **#73 fences this window only for event-carrying calls:** it compares the record absent and answers `Aborted` once active (#73 design.md:65–68, contracts.md:287). `ExpandVolume`'s row has no event (design.md:203), so that rule does not cover it.

**Fix:**
- Under `inactiveRules`, apply #73's race rule to `ExpandVolume`:
  - Each step's transaction compares #73's authorization record absent. That adds one compare, so the bound is 2 + D + 1.
  - Before each attempt, the loop checks `Service.active` and answers `Aborted` `tenancy was activated during this call: retry` once it is true, keeping the committed steps.
- Since `AssignOwner` is refused before activation, the stale compare cannot pass.
- Replace "absent or at its revision" with "at its revision".
- Reword the test as: activation, or activation and assignment, injected by `interleaveKV` between a step's read and its commit. The step does not commit uncharged. Activation between two steps answers `Aborted`, with the first step kept. A later assignment charges the grown size.

### R7-C1 — Low

**Once tenancy is active, the owner-kind role check has no place in Metadata growth's steps.**
- Authorize lets either role through for a method whose row allows both. The owner's kind is known only after the handler reads `owners/<id>` (#73 design.md:45–55). DeleteVolume makes this check in its handler (`wrong-kind`, #73 design.md:135–136).
- #70's steps read `owners/<id>` (design.md:50–51) but never state the refusal. "Answers: Metadata growth, steps 1–4" (:204) lists no `PermissionDenied`.
- The S4 row's `auth_test.go` case "the other of the two ... `PermissionDenied`" (:219) therefore has no approach. It also needs a store holding owner records, which a pure Authorize table does not read.
- Without the check:
  - `system-provisioner` can grow tenant volumes and spend their quota.
  - A call authorized before activation, with no roles in its context, keeps growing after activation (#73: "a handler that needs a role and finds none in its context refuses", design.md:54–55).

**Fix:**
- In step 3, after the `owners/<id>` read, state: "once active, the context lacks the role of the owner's kind: `PermissionDenied` `wrong-kind`, nothing written", before the quota check.
- Add the wrong-kind and no-role cases to `TestExpandRefusals` or `TestExpandChargesQuota`, on both stores.

### R7-C2 — Low

**#72's plan, cited at db06456, still has growth's lander removing a step-1 refusal that #70 no longer has. Nothing records the correction.**
- #72 design.md:89–90: "Whichever of #70 and #72 lands second adds this and removes `ExpandVolume`'s step-1 refusal."
- #72 design.md:172: "`TestExpandRefusals` without step 1's refusal".
- #72 decisions.md:147–149 says the same.
- #70 now has no such refusal (design.md:44–45) and is always the second lander (:77–79).
- The batch log notes the drop only "for #73's turn" (batch-2026-11-12.md:276). Neither #70's decisions nor the log carries a correction for #72.

**Fix:** record a correction for #72 in #70's decisions.md (round 6, "#72 first") and in the batch log. It strikes "and removes `ExpandVolume`'s step-1 refusal" and "without step 1's refusal" and keeps the rest of #72's growth bullet. Alternatively, state that these clauses are moot because #70 never adds the refusal.

## Nits

- **R7-C3 — Nit.** #70 pins #75 at 597b861 (design.md:107, 231; decisions.md:216). It quotes text that 597b861 does not have: "`code = NotFound` in the `2>&1` output" (design.md:235–236). #75 added that text in 9083e69, its clear commit, per batch-2026-11-12.md. Pin 9083e69.
- **R7-C4 — Nit.** Two passages still describe the old ordering:
  - The Docs list puts "growth's charges (#72, #73) until their landers add them" in `docs/followups.md` (design.md:261). #70 now adds #72's charge itself, so only #73's quota charge can be a follow-up. Say "#73's quota charge, until whichever of #70 and #73 lands second adds it".
  - decisions.md:27–28 (the round-5 entry) still says "growth is now refused until it charges #72's ledger", unmarked. Add "(replaced in round 6)", as at :181.

Reviewed commit: 24bd911f277a4aef09165d4c46f384eeb56fd3fa
