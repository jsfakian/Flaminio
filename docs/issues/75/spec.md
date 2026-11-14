# Issue #75: QoS, background scheduling and performance gates — specification

[Intent](intent.md). [Design](design.md). Approved 2026-11-13; not implemented. This
specification covers the issue's first step only, per-volume limits; the
[intent](intent.md#what-stays-open) lists the steps that stay open.

## Terms

- **Client request:** one READ or WRITE that the kernel hands the ublk frontend on one tag, of
  4 KiB up to the device's largest request (at most 1 MiB), before the engine splits it at chunk
  boundaries.
- **Policy:** a volume's `iops`, `bytes_per_second`, `burst_iops` and `burst_bytes`. A rate of 0
  leaves its dimension unlimited. A burst of 0 on a limited dimension is one second of its rate.
- **Effective burst:** a limited dimension's burst, or one second of its rate when the burst is 0.
- **Bucket:** per limited dimension, credit that grows at the rate up to the effective burst.
- **Held request:** a client request the engine has received and not yet handed to the target's
  state machine, waiting for credit. It keeps its ublk tag.

## The invariant

QoS acts only on client requests before the target's state machine sees them. It never holds,
reorders or drops anything the target emits (requests, heartbeats, replies, metadata calls),
never completes a request with an error, and never changes a lease, an epoch or an ISR. A volume
without a policy behaves as today.

## Behaviour

**S1. Policy.** A StorageClass sets a volume's policy with four decimal parameters: `qosIops`,
`qosBytesPerSecond`, `qosBurstIops`, `qosBurstBytes`. The policy is valid when each unlimited
dimension has a burst of 0 and, if bytes are limited, their rate and any nonzero burst are at
least 1 MiB (1048576). Create refuses an invalid policy. The metadata service stores the policy
with the volume at create; nothing changes it afterwards. PVC annotations are not read. A
volume created without a policy, or before this change, is unlimited.

**S2. Accounting.** A client request costs 1 I/O and its length in bytes, charged once, whatever
the queue, the chunks it spans or the replicas it reaches. Reads and writes share the buckets.
FLUSH and DISCARD cost nothing: the frontend answers them without the engine. A request is
admitted when no earlier request of its volume is held and every limited bucket holds its cost;
admission debits both. A cost above the effective burst is charged as the effective burst; the
frontend never sends one (its requests are at most 1 MiB, a limited effective burst at least
1 MiB). Over any interval of length Δ in one target's life, each limited dimension admits at most
`rate × Δ + b` requests or bytes, `b` its effective burst, while no request costs more than `b`.
- **Fast reads** are charged when the fast path admits them. A fast read that the fast path
  refuses is charged once, on the queued path. One refused after its reads, and submitted again,
  is charged again.

**S3. Target start.** A target reads the policy with the volume when it attaches, and starts
with empty buckets. A restarted or failed-over target starts empty again: no target gets a free
burst. When a target is detached or deposed, its held requests fail as its requests in flight do.

**S4. Waiting.** Held requests wait in their volume's arrival order. Each is admitted as soon as
its cost is covered: at most `max(1/iops, io_buf/bytes_per_second)` after its predecessor's
admission, 1 s at the floors of S1. One request per tag can wait, so a held request waits at
most `depth × queues` times that; requests beyond the tags wait in the kernel for a tag. QoS
never fails a request. Because the target's request timeout starts only when it emits a request,
a held request cannot fail a session or change an ISR.

**S5. Versions.** Limited volumes are meant to exist only once every metadata instance and every
data plane runs this version; operators ensure it:
- the metadata service refuses a limited create until it is started with its QoS switch (an
  existing id still answers that it exists, so a retry matches); the switch gates creates only,
  never how an existing limited volume is read. Operators turn it on,
  on every instance, in a second rollout once all instances and data planes run this version.
  Nothing enforces that order. Once a limited volume exists, an older metadata instance serves
  it unlimited and erases its policy for good on its next attach, whatever the switch: rolling
  back is unsupported, and the only safe rollback deletes the limited volumes first;
- while instances are mixed, the CSI controller deletes a volume it has just created whose
  stored policy differs from the request (an older instance dropped it), and fails the create;
  `metactl` reports such a volume and fails; an older CSI controller refuses the parameters;
- the metadata service refuses a limited volume to a data plane that does not declare that it
  enforces QoS. The refusal comes after the attach moved the target, so the volume has no
  serving target until it is attached on a node that enforces QoS.

## Tests

- **S1:** the validity rule as one table in Go and in Rust; create stores and returns the policy
  and refuses an invalid one; CSI parameter parsing, retries with the same and another policy.
- **S2:** fake-time bucket tests (empty start, refill, cap, both dimensions, arrival order, a
  cost above the effective burst, the `rate × Δ + b` bound on what is admitted over every window,
  with 0 bursts among the policies, overflow); engine tests of a request across chunks (with a
  burst of 2, so a double charge shows), fast reads sharing the bucket and the fast read charged
  again.
- **S3:** engine tests: each attach starts empty, timed from before the attach; held requests
  fail on detach and deposition; an unusable policy fails the attach.
- **S4:** an engine test with a short request timeout: held writes complete in order, each no
  sooner than its turn after the attach, all succeed, the ISR does not change. The lab check
  holds requests past blk-mq's timeout without an error.
- **S5:** the switch off refuses a limited create but answers that an existing id exists, and
  still refuses an undeclared data plane and returns the policy of a limited volume; the CSI
  controller deletes a create that lost its policy (returning the delete's error if that fails)
  and never deletes on `AlreadyExists`;
  `metactl` checks its policy argument and fails on a lost policy; an undeclared data plane's
  attach fails with no target; the lab's flag script test; the interop test checks the new
  fields.

## Open questions

- **Q1:** counting requests (not 4 KiB units), sharing the limits between reads and writes,
  starting each target empty and fixing the policy at create are Proposed product semantics.
  Settled at approval, 2026-11-13: as proposed (see decisions.md, Settled at approval).
