# Plan review: #47, round 3 (design-code, design-security), diff c4a3290..c4e321c

Reviewed commit: c4e321c (diff base c4a3290). Counts: Critical 0, High 0, Medium 0, Low 2, Nit 0.

## Round-2 findings
- R2-B1 (Low): fixed. The callers of `source` in target.rs are :670, :731 (the completion check, which the design splits from the preference), :1737, :1801, :1819, :1852 and :2345. The design names every one but :731. The check counts only with the judgement on, so `scenario_removal_in_flight_catches_reads_from_removing` (judgement off, `sim/tests.rs:204-209`) still fails with `not linearizable`. S7 now includes the digest and the probe.
- R2-B2 (Low): fixed in text: each mutation check passes only on its own line. Reachability is plausible (see R3-L1 for the untested third line). A queued-read catch needs the local replica to be skipped (slow) and a non-ISR replica with an open session. The `gray` campaign has a slow local replica, and `try_seed` loops over seeds.
- Nit 1 (46,075): fixed. I recomputed the node object (328 bytes at 128-byte ids, `slow:true`, 20-digit numbers); the 128-byte gap to 46,203 is `false` plus a 20-digit `slow_for_us` on 32 nodes in 4 targets.
- Nit 2 (order): fixed. `NodeId` is the interning index of `engine.rs:1623-1633`, and the design says so.
- Nits 3 and 4: fixed. Nit 5: partly fixed (R3-L2).

## Findings

### R3-L1 (Low): the resync line has no mutation check
Design, "The simulator": the source check now has three lines, but there are two mutation checks (queued read, fast read). `resync asked an unusable member` (the :1852 and :2345 sites) is produced by no injected bug and no named test. The wiring of those two sites could be omitted with every test still passing, and the bug in `prefer_open_replicas` does not reach them unless a resync has a non-ISR open-session replica other than the member being resynced (the copy and probe exclude that member).
Fix: add a third check, `catches_prefer_open_replicas_on_a_resync`, passing only on that line, or a target unit test with a hand-built chunk (a non-ISR replica with a session, the bug on) that asserts the count at :1852 and :2345. Or say that the line is covered only by the other two and drop it from "three".

### R3-L2 (Low): the `chaos.sh` timer is still under-specified against the script as it is
Design, "Removing the fault", against `scripts/guest/chaos.sh`.
- The script has no `die` (it is not defined or sourced there; the existing usage line calls it too, so it exits 127). "Refuses a SECONDS that is not decimal digits" needs an explicit message and `exit 2`, done before the first `tc` call; also bound the length, so that a huge number does not fail in `systemd-run` after the qdisc is already deleted.
- `slow on DELAY` is `$3`, so SECONDS is `$4`; say so, because the script's argument parsing otherwise puts the new argument where `${3:?delay}` is.
- "Stops an earlier timer of that unit": a transient service whose command failed stays loaded as failed and makes the next `systemd-run --unit` fail. Say `systemctl stop flaminio-chaos-unslow.timer flaminio-chaos-unslow.service` plus `reset-failed`, each `|| true`, and that the timer's command ends in `|| true` as the script's own delete does. Run `systemd-run` under `sudo` as the other commands are.
Fix: add these three points to the paragraph.
