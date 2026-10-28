# Issue #5: zero-copy local reads fail with EINVAL — specification

## Behaviour

**S1. Requests the disk cannot take directly.** With `--zero-copy`, a device read of a local
read goes into the request's pages (`ReadFixed`). If that read returns EINVAL, it is done again
through the tag's staging slot. The frontend reads the same range from the disk into the slot,
then copies it into the request's pages at the same place. This applies on the fast path and on
the queued path, one device read at a time, so a read with several device reads copies only the
ones that need it. The client gets its data, and the read does not count as failed.

Only EINVAL leads to the copy, because that is how the kernel refuses a buffer it cannot use for
direct I/O. Any other error, or a failed staged read, fails the local read (S2). Data copied
through the slot is not verified, like everything else read with zero copy.

Logging: a process logs its first staged read once, naming the reason ("copied through the
staging area"). The existing per-read line (`local read: device read returned …`) is logged only
for the errors that fail the read: any error other than EINVAL, and a failed staged read.

**S2. A failed local read fails over.** A local read fails when its plan cannot be made, a device
read fails, or, without zero copy, its checksums still do not match on its second attempt. The
engine then tells the target at once, without waiting for the request timeout, if the target
is still the one at the read's epoch. The target:
- sends that read again to another member of the chunk's ISR, ahead of the reads queued after it;
- avoids the local replica for that read only: every other read still prefers it;
- keeps the local replica in the ISR, and keeps its session open.

If the chunk's ISR has no member other than the local replica, the read ends as unreadable, and
the client gets EIO. Members being removed do not count. For example, on a volume with RF 2 and
`min_isr` 1, a failed local read while the other replica's session is down gets EIO: that member
is being removed. The ISR can also keep a member whose session is down, when `min_isr` requires
it. A failed local read then waits for that member's session to reopen, like any queued read.
The node logs each failed local read with its volume, as now.

A read that fails because a newer target fenced the local replica is not a failure of the
replica: the local replica answers it `Stale`, as it answers a read that succeeded after the
fence, and the target is deposed (§5 of the protocol, fast reads).

Only client reads from the frontend fail over this way. The target ignores a failure report
for any other request, which stays in flight as before.

**S3. The lab check.** `scripts/checks/reconcile.sh`:
- **Fails at the start** if `NODE` holds no replica of `VOL`. Without one, no read is local.
- **Probes a misaligned read.** Among the chunks `c ≥ 1` that have a replica on `NODE`, it finds
  the first whose first block holds data. It reads two blocks across the boundary of chunks
  `c − 1` and `c`, with `O_DIRECT`, twice: into a buffer 4 bytes past a page boundary, then into
  a page-aligned one. It fails if the two differ. It also fails if no such chunk exists, since
  the probe would then read only zeros or only remote data.
- **Checks the staged copy when zero copy is on.** The check tells the mode from the node's own
  log, not from its caller: zero copy is on if, since the attach, the node logged
  `zero copy: local reads go straight into the requests`. It then fails unless the node also
  logged the one-time "copied through the staging area" line.
- **Checks for failed reads:** it fails if, from the attach to the end of the check, `NODE`'s node
  logged a failure line, any line matching
  `local read(:| failed)|on a local read|a request timed out`. These are a failed device read,
  a failed local read, a checksum mismatch, or a request timeout. The node's startup lines,
  which also mention local reads, do not match, and neither do connection timeouts while a
  restarted peer comes up.

**S4. Done when.** Run with `ZERO_COPY=1`, the check passes. One block of every chunk of
`vol1` is readable within the time it takes with `ZERO_COPY=0` (0.83 to 0.92 s on 2026-10-28),
with 0.5 s of margin for the lab's run-to-run noise. The pull request records both runs.

**S5. Protocol document.** `docs/replication-protocol.md` §5 describes the failover of a failed
local read and the unreadable result. §9 describes the simulator's injected failures, and its
table of the seeds that catch each injected bug matches the simulator after this change.

## Tests

- **Target unit tests** (`flaminio-repl`), for S2:
  - **Failover:** another ISR member answers a failed local read. The local replica stays in
    the ISR, with its session open.
  - **Scope:** only that read avoids the local replica.
  - **Ordering:** the re-sent read starts before a later read of an overlapping extent.
  - **No other member:** a chunk whose ISR is only the local replica ends the read as
    unreadable. So does one whose only other member is being removed.
  - **Waiting member:** with `min_isr` equal to the ISR's size, a read waits for a member
    whose session is down. It is sent once that session reopens, within the lease.
  - **Ignored reports:** a report with a stale epoch or session, for an unknown request, for a
    request that is not a client read, or after deposition, changes nothing.
  - **Cleanup:** no avoided member is left behind once a read ends: answered, unreadable, or
    the target deposed.
- **Engine tests** (`flaminio-node/tests/engine.rs`), playing the frontend as
  `direct_reads_from_the_local_replica` does, the first two well within the request timeout:
  - **Single replica:** a failed local read completes with an error, and the ISR is unchanged.
  - **Two replicas:** a failed local read completes with the remote replica's data, and the
    ISR is unchanged.
  - **Stale report:** a failed local read reported after its target was replaced is ignored,
    and does not disturb the new target's read.
    - The test replaces the target by deposition followed by a new attach. A detach is
      not used: it purges the pending local reads, so the report would never reach the
      epoch check.
    - Both targets issue the same requests after their attach. So the new target's first
      local read is in flight with the same session and request numbers as the stale
      report. The test asserts that they match, through `Engine::local_read_ids`.
    - That read must still complete with its data after the stale report is delivered, well
      within the request timeout, and from exactly one `Output::LocalRead`. Without the
      engine's early return (design, "Engine"), the read times out and is sent again, so
      both conditions fail.
- **Simulation:** the protocol simulator fails some of the target's client reads from its own
  replica, during chaos only. It calls the new entry point instead of delivering the reply.
  Every seed still passes. The failure rate is part of each seed's random configuration.
- **Staging unit test** (`flaminio-uring`): `Staging::addr` returns addresses aligned to 4096
  bytes inside the slot, up to its last byte, and panics on a range past the slot's end.
- **Lab:** S3 and S4, with zero copy and without.
