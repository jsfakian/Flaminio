# Reads, QD1: `--defer-taskrun` (2026-10-20)

`IORING_SETUP_DEFER_TASKRUN` on the ublk queue ring (`--defer-taskrun` /
`DEFER_TASKRUN=1`), zero-copy local reads on fl-b, `randread-4k-qd1` only.

**Verdict: not a win.** Leave opt-in only; do not turn on by default.

## Invalid first run

After the write campaign, vol1’s ISR was `fl-a+fl-c` with target `fl-b` (no local
replica). Both arms reported ~580 IOPS (~4% of raw). That measured remote reads, not
the defer flag. Discarded.

## Re-run (local ISR)

vol1 recreated with a replica on fl-b (`fl-b+fl-a` / `fl-b+fl-c`, degraded 0), filled
once, then baseline and defer back-to-back.

| Arm | Flaminio IOPS | raw (median) | share | p99 (µs) | queue CPU % |
|---|---:|---:|---:|---:|---:|
| baseline (`defer=0`) | **13 009** | 15 458 | 84% | 85 | 11.0 |
| `--defer-taskrun` | **13 057** | 15 504 | 84% | 83 | 10.3–10.7 |

Flaminio spread ≤1%; raw spread ~28% (rep 3 cooler). Integrity: verified 16 GiB, 0 errors
each arm.

Data: [baseline](data/2026-10-20-reads-qd1-defer/baseline-summary.txt),
[defer](data/2026-10-20-reads-qd1-defer/defer-summary.txt).
