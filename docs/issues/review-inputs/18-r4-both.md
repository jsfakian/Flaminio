# Issue #18 plan review: round 4 (incremental), design-code and design-security

Scope: git diff c6c7373..22757c0 -- docs/issues/18/ (minus report.md). HEAD is 4048b5d (report.md only differs from 22757c0).
#72's plan is not in any local worktree, so claims about its texts are unchecked.

## Round-3 findings
- R3-C1: fixed. Five ordered reasons (design.md:139-150, spec S12) are disjoint by construction and cover all blocked cases; each has a test (1: refusal test; 2: existing unknown-words case; 3: TestDrain; 4: new two-chunk test; 5: unknown-staying-replica test). See R4-1 for a residue.
- R3-C2: fixed. Creation is filtered, labels are cleared/restored between steps, `skipStayingCheck` is caught only by the "beside" clause, `skipCollisionCheck` by both. Writing the store directly for absent labels, deleted and unparsable records is sound: `RegisterNode` merges labels (service.go:197-199). R4-2 is a gap.
- R3-C3: fixed in design (`LeftOut`, rack label, last). See R4-3.
- R3-B1: fixed. The proto comments, ARCHITECTURE 8.2 row (line 442), followups section 4 step 2 and the lab.md upgrade note are listed. The CSI mapping is right: `FailedPrecondition` becomes `ResourceExhausted` (controller.go:248-249).
- R3-S1: fixed (design Named, not accepted; decisions.md).
- R3-N1: fixed; the example is unquoted and the rule (a refusal is verbatim, #18's own reasons use %q) is stated.

## Findings

### R4-1 Low: reasons 1 and 4 can name another chunk or hide the real blocker
- Reason 4 uses "the step's last such refusal's message". That message carries `volume %q chunk %d` of the chunk that was refused (design.md:~135-141), so volume v3's `Why` can read `failure domain collision: volume "v1" chunk 7: ...`. The operator reads it as v3's cause.
- Reason 1 applies when this chunk was refused on any node, even when the remaining nodes of C were not taken out and `Strict` disallows them. The `Why` is then the refusal, not `Strict`'s reason for the nodes that remain.
Fix: in reason 4 say that the message names the chunk it was for, or say which volume/chunk it was in `Why`; in reason 1 either accept this and say so, or apply 1 only when no node of C is left, which leaves 5 to cover the rest. Add the cross-volume case to the reason-4 test (the design says "of a later one"; assert the text names the earlier chunk).

### R4-2 Low: the property test never clears a label at the level that matters
"Clearing a random node's label" does not say which label. At `rack` clearing a zone, or at `zone` clearing a rack, is a no-op for that volume, so a seed can pass with no unknown-node case reached. The `skipStayingCheck` "some seed within the same budget must show ..." has no stated budget or guarantee that a cleared node holds a replica of a chunk that is then moved.
Fix: clear the label of the level of each volume (or both), and pick the cleared node among those holding replicas; state the seed/step count the injected-bug check uses, and fix the seeds so the failure is certain.

### R4-3 Low: `LeftOut`'s condition and its duplication are unspecified
- "Appends, last, to a placement refusal" (design.md:105) against "either create error" in the #72 bullet: say what `createVolume` tests (`errors.Is(err, ErrPlacement)` only, until #72 adds `ErrCapacity`), and that it re-wraps with `%w` so `errStatus` still maps the class (service.go:143).
- `LeftOut` has to repeat `Place`'s filter predicate (empty label at level, replicas >= 2, strict, level not node). Two copies can drift. Have `Place`'s filter and `LeftOut` share one unexported function, or test them against each other (a table over level, policy, replicas).
- It also names unlabelled nodes on refusals that have nothing to do with labels (more replicas than live nodes). State that this is accepted.

## Nit
- decisions.md/design.md name the property test's claim "Clearing and restoring never put two labelled replicas in one domain, so only a move can" as a fact; its reason (an unknown staying replica blocks every other replica's move) should be written next to it, since the argument fails if S5 is weakened.
