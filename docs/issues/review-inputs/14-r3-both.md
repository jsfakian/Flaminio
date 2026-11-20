# Issue #14: plan review, round 3 (incremental), design-code and design-security

Scope: git diff fb13b26..5dfe023 for docs/issues/14 (report.md excluded).
Result: no findings, no Nits.

## Round-2 findings
- R2-B1 (Low): fixed. decisions.md, spec S5, design step 4 and Risks now say "the name created and
  attached by another caller ... loses its replicas on the nodes it is not attached on". The
  invariant's "unless the volume is attached on that node" agrees with engine.rs:696-699
  (`DeleteReplicas` refuses when `targets` or `attaching` holds the volume). "The deletion stops
  there, sparing the nodes after it" is as in round 2's check of lab.sh:1765.
- R2-B2 (Low): fixed. Every place that states either risk says "accepted (decisions)": the
  invariant (spec:29), S5 (spec:86), S7.8 (spec:121), design step 4 (108), step 7 (182), Risks
  (267) and decisions.md. No plan file outside report.md still says "not accepted", "named, not
  accepted" or "Risks named after approval". The two report.md lines that do (52, 56) are the
  round-1 history. The operator precondition is in S10 and in the next step (lab.md 214-223
  bullet, follow-ups lab-lock item).
- R2-C1 (Low): fixed. Follow-ups §14 step 1 is at docs/followups.md:281-282, and the item 14 row
  is at line 26 of that file; both are as the next step cites them. The "Later" lab-lock item
  and the incarnation item exist (followups.md:308, 306-307).
- R2-N1: fixed. docs/lab.md:246 is `vol-confirm`'s step 7, 248-252 the wrong-confirmation
  paragraph, 226-227 the campaign text ("never stops for fl-c" is on 227, the sentence starts on
  226). The 214-223 harness bullet list matches.
- R2-N2: fixed. "Outside #30's S8-S9 cases" matches docs/issues/30/spec.md S8 (a store whose
  revision went back) and S9 (a volume created by an old metadata service starts at epoch 0).

## What the revision broke or opens
Nothing found. The risks' acceptance is recorded once, in decisions.md, with the reason and what
follows from it (the lab.md sentence and the widened lock item).
