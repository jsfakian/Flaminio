# Issue #78 — report

## Review rounds

### Plan review round 1 — 73bfc39e8ffbed0212c057e25190f380f548638b

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: review78_code_r1 (design-code), review78_sec_r1 (design-security).

- **Review78-R1-C1 — Medium:** Absent new placement gate can classify later post-upgrade admissions as earlier than retained legacy drain markers Evidence: design.md gate absence, idempotent mark and rollout contracts.
- **Review78-R1-C2 — High:** New stale drain-generation completion rule lacks explicit phased model assertion and caught missing-final-compare mutation Evidence: design.md model checks/mutations; spec S4/S8.

Changes/resolution: Added bounded gate initialization required before all new admissions/idempotent mutation paths, preserving legacy D while new G>D; specified rollout and lost-response regression. Added phased scan/validation model, current-D assertion, omitted-final-marker-compare mutation and Go replay for unmark/re-mark schedules. Security independently confirmed C2 (rated Medium there), with no additional findings.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 2 — 9f0a4b0812a6781de8a03af43996d97e761b3b0f

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: review78_code_r2 (incremental design-code), review78_sec_r2 (incremental design-security).

No comments from the requested review scopes, including Nits.

Changes/resolution: Both fresh reviewers confirmed legacy gate initialization and explicit stale-generation model/mutation findings resolved, checked affected CAS/rollout/retry paths, and reported no new comments including Nits. Implementation and approval remain separate.

Status: clear. Approval remains separate; no approval is inferred.
