# Issue #83 — report

## Review rounds

### Plan review round 1 — 9b112896122794e21e2c041c0991c533b0a7bf04

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r83c1 (design-code), r83s1 (design-security).

- **Review83-R1-C1 — High:** Fresh-process recovery lacks authorized enforceable new-epoch agent-to-daemon handoff Evidence: design.md98–103,142–144; node.rs309–313/auth.go66–70.
- **Review83-R1-C2 — Medium:** Admitted clock resolution1ms exceeds2us takeover quantization margin Evidence: design.md52,69–71.
- **Review83-R1-N1 — Nit:** Spec/design link missing decisions.md Evidence: spec.md3/design.md3.

Changes/resolution: Both reviewers independently confirmed all three. Added exact signed FLCH local handoff bound to current32-byte process nonce and exact replica-token digest, issued only by authorized new-epoch Attach; preserve node/io restrictions and enforce every target-start entry. Compose #79 init credentials with same-daemon-only recovery and refuse new-daemon old epochs. Qualified error/resolution<=1us, elapsed errorE3us and conservative waitceil((L+E)*(1+rho))+E; independent bound tests/caught mutants. Added unapproved choices. No implementation/runtime/lab tests or approvals inferred.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 2 — 5a2fcd86c723b165c15ae0a1889929b81feaf204

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r83c2 (incremental design-code), r83s2 (incremental design-security).

- **Review83-R2-N1 — Nit:** Mutation test sentence split across Markdown table rows breaks mapping Evidence: design.md161–162.

Changes/resolution: Both fresh reviewers confirmed all three R1 findings resolved and reported only this Nit. Kept test/mutation description within one table row. FLCH134-byte binding and E3us/default5500007 bound independently checked; no implementation/runtime/lab test or approval inferred.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 3 — f9a0a519adf21d4ee2c9048f44139489446c94e7

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: r83c3 (incremental design-code), r83s3 (incremental design-security).

No comments from the requested review scopes, including Nits.

Changes/resolution: Both fresh reviewers confirmed the table-row Nit resolved and found no revision fallout, including Nits. Earlier R1 technical findings were confirmed resolved in R2. No implementation/runtime/lab tests or approval inferred.

Status: clear. Approval remains separate; no approval is inferred.
