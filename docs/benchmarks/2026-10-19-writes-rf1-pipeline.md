# Writes, RF 1, pipelined commits: short validations (2026-10-19)

Comparison A with up to 4 commits in flight (spec §6.2), on the W4.3 zero-copy build.
Same method as the [W4.3 validation](2026-10-19-writes-rf1-w43.md): one repetition, not a
campaign.

**Verdict: not a win.** Pipelined commits stay **off by default**
(`--pipelined-commits` / `PIPELINED_COMMITS=1` to opt in).

## First try: launch every staged group

Steady state, one repetition (build that launched as soon as anything was staged):

| Job | W4.3 | **Pipeline** | Notes |
|---|---:|---:|---|
| 4K random, QD1 | — | **383** IOPS (86% of raw 442) | |
| 4K random, QD32 | 10 512 | **10 004** | raw unsteady here |
| 70/30 random, 4K QD32 | 27 112 | **25 999** (108% of raw) | |
| 1M sequential, QD8 | 697 | **548** (29% of raw 1 887) | about **55%** of ~1 000 across-disk |

Integrity: 4 Flaminio runs, 12 checks, 0 errors, 0 checksum failures.

1M rounds (MiB/s): fresh 382; then **745**, 531, 566, 541, 547, 557 → steady **548**.
Gathering was bypassed; concurrent flushes on the file-backed disk cost more than they
gained.

Data: [`.lab/bench/writes-rf1-20261019T151438`](../../.lab/bench/writes-rf1-20261019T151438).

## Re-measure: gather before launch

Same flag, with the engine waiting the gather window before launching a staged group
(`debc3c8` lineage). Jobs matched by `qd32|write-1m` (so QD32, 1M, and the 70/30 mix):

| Job | W4.3 | **Gather then launch** |
|---|---:|---:|
| 4K random, QD32 | 10 512 | **10 229** |
| 70/30 random, 4K QD32 | 27 112 | **26 133** (106% of raw) |
| 1M sequential, QD8 | 697 | **588** |

Integrity: 3 Flaminio runs, 9 checks, 0 errors, 0 checksum failures.

1M rounds (MiB/s): fresh 478; then **812**, 549, 587, 620, 589, 597 → steady **588**.
Better than 548, still short of W4.3. Leave opt-in.

Data: [`data/2026-10-19-writes-rf1-pipeline-gather/`](data/2026-10-19-writes-rf1-pipeline-gather/).
