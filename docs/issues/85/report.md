# Issue #85 — report

## Review rounds


### Code loop, round 1 (reviewed `22a66d6`)

Code-reviewer and security-reviewer on the whole change.

Findings:
- Medium: oversized `start_commit` sync fallback advanced `next_gen` before layout/I/O
- Medium/Low: design-required publication-failure abort path untested
- Low: later-window preparation failure could hide an earlier window's device/short error
- Nit: commit SQEs built inside the armed publish loop

Addressed in the following fix commit (see decisions Review85-R2-F1…F4).


### Plan review round 1 — 6e877dfe213ce756d84984e11f5c35f99e64e456

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: review85_code_r1 (design-code), review85_sec_r1 (design-security).

- **Review85-R1-C1 — High:** Asynchronous teardown can unwind from a completed Bytes owner's destructor while other writes remain outstanding Evidence: design.md asynchronous ownership section; uring.rs reap_commits and Drop.
- **Review85-R1-C2 — Medium:** A known flush generation alone does not establish a submitted flush or valid completion result Evidence: design.md completion checks; uring.rs reap_commits Flush state handling.

Changes/resolution: Extended fail-stop unwind protection through asynchronous reaping/teardown; require exactly Submitted group owner and fsync result0; added safe retained-backing subprocess and invalid-state/duplicate/result regression plans. Security independently confirmed both findings, with no additional comments.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 2 — 19db1fe7168c3eff8579752df879f8f92f3dfb70

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: review85_code_r2 (incremental design-code), review85_sec_r2 (incremental design-security).

No comments from the requested review scopes, including Nits.

Changes/resolution: Both fresh reviewers confirmed Review85-R1-C1/C2 resolved, checked changed teardown/reaper/coalescing paths and planned regressions, and reported no new comments including Nits. Only plan changes were reviewed; runtime implementation/testing/approval remain separate.

Status: clear. Approval remains separate; no approval is inferred.

### Code loop, round 2 (reviewed `f747f4e`, diff `22a66d6..HEAD`)

Code-reviewer and security-reviewer: **CLEAR** (nothing Low or worse).
Prior round-1 findings confirmed fixed; no ownership/fail-stop weakening.

## Tests

| Rule | Result | Evidence |
|---|---|---|
| S1 | pass | `len32_boundary`, `window_preflight_rejects_before_allocation` |
| S2 | pass | prep reject unpublished; `rejection_at_window_boundaries` |
| S3 | pass | `errors_are_reported_after_draining`, `retryable_submit_errno_drains`, `drop_drains_owned_writes` |
| S4 | pass | `submission_lifetime_failures` (fatal submit, panic-armed, bad/dup CQE, dirty SQ, sync/commit push-prefix) |
| S5 | pass | boundary rejection + later I/O; flush CQE owner checks |
| S6 | pass | early-write/commit prepare/reject/drop tests; oversized sync-fallback `next_gen` regression |
| S7 | pass | aligned/bounced roundtrips; `prior_or` preserves earlier device errors |
| S8 | pass | retained-buffer regression; 32-bit omit stub; abort harness without freed-buffer UAF |

`make` targets:
- `make lint test race hygiene contracts` — pass (2026-11-20)
- `make miri` — pass (aligned-buffer helpers only; does **not** validate kernel io_uring)
- `make sanitize QUICK=1` — pass
- `cargo test -p flaminio-uring` — 26+ passed

## Conformance

PASS after report/followups update. Spec rules S1–S8 implemented and tested; public API unchanged; ADR 0001/0003 consistent; no open review findings.

| Rule | Implemented | Tested | Evidence |
|---|---|---|---|
| S1 | yes | yes | length-before-bounce; boundary + prep inject |
| S2 | yes | yes | unpublished reject windows |
| S3 | yes | yes | OwnershipGuard drain-before-return |
| S4 | yes | yes | ownership_fail / abort harness |
| S5 | yes | yes | CQE bounds/dup; flush Submitted owner |
| S6 | yes | yes | prep-before-next_gen; Drop abort-on-unwind |
| S7 | yes | yes | prior_or; roundtrips |
| S8 | yes | yes | safety comments; retained-buffer + platform stubs |

## Lab

None required by the design (disposable local files only). All lab checks: n/a / pending none.

## ADRs

Relies on accepted ADR 0001 and 0003. No new ADR.

