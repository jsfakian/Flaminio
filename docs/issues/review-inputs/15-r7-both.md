# Round 7, issue #15 (diff 5b20c61..2cb1f9d)
R6-1: fixed. 41 adds / 40 waits / last add at about 10 s matches ublk.rs:891-899 (attempts 0..39 fail with now<deadline and sleep; attempt 40 runs at >=10 s and returns). S2 test says 41.
R6-2: fixed. Spec S1 now carries default 1, refusal at start with its text, unparsable release = before 6.9, tests only on a failing gate; matches design S1 (design.md:56-60).

## Low
L1. Ceiling refusal "at once" is not tied to the loop order. design.md:111 still says a held id moves to the next id "at the next paced attempt", while :134 says the ceiling is checked "when the next id is chosen". The loop as specified is: add fails, deadline check, 250 ms sleep, next id chosen. If the id is chosen after the sleep, the refusal waits 250 ms, which contradicts R6-1. Fix: say the next id is chosen right after a held failed add, before the deadline check and the wait; the ceiling check follows at once. Make the S2 test assert no sleep (fake clock) between the held add at 1048575 and the refusal.
L2. Precedence when the held add at 1048575 is also the one that ends the deadline (the 41st add): "fails with the last error" (design.md:125) vs "refused ... never the last add's error" (:140). Say which wins (the ceiling text, by the order in L1), and cover it in one S2 case.

No other defects found in the diff.
