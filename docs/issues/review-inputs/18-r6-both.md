## Earlier findings
- R5-C1 (every seed admits a volume): fixed. Three labelled nodes in distinct zones and racks give 3 labelled domains at either level, so the first strict 2-replica volume is created (Place: resolveDomain needs distinctDomains >= 2). The test now asserts creation and that the forced step ran and was refused. Wording defects below (R6-B1).
- R5-C2 (TestLeftOutPredicate): fixed. Assertion 2's condition (ErrPlacement, some node left out, >= replicas nodes in all, < replicas domains among those kept) matches domain.go:360-366 and resolveDomain exactly (live-node check on all nodes, strict refusal on kept nodes). Assertion 1 fails on a wrong leftOut, assertion 2 on a LeftOut non-empty where Place succeeds or empty where it refuses, assertion 3 on a wrong count. Each can fail.

### R6-B1 — Low
Forced-step destination claim is wrong as written. design.md "The forced step": "the third of the first three nodes is always one". The chunk's two replicas are chosen by load, then id, so they can be any two labelled nodes, including the third of the first three. What holds: the chunk has 2 nodes, so at least one of the first three is outside it, labelled at the level. As written an implementer may hard-code node 3 and hit a chunk that holds it. The "no staying replica's labelled domain shares" clause is also vacuous here (the only staying replica is the cleared one).
Fix: "at least one of the first three nodes is outside the chunk and labelled; the step moves to the first such node".

### R6-B2 — Low
The forced step asserts only "the service refused it" and that it "was requested". A refusal or no-op for another reason (applied=false, a pending move, NotFound, Aborted from contention, #78's draining refusal) passes the "refused" assertion, and under an injected bug an unrelated refusal would still pass the "accepted" check's negation. The injected bugs are then caught only if the refusal is the right one.
Fix: assert the refusal is FailedPrecondition carrying the ErrCollision prefix and naming the cleared replica (`replica "<id>" has no <level> label`), and that the move response was neither applied=false nor an error of another code.

Reviewed commit: 1e5ec2d503a2e8324895b1b7f6f28b82a801d9c0
