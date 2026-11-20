# Issue #78 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Spec:** approved by Ioannis Sfakianakis, 2026-11-20, of the published clear snapshot (`docs/issues/publication-2026-11-20.json`, content_sha256=`7c401d26392fc01f7afd442229d1a091c4a7a20991dd1917f437324a1af270a4`).
- **Design:** approved by Ioannis Sfakianakis, 2026-11-20, of the same published clear snapshot.

Approved with the recommended answer to each open question (see Settled at approval). The 2026-11-20 approval covers the published clear post-review text (including every plan-review fix recorded after the earlier 2026-11-13 approval, where present). Nothing is implemented yet.

## Settled at approval

- **Q1, reservation precision** ([spec](spec.md), Open questions; [design](design.md), Risks;
  [ADR proposal](adr-proposal.md), Options): the Proposed choice. One global placement gate and a
  conservative completion scan that waits for every earlier creating header, accepting that an
  unrelated earlier creation can block a drain and that topology writes cause admission retries.
  A per-node reservation index is not pursued here. The [ADR proposal](adr-proposal.md) is
  accepted with the plan.
- **Q2, recovery of a stranded reservation** ([spec](spec.md), Open questions;
  [design](design.md), Inventory and completion and Risks): until #76 lands, explicit
  incarnation-fenced `DeleteVolume` is the recovery, and a stranded reservation may block a drain
  indefinitely. No timeout reaps a reservation; a #76 resume keeps the admitted plan and generation.
- **Q3, rollout** ([spec](spec.md), Open questions; [design](design.md), Risks): a maintenance
  barrier, following the design's upgrade steps: quiesce create/move/drain mutations, stop every
  old metadata writer and drain controller, deploy the new instances, initialize the gate above
  every retained legacy marker, verify admission refuses generation 0, then resume. No rolling
  mixed-version guarantee is claimed; that needs a separate capability or version fence.

## Choices

- Review78-R1-C1: initialize the positive placement gate above retained legacy marker revisions
  before new admissions; initialization does not rewrite marker generations.
- Review78-R1-C2: model scan and final generation validation separately and require a caught
  missing-final-marker-compare mutation. A completion certifies D only at its validation instant.
