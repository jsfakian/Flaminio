# Proposal: Publish isolated standby roots before promotion

- **Status:** Historically accepted with the plan,2026-11-13. Detailed review revision,
  2026-11-20, is Proposed/unapproved; no accepted ADR file is changed.
- **Date:** 2026-11-12

## Context

[#58](https://github.com/jsfakian/Flaminio/issues/58) must promote the last whole snapshot even
mid-transfer. Rewriting the only live head mixes old/new data. Existing v1 layers do not provide
an inactive durable root or rollback. Foreign trust must remain isolated from local authority.

## Proposed decision

Use one local fenced receiver target and per-ship inactive COW roots on destination replicas.
Commit a complete destination snapshot/root manifest only after durable all-current-ISR proof.
Metadata CAS orders ship commit and promotion. Promotion forks only committed roots under a
new local epoch/lineage, refuses further ships and acknowledges possible remote divergence
unless qualified exclusion exists. Foreign peers receive only dedicated readless ship authority;
never merge their roots with local metadata/replica trust. Shared #51/#52 reader and durable
pins are prerequisites. Exact proposed format/API contracts are now fixed in the issue-local root/wire/lifecycle appendices.
Historical settlement,2026-11-13: exact shared bytes would be fixed by coordinated review.
That review resolution is now proposed in the linked issue-local appendices; detailed revisions
remain unapproved (decisions.md).

## Options

Overwrite then rollback avoids a new root selector but can make promotion require the peer or
full local rewrite, and crash-safe rollback is itself a new durable protocol. Buffer entire
snapshot outside replicas postpones space/durability requirements and duplicates staging.
Inactive roots retain last-good data with local promotion but add a mandatory format feature,
copy/reference accounting, GC and crash/recovery complexity. The historical inactive-root choice was accepted with the plan; subsequent details are unapproved.
Settled at approval, 2026-11-13: inactive roots, accepted with the plan (see decisions.md, Settled at approval).

## Consequences

Additional disk/layer capacity and bounded prepared evidence are necessary. Local outages pause
shipping without blocking source I/O. Manual ships do not deliver periodic RPO/failback/groups.
Dual primaries remain possible after disaster promotion; lineage prevents silent failback merge.
Encrypted shipping is refused pending exported-key identity/lifetime contracts.

## Confirmation

See [design](design.md): model partial-ship/promotion races, caught mutations, root-format fuzz
and crash replay, trust refusals, reference pin lifetime and exact-point two-cluster verification.
Global ADR numbering remains deferred. Historical acceptance is preserved; the concrete review
revision is unapproved, not an open-mechanism gate.
Settled at approval, 2026-11-13: accepted with the plan; numbered when implemented (see decisions.md, Settled at approval).

## Review revision —2026-11-20

The proposed concrete contracts add O(1) pair-revocation/current-target barriers, FLDT purpose
isolation, explicit system ownership/reader outcomes and fail-closed root/source profiles.
See [lifecycle](lifecycle.md), [metadata](contracts.md), [root](root-format.md),
[wire](wire-contracts.md). Historical future-coordination notes describe the earlier state, not
a remaining permission to guess bytes or defer mechanisms. No runtime implementation/approval.
