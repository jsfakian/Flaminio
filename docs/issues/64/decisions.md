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
