# Proposal: Recover ublk requests only under a fresh volume epoch

- **Status:** Accepted with the plan, 2026-11-13; numbered and moved into docs/adr/ when
  implemented. Changed after approval, plan review rounds 1 to 3 (2026-11-17): the changes are
  listed in [decisions.md](decisions.md), and nothing in them is newly accepted.
- **Date:** 2026-11-12

## Context

[Issue 48](https://github.com/jsfakian/Flaminio/issues/48), [spec](spec.md), [design](design.md).
Devices currently end with the process. Planned #12/#15 records and root ownership establish
which devices a successor may recover; they are prerequisites, not implemented here. #74's
helper, where it runs, holds the ublk control device, and leaves recovery to this change.

## Decision

**The flags.** Devices are added with `UBLK_F_USER_RECOVERY` and `UBLK_F_USER_RECOVERY_REISSUE`
together, after a feature probe. Without both, or when the kernel returns a device without
both, devices are added as today and a restart ends them (Q5's (a), settled at approval).
Under #74's helper the helper probes and sets the flags.

**The record.** Root's record of each device (#15's, with a recovery block) outlives the
process and the pod. It holds no key or token. libublk's JSON is not preserved: a recovery
needs none. libublk's deletion on drop is disowned for a recoverable device; only a detach, an
end or the cleanup deletes one. A device whose record cannot prove it this node's is
quarantined for the operator: never deleted, never resumed (settled at approval). Under #74's
helper only root reads root's record, which holds the device and its identity; the volume and
its lifetime are in a note that the I/O user writes and its successor reads (spec, S3).

**The resume.** A successor resumes a kept device only with a token that names this node,
the agent's from its `only_if_target` attach or the administrator's, under an authorization
that does not change, and only when that token is:
- for the volume's recorded first epoch; and
- for exactly the epoch after the volume's durable run mark on the node, so that no other
  target ran since the device last served.

A gap, or another first epoch, ends the device. The bar on running twice at an epoch stays
#6's `RunEpochs` with #15's run mark; the continuity rule is a second method of that type,
which the simulator checks. Ordinary fencing and reconciliation, and new queues and buffers,
come before any reissued request is served. Unanswered writes may have landed: they are
reconciled, then issued again. Acknowledged writes stay durable. A recovery that did not
finish is never tried again: its device is ended. A resumed device keeps its
lifetime tuple (#15's contract). No QUIESCE, rolling-disruption policy or version negotiation
is introduced. On SIGTERM the node exits at once only while every attached device is
recoverable; with one that is not, #12's rule stays (spec, S4).

**The bound.** A kept device waits 60 s at most from when its server was last alive (settled
at approval); a server that resumed it and ended before the deadline the device then had does
not count. It is ended by the successor, by #74's helper, or, with no data plane, by the
agent through `flaminio-io cleanup --expired`, all under #15's one lock. The bound is an
availability policy, not fencing authority. It is not enforced while neither enforcer runs,
and a mounted kernel id may outlive it (both settled at approval).

**With #74's helper.** The kernel's recovery commands are the helper's requests. The helper
keeps a recoverable device at its child's exit, and still exits with its child; the next
helper takes the device over by root's record.

## Consequences

An agent recovery loop is required, and the CSI image carries `flaminio-io` for the cleanup.
No host-mounted libublk state is added. Older plugins do not resume; an agent that does not run
cannot enforce the bound. Reissue requires the verified backend's double-write tolerance. Store
replay, new epochs and an unreachable ISR member can exceed the 2 s target. A lost `Attach`
reply looks like a gap, and ends the device. Changes in #74, in #15's lifetime contract and in
#6's `RunEpochs` are needed, made by whichever plan lands second (design, Cross-plan items).
No on-disk feature or version rule changes.

## Open

The kernel facts the design relies on were read in Linux 6.1.112 and 6.8.12, not in the 6.6
baseline nor in the lab's 7.0: the design's Kernel check lists them. Until it is done they are
a risk named, not accepted. If Linux 6.6 lacks the reissue, what the baseline promises changes:
a superseding ADR (spec, Q5).

## Confirmation

Record, scan and deadline tests; the resume's steps on a fake kernel and, between the kernel's
two recovery commands, on a real one; the simulator's kept-restart scenarios and the mutation
`resume_over_gap`; the agent's loop against a blocked unmount; the helper's requests and
take-over; real Linux 6.6 reissue tests and separately authorized lab checks, all in the
design. No tests or performance results are claimed here. Accepted kernel and fencing ADRs
remain unchanged.
