# Issue #63: Shared guard slice — composition specification

[intent](intent.md) · [design](design.md) · [decisions](decisions.md).
Historical source approval is preserved; this reference revision is unapproved.

## Invariant

#91 is the single technical owner of the original guard slice. This wrapper does not narrow
its original promise, duplicate admission/reader/identity rules or claim runtime acceptance.

## Behavior

**S1. Delegation.** Guard coverage/enrollment/admission, Node UID accounting, strict metadata
collection, read-only credentials, Kubernetes permissions/status, resource bounds, failure/
restart behavior, API qualification and storage evidence live in #91. Consumers use that
contract rather than a #63 PDB/affinity/Reader mechanism.

**S2. Original acceptance.** Preserve the source intent's outcomes without substituting an
autoscaler or another platform feature:

| Original intent outcome | Owning evidence in #91 |
|---|---|
|1: automatic coverage for Kubernetes storage nodes|actual Node UID enrollment/coverage and new-node/controller-loss API tests|
|2: unsafe drain held; same ordinary drain proceeds when eligible|strict complete observed metadata and pinned real kubectl/eviction admission tests|
|3: one observation cannot allow a jointly unsafe pair|stable distinct-UID admission state and real concurrent/rollout/recreation tests|
|4: unknown/controller/metadata loss holds; bypasses explicit|concrete chosen Proposed ADR contract and real stale-permission/failure tests|
|5: visible held reasons|bounded owned status/Event/log and least-privilege tests|
|6: availability only, read-only metadata, no replica/epoch/ISR mutation|positive/negative auth plus unchanged-authority/reducer fixtures|
|7: RF2 stop/return/drain/re-admission and data proof|separately authorized pinned real API plus qualified composed storage environment evidence|
|8: accurate scope/docs/manifests/permissions|implemented guard scope, deployment contracts and truthful qualification/limitations|

A clear draft is not these runtime results. Current VMs cannot certify #83's physical profile;
the owner must retain the original evidence obligation with an honest qualified execution or
blocked/uncollected result, not quietly drop the row. Proposed owner amendments require their
own approval and cannot retroactively alter the source record.

**S3. Dependencies.** #63 and #48's later guard work point to #91. Current clear dependency
technical plans remain read-only; whoever lands second applies #91's exact integration hooks.
This wrapper promises no autoscaler removal/scale-up, evacuation, cloud claim or capacity work.

**S4. History and review order.** Preserve original approvals and the user's explicit deferred
choice in #63. All substantive owner and wrapper revisions remain unapproved. Original R1
findings follow the authority and are not resolved by deleting text. #63 waits for the owner's
real full fresh zero-comment loop, then receives its own fresh composition/reference review.

## Checks

S1–S4 require reference/acceptance/provenance consistency and fresh review against #91's exact
clear plan/report. Runtime/API/storage proof belongs to the owner's future implementation;
none is reported run here. No cluster, eviction, drain, stop/restart or lab action is authorized
by drafting this wrapper.

## Open questions

None for the composition-only move. The source choice remains its historical record; any
owner mechanism is proposed there with its exact APIs/race proof, not silently selected here.
