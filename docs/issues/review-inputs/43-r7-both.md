# Plan review #43, round 7 (b8ace90..abb4f5e), design-code + design-security

## Round-6 findings
- R6-1 (timeout set nowhere): fixed. design "The reconciler's client" sets Timeout on a copy of the config, manager keeps the original, Options.RequestTimeout is injectable, TestMidwayFailureResumes holds a request past a short timeout.
- R6-2 (status vs "no request"): fixed in the main rule (one patch, only when differing, failed patch rewritten, test added); one leftover wording defect, R7-2.
- R6-3 (60 s poll): fixed. 2x15 + 30 + 17 = 77 s, poll 120 s, consistent in spec, design, decisions. Gap: R7-4.
- R6-4 (cached read guard): fixed. Constructor builds one client by client.New; operator.New is shared by main and tests; TestRunsAsAccount runs it.
- Nit (every 30 s): fixed.

## New findings

### R7-1 Low - a failed read of the resource has no outcome
design "The period" / "Deleting the resource". The next pass is asked for by "a pass that found the resource"; a pass that "finds no resource" asks for none. A pass whose Get of the resource itself errors (timeout, 5xx) is neither, yet "an API error" is listed among the outcomes that ask for the next pass. If it is treated as "not found", the reconciler stops until a watch event and S6 fails on an unreachable API server. No test: TestRepair and TestMidwayFailureResumes hold a managed-object request, not the resource's Get.
Fix: say a failed Get of the resource (not NotFound) returns to the next-pass rule (requeue after Period); only NotFound ends the chain. Add a case to TestMidwayFailureResumes (the resource's Get fails once; the next pass runs).

### R7-2 Low - "no request about a managed object" contradicts the pass's first sentence
design l.64-66: the pass "reads the resource and each live object by name", then step 1 (Validate, render) "InvalidInput, with no request about a managed object". Reads of live objects are requests about managed objects. If they come first, TestRenderErrorIsInvalidInput as written (no request about a managed object) fails; if after, the pass text is misordered.
Fix: say the reads of live objects follow step 1, or reword both to "no write".

### R7-3 Low - the status patch wakes the watch; not stated
The manager's one watch is on the resource; a status patch is an update of it and, unless filtered, starts an extra pass at once (the first pass after any change, and after ApplyFailed). Harmless to safety, but "A pass starts 30 s after the one before it ended" (S6), the pass counts the tests wait on, and "a pass that changes nothing sends no patch" (no loop) all depend on it, and nothing says whether the watch ignores status-only updates (generation predicate) or not.
Fix: state which, and that a status write cannot make passes loop (a pass that writes the same status twice would; the compare-with-read rule prevents it); one line in TestSecondPassWritesNothing: no extra pass is started by a converged pass.

### R7-4 Low - the lab's 17 s for a change of leader omits the restart
design "Only the lab needs seconds". The operator has one replica and exits on loss of the Lease; a new leader is the restarted pod (kubelet back-off, start-up) or, for a killed pod, a Lease that must first expire (15 s) plus the retry. 17 s counts only expiry plus one retry. The 43 s margin probably covers it, but the derivation as written does not say the restart is in the margin.
Fix: add "and the pod's restart" to the margin's terms, or raise the figure.
