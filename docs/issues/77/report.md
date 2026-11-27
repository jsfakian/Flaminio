# Issue #77 — report

## Review rounds

### Plan review round 1 — bf18c22564987fa88bc3bdcf999f2e8d6bb1eb75

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: review77_code_r1 (design-code), review77_sec_r1 (design-security).

- **Review77-R1-C1 — High:** ChangeIsr rejected responses still mixed independently read header/named chunks Evidence: design.md service scope; service.go ChangeIsr false replies.
- **Review77-R1-C2 — Medium:** Pinned Attach missing/unpublished header incorrectly used ordinary NotFound without known-commit context Evidence: design.md validated helper and pinned status contract.
- **Review77-R1-C3 — Medium:** Existing TestConcurrentTargets equated successful API replies with committed Attach epoch increments Evidence: service_test.go TestConcurrentTargets Aborted branch and final epoch assertion.

Changes/resolution: Specified bounded atomic KV.ReadSet and coherent false ISR replies preserving conditional writes; defined every pinned/postcommit error code/context; revised concurrent test accounting and specific regression schedules. Security independently confirmed C1/C2, with no additional comments.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 2 — e2a14a366d43a6c367dae9a8d34c92a63128d40c

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: review77_code_r2 (incremental design-code), review77_sec_r2 (incremental design-security).

- **Review77-R2-C1 — Medium:** New pre-read uniqueness wording would change existing duplicate-index Applied=false/request-order behavior Evidence: design.md ReadSet prevalidation; service.go count validation and domain.CheckIsrChange duplicate refusal.

Changes/resolution: All round-one findings were confirmed resolved. Preserved the raw64 count limit, allowed repeated ReadSet keys and kept duplicate-policy refusal after coherent snapshot assembly; added exact duplicate response/order/no-write regressions and existing Rust vector coverage. Security independently reported the same issue as Low, with no additional comments.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 3 — 4ab5ed0e6f152bd1a740ed6f1ab6292f5b9ad466

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: review77_code_r3 (incremental design-code), review77_sec_r3 (incremental design-security).

No comments from the requested review scopes, including Nits.

Changes/resolution: Both fresh reviewers confirmed duplicate-input compatibility resolved, checked the changed ReadSet/policy/test contract against Go and Rust duplicate refusal, and reported no new comments including Nits. Previous round-one findings had already been confirmed resolved. This clears the draft plan, not implementation or human approval.

Status: clear. Approval remains separate; no approval is inferred.

### Code/security review round 1 — 4a5dcb05c82b82b1d7b2b58a2943ec8c9f18b3f2

Full range: `880066e..4a5dcb0`, all 13 source/test/documentation files. Fresh code and
security reviewers used direct high-effort review because built-in review skills are not
available. The security review is clear with no nits; code review found one Medium and no
production correctness findings or additional comments.

**R1-C1 — Medium:** `TestConcurrentTargets` observes successful returns from outer
backend.TxnRev, above its internal retry boundary. A CAS can apply epoch E+1, lose its reply,
retry to a failed compare, then a later Attach attempt applies E+2. Two writes applied but
only one revision is counted. The separate ambiguous-response test uses another recorder,
so it does not prove this observer. An isolated read-only reproducer confirms applied=2,
counted=1 and final epoch delta=2. Required fix: observe actual applied header revisions below
retries, deduplicate them and force this history through the same concurrency-test observer.
A fresh implementer replaces the observer with one shared header-history harness across
concurrent, superseded and ambiguous tests. It records direct Mem applications below retries
and replays native etcd header PUT revisions through the final revision under a bounded
context. Only service_test.go changes; exact epoch and ISR assertions remain. Restoring the
old observer in an isolated overlay fails on both backends with count=1 versus epoch delta=2.
The corrected observer, focused race checks and full service suite pass. Renewed
`make fmt lint test race hygiene contracts` also passes, including both mutual-TLS interop
cases. The prior `make sim` pass (20,000 normal seeds and 163 release tests) remains valid:
no Rust/model source changed. Fresh incremental review follows; no exception is accepted.

## Implementation — 2026-11-27

Branch: `issue/77-coherent-volume-reads`, based on
`880066e928369f26ff901b851e01b4312620a15a` after the user's merge of #86 / PR #103.
The published clear spec/design were approved on 2026-11-20. Dependency inspection for #80
confirmed that its authoritative compatibility RPC requires #77's coherent read, which is
not yet implemented on main. #77 therefore ships first as its own PR; #80 groundwork is
preserved separately and will resume only after the user's merge of this prerequisite.

One implementer completed the shared KV/service response contract in four production files.
`readVolumeView` validates header revisions around the atomic prefix, Attach pins responses
and token claims to TxnRev, and ChangeIsr assembles coherent header/named snapshots through
ReadSet. No proto, Rust, geometry or #80 runtime work is included. Independent acceptance
tests, code/security review and final conformance follow. No lab or new ADR is required.

## Tests

`make fmt lint` passes with installed cache access. Independent acceptance passes on both
memory and embedded etcd: `go test ./meta/kv ./meta/service -count=1 -v` (KV 6.032 s,
service 47.564 s). No production bug was found during acceptance.

| Rules | Passing acceptance evidence |
|---|---|
| S1–S2 | `TestGetVolumeSameCountReplacementIsCoherent`, `TestGetVolumeHeaderChurnIsBounded`: equal-count RF/geometry/incarnation replacements, revision-only churn, bounded retries and caller context |
| S3 | `TestGetVolumeClassifiesChunksOnlyAfterHeaderValidation`: changing-header JSON/missing/noncanonical chunks retry, stable corrupt chunks return Internal without partial success |
| S4, S7 | `TestAttachResponsePinnedToCommitRevision`, `TestAttachKnownCommitThenReadFailureKeepsEpoch`: phase hooks, token authority, missing/hidden/superseded pins, exact committed context and backend/cancellation statuses |
| S4, S7 | `TestAttachLostCommitReplyDoesNotRollBack`, `TestAttachSupersededCommitAccounting`, `TestAttachAmbiguousBackendRetryCountsEveryAppliedEpoch`: lost/ambiguous responses, two commits with one success, exact backend revision counting |
| S5 | `TestReadSet`: order/missing/repeated positions/cloning/context/65-key cap and concurrent atomic snapshots on both backends |
| S5 | `TestChangeIsrRejectedViewIsCoherent`, `TestChangeIsrDuplicatePositionsAndRawCap`, `TestChangeIsrCASReadRetriesAndCommitPoint`, `TestChangeIsrSnapshotErrors`: replacement/multi-key snapshots, duplicate refusal, raw 65 cap, CAS retries/commit point, backend/context errors |
| S6 | Existing lifecycle, legacy-header and authorization cases pass in the full scoped packages; vectors and Rust interoperability remain required final checks |

`TestConcurrentTargets` now deduplicates actual backend-applied Attach header modification
revisions and retains its exact final epoch and per-chunk ISR-version assertions. Both backend
variants pass. Baseline verification confirms raw header JSON errors keep the existing
Unavailable mapping; stable malformed chunk records are Internal.

Controlled-omission proof: an isolated temporary copy disables only the final header-revision
comparison. Both-backend replacement tests then return old RF 2 / 2 MiB authority with RF 3
replacement chunks, and changing-header corruption tests prematurely return Internal. Stable
corruption controls still pass. The unmodified counterparts all pass; production bytes on
the branch were untouched by the experiment.

Full required gates pass at `4a5dcb0`: `make fmt lint` and
`make test race hygiene contracts sim`. This includes both mutual-TLS interoperability cases,
20,000 normal simulation seeds and all 163 release tests with ignored mutation controls.
After the test-observer correction, `make fmt lint test race hygiene contracts` passes again.
Rust/model source is unchanged, so the same simulation result applies. Incremental review
of the test fix remains pending. No shared-lab or unsafe-code change is involved.

## Conformance

Pending implementation, tests and clear code/security review.

## Lab

None required by the approved design. No shared-lab actions are authorized for this run.

## ADRs

Existing Go/etcd and fencing decisions remain unchanged. No new architectural decision or
numbered ADR is proposed by this design.
