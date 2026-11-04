# Issue #10 findings (2026-10-29, fl-b, zero copy, 1 queue, RF 1, 1M QD8 O_DSYNC, 60 s traced)

Steady state = last 30 s (after the drive's fast cache, ~30 GiB written). Traces: block
tracepoints on vdb (and ublkb1), scripts in scratchpad (wt.sh, wt2.sh, wan3.awk, cycle.awk).

| Arm | steady MiB/s | MiB/flush | flush ms | gap ms (data phase) |
|---|---:|---:|---:|---:|
| raw (16-100 GiB) | ~984 | 7.6 | 4.73 | 3.06 (0.35 MiB written during flushes) |
| Flaminio base | ~686 | 4.0 | 5.5 | 0.3 (3.9 MiB early writes during each flush) |
| --commit-target-mib 8 | ~701 | 6.6 | 4.8 | 4.6 |
| --flush-coalesce 2 | lower (746 overall) | 2.8 | 5.2 | 0.3 |
| floor 8 + stage when idle (proto) | ~721 | 6.8 | 5.05 | 4.4 |
| + pump without waiting for staging copies (proto) | ~746 | 6.05 | ~4.95 | 3.16 |

Raw = the block layer defers a flush while data writes are in flight (blk-flush C2), so each
flush covers ~all 8 writes and runs with the drive quiet. Flaminio flushes as soon as a commit's
own writes are done, with the next group's early writes landing during it: slower flush, 4 MiB.

Per-cycle timeline with floor 8 (us after the previous flush completes): fio's writes complete
160-320, fio resubmits 394-1065 (ublk splits 1M into 2x512K: io_buf 512K), first disk write
2402 (W4.5 as is: no staging with nothing in flight, the data waits for the launch) -> 2070
(stage when idle) -> 819 (pump not waiting for staging copies). Disk writes then trickle out
~0.2 ms per 512 KiB request.

Queue thread profile (steady, nowait proto): memcpy ~40% (shared_copy_of: staging slot ->
AlignedBuf), CRC-32C ~19% (crc_fast sse v4s3x3), gup/pinning ~5-8%. io-wq workers ~10% each
(WriteFixed request pages -> memfd staging).

Lab: fl-b store formatted, wprof3 (RF 1, 16G, fl-b) exists and is attached; volb deleted
(tombstone pending), to be recreated (create volb 16G 2 1 256M fl-b) + filled for read benches.
Prototype diff: issue10-prototypes.diff.

## Later (same evening)
- + no second copy (prototype: Bytes over the staging slot, RF 1 only): ~797 MiB/s steady
  (3756 cycles x 6.37 MiB / 30 s), 81% of raw; queue thread 30% (from 40%).
- --io-buf 1M with all prototypes: ~779 MiB/s: no gain.
- Commit sizes per cycle (ublk requests completed per flush) vary widely: 15-16 often (791 and
  641 cycles of ~3750), but many 8-12. Launch-early or refused staging; suspects: journal/pool
  limits in stage_check, checkpoints held off while a group is staged, the count target. Full
  8 MiB commits every cycle would give ~1000 MiB/s (raw).
- Prototype diff v2 (all four, unsound no-copy): issue10-prototypes-v2.diff.
- + arrivals hold (the floor's window does not end while writes are copied in, up to 8 ms more):
  with the no-copy shortcut ~926 MiB/s steady (3478 x 7.99/30), 94%; 16 requests in 3451 of
  3478 cycles. Without no-copy (sound): ~849 MiB/s (3184 x 8.00/30), 86%; node stats: 16.0
  writes per commit, 49 of 8684 started on timeout. Diff v3: issue10-prototypes-v3.diff.
