# Proposal: Admit only qualified physical lease clocks and quarantine discontinuities

- **Status:** Accepted with the plan, 2026-11-13; numbered and moved into docs/adr/ when implemented.
- **Date:** 2026-11-12
- **Issue:** [#83](https://github.com/jsfakian/Flaminio/issues/83)

## Context

[Intent](intent.md), [spec](spec.md), [design](design.md). Lease safety needs bounded relative
clock rates throughout takeover. Runtime uses Instant, and current simulation uses affine
positive-rate clocks. Neither establishes support for physical suspend, stopped virtual clocks
or coordinated rollback. Audit ADR 0007 proposes the corrected takeover/clock contract and
superseding accepted ADR 0005; it remains unaccepted in the audit checkout.

## Decision

Propose initial production profile physical-boottime-v1: qualified physical Linux hosts,
CLOCK_BOOTTIME elapsed authority, L=5 s, rho=100,000 ppm and a qualified<=1 us read-error/resolution bound and
takeover `ceil((L+3 us)*(1+rho))+3 us` (5,500,007 us at defaults). The operator explicitly attests physical provenance, hardware rate behavior
and no rollback of running storage. Startup probes reject missing prerequisites, detected
virtualization and unknown provenance; they do not certify relative rate from observations.
Known-physical container deployments require host provenance. No arbitrary VM pause/migration
or memory/disk snapshot rollback support, and no unsafe production bypass, is provided.

All authority transitions and completion paths resample one clock domain. Detectable
suspend-offset discontinuities, backwards samples, failures and arithmetic overflow quarantine
the whole daemon before admitting more success or sessions. Generation stamps reject old
callbacks; disk epochs remain the authority. Physical suspend shorter than observation
uncertainty can remain safe because the qualified clock counts the actual elapsed interval.
No claim is made that joint rollback or a consistently stopped virtual clock is detectable.

Quarantine is sticky until operator restart. Restart recovers storage and obtains a fresh
metadata attach epoch under only-if-target semantics via the [signed agent-owned process
handoff](fresh-process-contract.md), never daemon/io self-authorization or an old token, then full fencing/reconciliation before
serving. Already authorized pre-fault successes may deliver later; new post-fault success
permits are forbidden. Unanswered writes can have landed and are never rolled back for recovery.

## Options

- **BOOTTIME alone:** covers physical suspend elapsed time but cannot certify virtual timing,
  clear stale callbacks or establish a relative-rate guarantee; insufficient by itself.
- **Document exclusions only:** leaves existing runtime paths unguarded against detectable
  faults; retain explicit exclusions but also implement fail-closed transitions.
- **Support arbitrary VM pause/rollback:** requires enforceable hypervisor/external incarnation
  controls beyond local userspace sampling; no such contract is qualified in this proposal.
- **Reopen automatically after a good sample:** can reuse stale sessions or lose evidence of a
  discontinuity; require operator restart and normal fresh-epoch recovery instead.

## Consequences

Initial support excludes current unqualified lab VMs and can block existing deployments.
Trusted operators remain responsible for hardware qualification and prohibited rollback;
userspace cannot prevent a malicious hypervisor from lying. Multiple clock samples cost
syscalls on I/O paths and must be measured. Detected faults sacrifice availability and may
return errors after writes landed. Recovery can wait for metadata quorum. None is accepted
merely by drafting the proposal.
Settled at approval, 2026-11-13: these consequences are accepted with the plan (see decisions.md, Settled at approval).

## Confirmation

Shared pure guard/runtime fault tests and simulator event classes, generation checker and
skip-invalidation mutation described in the design. Preserve existing no-lease/no-wait and
fast-read mutations. Hardware qualification/lab tests need separate authorization and cannot
be inferred from fixed-rate model success. This issue-local proposal has no global ADR number;
it does not mark audit ADR 0007 accepted or modify accepted ADR 0005.
