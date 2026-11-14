# Issue #77: Consistent volume reads across replacement — design

[Specification](spec.md). Approved 2026-11-13; not implemented.

The facts it starts from:
- `readHeader` returns parsed header and modification revision; Creating/Deleting are hidden.
- `readVolume` independently lists a whole sorted chunk prefix (`meta/service/service.go:303`).
- etcd List is one linearizable Get with prefix; Mem.List snapshots under its mutex
  (`meta/kv/{etcd,mem}.go`). Future paginated reads cannot silently weaken that snapshot.
- Attach currently uses Txn rather than TxnRev, reads again and replaces `v.Header` afterwards.
- Header CAS already protects ISR/move transitions and deletion marks. This change does not
  serialize I/O or alter which replica is authoritative.

## Validated reads (S1–S3, S5–S7)

Add private `readVolumeView(ctx,id,expectedRevision)` returning a volume and its validated header
revision. `expectedRevision=0` means any current published view; a positive value pins the view
to one successful attach. `readVolume` calls it without a pin and preserves its existing callers.

One attempt reads header+revision H, performs one atomic chunk-prefix List, then re-reads the
header revision **before publishing or classifying collected chunk errors**. Use readHeader's
existing Creating/Deleting behavior for ordinary reads: missing/unpublished returns NotFound;
revision change discards the candidate and retries. In pinned mode, a missing, Creating,
Deleting or differently revised header at either read returns Aborted/committed-superseded.
Never replay the attach inside this helper. A new revision with equal values still counts as changed.

Only after validation succeeds, decode/check the candidate prefix and require exactly the
header's chunk count with canonical consecutive chunk keys. Preserve deterministic ordering;
a corrupted key must not shift every subsequent chunk's index. A stable malformed record or
missing/duplicate index is Internal, with no successful partial response. Header parse errors
remain explicit errors; do not turn them into a nonexistent replacement.

Why this is a snapshot: H precedes the atomic prefix read; final H proves no header write
occurred across it, because revisions never go backwards. Thus that prefix and header coexisted
at the List read point, even if individual chunks changed before/after it. This depends on the
published KV snapshot contract, not wall-clock timing. Do not substitute one Get per chunk.

Use the service's existing bounded retry/backoff discipline and caller context. Return Aborted
when contention persists; List/RPC failure keeps its normal error classification. Do not restart
creation, clear flags, issue tokens or mutate epochs as a side effect of an ordinary read.

## Pin Attach to its committed header (S4, S6, S7)

Change Attach's successful conditional header write to `kv.TxnRev`. Capture revision A from the
same successful commit; a separate Get cannot establish it. Call the pinned view helper with A.
Require that the first and final header revisions equal A, with header epoch/incarnation/target
matching the local committed header, before signing the token and building the response.
Remove the unconditional `v.Header=h` overwrite. Token claims come from the validated pinned
view; no token is issued with replacement chunks. A later Attach can still supersede a returned
response immediately, which ordinary replica fencing handles; the snapshot is not a lease.

An ambiguous/lost TxnRev response keeps existing conditional retry behavior. After a known
commit, Attach wraps every subsequent error with `attach committed volume=ID epoch=E
revision=A; response unavailable: REASON`. Missing/unpublished/superseded pin is Aborted;
backend, stable corruption, Canceled and DeadlineExceeded preserve their mapped code and add
that context. Map context errors before wrapping. Never replace them all with Aborted or claim
cancellation revoked an already committed epoch. No token is returned on failure. The caller
may perform a fresh attach, but this helper cannot pretend the first operation rolled back. Keep OnlyIfTarget
and UnlessTarget checks; the latter uses the same coherent ordinary-read helper when it takes
the no-write path. Do not introduce durable attach-operation IDs under this issue.

## Coherent named ISR responses (S5–S7)

Add internal `KV.ReadSet(ctx, keys)` returning exactly one `{Entry, Found}` per requested key,
in request order, at one linearizable read point. Maximum65keys, bounded response under the
existing metadata receive cap; cap the raw ChangeIsr list at64 before construction, as today.
ReadSet accepts repeated keys and returns each occurrence in order from the same snapshot;
do not reject or deduplicate a request merely because it repeats a valid chunk index. Mem clones those values under one mutex; etcd performs one read-only
Txn with one OpGet per key (not sequential Get calls or writable TxnRev). Backend errors return
no partial snapshot. This changes only internal storage API, no external RPC or steady I/O path.

ChangeIsr reads the header and all requested chunk keys through that one ReadSet. Decode the
published header first, validate ranges/missing/invalid chunks against the same snapshot, then
perform caller/current-epoch/version/policy checks. Existing duplicate-index policy remains
in CheckIsrChange after snapshot assembly: return Applied=false with repeated named chunks in
request order and no writes. Both wrong-target and invalid-change Applied=false replies use
this coherent header/named set. Do not call a new ordinary header
Get and replace only the epoch. Missing publication => NotFound; stable malformed/missing
named state => Internal; read/backend/context failures keep existing mapped status.

Applicable changes keep current header+chunk revision comparisons and atomic writes. On a
successful CAS, the modified named chunks and checked header coexisted at the commit point,
so Applied=true needs no independently assembled post-write view. A failed CAS retries with a
fresh ReadSet under the same32-attempt/context budget. Chunk order remains request order;
Rust's reply zip semantics do not change. No caller gains new authorization or source authority.

## Files touched

- `meta/service/service.go`: validated private read helper, Attach pin/error context and coherent
  ChangeIsr snapshot/no-write responses S1–S7.
- `meta/kv/{kv,mem,etcd}.go`: List snapshot contract and bounded atomic ReadSet S5.
- `meta/kv/kv_test.go`: backend contract tests for prefix/ReadSet snapshot, missing/order/bounds S5.
- `meta/service/service_test.go`: deterministic interleavings and pinned response tests S1–S7.
- `scripts/dev/meta-interop.sh` / existing Rust interop tests only as needed to cover refusals S6.
- `docs/audits/2026-11-12`, followups and relevant metadata docs: record closure evidence.

## Contracts

No proto/control-socket/wire/disk changes; keep method authorization in `auth.go`. Internal
read helper returns only a complete validated view. Unstable header: bounded retry or Aborted;
ordinary missing publication: NotFound; stable malformed records: Internal; canceled caller:
context status. Pinned missing/hidden/revised publication: committed-superseded Aborted. Every
known-commit error includes exact committed epoch/revision, preserving backend/context code;
it states a commit occurred, not merely may have occurred. Unknown lost replies remain ambiguous.
ReadSet is an atomic ≤65-key snapshot; ChangeIsr no-write replies use its header and named set. KV List must be one snapshot, even if an eventual
implementation paginates against a fixed revision.

## Tests

- S1/S2: `TestGetVolumeSameCountReplacementIsCoherent` on both backends, seeded by the audit
  reproduction; change RF, size/chunk geometry and first epoch while keeping chunk count equal.
- S2/S7: `TestGetVolumeHeaderChurnIsBounded`, cancellation/deadline cases and retry counter.
- S3: `TestGetVolumeClassifiesChunksOnlyAfterHeaderValidation`, stable bad JSON/missing index,
  noncanonical key and concurrent deletion/replacement; all must return no partial response.
- S4: `TestAttachResponsePinnedToCommitRevision`, with hooks after commit, after initial header
  read, after prefix read, before final validation; newer attach/delete/recreate never mixes claims.
- S4/S7: `TestAttachKnownCommitThenReadFailureKeepsEpoch`: delete/mark Creating/Deleting before
  either pinned read yields Aborted+exact committed context; backend Unavailable, stable Internal,
  Canceled/DeadlineExceeded preserve code+context. Lost response/retry never rolls epoch back.
- S4/S7: update existing `TestConcurrentTargets`: count actual committed Attach header writes at
  the backend apply boundary (deduplicate modification revision), not successful API responses.
  Keep exact final epoch assertion and per-chunk ISR-version assertions. Force A commit, B commit,
  A postcommit Aborted; assert two increments and one successful response. Separate ambiguous-reply
  tests use actual backend application evidence, not the outer TxnRev retry result.
- S5: `TestChangeIsrRejectedViewIsCoherent` on both backends: replacement after first old read and
  atomic multi-chunk update between attempted named reads; false replies are one actual snapshot.
  Assert true CAS replies at their commit point, retries/cancellation, request order and auth.
- S5/S6: duplicate valid index0 twice returns Applied=false, the coherent epoch and two repeated
  response entries, with unchanged records, on both backends; retain the Rust duplicate-refusal
  vector. Raw65changes still fails the existing count cap before ReadSet.
- S5: backend ReadSet repeated keys/missing/order/65-key cap, interleaved multi-key update atomicity and no
  partial response; existing prefix List atomicity, concurrent ISR/move and full chunk order.
- S6: legacy first_epoch=0, existing authorization tests, metadata vectors and Rust client interop.
- Required local targets: `make lint test race contracts`; `make sim` confirms unchanged model
  behavior. No new replication safety rule is introduced; metadata view consistency is tested
  in the real storage/service boundary rather than invented as an I/O permission rule.

## Lab

None required to prove the deterministic race. Optional later CSI provisioning/failover smoke
requires explicit shared-lab authorization; do not run it during planning or silently reformat.

## Docs

Record #77 reproduction-to-regression evidence in the audit/followups. Clarify prefix snapshot
and stale-attach response behavior. No new command, benchmark claim or Makefile target.

## Risks

An unbounded/changing header loop could starve callers; bounded retries and context checks are
mandatory. Current whole-prefix memory/RPC limits remain #80's concern. On old worktrees the
historical ADR0005 immediate-read wording conflicts with the implemented protocol; this change
does not relax fencing/readiness or make it authoritative. Error classification is observable,
so pin expected statuses in CSI and Rust interop tests. A client must never interpret Aborted
as evidence that an earlier committed attach was undone.

## ADRs

Consistent with ADR0001/0002 (Go service, dedicated etcd) and #30 monotonic epochs. No new
architectural choice or global ADR is needed: revision validation uses the existing KV boundary.
Proposed ADR0008 in audit commit10265b6 governs disaster recovery, not normal coherent reads.

## Parallelizable?

No: read helpers, atomic named snapshots, Attach pinning and tests share one response contract.
Rust interoperability tests may be added after the exact observable status behavior is fixed.
