# 0007. Takeover readiness includes reconciliation and a clock contract

- **Status:** Proposed
- **Date:** 2026-11-12

## Context

This proposes superseding ADR 0005's early-read rule, while retaining replica-confirmed leases and keeping metadata off steady-state I/O. ADR 0005 permits reads after fencing one member; the implemented [protocol §5–6](../replication-protocol.md#6-failover-fencing-and-reconciliation) and `Target::advance_chunk` require the whole ISR to be fenced and uncertain extents reconciled. The protocol's recorded counterexamples establish why early reads are unsafe. The [2026-11-12 audit](../audits/2026-11-12/README.md) also identifies a missing host-clock contract.

Supersedes **0005 only upon acceptance**. Until then 0005 remains historical accepted policy with a documented contradiction; the current protocol and code use the stricter gate. Do not implement the old early-read shortcut.

## Decision

- A chunk serves no client I/O until every member of its effective ISR is durably fenced at the new epoch and reconciliation has completed. Removing an unreachable member requires fencing at least one existing ISR member, waiting the takeover interval, preserving the minimum ISR, and committing the conditional metadata transition.
- Lease confirmations expire at request **send time** plus L, not response receipt plus L. The lease set includes pending joiners from proposal and pending removals until commit; removing members are not read sources. Fast reads also validate lease, membership generation and fencing when they complete.
- Clocks used by old and new targets must advance at relative rates within 1+ρ throughout a takeover. Process scheduling pauses do not relax this assumption. Select a suspend-aware elapsed clock for supported Linux host suspend; `CLOCK_BOOTTIME` addresses host suspend but does not prove safety across arbitrary VM pause or snapshot rollback.
- Until enforced otherwise, host suspend, VM pause/live migration with unbounded clock discontinuity, and VM/disk snapshot rollback are unsupported while storage runs. After a detected discontinuity, invalidate admission and sessions and reattach/reconcile before I/O. Document which events are detectable; do not claim a userspace clock check makes arbitrary rollback safe.
- Node-wide shutdown taints require verified power/shutdown evidence (#82). A live partition does not supply it; per-workload storage recovery needs explicit opt-in and cannot fence unrelated application side effects.
- The new target may omit the wait only when every existing ISR member is fenced. Readiness is per chunk; a volume-wide status summarizes it without granting permission.

## Consequences

The stricter gate can delay first reads and can keep a volume unavailable when authoritative ISR members cannot be reached. Keeping I/O available during metadata outages is preserved for healthy existing sessions. Clock support becomes a deployment requirement and needs enforcement, not just documentation.

## Confirmation

Keep takeover/fast-read simulator scenarios and mutations for missing waits, early reads, pending membership and skipped reconciliation. Add stopped-clock, suspend/resume and VM-rollback scenarios that demonstrate the unsupported cases fail closed when enforcement exists. Runtime enforcement is #83; the current engine uses `std::time::Instant` and is not certified suspend-safe.
