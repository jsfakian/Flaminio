# Plan review: #47, round 4 (design-code, design-security), diff da2c16c..fc5b072

Counts: Critical 0, High 0, Medium 0, Low 2, Nit 0.

## Round-3 findings
- R3-L1: fixed in substance. `target.rs` has `mod tests` (:2757) and a test-only `Stats` (:109, a pub field), so a unit test can build a chunk by hand (an ISR member, the resynced member, a second non-ISR replica with an open session) and read the count. Both sites exclude only the resynced member (`source(i, Some(member))`, :1852 digest, :2345 probe), so the second replica is reachable by the bug. No line in the design still says every failure line has a mutation check: :259 says the resync line has none, :279 says "two mutation checks". See R4-L1 for a gap in the count.
- R3-L2: fixed. The paragraph is consistent with the script (`$4`; no `die`, and the usage line calls it; `sudo` on every command). See R4-L2.

## Findings

### R4-L1 (Low): the unit test asserts two sites, but the count has no per-site form
Design, The simulator: the source check says "a failed verdict is counted", and `Sim::after` picks one of three lines, so the count must at least be per line. The unit test asserts it "rises at the digest (:1852) and at the probe (:2345)", which needs a count or record per site, or two tests. A single resync-line count passes if only one of the two sites is wired.
Fix: say the counts are kept per line and per site (or that the test has two cases, one that queues a `ResyncDigest` and one that starts a resync, and asserts each alone). Say too that the digest case needs a queued `Work::ResyncDigest` with a live `resync` round, and the probe case a chunk whose first non-ISR replica with a session is the resynced member.

### R4-L2 (Low): a failed `systemd-run` leaves the fault armed with no timer
Design, Removing the fault. The script has no `set -e`. The text says the timer is armed "first" but not that a failure to arm it stops the run before `tc qdisc add`. If `systemd-run` fails (for example, the unit is still loaded), the netem fault goes on with no margin, which is the failure the timer exists to prevent. The validation of SECONDS is otherwise sound: only `[0-9]{1,5}` reaches `--on-active=${4}s`, and nothing else is built into a shell string.
Fix: add "if `systemd-run` fails, the script exits non-zero before the `tc` add", and a script test (in `scripts/tools/`) for the refusal of a non-digit, a six-digit and an empty SECONDS, before any `tc` call.
