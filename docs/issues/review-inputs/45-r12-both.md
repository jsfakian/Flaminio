# Plan review: issue #45, round 12 (incremental), design-code and design-security
Reviewed commit: 1da70f8500a256a5bc97cf8e6aac0ff620368d4b (diff c9e3472..e85c63c, design.md and decisions.md).

## Round-11 finding
- R11-L1: fixed. design.md:141-146 says the pre-ownership ticks credit zero and still advance the tick time, the newest revision and its rise; no later tick credits that span; none is dropped as late. Consistent with :175-178 (an ownership change unsets them, so the first read is a first sighting, then rises count).

## Findings

### L1 - Low: the stated delay of the first credit is short by one tick
design.md:136-137, 144-146 and decisions.md (R11-L1): the renewal runs "at each 1 s tick, after the tick's read", and credit is decided from that read. So the tick in which the first renewal succeeds has already credited zero (the ownership was not held when it ran); the first credit is the next tick. The delay is therefore the renewal round trip (at most 1 s) plus that one tick, plus one tick per failed renewal; the text omits the tick after the success. Harmless (it only delays a failure) but the stated bound is wrong. Fix: say "up to the next tick after the first renewal succeeds".

### L2 - Low: the "first renewal fails" test does not pin either property
design.md:419 (S2-S5 row): "credit starts from that tick only, at most 1 s for it, the span before it never credited and that tick not dropped as late". "At most 1 s" passes if the rise was not advanced (the first credited tick then credits zero as a first sighting) and the span-before check passes only if the failures span more than 2 s, which is not required. Nor does it assert the stated delay. Fix: name the test's shape: at least three failed renewals (over 2 s), a heartbeat rise seen during them, then success; assert the first credited tick adds exactly its own 1 s interval (not zero, not the span), and that the first credit comes at the tick after the success.

### Nits
None.
