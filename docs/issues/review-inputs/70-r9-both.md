# Issue #70, plan review round 9 (incremental), design-code and design-security

Scope: `git diff e1ad7c9..c3df855 -- docs/issues/70/ ':!docs/issues/70/report.md'` in
`/tmp/flaminio-plans-2026-11-12/70`. HEAD 862745d adds only report.md, so line numbers are at HEAD.
Dependencies read: #73 at ac399a6 (design.md:37–68, contracts.md:83–84, 276–300), #78's
`ensurePlacementGate` (#78 design.md:31), `meta/service/service_test.go:615–668` (`interleaveKV`),
`meta/service/service.go:275–283, 380–396`, `meta/service/auth.go`.

## Earlier findings

- R8-B1: partly fixed. The check now runs before each attempt (design.md:48–51). The race test's hook is reachable: `s.active` never reads `owners/<id>`, so the step makes the first Get of that key. The test now fails without the `tenant/authorization` compare (design.md:228). The fix left out the other half of the round-8 fix: a marked step that reads the record present goes on to commit. That opens the window in R9-B1.
- R8-B2: fixed. S2 limits `Aborted` to calls that began before activation (spec.md:50–53). Step 3 and the test answer `unassigned` for a step that read no owner record (design.md:59–61, :228). The Quota bullet no longer says an assignment makes the step read again (design.md:95–98). The quota-key hook is reachable: on a direct handler call, the step makes the first Get of `quota/<ns>/default`. Without the compare, the other volume's charge is lost. The wording has a Nit (N1).
- R8-C1: fixed. `no-role` is used active or not, and `wrong-kind` only for the other role (design.md:57–61). The test covers both cases (:228). decisions.md:237–239 now credits the race check. Where the check sits in the step is R9-C1.

## Findings

### R9-B1 — Medium

**A call marked `inactiveRules` can still commit a step on an active store. The window is from the race check to the step's own read of `tenant/authorization`.**

- **The race check comes first:** a marked call checks `s.active` before each attempt (design.md:48–51).
- **The attempt then reads more keys:** the attempt reads the gate, the header (`readHeader`, a Get of `volumes/<id>`, service.go:276), the nodes and the ledgers. Only after these does it read `tenant/authorization` (design.md:52–55).
- **The compare does not catch it:**
  - The transaction compares `tenant/authorization` "at the revisions read, absent included" (design.md:95–96; decisions.md:231).
  - If activation commits inside that window, the step reads the record present. The compare then holds.
  - None of #73's checks applies to a marked call: `no-role` needs no mark, and `unassigned` and `wrong-kind` need a role (design.md:57–61).
- **What the step does next:**
  - With no owner record, the step commits uncharged on an active store. This breaks S2's "A call that began before activation never commits a step uncharged: it is refused with `Aborted`". It also breaks S2's "On an active store, a step that reads no owner record answers `unassigned`" (spec.md:49–53).
  - With an owner record, a caller with no role binding grows a tenant volume after activation.
- **It contradicts #73:** #73's race rule has a marked call compare the record **absent** in each transaction (#73 design.md:65–67), not "at the revision read".
- **No test reaches the window:** the race test's hook comes after the read of `owners/<id>` (design.md:228), so it fires after the step has read `tenant/authorization`.
- **The window is reachable in a test:**
  - On a direct handler call, the step's `readHeader` makes the first Get of `volumes/<id>`. The load step only Lists `chunks/` (service.go:385), and `s.active` reads only the tenant keys.
  - So an `interleaveKV` hook keyed on `volumes/<id>` fires inside the window (service_test.go:615–644).

**Fix:**
- In step 3 and the Quota bullet, state that a call marked `inactiveRules` compares `tenant/authorization` absent, as #73. A call with a role compares it at the revision read.
- Alternatively, state that a marked step that reads the record present answers `Aborted` at once. Correct decisions.md:231 and :248–252 to match.
- Add a race case to `TestExpandChargesQuota`:
  - Use the `inactiveRules` mark and an `interleaveKV` hook after the step's read of `volumes/v` that activates tenancy.
  - The step must answer `Aborted`, with nothing written.
  - With the compare taken "at the revision read", the step commits uncharged, so the case fails.

### R9-C1 — Low

**The `no-role` check needs only the context, but it runs after the step's reads and after the "already large enough" answer.**

- **Where the check sits:** step 3 reads the gate and the header first, which answers `NotFound` when the header is absent. It then answers `readVolume` when `h.SizeBytes >= size_bytes`. Only after both does it check for a context with neither the mark nor a role (design.md:52–58).
- **What a no-role context gets:** `NotFound`, or a success with the volume, depending on the request.
- **The design contradicts:**
  - #73: "A handler that needs a role and finds none in its context refuses, active or not" (#73 design.md:54–55).
  - The test row, which expects `PermissionDenied` `no-role` without condition (design.md:228).
  - #73's refusal log, under which a refusal writes nothing. `ensurePlacementGate` creates the gate when it is absent (#78 design.md:31).

**Fix:**
- Move the `no-role` check to step 1, before any read. It is added by whichever of #70 and #73 lands second.
- Leave `unassigned` and `wrong-kind` in step 3, since they need the owner record.
- Add a test case with a volume already at the requested size, which must still answer `no-role`.

## Nits

- **N1:**
  - **"A later step" cannot happen:** spec.md:51–53, design.md:59–61 and decisions.md:253–255 say a step that read no owner record answers `unassigned` "even if the volume is assigned before its commit; a later step or call reads the new owner". That refusal ends the call, so no later step of it runs.
  - **"Its commit" does not exist:** a refused step has no commit.
  - **Fix:** write "even if the volume is assigned after the step's read; a later call reads the new owner and charges it".
- **N2:** spec.md:8 says the Tests list changed in "rounds 2 to 8". Round 8 changed only the header and S2 in spec.md (`git diff e1ad7c9..c3df855 -- docs/issues/70/spec.md`). It should stay "rounds 2 to 7".

Reviewed commit: 862745df93d1ab2189c4283c412f57d647d7d697
