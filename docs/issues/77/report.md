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

Implementation checkpoint: `make fmt` passes. Rust/fuzz clippy and formatting, Go vet
and Go formatting pass; unprivileged lint reaches buf but its module-cache lock is read-only.
Scoped Go tests compile; localhost restrictions block etcd fixtures. Executed memory cases
pass except the expected `TestConcurrentTargets` accounting change: known committed but
superseded Attach responses can advance the epoch while returning Aborted. The independent
test stage must count actual backend-applied modification revisions and preserve final epoch
and ISR assertions, as the approved design requires.

Independent S1–S7 acceptance is pending. Required project checks will run with the authorized
local cache/socket access: `make lint test race hygiene contracts sim`.

## Conformance

Pending implementation, tests and clear code/security review.

## Lab

None required by the approved design. No shared-lab actions are authorized for this run.

## ADRs

Existing Go/etcd and fencing decisions remain unchanged. No new architectural decision or
numbered ADR is proposed by this design.
