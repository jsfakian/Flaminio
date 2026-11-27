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

## Implementation integration — 2026-11-27

Existing commits `62f7424` (implementation) and `19a45ac` (tests), authored before the recent
merges, are preserved on `issue/78-placement-admission-generation`. Merge current main at
`7d03b6b08bee86058b3dfbc6942ca7b053aab547` after the user's merge of #80 / PR #105.
The approved clear snapshot dates to 2026-11-20. One implementer owns semantic integration
of the existing gate/controller/model work with #76 recovery, #77 coherent responses and
#80 admission/compatibility, including source conflict resolution and API regeneration.

Semantic integration preserves the #76 root claim (four comparisons / two writes), #77 coherent
views and pinned Attach replies, and #80 geometry/identity refusal. Regenerated protobuf output
contains both CheckCreateVolume and the new administrator-only CheckDrain. Raw drain inventory
validates schema and set consistency without filtering unsupported legacy geometry. Nested JSON
objects reject duplicate fields so a second move destination cannot conceal incoming work.

The independent acceptance audit found the original schedule replay skipped publication and
inventory phases. The model now separates private planning, claim, copy batches, publication,
deletion fencing and move planning/commit. Acceptance repairs the shared Go replay to exercise
real service KV boundaries on memory and embedded etcd, rather than treating those actions as
no-ops. Fresh code/security review, address rounds and final S1–S8 conformance follow. No lab
runs are required or authorized.

Transaction audit: gate initialization 1 compare/1 operation; changed registration 2/2; mark
3/2; unmark 2/2; MoveReplica 3/1; CheckDrain 1/0. Creation retains #76's bounds: initial root
2/2, staging 1/(up to 33), sealing 1/1, admission 4/2, cleaning/copy 2/(up to 33), finalization
2/2. No comparison list grows with selected-node count.

## Tests

Independent acceptance is completing. Required project checks are
`make lint test race hygiene contracts sim`, including real service CAS schedule replay and
all three placement-admission mutation controls.

Controlled negative evidence: temporarily omitting recursive raw-inventory validation makes
`TestCheckDrainRejectsAmbiguousNestedMove` fail on memory and embedded etcd with
`complete:true, err:nil`; the fixture contains duplicate nested `moving.to` fields. This was
a controlled omission after implementation, rather than a historical pre-fix run. Validation
was restored exactly before clean verification. The five focused admission model tests pass,
including 827 bounded interleavings and paired clean/mutant schedule controls. A five-second Go
fuzz campaign completed 68,699 executions from 14 seeds. The complete flaminio-repl crate passed
140 tests (30 intentionally ignored by the normal debug run).

The first full service/drain run exposed a test-fixture error: the witness-change case used
`witness` instead of the existing `flaminio.io/witness` label. The fixture was corrected while
retaining fresh-planning, generation and replica assertions; no runtime change was needed.

## Conformance

Pending integration, acceptance and review clearance.

## Lab

None required by the approved design. No shared VM targets will run.

## ADRs

The global gate/conservative inventory/maintenance-barrier policy was accepted with the plan.
Prepare the next unused numbered ADR for review, then accept it after review/conformance pass.
Existing linearizable-etcd and replication/fencing decisions remain unchanged.
