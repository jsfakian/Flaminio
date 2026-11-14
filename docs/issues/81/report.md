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
