# Issue #70, plan review round 8 (incremental), design-code and design-security

Scope: `git diff 2511b40..e1ad7c9 -- docs/issues/70/ ':!docs/issues/70/report.md'` in
`/tmp/flaminio-plans-2026-11-12/70` (HEAD 467ff30 adds only report.md). Line numbers are at HEAD.
Dependencies read: #73 at ac399a6 (design.md, contracts.md), #72 at 5c76e87, #75 at 9083e69,
`meta/service/auth.go`, `meta/service/service_test.go` (`interleaveKV`), `meta/kv/kv.go`, `meta/kv/etcd.go`.

## Earlier findings

- R7-S1: fixed in mechanism. Every step compares `tenant/authorization` and `owners/<id>`, and an `inactiveRules` call that finds the store active answers `Aborted` (design.md:50–58, 92–97). No path is left where a step commits uncharged. Its test cannot fail without the compare (R8-B1), and the assignment case it names is wrong (R8-B2).
- R7-C1: fixed. Step 3 states the `wrong-kind` refusal from the compared owner record (design.md:56–58), and `TestExpandChargesQuota` covers it (:224). The "no role at all" case takes the wrong reason (R8-C1).
- R7-C2: fixed. #72 was aligned at 186074f (#72 decisions.md:149; batch log :278), and #70's decisions record it (:235–237).
- R7-C3: fixed. Design.md:114 and :238 cite #75 at 9083e69, which has the quoted `code = NotFound` text (#75 design.md:266 at 9083e69).
- R7-C4: fixed. The followups line is at design.md:268, and decisions.md:28 now reads "(replaced in round 6)".

The 120 bound holds. The bound is 1 + k + D + 2 operations and 2 + D + 4 compares (design.md:98–101), against `kv.MaxTxnOps` (`meta/kv/kv.go:33`), which `meta/kv/etcd.go:91` enforces. With one chunk, D ≤ RF.

## Findings

### R8-B1 — Medium

**The compare that R7-S1 added has no test that fails without it. `s.active` re-reads activation after every step read, so the named hook ends the attempt before any commit.**
- **Where the design checks:** step 3 runs the `inactiveRules` check after all of the attempt's reads, through `s.active` (design.md:47–55).
- **What `s.active` does:** #73's `Service.active(ctx)` reads the activation records again each time until one exists (#73 design.md:40–41).
- **What the test does:** `TestExpandChargesQuota` activates tenancy in "an `interleaveKV` hook after the read of `tenant/authorization`" (design.md:224).
- **What happens:** `s.active` then sees the activation in the same attempt and answers `Aborted` without a `Txn`. The row says instead that the hook "makes the step's commit fail, and the next attempt answers `Aborted`".
- **Why the test cannot catch a missing compare:** the outcome is the same with the compare removed.
- **No other hook reaches the window either:** `s.active` runs after every read, and `interleaveKV` runs hooks only after a Get or List, or after an applied write (`meta/service/service_test.go:615–668`). No read hook can land between the check and the commit.
- **Authorize can also consume the hook:** #73's Authorize reads both records first (#73 design.md:51). A test that goes through it fires the hook before the handler runs.
- **The check also drifts from #73:** #73's race rule checks `active` before each attempt (#73 design.md:66–67), and #70 cites that rule ("as #73's race rule") while checking after the reads.

**Fix:**
- Decide "now active" from the step's own compared read of `tenant/authorization`, as the owner kind is decided from the compared owner record. Alternatively, call `s.active` before the attempt's reads, as #73 does, and answer `Aborted` when the step's read finds the record present.
- State that the test calls the handler directly with the `inactiveRules` mark, so the step's read is the first Get of the key.
- State that removing the compare makes the test fail, because a step would commit uncharged.

### R8-B2 — Medium

**The spec, design and test give three different answers when a volume is assigned during a call on an active store. Two of them cannot happen.**
- **Spec S2** (spec.md:50–51, new): "A call that began before activation, or before the volume was assigned, never commits a step uncharged: it is refused with `Aborted`."
- **Design step 3:** once active, a step that reads `owners/<id>` absent answers `FailedPrecondition` `unassigned` at once (design.md:56–57).
- **Test:** "a hook after the read of `owners/<id>` on an active store that assigns the volume makes it read again and charge" (design.md:224).
- **Why neither the spec nor the test can hold:**
  - `AssignOwner` needs `owners/<id>` absent (#73 design.md:161) and is refused on an inactive store (#73 contracts.md:49).
  - So that hook always leaves the step holding an absent read. The step answers `unassigned`; it neither reads again nor answers `Aborted`.
  - A call that began before the assignment but reads after it grows and is charged.
- **The Quota bullet has the same error:** "an assignment between a step's read and its commit makes it read again" (design.md:93–94) holds only together with an activation.
- **Effect:** an implementer would meet a test that cannot pass, or loosen the `unassigned` rule to make it pass.

**Fix:**
- In S2, limit the `Aborted` sentence to calls that began before activation. Add: "on an active store, a volume assigned after a step's read answers `unassigned`".
- In the test row, replace the `owners/<id>` case with two cases:
  - That hook answers `FailedPrecondition` `unassigned` with nothing written.
  - A hook after the read of `quota/<ns>/default` that charges another volume of the namespace makes the step read again and charge once.
- Drop "or an assignment" from the Quota bullet.

### R8-C1 — Low

**The test calls a context with "no role at all" `wrong-kind`, but #73's reason for that case is `no-role`, which #73 applies active or not.**
- **#73's reasons:**
  - `PermissionDenied` for "no role for the method" has reason `no-role`. `wrong-kind` is for "the role of the other owner kind" (#73 contracts.md:281, 298).
  - #73 also says: "A handler that needs a role and finds none in its context refuses, active or not" (#73 design.md:54–55).
- **What #70 has:**
  - `TestExpandChargesQuota` expects `PermissionDenied` `wrong-kind` for "(the other role, or no role at all)" (design.md:224).
  - Step 3 checks roles only "once active" (design.md:56–58).
  - A handler called without Authorize, with neither the mark nor a role, grows on an inactive store.
- **The R7-C1 decision is misleading:** it says the `wrong-kind` check means "a call authorized before activation cannot keep growing after it" (decisions.md:233–234). In fact such a call meets the `Aborted` check first (design.md:54–56) and never reaches `wrong-kind`.

**Fix:**
- In step 3, add before the other checks: "a context with neither the `inactiveRules` mark nor a role: `PermissionDenied` `no-role`, active or not, as #73".
- Test the case under that reason.
- In decisions.md:233–234, attribute the "keeps growing" case to the `Aborted` check.

Reviewed commit: 467ff30e1d35ce280f875b979d9c7eb7f38ad839
