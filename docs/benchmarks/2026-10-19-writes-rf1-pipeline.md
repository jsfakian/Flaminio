# Writes, RF 1, pipelined commits: short validation (2026-10-19)

Comparison A after enabling up to 4 commits in flight (spec §6.2), on the W4.3
zero-copy build plus the uncommitted pipelining work. Same method as the
[W4.3 validation](2026-10-19-writes-rf1-w43.md): one repetition, not a campaign.

**Verdict: not a win.** 1M sequential fell about 20%. Pipelined commits stay off by
default (`--pipelined-commits` / `PIPELINED_COMMITS=1` to opt in).

## Results

Steady state, one repetition:

| Job | W4.3 | **Pipeline** | Notes |
|---|---:|---:|---|
| 4K random, QD1 | — | **383** IOPS (86% of raw 442) | |
| 4K random, QD32 | 10 512 | **10 004** | raw unsteady here |
| 70/30 random, 4K QD32 | 27 112 | **25 999** (108% of raw) | |
| 1M sequential, QD8 | 697 | **548** (29% of raw 1 887) | about **55%** of ~1 000 across-disk |

Integrity: 4 Flaminio runs, 12 checks, 0 errors, 0 checksum failures.

## 1M sequential

Rounds for Flaminio (MiB/s): fresh 382; then **745**, 531, 566, 541, 547, 557.
Steady window means **548**. Round 1 nearly matched W4.2/W4.3; later rounds sat near
W4.1 (541).

**Likely cause:** the engine launched every staged group as soon as anything was
staged, so gathering never filled a ~4 MiB batch before the next flush. Concurrent
flushes on the file-backed disk then cost more than they gained. A gather-before-launch
change is in the tree for a later re-measure with `PIPELINED_COMMITS=1`.

Data: [`.lab/bench/writes-rf1-20261019T151438`](../../.lab/bench/writes-rf1-20261019T151438).
