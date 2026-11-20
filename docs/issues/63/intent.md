# Issue #63: Shared guard slice — composition intent

[Issue63](https://github.com/jsfakian/Flaminio/issues/63) · [owner91](https://github.com/jsfakian/Flaminio/issues/91) · [spec](spec.md).

This remains the original storage-disruption-guard first half of step1. The user approved
moving that shared root-cause slice to its own issue, used by #48's later step3 and #63.
That authorized move is now #91. All guard technical authority lives once in its
`docs/issues/91/` intent/spec/design/appendices, branch `issue/91-storage-disruption-guard`.
This folder is only the original-slice composition/acceptance wrapper; no new platform stage.

Original source: `1a4a14df1a8b1ae6c2762bbaabf4ba5489305d31`. Historical2026-11-13 approvals and the explicit
neither-(a)-nor-(b) choice in that plan remain unchanged history. The original guard promise
is not narrowed by the move. #91 may supply a concrete Proposed owning ADR/mechanism; that
is not human acceptance, implementation permission or a retroactive choice in #63.

The original outcomes remain automatic coverage, holding an unsafe voluntary drain, preventing
jointly unsafe concurrent drains, unknown/controller/metadata failure holding, visible reasons,
read-only metadata/availability-only behavior, real RF2 drain-and-data evidence and accurate
docs. [Spec](spec.md) maps each to the owner; missing evidence remains missing.

The rest of step1 (replica draining before removal), actual autoscaler removal, scale-up, disk
claiming/rebalancing, CSI capacity, maintenance/ordered upgrades and involuntary failure work
remain this issue's already-described later work. #63 stays open. No cloud classification,
claim stage, autoscaler policy or replica mutation is introduced by this wrapper.

Filing or reviewing #91 does not deliver the runtime guard or the issue's cloud done-when.
The transferred R1 findings remain unresolved until #91's full fresh review/address loop
actually clears; then this composition diff gets its own fresh review.
