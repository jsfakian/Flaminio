# Issue #61: Delegate the original identity slice to #90 — design

[spec](spec.md) · [intent](intent.md) · [decisions](decisions.md).

Unapproved reference/composition revision; no implementation.

## Facts and ownership

The source plan at `5696573a126c0abe26347d3c9700e557489613cf` already isolated store identity
from later cloud work and recorded the approved dedicated-issue move. #90 now owns that exact
root-cause scope and its proposed technical revisions. Its canonical issue-plan branch is
`issue/90-store-incarnation-authority`; no store-identity authority is retained in this folder.
Original R1 findings are preserved in this report and the audit review artifacts. They remain
unresolved until #90's actual full fresh review/address loop clears; then this composition diff
is reviewed separately. No fake #90 round or inherited new-issue approval is recorded.

## Composition and acceptance (S1–S4)

Use #90's single ref/generation, bootstrap/replacement, cleanup, wire/size, capacity, migration
and model contracts. This wrapper supplies only the acceptance mapping in spec.md and tracker
links. Whichever implementation lands second adds the explicit #90 integration hooks to #13,
#45's later recovery, snapshots/readers/DR, capacity, metadata and token consumers. Their current
review-clear technical plans are not reopened or edited by this move.

The original no-manual-step cloud Done-when remains a later #61 result. No cloud classification,
claiming, automatic format, placement default or surviving-copy policy is selected here. #90
likewise does not supply those later stages. Unsupported same-UUID rollback, no surviving earned
source and minimum-ISR restrictions remain exactly the owning plan's stated limitations.

## Files, contracts and tests

Only `docs/issues/61/{intent,spec,design,decisions,adr-proposal,report}.md` change here. Runtime
API, wire, disk-format and control-socket additions: none in #61; #90 owns its proposed changes.
The local ADR file is a historical pointer, not another architectural decision.

Check whitespace, local references and the S2 acceptance mapping against the exact clear #90
report. All runtime/model/adapter/authorized-environment checks belong to #90's implementation;
none is reported run by this document move. This composition is reviewed after #90 clears.

## Lab, risks and ADRs

No lab or cloud action. No accepted ADR is modified. Preserve the original Q1/Q2 approvals as
historical provenance in decisions.md; #90's changed contracts are proposed/unapproved. The
main risk is confusing a moved/clear draft with implementation or cloud protection; the spec
explicitly refuses that inference and keeps #61 open for its existing later work.

## Parallelizable?

This wrapper needs only the final #90 contract/report for composition review. It creates no
independent implementation workstream.
