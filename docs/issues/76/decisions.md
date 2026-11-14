# Issue #76 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.

Approved with the recommended answer to each open question (see Settled at approval). The plan's
review loop continues after approval; later fixes are recorded as changes made after approval.
Nothing is implemented.

## Settled at approval

- **Q1, duplicate chunk metadata from staged plans** ([spec](spec.md), Open questions;
  [ADR proposal](adr-proposal.md), Options; [design](design.md), Lab, docs, risks and ADRs):
  the Proposed choice. Persist the immutable staged plan before admission and accept its cost:
  peak chunk metadata near twice the initial plan per operation while it is staged. The compact
  replayable plan is not pursued. The [ADR proposal](adr-proposal.md) is accepted with the plan.
- **Q2, legacy visible headers without full policy identity** ([spec](spec.md), Open questions;
  [design](design.md), Deletion classification and legacy rollback): keep their current scalar
  matching as a documented compatibility exception; #76 does not reconstruct historical policy.
- **Q3, rollout and legacy cleanup** ([spec](spec.md), Open questions; [design](design.md), Lab,
  docs, risks and ADRs; [ADR proposal](adr-proposal.md), Consequences): upgrade under a
  maintenance barrier that excludes old writers. Rolling mixed-version safety is not claimed; a
  version fence for rolling upgrades stays outside this issue. Unproved legacy states get
  conservative cleanup, existing tombstones still gate reuse, and S8's proof-based never-published
  rollback is the only cleanup exception.

## Choices

- R1-C1/R1-S3: bounded latest keys-only discovery and bounded values replace full-prefix
  materialization; cooperative recovery shares explicit work/mutation budgets across sources.
  The API needs no historical snapshot; Mem maintains an ordered index rather than cloning data.
- R1-C2: Deleting takes precedence over Creating/missing-root checks; public published deletion
  remains distinct from resumable unpublished creation rollback.
- R1-S1: private reclamation uses root deletion as fence then orphan deletion under root absence,
  with no Put in GC. Sixteen permanent birth-revision shards make initial-root retries non-ABA;
  operation IDs are never resurrected. Quota pressure alone cannot cancel admitted work.
- R1-C1/R1-S2: the recognized never-published shape plus writer-exclusion/monotone-first-epoch
  proof permits exact CAS header removal without creating a tombstone. Unknown shapes retain
  conservative handling. Every destructive header/orphan-key fence gets a caught mutant and
  real CAS interleaving; no by-name deletion substitution is permitted.
- R1-S4: validate bounded preferred-node/canonical identity before staging and bounded persisted
  records before decoding/using them; rejection does not leave a private operation behind.
- R2-C1: model retired operation IDs and assert NoRootResurrection; a caught missing_birth_compare
  mutation and original-transaction Go replay cover the birth fence separately from progress.
- R2-C2: retain each bounded page's unexamined tail; cursor/wrap advance follows examined keys,
  never merely fetched next. Four-source, budget-limited fixtures must prove no tail starvation.
- R2-S1: assert NoUnprovedTombstoneBypass from independent per-incarnation publication/Attach/
  token history plus recognized proof conditions. Catch weakened_unpublished_classifier and
  replay real Go authorization transitions, unsupported shapes and delayed prior-incarnation tokens.

These are technical plan refinements addressing review, pending incremental review. Q1–Q3 and
all architectural/availability approvals remain open; no accepted risk or user approval is added.
