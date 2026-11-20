# Plan review: issue #48, round 5 (incremental), design-code and design-security

## Earlier findings
- R4-B1: fixed. Spec S8 (spec.md:147-150), design.md "The bound" (:391-393), "With #74's helper, The deadline" (:509-516) and decisions.md (round 4, "Rules narrowed", "Risks named") say the same. No other passage of spec, design, intent or adr-proposal promises the bound never cuts a resume (grep). The refusal text `error not kept DEV` is the one the `ending` entry gives for `recover` (:523).
- R4-B2: fixed. (a), (b) and (c) each have a test; see the Nit below on (a).
- N1-N4: fixed. The phase sentence (:260-264) agrees with the record's description (:463, "back to running at an error busy"), design.md:102 and step 3 of the table.

## Findings
None of Low or worse.

## Nits
- N1. design.md:605: `the_stamp_comes_at_the_old_deadline_under_the_helper` does not say the 1 s tick has not run; without that it can pass by the tick (cf. :597 "the tick not yet run").

Reviewed commit: 237f7f5bedb2ae03d0696b66312b6fdc0535194e (diff 69d93ad..5902b14 reviewed)
