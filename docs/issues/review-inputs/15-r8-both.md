# Round 8, issue #15 (diff cd787d9..ce26a4f)
R7-L1: fixed. Design S2 (design.md:124-131) pins the order: next id, ceiling, deadline, wait and add; :143 and :151 agree; the S2 test asserts no wait on a fake clock between the held add and the refusal.
R7-L2: fixed. Step 2 before step 3 makes the ceiling text win; :151 says so; a new S2 case covers the 41st add.
Order agrees with 41 adds / 40 waits: steps 3-4 are ublk.rs:891-899. Both tests can fail (a wait after the held add, or the last error returned, fails them).
Rewrap: fc755c0 vs ce26a4f, whitespace-normalized word diff of design.md and decisions.md is empty. No wording changed.

## Low
L1. design.md:732-733 (S2 id test) still reads "moves to the next id at the next attempt". That is the phrase R7-L1 removed from the design body. The id is chosen right after the failed add, before the wait. Fix: "moves to the next id straight after the failed add".
(The spec has no ceiling or pace rules; nothing to align there.)
