# #13 plan review, round 5 (incremental), focuses: design-code and design-security

Scope: `git diff e350277..3b2564a -- docs/issues/13/ ':!docs/issues/13/report.md'` in
`/home/jsfakian/Documents/src/flaminio-13`. HEAD (1de5da6) adds only report.md. I read what the
changed lines depend on:
- `service.go`: `nodes()` (239-270), `createVolume` (372-473), `DeleteVolume` (515-578),
  `DrainNode` (827-842), `MoveReplica` (866-918); `retries = 32` (:45);
- `kv.go:31-33`, `etcd.go:90-93`; `domain.Place` (`domain.go:355-400`);
- `service_test.go:615-665` (`interleaveKV`);
- #72 `capacity-contracts.md:27-45` and `design.md:10, 55-73`; #76 `design.md:37-45, 159`;
  #78 `spec.md:10-21, 49` and `design.md:24-70, 94-104, 140-142`.

## Earlier findings

- R4-C1: fixed. Steps 3 and 4 skip a `deleting` volume. `pending` names only volumes being
  created. S19, design.md:181-184, the proto comment, the `docs/lab.md` line and
  `TestForgetNodeClears` all agree.
- R4-C2: fixed.
  - `TestForgetRacesAPlacement` hooks on the `drain/` `List`, which is the last read of `nodes()`
    (`service.go:240-256`). With `PreferNode` set, `Place` puts the node on every chunk
    (`domain.go:356, 367`). The test fails without the claim's compare.
  - `TestForgetRacesADeletion` now states its fixture and where the read sits. It fails without
    the tombstone's compare. The header's compare passes, because `ForgetNode` writes nothing of
    a `deleting` volume.
  - Its "(with #78 …)" variant of the move-back test is wrong (R5-C1).
- R4-C3: fixed. The #72 and #78 hand-offs are recorded (decisions.md:270-288).
  - The arithmetic holds:
    - #72's claim: 2 + 1 + 116 = 119.
    - #76's admission without #78 (sealed root, name absent, tombstone absent): 3 + 1 + 116 = 120.
    - #76's admission with #78, the mark compare gone: 4 + 116 = 120.
    - `etcd.go:91` rejects only more than 120.
  - "Never stand together" holds by the hand-off's construction, which ever plan lands second.
  - The hand-off's mark rewrites an existing drain marker (R5-C2).
- R4-B1: fixed (design.md:792, 1255; decisions.md:208, 460).
- R4-N1: fixed for `MoveReplica` (design.md:187-189 matches `service.go:870, 876`). Some
  wording left over in decisions.md, see R5-N2.
- R4-N2: fixed (design.md:167, 1008-1010).

### R5-C1 — Low

**Summary.** The #78 variant of `TestForgetRacesAMoveBack` cannot fail when the gate compare
is removed.
- The design says the test "fails with `MoveReplica`'s mark compare removed (with #78, its gate
  compare)" (design.md:1003-1005).
- With #78, `MoveReplica` reads the gate and then the destination's state in each attempt. It
  refuses a destination that has a drain marker (#78 design.md:55-58; #78 spec S5).
- `ForgetNode`'s mark writes `drain/<node>` (design.md:164-165). The hook runs on the move's
  `Get` of `forgotten`, which comes before the `drain/<to>` read in either order of the gate and
  the mark. So the move then finds the node draining and is refused, whatever the gate compare
  does.
- The test passes with the gate compare removed. With #78 in place, nothing tests the
  forgetting's move-back race against the compare that #13 hands to #78.

**Evidence.** design.md:187-192, 1001-1005; #78 design.md:55-58; `interleaveKV` runs its hook
after the read it matches (`service_test.go:638-643`).

**Fix.** Hook `ForgetNode` on the move's header `Get` (`readHeader`), the first read after
every destination check.
- Without #78: the move read the mark as absent and reads the chunk after step 3, so only the
  mark compare stops it.
- With #78: it read the gate and `drain/<to>` before the mark, so only the gate compare stops
  it.
- State this as the test's hook for both cases.

### R5-C2 — Low

**Summary.** Under the #78 hand-off, `ForgetNode`'s mark rewrites an existing drain marker. That
moves #78's drain generation and fails a drain of that node that is still running.
- Step 2 always puts `drain/<node>` (design.md:164-165). The hand-off keeps that, "as #78's own
  drain mark does (2 compares, 3 writes)" (design.md:202-204; decisions.md:272-275).
- #78's own mark does not do this. An existing marker is left as it is ("idempotent: return its
  modification revision"). A new one compares the marker's absence (#78 design.md:60-62).
- #78's drain generation is the marker's modification revision: "one continuously marked
  draining interval" (#78 spec.md:10-11). `drain.Run` keeps it and calls `CheckDrain` with it
  (#78 design.md:94-104).
- Forgetting a node whose drain is still running is the documented cure for residual (1): a
  chunk waits until one of its two dead release holders is forgotten. The rewrite then turns
  that node's drain into `FailedPrecondition` ("drain generation is no longer current"), and the
  drain must be restarted.
- Safety holds either way. An existing marker already keeps the node out of every claim and
  move that reads it, and the gate compare stops those that read before it.

**Evidence.** design.md:164-165, 202-205; decisions.md:272-275; #78 spec.md:10-11; #78
design.md:60-62, 94-104.

**Fix.** With #78, the mark writes the drain marker and the gate only when the marker is
absent, comparing its absence (3 compares, 3 writes). An existing marker is kept with its
generation. Correct the counts and "as #78's own drain mark does" to match.

### Nits

- **R5-N1.** `TestForgetRacesAConfirmation`'s "two `interleaveKV` hooks" (design.md:1006) is
  more than one `interleaveKV` gives.
  - One wrapper runs one hook, once (`service_test.go:615-636`).
  - Suppose one wrapper carries the `forgotten` key and the `volumes/` prefix, with a second hook
    stored after `ConfirmRelease` takes the first. `ForgetNode`'s own step-2 `Get` of
    `forgotten` then runs that second hook before the mark is written. The confirmation commits
    early, and the test passes even if step 4 reuses step 3's listing.
  - Say "two stacked wrappers: the outer on the `Get` of `forgotten`, the inner on the `List` of
    `volumes/`".
- **R5-N2.** "Each attempt" describes loops that are not as stated.
  - `createVolume` has no retry loop today (`service.go:372-473`: one claim, and a failure
    returns). The design never says it adds one, or what the loop covers: the mark, `nodes()`,
    placement and the claim, with the tombstone check and the chunk scan before it or inside.
  - decisions.md:290-292 says `createVolume` and `DeleteVolume` read the mark "likewise", as the
    first read of each attempt. But `DeleteVolume` reads it after the header and its deleting
    mark (design.md:209-210).
  - State that the `createVolume` loop is new and what it covers, and correct the decisions
    sentence.

Reviewed commit: 1de5da6585f11782678ecbc9da394370b400c133
