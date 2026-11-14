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
