# Issue #81 — report

## Review rounds

### Plan review round 1 — f7be923d3255fe7cf55a41631969cedd54721fbe

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r81c1 (design-code), r81s1 (design-security).

- **Review81-R1-C1 — Medium:** Malformed-count handling can suppress a higher authoritative epoch, conflicting with #80 Evidence: design.md55–62; #80design.md79–86.
- **Review81-R1-N1 — Nit:** Spec/design link nonexistent decisions.md Evidence: spec.md3/design.md3.

Changes/resolution: Both reviewers independently reported the same two findings and no others. Reordered generation/request association then higher-epoch deposing using safe current geometry before payload validation on ISR and refresh paths; retain same/lower malformed-reply refusal without cache mutation and add explicit regressions. Added an explicitly unapproved choices record. No implementation/runtime/lab tests or approvals inferred.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 2 — 0a8bfca37184f3dd4a45355c6020a58827017dae

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r81c2 (incremental design-code), r81s2 (incremental design-security).

No comments from the requested review scopes, including Nits.

Changes/resolution: Both fresh reviewers confirmed higher-epoch ordering and missing choices record resolved, checked affected core/engine paths and #80 contract, and reported zero comments including Nits. No implementation/runtime/lab test or approval inferred.

Status: clear. Approval remains separate; no approval is inferred.

## Implementation — 2026-11-27

Branch: `issue/81-bounded-isr-updates`, based on
`1d9b26b5b2ffbd5b3c956093aef45531e06683a5` (`origin/main`). The published spec and design
were approved on 2026-11-20, as recorded in [decisions.md](decisions.md).

The core now selects at most 64 distinct pending chunks per request with a cyclic cursor;
deferred membership and acknowledgement requirements are retained. Engine replies are guarded
by attach generation, live request, complete payload and monotonic version checks, with a
higher authenticated epoch deposing before payload conversion. Production models enforce the
same service bound. Wire/API/disk formats and replication policy are unchanged.

Targeted acceptance tests pass. Independent code/security review, final required checks and
conformance remain pending. No issue closure is claimed by this entry.

## Tests

- Initial `make fmt lint`: pass (the Go tooling needed access to its existing module cache).
- Go service boundary/authorization regressions on memory and embedded etcd: pass.
  `go test ./meta/service -count=1`: pass.
- Replication library: 91 pass, 24 intentionally ignored release-only campaigns, no failures.
  New target and simulator regressions cover boundaries, deferred membership, retries,
  fairness and all four relevant mutations, including proposals dropped before delivery.
- Node package: 43 library and 19 integration tests pass. After adding one further unit case,
  all 11 metadata unit regressions pass. Large integration cases verify 129-chunk
  shrink/rejoin with a deferred tail write and mixed moves with data verification.
- `make meta-interop`: both tests pass, including real target recovery across 129 chunks.
- Pending final checks: `make lint test race hygiene contracts sim`.

Pre-review correction: `max_meta_batch` records proposals at emission, before network loss,
with a regression proving dropped oversized proposals remain visible. The receiver's atomic
oversized refusal is unchanged.

## Conformance

Pending final implementation, review clearance and acceptance results.

## Lab

None required by the approved design. No shared-lab commands are authorized for this run.

## ADRs

Preserves accepted ADRs 0001, 0002, 0005 and 0006. No new ADR or wire/disk format change;
proposed ADRs 0007–0011 remain unaccepted.
