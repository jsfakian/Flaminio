# Issue #85: io_uring batch lifetime validation — design

How the [spec](spec.md)'s rules are built. [Intent](intent.md).
Settled choices: `decisions.md`, when present. Approved 2026-11-13; not implemented.

The facts it starts from:
- `run` publishes entries while still calling fallible `len32` and allocating bounce buffers
  (`dataplane/crates/flaminio-uring/src/uring.rs:175–228`). Dropping the SQ handle publishes
  the prefix even when `?` returns before the completion loop. The existing lifetime comment
  at lines 218–222 therefore does not cover every exit.
- The normal completion loop drains a window before returning device errors, and aborts for
  non-retryable submission failures (`uring.rs:230–274`). Its identifiers restart at zero
  each window, so a stale CQE can be interpreted using a later window's lengths.
- `submit_commit_writes` checks lengths first, but constructs `Held` and inserts into its
  ownership map while publishing (`uring.rs:297–329`). `start_commit` increments `next_gen`
  before that fallible call (`556–575`); rejection can separate earlier writes from their
  intended next successful commit. `Drop` drains owned commit writes (`496–509`).
- `AlignedBuf` allocations abort on allocation failure; layout construction can panic
  (`dataplane/crates/flaminio-uring/src/aligned.rs:29–64`). Containers and bookkeeping also
  need preparation before publication. `BlockDevice` remains a safe trait with synchronous
  completion and asynchronous ownership promises (`flaminio-store/src/device.rs:17–66`).
- The [audit reproduction](../../audits/2026-11-12/repro/uring.rs.txt) retained the small
  allocation, observed a delayed `0x5a` write, and avoided deliberately dangling pointers.
  The audit evidence lives on the audit branch if absent from this issue branch.

## Prepare a whole synchronous window (S1, S2, S7)

Keep `QUEUE_DEPTH = 256`, the two-ring model and per-window processing. Split `run` into
preparation, publication and draining phases; retain the existing public signatures.

1. Before opening the SQ, collect checked `u32` lengths for every read/write; flush has no
   data length. Preserve `InvalidInput` and `I/O larger than 4 GiB` for unsupported lengths.
2. Prepare every required `AlignedBuf`, lengths, SQEs and completion-seen storage before
   publishing any SQE. Validate layouts and reserve containers here. Any recoverable
   preparation failure returns with this window unpublished. Test hooks may inject errors
   here; they must not enter production builds. No oversized allocation precedes validation.
3. Inspect available SQ capacity before publication; an unexpected dirty ring is an internal
   ownership failure and fail-stops. Push the complete prepared window without allocation,
   length conversion, `?`, or panic-producing `expect` in the publication loop. An unexpected
   push failure after the first entry fail-stops.
4. Submit and drain the window, then copy bounced reads back. Completed earlier windows may
   remain on disk when preparation of a later window fails. Do not promise batch atomicity.

Preparation keeps the existing aligned-buffer fast path. It does not create a bounce copy for
an aligned operation or change read copyback policy after completed short/device-error I/O.

## Keep ownership through every exit (S3, S4, S5, S8)

Use an internal window lifetime guard armed immediately before the first SQ mutation and
released only after all expected CQEs have been consumed. Declare it after prepared buffers,
so its unwind handling runs before their destruction. While armed, unwinding must abort;
ordinary error paths must drain first. This closes panic exits as well as explicit `?` exits.
The guard does not attempt cancellation or claim that closing an fd drains borrowed requests.

Retain `EINTR`, `EAGAIN` and `EBUSY` retry handling; partial successful submission does not
mean the window completed. Continue until every published operation has one completion.
Record the first device/short-transfer error, but return it only after draining. Retain existing
fatal-submission diagnostics and process abort when ownership cannot be established.

Check CQE identifiers before indexing the window and track duplicate completions. An unknown,
duplicate or otherwise impossible CQE fail-stops rather than counting toward a successful
drain. A completed window leaves no SQ prefix or CQE for another call. Keep the guard armed
through completion checking; disarm before read copyback once the kernel owns no buffers.
Update each unsafe push comment to name the prepared storage, guard and exact release point.

## Prepare asynchronous bookkeeping before publication (S1, S4–S6)

Run full length/layout preflight before `start_writes`/`start_commit` change this request's
generation or ownership state, including their oversized synchronous fallback. `make_room`
may still finish older work. A rejected commit does not consume `next_gen`, erase early-write
errors, or strand early writes: the next successful commit covers them.

For `submit_commit_writes`, prepare all `Held` buffers, SQEs and tags before publication.
Reserve the ownership map and commit queue, and check counter/tag arithmetic (`FLUSH_TAG`
separates writes from flushes). If identifiers cannot be represented, return a preparation
error before publication; never wrap a tag into the flush namespace or reuse a live tag.
Install ownership/accounting before an entry can reach the kernel. The publish phase contains
no fallible allocation or unchecked increment. Use the same armed fail-stop discipline for
unexpected publication/unwind failures. Safe pre-publication errors discard only newly
prepared data; accepted earlier commits remain owned and pollable.

Preserve flush coalescing, generation order and `poll_commit`'s oldest-first outcomes. Validate
CQEs before changing retained state: write tags identify one live Held and its expected length;
flush tags identify exactly a `Flush::Submitted` group owner. `Waiting`, `Covered`, `Done`,
unknown and duplicate flush CQEs fail-stop. Only result0 is successful fsync; negative results
remain errors, positive results are impossible and fail-stop. Covered generations receive the
owner's recorded outcome through coalescing logic, never an independently accepted CQE.

Extend ownership unwind protection beyond publication. Reaping a completed `Held::Shared`
may run a safe `Bytes::from_owner` destructor which panics while other writes remain pending.
Every release/reap path retains outstanding allocations; `UringDevice::drop` arms a local abort-
on-unwind guard before any such destructor can run, and disarms only after both queues and all
accepted writes/commits are drained. Its stack guard runs before device fields can unwind-drop.
The same protection covers any path where unwinding would destroy outstanding ownership;
ordinary polling may unwind only while retained device ownership remains intact. Closing the
ring is not proof that borrowed/owned memory is safe to release. Synchronous fallback uses the
corrected `run` and still completes only after its required flush.

## Files touched

- `dataplane/crates/flaminio-uring/src/uring.rs`: preparation helpers, lifetime guard,
  publication/accounting checks, safety comments and regressions (S1–S8).
- `dataplane/crates/flaminio-store/src/device.rs`: clarify lifetime and non-atomic batch error
  semantics in trait comments without changing signatures (S2, S3, S6).
- `docs/issues/85/report.md`: implementation evidence and check limitations (S8).
- `docs/followups.md`: record the fixed audit finding and actual validation result after shipping.

## Contracts

- **Public Rust API:** unchanged `BlockDevice` and `UringDevice` signatures. Invalid lengths
  return `io::ErrorKind::InvalidInput`; no operation is silently split/truncated. No transactional
  promise across windows. Existing short-transfer/OS-error behavior remains.
- **Ownership:** synchronous error/success releases borrowed data only after draining;
  asynchronous acceptance retains owned data through completion/drop. Pre-publication rejection
  leaves no new queued work; a failed commit start does not advance the early-write generation.
- **Private test seam:** inject preparation failure at operation index, transient/fatal submit
  results and CQE sequences under `cfg(test)`. No runtime flag, environment variable, RPC,
  wire record, format version or production dependency is added.

## Tests

All named cases below belong in `flaminio-uring/src/uring.rs`'s test module unless stated.

| Rules | Concrete case and level | Proof |
|---|---|---|
| S1, S2, S8 | `rejected_batch_does_not_retain_borrowed_buffers`, Linux temporary-file regression | Reserve a >4 GiB readable mapping on 64-bit Linux, keep buffers alive until device drain; reject later length, run unrelated read/write/flush, verify no delayed write or stale completion. |
| S1, S2, S7 | `window_preflight_rejects_before_allocation`, unit/injected preparation | Check `len32` at `u32::MAX` and one above using integer lengths; inject first/middle/last preparation failure and assert no SQ publication or rejected huge bounce allocation. |
| S2, S5, S7 | `rejection_at_window_boundaries`, kernel/injected preparation | Reject indices 0, 255, 256 and 257 with aligned/bounced read/write, direct/buffered mode; only earlier completed windows may change data; next read/write/flush succeeds. |
| S3, S5, S7 | Extend `errors_are_reported_after_draining` and roundtrip test, kernel | errno/short read with later valid operations; empty, 256 and 600 operations; every expected CQE reaped before return and later reuse correct. |
| S3–S5, S8 | `submission_lifetime_failures`, injected backend/subprocess | Partial submit and retryable errors drain; fatal submit, bad/duplicate IDs, publication failure and injected panic while armed terminate the child, with no returning error path. |
| S1, S5, S6 | `rejected_commit_keeps_early_writes`, injected preparation/kernel | Start early writes, reject a later commit, then commit successfully; retain early errors/ownership, wait for required writes and report outcomes once in order. |
| S3, S6, S7 | Extend asynchronous ordering tests; `drop_drains_owned_writes`, kernel | Shared/bounced buffers dropped by caller, overlapping synchronous activity, coalescing and oversized fallback; drop waits before releasing tracked allocations. |
| S6, S8 | `commit_prepare_failure_keeps_accounting`, unit/injected preparation | Fail each preparation index/reservation and test tag bounds; no newly published SQE, no generation skip, no live tag reuse, existing commits remain pollable. |
| S3–S6, S8 | `panicking_owner_during_drop_aborts_before_release`, injected production reaper/subprocess | Complete a panicking Bytes owner while another tracked write is outstanding; assert SIGABRT before that owner's release marker. Keep an independent backing-allocation reference so the negative regression never creates dangling kernel memory. |
| S5, S6 | `commit_flush_cqe_requires_submitted_owner`, injected production reaper | Inject Waiting/Covered/Done/unknown tags, duplicate error-then-success and positive fsync result; each aborts before outcome mutation. Valid Submitted error/success and coalesced covered outcomes remain once-only. |

Use a small private submission adapter or injected internal helper to exercise the same control
flow as production; do not test a second implementation of the algorithm. Preparation-failure
hooks run only while unpublished. On 32-bit platforms omit only the oversized-mapping case
with an explicit reason; integer-boundary and injected tests still run. Never construct a
read buffer from a read-only mapping; use a writable anonymous mapping for read-batch coverage.

Validation at implementation time: `make lint test`, `make miri`, `make sanitize QUICK=1`;
`make race hygiene contracts` before a PR. Miri exercises aligned-buffer helpers only and
cannot validate kernel io_uring. Kernel regressions require Linux with io_uring permitted;
record unavailable kernel/sanitizer coverage explicitly. No simulator mutation is needed:
this changes the kernel boundary, not replication or membership safety rules.

## Lab

None. Use disposable local regular files; no shared VM, raw disk, reformat or lab restore.
Anonymous mappings must be unmapped after the device is drained/dropped; remove temporary files.

## Docs

Keep safety proof comments with the actual publication/drain boundary. Report commands and
results for #85 in `report.md`; update follow-ups when fixed. No new lab command, benchmark
claim or README/performance-plan change is required.

## Risks

- **Fail-stop is retained policy.** Unknown kernel ownership can end the process. Do not replace
  it with an ordinary error without a separate proven cancellation/draining design.
- **Unwind ordering matters.** A guard declared before prepared buffers could run too late;
  test synchronous publication and asynchronous reaping/teardown with subprocesses, including
  safe owners whose destructors panic, and inspect every ownership-releasing unwind path.
- **Memory pressure and overhead.** Preflight adds bounded per-window preparation; retain
  aligned zero-copy I/O and queue depth. Do not convert this fix into performance tuning.
- **Commit bookkeeping.** Rejecting a start must preserve already-accepted early writes and
  their errors. Scope cleanup to newly prepared work and test older commits concurrently.
- **Security/compatibility.** Safe Rust callers can supply invalid lengths even when network
  frames are bounded. No new privilege, authentication behavior, disk or protocol contract.

## ADRs

Consistent with accepted ADR [0001](../../adr/0001-languages.md) (auditable Rust unsafe boundary)
and [0003](../../adr/0003-kernel-baseline.md) (Linux io_uring baseline). No new architectural
choice or ADR is required: retain existing fail-stop and two-ring ownership policy. Audit ADRs
0007–0011 are proposed, not accepted, and are not prerequisites for this memory-lifetime fix.

## Parallelizable?

No. One Rust unit owns preparation, unsafe publication, accounting and tests. No Go/API work.
