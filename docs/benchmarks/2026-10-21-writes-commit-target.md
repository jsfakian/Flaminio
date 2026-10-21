# Writes, RF 1: larger commits via a byte gather floor (W4.5, 2026-10-21)

Short validation of `--commit-target-mib`: after a commit, gather until queued/staged
write-data bytes reach N MiB (or the gather window ends), so one flush can cover a full
client depth of 1 MiB traffic instead of sticky ~4 MiB commits. Same method as W4.4: one
repetition, idle/settle 60 s / 120 s, 30 s rounds, at most 8. Filter:
`write-1m|randwrite-4k-qd32`. Zero copy. Integrity after every Flaminio run. Queues 1×1.

**Verdict: not a win for single-stream 1M; leave the floor off by default (`0`).**
Integrity clean on every arm. 4K QD32 stays in the W4.3 band when the floor is gated on
large writes; holding the staged group to pack bytes regresses 1M.

## Results (steady state where reached)

| Arm | 1M Flaminio | 4K QD32 Flaminio | Notes |
|---|---:|---:|---|
| `--commit-target-mib 0` (baseline) | **676** MiB/s | **10 517** IOPS | same band as W4.4 1×1 |
| `--commit-target-mib 8` (count floor only) | 668 MiB/s | 10 484 IOPS | flat |
| 8 MiB + 2 ms window on all sizes | unsteady (fresh 571) | **6 631** IOPS | 4K waits the long window |
| 8 MiB, large-write gate (≥ 64 KiB) | **685** MiB/s | 9 948 IOPS | flat on 1M; QD32 OK |
| 8 MiB + hold staged until floor | unsteady (fresh 469) | 10 654 IOPS | 1M regresses |

Across-disk raw for these 1M runs was ~1 900–2 000 MiB/s; Flaminio share stays ~34%
(~70% of the ~1 000 MiB/s outside the drive cache). Success bar was ≥ ~800 MiB/s steady.

Disk WA unchanged: 1.00 on 1M, ~2.22 on 4K Flaminio.

## Notes

- Engine: `commit_target_bytes` raises the gather count as
  `max(previous + queued, ceil(target / avg_write))` when avg write ≥ 64 KiB, and extends
  that wait to at least 2 ms. Small writes keep the 300 µs / previous-count path.
- Hold (`Store::set_hold_staged`): defer promoting the staged group so it can grow to the
  floor. Packs more bytes per flush but loses the overlap that W4.2 relied on; measured
  worse on 1M.
- Default remains **0** (count/time gather only). Opt in with `--commit-target-mib` /
  `COMMIT_TARGET_MIB` in the lab.
- **Not pursued next from this slice:** journal bypass (amp already 1.00), pipelined
  commits (already opted out). The remaining 1M gap is still one flush at a time; a
  different commit shape is needed.

Data: [baseline](data/2026-10-21-writes-commit-target/baseline-target0.txt),
[target 8](data/2026-10-21-writes-commit-target/target8.txt),
[large-write gate](data/2026-10-21-writes-commit-target/target8-large-gate.txt),
[hold](data/2026-10-21-writes-commit-target/target8-hold.txt).
