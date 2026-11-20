# Issue #64 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.

Approved with the recommended answer to each open question (see Settled at approval). The plan's
review loop continues after approval; later fixes are recorded as changes made after approval.
Nothing is implemented. Approved before its plan review cleared (status: pending).

## Settled at approval

- **The study's outcome** (`spec.md`, Open questions and S10; `design.md`, Study artifact and
  outcome; `study.md`, Proposed outcome): as recommended, **not yet**. Reopen when a real
  workload shows a space need beyond backup and cold storage, #72's accounting and classes
  exist, a coding commit and recovery model catches the write-hole mutations, and hardware with
  independent domains can verify m-loss and rebuild; then a Proposed ADR chooses scope,
  geometry, acknowledgement policy, library, format and the simulator-first order. No coding
  protocol, library or profile is accepted, and #64 stays on the roadmap and open.

## Changes made after approval — review round 1, 2026-11-20 (unapproved)

- **R1-C1 — prerequisite attribution.** The approval record above is historical. The reviewed
  #72 increment is a per-node capacity ledger and create/move admission; it explicitly excludes
  device classes and supplies no cold classifier. Device-class policy remains a separate future
  prerequisite; a later cold-only scope also needs a cold classification policy. This corrects
  attribution without reopening the approved not-yet outcome or #72.
- **R1-C2 — first-order cost study.** Add an assumptions-labeled RF2/RF3/systematic-coding
  paper comparison, including conversion/coexistence and baseline transfer dependency rounds.
  Coding durable-decision rounds remain unknown; no geometry, method or protocol is selected.
- **R1-C3 — approval status.** Not yet and its reopening criteria were settled on 2026-11-13.
  These later review corrections are unapproved; no coding runtime/library/profile or accepted
  superseding ADR follows from either the approval or this review. #64 stays open for later steps.
- **R1-C4 — lease rationale.** Attribute the single-source quotation to ARCHITECTURE §8.5.
  Preserve the stricter current effective-ISR fencing/reconciliation behavior and leave ADR0005
  unchanged.
