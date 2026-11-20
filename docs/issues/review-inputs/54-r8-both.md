# #54 plan review, round 8 (incremental), design-code and design-security

Scope: `git diff 8a485ad..3fd4cef -- docs/issues/54/ ':!docs/issues/54/report.md'` in /home/jsfakian/Documents/src/flaminio-54. Compared the changed lines with the round-7 findings and the surrounding plan text; #12, #15 and #48 not re-read. Nothing run.

## Round-7 findings
- **R7-C1: fixed.** The SIGTERM refusal reads "with #12, by whichever of #12 and #54 lands second" at contracts.md:194, contracts.md:302 and design.md:155; `sigterm_waits_for_restore_entry` (design.md:321) carries the same gate; the cross-plan item is at design.md:491-492 and decisions.md:433; the three-plan `should_exit` hand-off reads "lands last".
- **R7-C2: fixed.** `cleanup_leaves_restore_record` asserts `first_id` only for the record left and a start with no prior `cleanup`; the deleted branch asserts only that record and device are gone.
- **Nits 1–5: fixed** (retry summary, the 30 s wording, a running restore entry under #48, the sweep case's precondition, the ADR's `--privsep` sentence).

## New findings
None. The changed lines add no new rule, state or input; they gate existing rules on #12 and narrow wording, and each changed rule has its test.

Cosmetic, not a finding: decisions.md:397 (the round-6 entry) still says "by whichever lands second" about `should_exit`; the round-7 entry at decisions.md:433-434 records the change to "last".

The plan can clear at nothing Low or worse.
