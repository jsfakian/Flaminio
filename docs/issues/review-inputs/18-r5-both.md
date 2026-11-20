## Earlier findings
- R4-1 (reasons 1/4): fixed. Reasons 1, 4, 5 are disjoint and complete (chunk refused and Strict refuses nothing in Left -> 1; Left empty, not refused -> 4; else 5). Reason-4 and reason-5 tests assert exact text.
- R4-2 (forced step): fixed in part; see R5-C1 (the guarantee holds per volume, not per seed).
- R4-3 (LeftOut): fixed; createVolume's %w re-wrap keeps errStatus mapping; #72's short-domain refusal stays ErrPlacement (its design.md:101-104), ErrCapacity gets no suffix. See R5-C2 for the table test.
- R4-N1 (S5 argument): fixed. The argument holds: a cleared node's chunks cannot move other replicas or enter it, and the multi-cleared case is covered by "unknown staying replica refuses every destination".

### R5-C1 — Medium
"Every seed, for certain" is not guaranteed. Design Tests (invariant) says volumes are "created only when the topology has at least one labelled domain more than its replicas": that is a filter. With 4 nodes, random zones and racks and some unlabelled, a seed's topology can admit no volume at either level. Then there is no chunk, the forced step does nothing, and the injected bugs pass on that seed. This contradicts spec Tests ("must fail on every one of its fixed seeds") and the claim in the round-4 entry. The counting argument itself is sound when a volume exists: D >= r+1, clearing the first node can drop one domain, r-2 staying domains, leaving at least 2 free domains, one of which has a node outside the chunk.
Fix: build the topology so it admits a volume (draw until some level has >= 3 labelled domains, or place 3 labelled domains first and randomise the rest), and have the test assert on every seed that the forced step ran and was refused (and, under each bug, accepted).

### R5-C2 — Low
`TestLeftOutPredicate` is specified as "`Place` leaves out exactly the nodes `LeftOut` counts" (design Tests). `LeftOut` is non-empty only for the refusal case, so on every row where `Place` succeeds it is empty while `Place` still leaves nodes out. `Place` also returns only the placement, so the nodes it left out cannot be read from it.
Fix: state the table test as three assertions per row. `leftOut` per node matches the table; `LeftOut` is non-empty exactly when `Place` returns `ErrPlacement` and some node is left out, at least `replicas` nodes are in all, and fewer than `replicas` domains remain among the kept nodes; and when non-empty its count equals the number of nodes with `leftOut` true.

Reviewed commit: b71f1c6e8b17b3c9fa56135441fa9bebe7f1da10
