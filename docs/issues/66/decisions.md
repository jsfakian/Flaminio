# Issue #66 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.

Approved with the recommended answer to each open question (see Settled at approval). The plan's
review loop continues after approval; later fixes are recorded as changes made after approval.
Nothing is implemented. Approved before its plan review cleared (status: pending).

## Settled at approval

- **When** (`spec.md`, Open questions; `intent.md`, Decisions the spec will ask for): as the
  intent recommends. After #15, kept small.
- **Shipped or test-only, for the frontend inside the process** (`spec.md`, Open questions;
  `intent.md`, Decisions the spec will ask for; `design.md`, Node and binding): as the intent
  recommends, test-only (`cfg(test)` `Endpoint::Memory`), never registered or selectable in
  production.
- **The exact shared interface** (`spec.md`, Open questions; `design.md`, opening paragraph,
  Contracts and Docs, risks and ADRs): as proposed. The internal contracts `Binding`, `Request`
  and `RequestKey`, `Geometry`, `QueueLimits`, `Endpoint`, `FrontendControl` and
  `LocalReadExecutor`, fitted to #15's registry and router and reconciled with #17's device-less
  and #48's kept endpoint states before coding; unknown details are settled by a compilable thin
  extraction prototype before broad moves.
- **The kernel survey ADR 0003 asks for** (`spec.md`, Open questions; `intent.md`, Decisions the
  spec will ask for; `design.md`, Docs, risks and ADRs): as the intent recommends. `ublk_drv` and
  the kernel's NVMe/TCP host in each targeted distribution are surveyed before any plan of steps
  2–4; it is not part of this step.
- **The kernel's NVMe target or our own, for external hosts** (`spec.md`, Open questions;
  `intent.md`, Decisions the spec will ask for; `design.md`, Docs, risks and ADRs): no
  recommendation in the plan. Settled 2026-11-13 (user's instruction): neither in this step;
  step 2's ADR chooses, after the kernel survey (see Settled at the user's instruction of
  2026-11-13, below).

## Settled at the user's instruction of 2026-11-13

- **The kernel's NVMe target or our own, for external hosts:** neither is chosen in this step.
  Step 2's plan chooses it in an ADR, after the kernel survey that comes before any plan of
  steps 2–4, together with that step's authentication of external hosts. Recommended by Claude,
  no recommendation in the plan: this step builds no export, and the choice rests on the survey's
  result and step 2's security design, which the plan assigns to that ADR.
