# Issue #78 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Spec:** approved by Ioannis Sfakianakis, 2026-11-20, of the published clear snapshot (`docs/issues/publication-2026-11-20.json`, content_sha256=`7c401d26392fc01f7afd442229d1a091c4a7a20991dd1917f437324a1af270a4`).
- **Design:** approved by Ioannis Sfakianakis, 2026-11-20, of the same published clear snapshot.

Approved with the recommended answer to each open question (see Settled at approval). The 2026-11-20 approval covers the published clear post-review text (including every plan-review fix recorded after the earlier 2026-11-13 approval, where present). Integration and validation status is recorded in [report.md](report.md).

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

## Implementation integration — 2026-11-27

Preserve and integrate the existing #78 implementation/tests with main after #77 and #80.
Retain #76's immutable admitted plan, original incarnation/root comparisons and generation
on recovery; retain #77 snapshot/Attach response contracts and #80 geometry/identity checks.
New admission bookkeeping must not weaken those landed guarantees. The integrated claim
retains #76's root revision compare and root write: four compares
(root, volume absence, tombstone absence, placement gate) and two writes (header, root).
This replaces the historical pre-#76 estimate of three compares / one write; it does not
weaken the root fence or add a node-count-sized transaction.

Move admission retains current main's chunk/header comparisons and adds the gate (3/1).
Issue #18's additional move failure-domain policy has not landed; #78 preserves existing direct
move eligibility and creation's `domain.Place` constraints. Changed registrations advance the
gate atomically, fencing stale creation planning. Future #18 node-record comparisons must compose
with this gate; no new move-domain policy is claimed here.

## Implementation review round 1

- R1-G-C1: inventory must reject aliases of known struct field names, including Go's Unicode
  case folding. Keep canonical field spelling, exact duplicate refusal and valid case-sensitive
  map keys distinct. This tightens corruption refusal without filtering unsupported geometry.
- R1-M-C1: shared fixture expectations must be checked by both implementations. The Rust model
  needs an occupancy oracle independent of its scanners and explicit stale-publication controls;
  blocked completion alone cannot prove a fenced writer was refused. Add positive pre-mark
  incoming move accounting and retain all original witnesses and the 827 exploration budget.
- R1-CI-C1: remove the two statically dead expressions; preserve positive revision checks and
  missing-gate initialization on no-op mutations. No protocol change or accepted risk.
