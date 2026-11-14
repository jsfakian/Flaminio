# Issue #85: io_uring batch lifetime validation — intent

[Issue #85](https://github.com/jsfakian/Flaminio/issues/85), correctness audit finding F08.
This plan covers the issue in full. Approved 2026-11-13; not implemented.

## Why

1. **A returned error can leave borrowed buffers queued.** `UringDevice::run` constructs one
   submission window at a time (`dataplane/crates/flaminio-uring/src/uring.rs:175`). It pushes
   each prepared SQE immediately (lines 223–225), but converts later buffer lengths with
   fallible `len32` inside the same loop (lines 195, 207). If a later length exceeds
   `u32::MAX`, the call returns before submission and draining. Earlier SQEs still refer to
   caller memory or bounce buffers whose lifetime ends when the call returns.
2. **A later call can submit those entries.** The audit's temporary-file reproduction submits
   a 4 KiB write followed by an oversized slice. The batch returns `InvalidInput` while the
   first block remains zero. After an unrelated read and flush, that block becomes `0x5a`;
   the flush can report `short transfer: 4096 of 0 bytes`. This demonstrates delayed I/O and
   completion contamination without freeing the original small buffer. It does not demonstrate
   an exploit. The huge slice uses reserved virtual address space, not 4 GiB of resident data.
3. **The public trait's lifetime promise is stronger than the implementation.** `BlockDevice`
   says `write_many` and `read_many` return when their operations are complete
   (`dataplane/crates/flaminio-store/src/device.rs:27–35`). The unsafe-code explanation in
   `uring.rs:218–222` relies on the same condition. Network frame limits cannot establish
   soundness for callers of this safe Rust API.
4. **Other paths already contain useful defenses.** `submit_commit_writes` validates every
   length before modifying the queue (`uring.rs:304–307`) and retains owned buffers until
   completions (`358–375`). Synchronous kernel errors normally drain before returning
   (`230–274`); unrecoverable submission errors fail-stop (`235–240`). Existing tests cover
   kernel errors and later successful calls, but not a preparation error after a queued prefix
   (`errors_are_reported_after_draining`, line 773).

## What we want

1. Reject unsupported operations before publishing any entry of their submission window or
   allocating an avoidable oversized bounce buffer.
2. On every returning path, leave no kernel or queued request able to access expired borrowed
   or bounce-buffer memory. Keep completion ownership isolated between windows and calls.
3. Preserve asynchronous commit ownership, flush ordering, short-I/O errors and fail-stop
   behavior when the kernel's ownership cannot safely be resolved.
4. Add regression tests for invalid later operations, aligned and bounced buffers, queue
   boundaries, subsequent operations and error/drop paths. Reproduce with retained buffers
   and disposable regular files; never deliberately submit a dangling pointer.

## Out of scope

- Changing replication, epoch or ISR rules, durability policy, the on-disk format or the wire API.
- Raising the per-operation length limit, introducing a new I/O library or changing queue depth.
- Claiming network reachability of the defect or a demonstrated memory-corruption exploit.
- Shared-lab fault campaigns, raw disks or destructive hardware tests.
