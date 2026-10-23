# Writes, RF 1: flush coalesce depth 2 (W4.6, 2026-10-21–23)

Short validation of `--flush-coalesce 2`: when commit A’s writes are done, defer its
`fdatasync` if a staged partner is expected, write B’s journal (data already early-written),
then one shared flush retires both. Keeps W4.2 overlap; only merges the flush. Same short
method as W4.5: one repetition, idle/settle 60 s / 120 s, 30 s rounds, at most 8. Zero copy.
Queues 1×1.

**Verdict: miss for single-stream 1M; leave coalesce off by default (`0`).**
1M steady dropped from **696** to **511** MiB/s. 4K QD32 stayed in the W4.3 band (~10.6k)
on partial runs. Integrity reattach after Flaminio jobs timed out under coalesce (`attach
wbench` control reply >120 s), so those campaigns exited 255; fio’s pre-restart checks had
errors 0.

## Results (steady state where reached)

| Arm | 1M Flaminio | 4K QD32 Flaminio | Notes |
|---|---:|---:|---|
| `FLUSH_COALESCE=0` (baseline) | **696** MiB/s | **10 567** IOPS | clean integrity |
| `FLUSH_COALESCE=2` | **511** MiB/s | ~10 600 IOPS | 1M regresses; integrity attach hung |

Across-disk raw on the 1M coalesce arm was ~1 870 MiB/s steady; Flaminio share ~27%.
Success bar was ≥ ~800 MiB/s steady of ~1 000 across-disk.

## Notes

- Device: up to two flush-ready commits share one `IORING_OP_FSYNC`+`DATASYNC`; partners are
  marked `Covered` and complete with the tagged CQE. `set_expect_coalesce_partner` holds a
  lone ready commit until a partner joins or the engine clears the expect.
- Engine: after `stage`, `coalesce_partner()` promotes like `launch()` so committing/staged
  tracking stays aligned (an earlier path promoted only in the replica and panicked under
  QD32 with “a commit is in flight”).
- Default remains **0**. Opt in with `--flush-coalesce` / `FLUSH_COALESCE` in the lab.
- Integrity: after a coalesce workload, restart + attach often got no control reply within
  120 s; not chased further once 1M clearly regressed.

Data: [baseline](data/2026-10-21-writes-flush-coalesce/baseline-coalesce0.txt),
[coalesce 1M](data/2026-10-21-writes-flush-coalesce/coalesce2-1m.txt),
[coalesce QD32](data/2026-10-21-writes-flush-coalesce/coalesce2-qd32.txt).
