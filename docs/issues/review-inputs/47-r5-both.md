# Plan review: #47, round 5 (design-code, design-security), diff ebf723d..e998e2a

Counts: Critical 0, High 0, Medium 0, Low 3, Nit 0.

## Round-4 findings
- R4-L1: fixed in substance (two cases, each asserting the resync count alone). The digest case
  (`target.rs:1846-1853`, a live `resync` round with a matching `round`, a session for the member)
  and the probe case (`drive_resync`, first non-ISR replica with a session is the member, then
  `source(i, Some(member))` at :2345) are both buildable in `mod tests`. See R5-L1: the setups
  do not yet guarantee that the bug picks the second replica.
- R4-L2: fixed in substance (exit before `tc qdisc add`, argument-count rule, tests with the three
  tools faked). See R5-L2 and R5-L3.

## Findings

### R5-L1 (Low): the two cases do not fix the order that makes the bug pick the outside replica
`source` (:1644-1653) prefers the local node, then the first usable ISR member. The bug iterates
replicas with an open session, so with the ISR member first in `replicas` (or the local node) the
bug returns that member, a usable one, and the count stays zero: "one with the bug on" would fail.
The design names neither the local node's role nor the order. The probe case also needs the
second outside replica after the member in `replicas`, since `drive_resync` takes the first
(:2330-2332).
Fix: name the setup: the local node is not a replica of the chunk (or is outside the ISR with a
session only if the bug's order is stated); `replicas` in the order member, second outside
replica, ISR member; sessions open on all three. Say which of those the bug's iteration reaches
first.

### R5-L2 (Low): a failed arming leaves an earlier fault with no timer
Order in the paragraph: refusal, stop of the earlier timer, arming, delete, add. The stop runs
before the arming and the delete after it. If `systemd-run` fails, the script exits before the
delete, so a netem fault left by a killed earlier run stays on `wg0` and its timer has just been
stopped: the very state the timer prevents.
Fix: on a failed arming run the script's delete (`tc qdisc del`, `|| true`) before the non-zero
exit, and test that the fake `tc` saw a delete and no add.

### R5-L3 (Low): the script test's fakes miss `sudo`, and two cases have no test
Every command in `chaos.sh` runs under `sudo`, which resets PATH on most hosts; faked `tc`,
`systemctl`, `systemd-run` on PATH are not found unless `sudo` is faked too (a pass-through
function or a stub first on PATH, the script-test's way). The S15 row names three fakes only.
The listed cases also leave out `slow off` (still deletes and stops the timer) and `slow on DELAY`
with three arguments (no `systemd-run` call, today's command), which "given or not by argument
count" and "`make chaos` is unchanged" rely on. A valid five-digit SECONDS (accepted) is the
boundary opposite the six-digit refusal.
Fix: add `sudo` to the fakes and the three cases to the S15 row.
