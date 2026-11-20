# Issue #14: plan review, round 2 (incremental), design-code and design-security

Scope: `git diff 24877eb..fb13b26 -- docs/issues/14/ ':!docs/issues/14/report.md'`, and what
the changed lines depend on. Checked against main.go, main_test.go, service.go, engine.rs,
replica/mod.rs, scripts/lab.sh, scripts/lib.sh, scripts/guest/{confirm,common,node}.sh,
node-scripts-test.sh, docs/lab.md, docs/followups.md, docs/issues/30/spec.md, and #13's plan
(/home/jsfakian/Documents/src/flaminio-13/docs/issues/13/). Between fb13b26 and HEAD only
report.md changed.

## Earlier findings
- R1-C1 (Medium): fixed. Intent Why 5-6, the invariant, S2 and the design's facts now describe
  the code after #30. The shipped main.go and TestUsage are listed in "The next step".
- R1-C2 (Low): fixed in the plan (intent, decisions, design step 3; agrees with #13's S13 and
  #13 design.md:552-561). The shipped follow-ups still say it: see R2-C1.
- R1-C3 (Low): fixed (S7.8, design step 7, a named risk). The lab.md line cited in the next step
  is wrong: see Nits.
- R1-C4 (Low): fixed (S6; lab.md:226-227 in the next step).
- R1-C5 (Low): fixed. Every corrected statement matches confirm.sh:21-26, 41-50 and 104-105,
  lab.sh:1819, 1640-1642 and 1671, lab.sh:713 and 721-725, and the tests in
  node-scripts-test.sh:175-185 and 231-246.
- R1-C6 (Low): fixed.
- Nit (code, S4 exception): fixed (spec S4, design, accepted risk).
- R1-S1 (Medium): fixed. The remaining hazard is named: a fence-blind delete after the id is
  freed. Two #30 residual overclaims remain: see Nits.
- R1-S2 (Low): fixed as worded: invariant restated, residual named, risk recorded as not
  accepted. The residual is stated wider than the code allows (R2-B1), and its status is
  ambiguous outside decisions.md (R2-B2).
- R1-S3 (Low): fixed.
- Nits (security): fixed (intent references; design.md facts on `node.sh delete` and the
  overrides).

### R2-B1 — Low
**The named residual of the harness's deletes is stated wider than the code allows. Its example
cannot trigger it.**

What the plan says:
- decisions.md, "Risks named after approval": "A creation of the name by any other caller
  meanwhile (a `make vol-create` of the same name, by hand) loses its replicas on every data
  node".
- design.md:104-105 (step 4), design.md:260-262 (Risks) and spec.md:79-80 (S5) say the same.

What the code does:
- A creation writes only metadata. A node gets chunks only when a target opens a session there
  (`hello`, replica/mod.rs:177, 253, 285). `make vol-create` only creates (lab.sh:702-704), so
  on its own it gives the harness nothing to delete. An attach of a header still being created
  is refused (`NotFound`, service.go:286-288).
- So the loss needs the name created *and attached*, with its first session reaching a replica
  node after step 4's `get` (lab.sh:1758) and before that node's delete (lab.sh:1761-1767). That
  window lasts seconds: one ssh round trip per node.
- The node the new volume is attached on refuses the delete ("attached here",
  engine.rs:696-698). `delete_volume` then stops (lab.sh:1765), which spares the nodes after it.
  "On every data node" is therefore false. The same applies to the invariant's "wipes a volume
  created again with the name on that node" (spec.md:24-25): not the node where it is attached.

In practice only another lab command does this: a second campaign's `write_volume`, or
`vol-create` followed at once by `vol-attach`. That is close to the accepted "two harness runs
at once". The user must decide whether to accept this risk, and the current wording overstates
it.

**Fix:** state the residual as "the name created and attached by another caller, whose first
session reaches a replica node between `delete_volume`'s re-check and its delete there
(seconds), loses its replicas on the nodes it is not attached on". Replace the example with
`make vol-create` then `make vol-attach`, or a second campaign. Apply this in decisions.md,
spec S5, design step 4 and the design's Risks.

### R2-B2 — Low
**The two risks are unaccepted in decisions.md, but the spec states them as settled. The
operator precondition the new invariant relies on is documented nowhere.**

- decisions.md says "none is accepted yet: accepting them, or a change that closes them, is the
  user's call".
- The spec's invariant, however, has absorbed the first risk as a rule with no marker: "one
  deleter and confirmer per node, plus the harness while no other lab command runs"
  (spec.md:27-28).
- S5 (spec.md:78-80) and S7.8 (spec.md:112-115) state both risks as behaviour.
- The design's Risks (design.md:260-262) lists the first risk next to accepted ones.
- If the user closes the risk instead, the approved invariant becomes wrong.
- If the user accepts it, the invariant depends on a precondition that operators are never told.
  For `vol-confirm`, S7's precondition is in lab.md:237 and in lab.sh's comment. For the
  harness's deletions, lab.md:214-223 says nothing. Neither S10 nor "The next step" adds it.

**Fix:**
- Mark the invariant's harness clause, S5's last sentence, S7.8's last sentence and the design's
  Risks entry as "named, not accepted (decisions)".
- Record in decisions.md what follows from each answer:
  - accepted: lab.md states "no other lab command while a deletion runs" (S10, "The next step"),
    and follow-ups §14's lab-wide lock item widens to cover it;
  - closed: the invariant is restated.

### R2-C1 — Low
**"The next step" leaves out the shipped follow-ups text that R1-C2 corrected in the plan.**

- docs/followups.md:281-282 still says "The sweep moves into the node agent (step 6) … and
  `vol-confirm` goes".
- docs/followups.md:26 still says "With the node agent (step 6)".

#13 puts the sweep in `flaminio-io node`. Its S13 also removes `confirm.sh` and
`flaminio-meta confirm` (#13 spec.md:81, design.md:560-561), as #14's intent (lines 78-80) and
design step 3 now say. The next step corrects §14 "Later" only (design.md:283-284). If #14's
next step lands before #13, which marks step 1 done (#13 design.md:822-824), the follow-ups keep
the claim the plan dropped.

**Fix:** add to the next step's `docs/followups.md` bullet:
- §14 step 1: the node daemon (`flaminio-io node`, #13); `vol-confirm`, `confirm.sh` and
  `flaminio-meta confirm` go with it;
- line 26's "When" column: #13.

Name it among the corrections that go with #13 if #13 lands first.

## Nits
- design.md:280-282 cites the wrong lab.md lines: `vol-confirm`'s step 7 is docs/lab.md:246 (not
  243), and the wrong-confirmation paragraph is 248-252 (not 245-249). lab.md is unchanged since
  24877eb.
- Two statements leave out #30's own residual cases:
  - spec.md:58 (S2): "Not that the new volume serves the old one's data: not since #30".
    docs/issues/30/spec.md:86-87 (S8) says the adoption and the deposition come back when a
    store's revision went back.
  - decisions.md, accepted risks, the bounded delete "would close the wipe by a late delete".
    A volume created by an old metadata service starts at epoch 0 (#30 S9), below the
    tombstone's revision, so a bounded delete still wipes it.

  Qualify both with "outside #30's S8-S9 cases". The follow-ups text the next step adds should
  carry the same qualification.

Reviewed commit: de8d800aa770048e902c95f2d7b682eb4664a760 (plan files identical to fb13b26)
