# Plan review: issue #45, round 13 (incremental), design-code and design-security
Reviewed commit: bac60f2ef3cf1d30158ac4309f46e92a0aa2182a (diff 1da70f8..0d3e624).

R12-L1: fixed. design.md:143-148 and decisions.md:542-544, 552-556 all state: round trip (<=1 s) + the tick after the success + one tick per failed renewal.
R12-L2: fixed. design.md:418 requires >=3 failed renewals (>2 s), a rise seen, then success; success tick credits zero, next tick adds exactly 1 s (not zero, not the span, not dropped as late).

Findings
- Low: the test does not pin the delay's "round trip" term. The row asserts the credited tick and amount but not that the first credit comes at the first tick after the success when the successful renewal is answered at its 1 s limit (the case design.md:146 names). Fix: add a variant where the success is answered at the 1 s limit; the first credited tick is still the first one after the answer, adding exactly its own interval.
- Nit: decisions.md:542-544 (round 11 entry) reads as a sentence with a parenthetical "(added in round 12, R12-L1)" inside a list of the delay's terms; fine but easy to misread. Optional.

Nothing else broken by the revision; no new inputs, waits or auth surface.
