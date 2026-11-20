# Issue #66: Frontend boundary — specification

[intent](intent.md) · [decisions](decisions.md) · [design](design.md)

Approved 2026-11-13; not implemented.
Existing intent's step 1 only, after #15. No NVMe/TCP, vhost-user-blk, external host,
loopback fallback or new security protocol. Those and their failover checks keep #66 open.

## Terms

- **Frontend:** adapter exposing one target to a client, with its own bounded buffers and I/O.
- **Binding:** volume incarnation, node process instance and attachment generation.
- **Request:** one validated operation whose id belongs to one binding and queue/tag lifetime.
- **Permit:** engine-granted authority to perform a direct/fast read; it includes normal fencing,
  range-lock and lifetime constraints. It is not a reusable cache of authorization.
- **Test frontend:** process-local adapter, reachable only by tests, using the same coordination
  and read-verification logic as ublk without creating a kernel device.

## The invariant

Extraction preserves existing durable acknowledgement, read admission, fencing and buffer
lifetimes. Only a completion for the current binding/request can answer its client. A frontend
cannot manufacture a successful read or write without the corresponding engine authority.

## Behaviour

**S1. Separation.** Node lifecycle, registry and routing have no dependency on ublk identifiers,
queue types or device operations. Registry entries name a frontend binding and typed endpoint;
ublk is one adapter. Its id/path exist only in its endpoint and adapter implementation.

**S2. Compatibility.** Existing ublk devices, CLI/control-socket output, startup/detach rules,
queue options, zero-copy modes, batching and error results remain compatible. Non-Linux builds
continue to refuse production attachment without an available frontend. No new attach selector
or way to start the test frontend is exposed.

**S3. Validate and bound.** Before engine admission, validate operation, checked offset/length,
4 KiB alignment, geometry, buffer bounds and supported flags. Per-binding requests, routes,
channels and buffer memory are bounded by configured queue count/depth and I/O size. Reject
invalid or excess requests without allocating unbounded work or mutating the engine.

**S4. Completion.** Writes complete only after the matching engine Done. Slow reads combine
validated local/remote pieces and succeed only on matching Done; duplicate/stale pieces and
completions cannot alter a reused buffer or another volume. Flush/discard retain the current
cacheless/no-op behavior subject to #84's conformance contract; no new barrier guarantee.

**S5. Read lifecycle.** Both adapters execute the same read admission, planning, verification,
one checksum retry, fallback and completion policy. A fast read needs final engine approval
before success. Plans and buffers remain alive until every submitted disk operation completes,
including error, cancellation, detach and unwind paths; each plan releases exactly once.

**S6. Stop.** Deposition or detach stops admission and ends all current pending requests once;
late events cannot revive the generation. Target shutdown precedes frontend/device disposal.
A buffer/tag cannot be reused while old local I/O still references it. An unprovable safe drain
fails the node closed rather than releasing borrowed memory to outstanding kernel operations.

**S7. Scheduling.** ublk keeps its queue thread, ring and buffer ownership, including registered
zero-copy buffers. Shared policy preserves round grouping, staging/arrival accounting and
wake/timer behavior. No extra engine thread ownership, lock or per-request allocation is added
just by introducing the interface.

**S8. Test access.** The test frontend drives the same shared policy and real engine/store test
harness. It supports controlled disk results, checksum errors, delayed completions, stop races,
queue saturation and reused tags. Tests no longer duplicate production frontend policy.

**S9. Performance and evidence.** Measure the single-volume ublk A/B against the same build
before extraction: #15 S12's read/write limits, zero checksum failures and clean data checks.
Unstable runs are inconclusive, never a pass. Ordinary CSI/node fault/zero-copy lab checks pass
before release. Documentation distinguishes this boundary from unimplemented export protocols.

## Tests

- S1–S3: compile/module checks and unit validation matrix; exact socket compatibility, limits,
  overflow and unknown flags; no test adapter reachable through production commands.
- S4–S6: shared-policy test frontend with real engine: withheld write Done; mixed read pieces;
  checksum retry/fallback; all stop/late-generation/cancellation/reused-buffer cases.
- S7–S8: round batching, staging guard cleanup, event routing and bounded channel saturation.
- S9: authorized A/B benchmarks and existing lab correctness checks; no lab run in this draft.

## Open questions

Existing intent recommends after #15 and a test-only adapter; neither is recorded as accepted
here. Exact shared interface must fit #15's future registry/router and #48's recovery changes.
Kernel distribution survey and authenticated external export/failover designs remain later steps.

Settled at approval, 2026-11-13: after #15, kept small; the test frontend is test-only; the shared
interface as the design proposes, its unknown details fixed by a thin compilable prototype; the
kernel survey before any plan of steps 2–4 (see decisions.md, Settled at approval).

Settled 2026-11-13 (user's instruction): the kernel's NVMe target or our own is chosen by step
2's ADR after the kernel survey, neither in this step (see decisions.md).
