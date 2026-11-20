# Plan review: #47, round 6 (design-code, design-security), diff ae5c388..d12ebf8

Counts: Critical 0, High 0, Medium 0, Low 0, Nit 1.

## Round-5 findings
- R5-L1: fixed. Local node not a replica; replicas = member, second outside replica, ISR member; sessions
  to all. `source` (target.rs:1644-1653) with the bug iterates replicas with a session: reaches the second
  outside replica (or the member, if the bug ignores `exclude`): a failed verdict either way, one count;
  unmutated returns the ISR member: zero. Probe: `drive_resync` (:2338-2341) takes the member first.
  Digest: hand-built live round, `source(i, Some(member))` at :1852. Each yields one count.
- R5-L2: fixed. Delete (step 3) precedes arming (step 4); a failed arming exits with no fault and no timer.
  A kill between 4 and 5 leaves a timer and no fault (harmless). A later run stops the earlier timer at
  step 2 before any add, so no timer removes a later fault. `slow off` (steps 2, 3) and three-argument
  `slow on DELAY` (2, 3, 5) match chaos.sh:6-9 as it is.
- R5-L3: fixed. S15 row adds `sudo` pass-through and the cases (five digits in order, failed arming,
  three arguments, `slow off`); they match the five steps.

## Nit
- A SECONDS of `0` (or `00000`) passes "one to five digits"; `--on-active=0` could fire before step 5's
  add, leaving a fault with no timer. The check passes length+30, so it cannot happen in practice;
  refuse zero or require a positive value if wanted. (Likewise `slow off SECONDS` arms a timer with no fault.)
