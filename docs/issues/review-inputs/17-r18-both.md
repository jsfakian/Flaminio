# Round 18 (incremental), #17, design-code + design-security
Reviewed commit: c3e6ac598cb74118cd6a9072d9b5557c110e2989 (worktree HEAD; diff f28981a..f802039)

Round 17: L1 fixed (design.md:222-227, :234-240 now "a device entry in any state", as #15 :71/:726; no stale "running only" remains in design.md; decisions item 90's done() bullet is marked withdrawn by item 91). L2 fixed (winner's metadata phase held, then released, both orders). L3 fixed (startup gate between done() and reserve; "stops at done()" is a separate case).

## Findings
1. Low. Socket-first needs the test to know the startup runner is parked at its gate before it opens the socket runner's gate; the design does not say how (design.md:762-764 only says it "passes its done() re-check ... then waits"). If the socket client reserves first, the startup runner's done() (any state now counts a starting entry) stops it without a reserve, and the refusal is never seen: the R17-L3 hazard, via the test's own ordering. Fix: a cfg(test) "reached gate" count or signal for the startup runner (like the waiter count), and the test opens the socket gate only once it is 1.
2. Low. decisions.md item 90 still states the old gate position ("after each runner's wait, before its reserve") and the old end of the retry ("at the done() after the interval"). Items 91-93 supersede both, but item 90 does not say so (only its done() bullet is marked withdrawn). Fix: mark the gate and "ends at the done() after the interval" bullets as superseded by 92/93.
3. Low. Under the any-state rule, in socket-first the retry ends at the first done() after the Err, while the socket client's entry is still `starting` (held), not "after the interval" or at "the socket client's device entry". The text (design.md:768-770) says "device entry"; the entry is still starting. Behaviour is the same; wording misleads about what the test asserts. Also note, as an accepted cost of #15's rule, that if that held attach then fails, the startup `--attach` has ended and nothing retries it. Fix: say "entry" and name that outcome; no test needed beyond #15's.

## Security
No new finding. Gates and counts are cfg(test) only; retry still bounded by STARTUP_ATTACH_LIMIT; a starting entry of another client can now end the startup retry early, which is #15's accepted rule and needs a tenant able to attach that volume anyway.
