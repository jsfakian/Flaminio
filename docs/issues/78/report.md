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

### Code/security round 1 — fd279dbcd6e8af9319f367bb1e87bf46e16f1fbe

Full change reviewed against main `7d03b6b08bee86058b3dfbc6942ca7b053aab547`, excluding
generated bindings. Split into Go/service/controller/docs and Rust model/shared schedules
because the non-generated change exceeds 3,000 lines. Each part has independent code and
security reviewers. Built-in review Skill tools are unavailable; reviewers use the repository
standards directly at high effort.

- **R1-G-C1 — Medium:** exact-spelling duplicate JSON detection misses Go's case-insensitive
  struct field aliases. Conflicting `moving.to`/`moving.To` or `replicas`/`Replicas` can conceal
  incoming or placed work and return `complete:true`. Independently reproduced by both Go
  reviewers through production CheckDrain. Requires malformed persisted inventory; no public
  RPC injection path was demonstrated. Fix semantic aliases in struct namespaces while
  retaining valid case-sensitive user map keys; add both-store regression cases.
- **R1-M-C1 — Medium:** Rust completion's supposedly independent occupancy oracle calls the
  same inventory scanners under test; serde ignores the shared fixture's explicit expected
  counters. Scanner omissions can escape its tests/exploration. Parse and assert independent
  fixture counts, derive occupancy separately, and cover a positive admitted incoming move.

Model security independently confirms R1-M-C1: removing the copy header comparison can publish
an extra eight chunks after deletion fencing while the ignored count expectations leave the
Rust tests green. This adds explicit stale-publication control to the same fix.

- **R1-CI-C1 — Medium:** required hygiene fails staticcheck: `admission.go:113` compares an
  int64 with a greater-than-MaxInt64 condition (SA4003), and `service.go:197` assigns an unused
  gate revision (SA4006). Remove the dead expressions while preserving validation and gate init.

All four reviewers are complete with no other findings or Nits. A fresh implementer addressed
all three findings in the single R1 fix commit:

- Known struct field aliases (including Unicode case folding) are DataLoss; unknown/user map
  keys retain case semantics. Both-store regressions failed before the fix with `complete:true`
  for all five alias classes, then passed. The 22 committed fuzz seeds and a five-second fuzz
  run (265,047 executions) pass.
- Rust validates fixture count/completion expectations and uses independent successful
  admission/publication history. Positive incoming-move replay now succeeds on both backends.
  A new explicit FencedPublication control catches delayed copy after deletion fencing.
  The original three mutation witnesses and 827 interleavings remain covered; eight focused
  model tests pass. The new count-expectation test failed before the fix.
- Controlled omissions after the fix all fail as intended: placement expected 32/got 0;
  incoming accounting triggers PrematureOrStaleCompletion from independent history; missing
  copy-header comparison triggers FencedPublication. Every omission was restored before checks.
- Removed the statically dead comparison/assignment. The first post-fix full run stopped on
  Clippy's collapsible-if lint; that statement was corrected, scoped Clippy/model tests passed,
  and the complete required suite restarted.

Post-fix `make lint test race hygiene contracts sim` passed in full. This includes all three
TLS interop tests, 20,000 simulation seeds (zero failures), and all 173 release replication
checks including ignored mutation controls. R1 fixes are committed as `c63e49b1d66e8f32ca00eb812ccb9800ae1656a0`.
Status: resolved by the clear incremental review below.

### Code/security round 2 — c63e49b1d66e8f32ca00eb812ccb9800ae1656a0

Fresh code and security reviewers examined only `fd279db..c63e49b` and necessary callers,
covering all seven changed paths. Both confirm R1-G-C1, R1-M-C1 and R1-CI-C1 fixed, with
no new Low-or-higher findings or Nits. No additional source changes were requested.

Later-round Sonnet is unavailable in this session; code review used the inherited default
model at medium effort. Security retained the default model at medium effort. Both reviews
were direct, as required for incremental rounds. Status: **clear** after two code/security
rounds. Final conformance passed all S1–S8 rules.

## Implementation integration — 2026-11-27

Existing commits `62f7424` (implementation) and `19a45ac` (tests), authored before the recent
merges, are preserved on `issue/78-placement-admission-generation`. Merged current main at
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

Independent acceptance passed S1–S8 at `fd279db`. Required project checks are
`make lint test race hygiene contracts sim`, including real service CAS schedule replay and
all three original placement-admission mutation controls plus the publication-fence control.
Final post-fix `make lint test race hygiene contracts sim` passed in full; the three TLS
interoperability tests, 20,000 seeds and 173 release replication checks passed.
The baseline run at `fd279db` passed
`make lint test race`, then stopped in `make hygiene` at R1-CI-C1; contracts and sim were not
reached in that baseline run. The complete post-fix run passed all required checks.

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
retaining fresh-planning, generation and replica assertions; no runtime change was needed. The
corrected focused case and complete service/drain packages then passed (78.310s / 0.121s).

## Conformance

Final checker output at `c63e49b`, before ADR acceptance (verbatim):

PASS

| Rule | Implemented | Tested | Evidence |
|---|---|---|---|
| S1 Atomic admission | yes | yes | `meta/service/create.go:546` compares root/name/tombstone/gate; `MoveReplica` compares gate/header/chunk. Both-store `TestAdmissionDrainBeforeClaim`, `TestAdmissionDrainAfterClaim`, `TestAdmissionCreateDoesNotPlaceOnConcurrentDrain` and `TestAdmissionWitnessChangeForcesFreshPlan`. |
| S2 Earlier reservations | yes | yes | Immutable header generation and original claim/root publication fences preserve admitted plans. Both-store `TestAdmissionLostRepliesRecoverOriginalGeneration`, `TestAdmissionConcurrentReservationsRemainAccounted` and phased `TestAdmissionScheduleReplay` cover restart, identical helpers, publication and deletion fencing. |
| S3 Complete accounting | yes | yes | `meta/service/admission.go:137` scans raw headers before chunks, counts earlier creations/replicas/incoming moves, and rejects corrupt inventory. Both-store `TestCheckDrainInventoryAndValidation`, `TestCheckDrainRawInventorySchema`, `TestCheckDrainRejectsAmbiguousNestedMove` and `TestCheckDrainRejectsStructFieldAliases`. |
| S4 Validated completion | yes | yes | `CheckDrain` compares the original marker after both scans; `control/drain/drain.go:181` retains its generation and requires validated completion. Phased both-store replay and controller tests cover empty visible inventory, unmark/re-mark, cancellation and repeated marks. |
| S5 Direct move destinations | yes | yes | `MoveReplica` checks draining eligibility before `CheckMove`, then commits under the gate/header/chunk CAS. Both-store `TestAdmissionMoveRefusesDrainingDestination`, `TestAdmissionMoveNoOpStillRejectsDrainingDestination`, positive incoming replay and `TestCheckDrainAfterChangeIsrCompletesMove`. |
| S6 Bounded and restartable | yes | yes | Admission remains 4 comparisons/2 writes; publication retains bounded root/claim batches; completion uses 1 comparison/0 writes. `TestAdmissionScheduleReplay` asserts exact fences/bounds; lost-response, concurrent reservation and storage-error tests cover restart and fail-closed behavior on both stores. |
| S7 Compatibility and authority | yes | yes | Administrator-only `CheckDrain` authorization; positive gate initialization precedes admissions/no-op mutations. `TestAuthorization`, legacy initialization/lost-reply tests and unchanged geometry/identity tests pass; `make contracts` passes all three Rust/Go TLS interoperability tests. |
| S8 Executable checks and documentation | yes | yes | Eight placement-admission model tests pass, including 827 interleavings, independent occupancy/count expectations, three required mutants and publication-fence mutation. Shared schedules replay actual phased Go CAS on both stores. Required `make lint test race hygiene contracts sim` passes; 20,000 simulation seeds and 173 release replication tests pass. |

Contract drift: none. Generated protobuf bindings match the additive generation/CheckDrain contract. The approved integration decisions account for #76’s root comparison/write and defer unlanded #18 move-domain policy.

ADR conformance: consistent with ADRs 0001/0002/0005/0006/0015/0016/0018. ADR 0019 exists and is indexed as Proposed, correctly awaiting this final gate.

Scope drift: no functional drift. Additional paths beyond the original file list are `meta/service/admission.go` (extracted admission/inventory implementation), `meta/service/create.go` (#76 integration), `meta/service/service_test.go` (interception helper), numbered ADR/index and issue pipeline documentation. All listed implementation areas are covered.

Docs: done. Architecture, protocol, lab guidance and follow-ups document linearization, conservative reservations, generation meaning, maintenance-barrier rollout and metadata-completion limits. No lab checks are required.

Open review findings: none. R1-G-C1, R1-M-C1 and R1-CI-C1 are fixed at `c63e49b`; fresh incremental code/security reviews are clear.

## Lab

None required by the approved design. No shared VM targets were run.

## ADRs

The global gate/conservative inventory/maintenance-barrier policy was accepted with the plan.
[ADR 0019](../../adr/0019-fence-placement-admission-and-drain-completion.md) is accepted
and indexed after the clear review loop and final conformance PASS. ADRs 0001/0002/0016/0018
remain consistent; replication/lease and node-shutdown authority decisions are unchanged.
