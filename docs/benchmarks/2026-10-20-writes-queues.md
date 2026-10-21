# Writes, RF 1: several ublk queues (W4.4, 2026-10-20)

Short validation of `--queues` with matching fio `--numjobs`, after W4.3. Same method as
the pipeline note: one repetition, shortened idle/settle (60 s / 120 s), 30 s rounds, at most
8. Filter: `write-1m|randwrite-4k-qd32`. Zero copy. Integrity after every Flaminio run.

**Verdict: a win for multi-submitter aggregate throughput; leave `--queues` default 1.**
A single fio job still maps to one ublk queue, so this does not close the single-stream 1M
gap (~70% of across-disk raw).

## Results (steady state where reached)

| Job | 1 queue, 1 job | **2 queues, 2 jobs** | Change |
|---|---:|---:|---|
| 4K random, QD32 (Flaminio) | 10 444 IOPS | **12 389** | +19% |
| 1M sequential, QD8 (Flaminio) | 676 MiB/s | **1 108** | +64% |
| 1M share of that run's raw | 34% (raw 1 955) | 40% (raw 2 725) | |

Raw 4K QD32 was unsteady in both arms (as in some earlier campaigns). Disk WA unchanged:
2.22 on 4K Flaminio, 1.00 on 1M.

Integrity: both Flaminio jobs in each arm passed (sequential + random verify, restart +
re-read, 0 checksum failures).

## Notes

- Harness: `QUEUES` / `NUMJOBS` on `bench-writes` (sequential multi-job splits the 16 GiB
  span). Before the campaign, `$VOL` is deleted instead of drained (drain can stall).
- Compare aggregates to aggregates: 2×2 uses two submitters. Against W4.3's single-job 1M
  (~697 MiB/s), 1×1 here is in the same band (676); 2×2 is a different workload shape.
- **Not pursued:** engine-lock contention fixes — 2×2 already beat 1×1.

Data: [q1](data/2026-10-20-writes-queues/q1-jobs1-summary.txt),
[q2](data/2026-10-20-writes-queues/q2-jobs2-summary.txt).
