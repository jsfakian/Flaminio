# Plan review #43, round 8 (abb4f5e..36670aa), design-code + design-security

## Round-7 findings
- R7-1 fixed: step 1 error goes to step 10; only NotFound ends the chain; the case is in TestMidwayFailureResumes; TestDeleteResourceRemovesNothing checks one pass then none.
- R7-2 fixed: step 2 (no request) precedes step 3 (reads); tests say "no request about a managed object" consistently.
- R7-3 fixed: generation-only watch stated (design pass preamble, decisions); TestSecondPassWritesNothing checks no pass after a converged or status-patching pass.
- R7-4 fixed: margin named as holding the restart in design, spec, decisions.

## State machine walk
Chain: every path ends at 10 except step 1 NotFound; panics are recovered by the framework and requeued. Writes after a refusal: none (7 precedes 8; an error in 6 skips 8 too). Loop via status: ruled out (patch only if differs from step 1's read; generation predicate). No defect found there.

## New findings

### R8-1 Low - reason precedence when two outcomes occur in one pass is unstated
Step 9: "Blocked takes the reason that led here, else UnsafeDrift". The condition holds one reason. Cases the list leaves open: (a) step 5 sets UnsafeDrift, step 8 then fails: ApplyFailed replaces it, so the status stops naming the drifted fields (S6 says the status names them) until a pass without the failure. (b) a refusal is found in 4 or earlier objects of 6, then a later dry run errors: go to 9 with ApplyFailed, the refusal is hidden. (c) refusal in 4/6 and a drift in 5: PreflightRefused hides the drift. No write happens in any case, so this is status accuracy, not safety.
Fix: one line in step 9: precedence (e.g. InvalidInput, ApplyFailed, PreflightRefused, UnsafeDrift) and that the message lists every object and field found, or say the hiding is accepted. Add one case to TestUnsafeDrift or TestMidwayFailureResumes (drift plus a failed apply).

### R8-2 Low - errors in steps 3 and 6 have no test
Rules: a failed read of a managed object (step 3) and a failed or timed-out dry run (step 6) are ApplyFailed with no write. TestMidwayFailureResumes covers a failed or held apply (step 8), the status patch and the resource read (step 1); TestPreflightRefusalWritesNothing covers a refusal. A step-6 error that wrongly fell into the refusal path, or a step-3 error read as "absent" (which would make the pass create or apply over a live object, and for the StorageClass attempt a create), passes every listed test.
Fix: add to TestMidwayFailureResumes: one read of a live object fails (reason ApplyFailed, no write of any object, next pass completes) and one dry run fails (same). The first guards "error is not absent".

### R8-3 Low - the objects of step 4 refusals are not said to leave step 6
Step 5 says "each object left" only after skips; step 4's refused object stays in step 6's set by the text. Harmless (dry run only) but the status can then carry a second message about the same object, and the "refusal" test on adoption (TestAdoptionRefused) does not say if a dry run of it is sent.
Fix: say whether a refused object is dry-run; the counting transport in TestAdoptionRefused should assert it.

### R8-4 Nit - step 9 on the first pass
"One that was not read keeps the counts read in step 1": on a fresh resource there are none, so a step-3 failure reports zero counts for unread workloads. State "empty", and that Converged is then False (Blocked is True, so it is).

### R8-5 Nit - S3 says what shows later is "reported for its workload"
Step 3 makes ApplyFailed the reason for a failed request about any of the six objects (CSIDriver, StorageClass included); S3 line 52 says "for its workload". Reword to "for its object".
