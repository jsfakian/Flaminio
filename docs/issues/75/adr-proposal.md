# Per-volume QoS limits count client requests and act before the replication state machine

- **Status:** Accepted with the plan, 2026-11-13: binding for this plan's implementation from the
  approval on; numbered and moved into docs/adr/ only with the implementation.
- **Date:** 2026-11-12
- **Scope:** Issue-local until it is numbered; approved with the plan (decisions.md).

## Context

[Issue #75](https://github.com/jsfakian/Flaminio/issues/75) needs per-volume limits that splits
and multiple queues cannot bypass, and that hold across failover and restart. A volume's target
fails a session when a request it sent waits longer than `request_timeout`
(`flaminio-repl/src/target.rs:811-828`), and the ISR then shrinks; anything that delays
requests after the target emits them turns throttling into lost redundancy. Logical blocks are
4 KiB (`flaminio-io/src/ublk.rs:187`), so a count of 4 KiB units is the byte count divided by
4096.

## Decision proposed for review

Settled at approval, 2026-11-13: accepted as proposed (see decisions.md, Settled at approval).

- **What is counted:** client READ and WRITE requests per second and client bytes per second,
  reads and writes together, each request charged once when it is admitted, before the engine
  splits it at chunk boundaries or replicates it. FLUSH and DISCARD cost nothing.
- **Where:** the volume's target, in the engine, before `Target::submit` and
  `Target::try_fast_read`. Throttled requests wait in arrival order, holding their ublk tags, and
  never complete with an error. QoS never holds, reorders or drops what the target emits.
- **Buckets:** one per limited dimension, empty when each target starts; the burst defaults to
  one second of the rate.
- **The policy:** set by StorageClass parameters, stored with the volume at create, fixed for
  its life. A data plane declares that it enforces QoS when it reads a volume; the metadata
  service refuses a limited volume to one that does not.
- **Activation:** the metadata service refuses limited creates until its `--qos` switch is on;
  the switch gates creates only. Operators turn it on, on every instance, in a second rollout
  once every metadata instance and data plane runs this version; nothing enforces that order.
  Once a limited volume exists, an older instance serves it unlimited and erases its policy for
  good on its next attach, whatever the switch: rolling back is unsupported, and the only safe
  rollback deletes the limited volumes first.

## Options

- **Request count and bytes (proposed):** as cgroup `io.max`, two independent limits; a client
  that splits pays more, and the engine's own splits are not charged.
- **Normalized 4 KiB units and bytes:** with 4 KiB blocks the two are one limit at two scales.
- **QoS in the target state machine or the transport:** the delay counts against the request
  timeout, so a saturated volume loses ISR members.
- **Per-queue buckets:** multiply the limit by the queue count.
- **A full burst on each new target:** every failover or restart grants another burst.

## Consequences and open choices

Q1 (these semantics) stays open until the design is approved.
Settled at approval, 2026-11-13: Q1 as proposed (see decisions.md, Settled at approval).
Changing a volume's policy, PVC overrides under tenant ceilings, background scheduling and
bounded transport queues are later steps, each with its own design; the last two must keep the
rule that the target's emitted requests are not delayed by scheduling. Accepted ADRs 0001, 0002,
0003 and 0005 stay consistent; the protocol, the on-disk format and capability tokens do not
change.

A throttled request can wait up to `depth × queues × max(1/iops, io_buf/bytes_per_second)`,
plus any wait in blk-mq for a tag: 128 s at the floors with the default depth 128, one queue
and 512 KiB requests. Tasks blocked that long trip the hung-task warning, so no node that may
attach a limited volume (any node its pod may run on) may set `kernel.hung_task_panic=1`.
Changed after approval (round 3): widened from the nodes serving limited volumes (decisions.md).
Settled 2026-11-13 (user's instruction): the widening is accepted (see decisions.md).
A held request never fails because the kernel re-arms a privileged ublk request's timeout; that
is assumed for kernels from 6.6.

## Confirmation

[Design](design.md): the bucket's fake-time tests, the engine test that holds writes past a
short request timeout without a session failure or ISR change, the switch and mixed-version
refusals, and `make qos-check` on the lab, which prints the node's `uname -r` with its verdict.
Kernels the timeout re-arm was checked on: none yet; each passing run adds its version here.
No implementation or result is claimed yet.
