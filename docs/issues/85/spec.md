# Issue #85: io_uring batch lifetime validation — specification

[Intent](intent.md). Settled choices: [decisions](decisions.md), when present.
Implementation: [design](design.md), when drafted. Approved 2026-11-13; not implemented.

## Terms

- **Operation:** one positioned read, write or flush passed to a device method.
- **Window:** the bounded group prepared and submitted together on one ring.
- **Published entry:** an SQE visible to the ring for a present or later submission.
- **Borrowed buffer:** caller memory valid only for the duration of its synchronous call.
- **Held buffer:** an owned/reference-counted allocation retained through asynchronous completion.
- **Preparation failure:** an ordinary error before a window is ready, such as an unsupported length.
- **Drained:** every published operation has finished or been cancelled with completion confirmed;
  none can still access its buffers and none of its completions can satisfy a later call.

## The invariant

A synchronous method may return only when no queued or submitted operation can access memory
whose borrow or ownership the return releases. A completion is attributed only to the operation
that created it. An asynchronous method may return pending only while the device retains every
allocation the kernel may access, until its completion/cancellation is confirmed.

## Behaviour

**S1. Complete window validation.** Before publishing any entry in a window, reject unsupported
lengths and every other fallible preparation condition required by that window. An operation
that fails length validation does not cause an oversized bounce allocation. Validation preserves
the existing representable-length boundary; it does not silently truncate or split an operation.

**S2. Rejected windows publish nothing.** A preparation failure returns an error without leaving
any entry from that window available to a later submission. Its buffers are untouched by that
window's I/O. Earlier windows of the same call may already have completed; this issue does not
make a multi-window batch transactional or roll back their effects.

**S3. Lifetime through completion.** After a window publishes an entry, retain all caller borrows
and bounce allocations until every entry that may access them is drained. A recoverable
submission, device or short-transfer error cannot release the remaining buffers early. Read
bounce-buffer copyback occurs only after the relevant kernel access has ended.

**S4. Unrecoverable ownership is fail-stop.** If kernel ownership cannot safely be resolved, do
not return an ordinary error and continue using the device. Preserve the existing fail-stop
policy rather than risking a dangling buffer. No success may be fabricated from incomplete I/O.

**S5. Completion isolation.** Each completion belongs to the correct window/call or retained
asynchronous commit. An error, empty batch or rejected operation leaves no completion that can
be mistaken for a subsequent read, write or flush. Reusing request identifiers is safe only once
their earlier uses can no longer produce completions.

**S6. Owned asynchronous paths.** Apply the same preparation/publication audit to early writes,
asynchronous commits and their synchronous fallback. A pre-publication error leaves their
accounting usable; published owned buffers remain retained through completion or safe teardown.
Flushes still wait for their required writes, commit outcomes retain their ordering, and drop
never releases a kernel-owned allocation early.

**S7. Preserve supported I/O.** Valid aligned and bounced operations continue to work in direct
and buffered modes, including empty batches and calls crossing queue-window boundaries. Existing
kernel errno and short-transfer errors remain errors. This fix changes no persistence, format,
replication or transport contract.

**S8. Regression evidence and documentation.** Tests and unsafe-code comments explicitly cover
all fallible exits around publication. Tests for the historical defect keep buffers alive and
use temporary regular files or controlled fault injection, not deliberately freed memory. A
platform unable to run a kernel-specific case reports the limitation rather than claiming the
kernel path was validated by Miri.

## Tests

- **S1–S2:** unit tests at the representable-length boundary, with invalid operations first and
  after a valid prefix. Fault-inject preparation failure without giant resident allocations;
  assert that rejected windows publish no SQEs and no avoidable bounce allocation occurs.
- **S2, S5, S7:** invalid operations in later windows; earlier completed windows may retain
  their effects, but subsequent unrelated read/write/flush calls must neither submit rejected
  work nor consume its completions. Exercise empty and full windows.
- **S3–S5:** kernel/device failure and short-I/O tests prove every outstanding operation is
  drained before error return; controlled unrecoverable submission tests verify fail-stop.
- **S1–S5, S8:** adapt the audit's retained-buffer temporary-file regression: after rejecting
  a valid-small/oversized batch, later I/O cannot cause its small write to land or contaminate
  a flush result. Test available 64-bit Linux configurations explicitly.
- **S3, S6–S7:** aligned/bounce read/write cases, asynchronous early writes/commits, synchronous
  fallback and drop; check retained ownership, commit ordering and later device reuse.
- **S7–S8:** `make lint test`, then `make miri sanitize` for applicable memory/FFI coverage.
  Miri covers pure Rust buffer behavior, not real kernel io_uring; no shared lab is needed.

## Open questions

None. The design must select the smallest preparation/draining change that meets these rules;
no new architectural decision, dependency or format change is required.
