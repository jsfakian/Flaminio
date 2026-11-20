## Earlier findings
- R5-1: fixed in substance (every add paced); opens R6-1 (the bound is 41, not 40).
- R5-2: fixed (edge tests added, code named, `limit:` prefix; the plugin's `limit:` mapping is #15's own planned change, design.md:75, :424, not existing code, but the design says so).
- R5-3: fixed as a gate with two outcomes; consistent in design, lab plan, Tests, Risks, decisions; opens R6-2.
- R5-N1: fixed (`95a1dba` exists and is the round-4 commit).

### R6-1 - Low
"At most 40 adds and 40 skipped ids per attach" is off by one against the loop it copies. design.md:125-128. ublk.rs:891-899: attempt, then `if now < deadline { sleep 250ms; retry }`. Attempts at t=0, 0.25 ... 9.75 all fail with now < deadline, so a 41st attempt runs at t of about 10.0 and only then returns. The bound is 41 adds (40 paced gaps). The S2 test "one attach tries at most 40" (design.md:719-720) would fail on the loop as specified, or the implementer changes the loop to check the deadline before sleeping (which the plan doesn't say).
Fix: say 41 adds (or "at most 40 sleeps"), or specify that the deadline is checked before the sleep so the last add is at 9.75. Say whether the ceiling refusal after a held skip waits the pace first (design.md:111-112 "next paced attempt" implies it does; :134-135 says the same attach is refused).

### R6-2 - Low
Spec S1 does not carry the gate's Fail outcome fully. spec.md:30-32 says only "the limit is also 1"; the refused `--max-volumes` above 1 at start, its error text, and "an unparsable kernel version counts as before 6.9" are only in design.md:56-60, :708-709. S1 is the rule the tests prove, and the spec's S1 test list has no entry for it.
Fix: add one line to S1 that on a failing gate a larger value is refused at start and an unparsable release counts as before 6.9. Also mark in the spec/design that the Fail branch's tests are written only if the gate fails, so the Pass outcome has no dead tests.

Reviewed commit: d39e372 (diff head; worktree HEAD 5b20c617e8a24558441b34c68803d966a199e7ee adds report.md only)
