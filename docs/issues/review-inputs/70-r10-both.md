# Issue #70, plan review round 10 (incremental), design-code and design-security

Diff: c3df855..75fe538 (docs/issues/70, report.md excluded), in /tmp/flaminio-plans-2026-11-12/70.

## Round-9 findings
- R9-B1 (Medium): fixed. design.md step 3 and the Quota bullet say a call marked `inactiveRules` compares
  `tenant/authorization` absent, as #73 design.md:65-67, and a role-bearing call compares it at the revision read.
  A marked step that reads the record present fails its commit; the next attempt's race check answers Aborted.
  The new TestExpandChargesQuota case (hook after the step's read of `volumes/v`) fails without the fix: with
  no owner record and a compare at the revision read, the step would commit uncharged.
  The earlier hook (after the `owners/<id>` read) still tells the two compares apart.
- R9-C1 (Low): fixed. `no-role` is in step 1 after the flag check and before any read; the test row adds a volume
  already at the requested size, with nothing written, which fails without the move (readVolume would answer).
- N1: fixed in spec S2, design step 3, decisions. N2: fixed (spec header says "rounds 2 to 7").

## What the revision broke
Nothing found. The settle commit (75fe538) changed only markers, the Risks/Settled wording and decisions.md; no
rule or test text moved. No "not accepted" or "open" marker is left that contradicts it (grep of the plan files:
the three remaining "not accepted then" / "carry no recommendation" phrases are historical or describe the
alternatives).

## Findings
None at Low or above.

## Nits
None.
