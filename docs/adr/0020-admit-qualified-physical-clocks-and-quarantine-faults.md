# 0020. Admit qualified physical lease clocks and quarantine discontinuities

- **Status:** Proposed
- **Date:** 2026-11-28
- **Issue:** [#83](https://github.com/jsfakian/Flaminio/issues/83)

## Context

Lease safety assumes bounded relative clock rates throughout takeover. Runtime Instant and
fixed-rate simulation did not establish physical-suspend or virtual-pause/rollback behavior.
CLOCK_BOOTTIME counts physical suspend but cannot prove a hardware rate guarantee or prevent
joint clock/memory/disk rollback. See the approved [spec](../issues/83/spec.md),
[design](../issues/83/design.md), [fresh-process contract](../issues/83/fresh-process-contract.md)
and [settled decisions](../issues/83/decisions.md).

## Decision

Admit the initial physical-boottime-v1 production profile only on explicitly qualified physical
Linux hosts. Require root-owned operator attestation, matching node/source/profile, fixed
cluster-wide L=5 s and rho=100,000 ppm, qualified absolute read error and reported resolution
at most 1 us, and prohibition of live rollback. Reject virtual or unknown host provenance;
containers require observed provenance and qualification of the underlying physical host.
Startup probes observe prerequisites and cannot certify future relative rate or rollback safety.
There is no force flag or environment bypass.

Use checked BOOTTIME elapsed authority for every admission and final completion, with per-thread
MONOTONIC/BOOTTIME observers and one process clock generation. The offset anchor includes 1 ms
suspend-detection uncertainty; clock accuracy remains a separate <=1 us bound. With elapsed
observation error E=3 us, takeover waits at least ceil((L+E)*(1+rho))+E, or 5,500,007 us at the
fixed defaults. Timers and wall clocks confer no storage authority.

A detected discontinuity atomically quarantines the whole process before its event can admit
more work. Final success permits and quarantine have a total order; earlier permits may deliver
later, while no new old-generation permit/session is admitted. Invalidate targets, handles,
callbacks and both directions of replication sessions without erasing durable epochs or
releasing buffers before I/O ends. A good later sample never clears quarantine.

After operator diagnosis and process restart, recover storage and obtain a fresh Attach epoch
from the authorized agent/administrator with only-if-target semantics. Bind its exact FLCT
capability to the new daemon's unpredictable 32-byte challenge using the signed 134-byte FLCH
envelope. Node/io cannot mint authority. Every target-start path verifies the current process
proof before effects; old standalone tokens and startup/fallback paths refuse. Ordinary full-ISR
fencing, lease waits and reconciliation remain necessary. Private initialization, when #79
lands, cannot rebind old reserved epochs to a restarted daemon.

## Options

- **BOOTTIME alone:** covers physical suspend duration but neither invalidates stale callbacks
  nor qualifies relative rates or hypervisor behavior; insufficient.
- **Document exclusions only:** leaves detectable runtime faults unguarded; combine explicit
  exclusions with admission and quarantine enforcement.
- **Arbitrary VM pause/rollback support:** needs enforceable external incarnation/clock contracts
  not supplied by local sampling. Current lab VMs remain unqualified.
- **Automatic reopening after a good sample:** can reuse stale sessions and uncertain work;
  require operator restart and fresh-epoch recovery.

## Consequences

- Existing unqualified virtual/container deployments are refused. Trusted operators must qualify
  hardware and honor exclusions; deliberate concealment by root/hypervisor is outside detection.
- Multiple fresh samples add syscall cost on I/O paths; #75 must measure that cost rather than
  weakening checks based on an unmeasured assumption. No steady-state metadata round trip or
  new mutex is introduced.
- Quarantine sacrifices availability; operations returning errors may already have written data.
  Metadata outage or changed ownership can block recovery. Durable data is never rolled back.
- The FLCH local ABI and additive Attach fields change target-start compatibility; replica FLCT,
  replication frames and on-disk formats remain unchanged. #15/#79 compose their own lifecycle
  contracts without using instance counters as cryptographic process challenges.
- Audit ADR 0007 remains Proposed. This decision does not accept its separate amendment or edit
  accepted ADR 0005; current stricter takeover/readiness gates remain binding implementation.

## Confirmation

The shared guard, runtime fault/completion paths, strict startup probes and actual authenticated
handoff must be checked with injected clocks, independent ideal-time/history oracles and real
CAS/crypto replay. Catch skip_clock_invalidation, ignore_observation_error,
accept_old_process_handoff and mint_handoff_without_epoch_advance while retaining existing
lease/wait/fast-read controls. Distinguish supported process pause/physical suspend from excluded
virtual stop/rollback. Local/model checks do not qualify hardware. Evidence and code/security
review results are recorded in the [implementation report](../issues/83/report.md).
