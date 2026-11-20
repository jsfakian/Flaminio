# Issue #65 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.

Approved with the recommended answer to each open question (see Settled at approval). The plan's
review loop continues after approval; later fixes are recorded as changes made after approval.
Nothing is implemented. Approved before its plan review cleared (status: pending).

## Settled at approval

- **What the intent's "bound" means** (`spec.md`, Open questions and S6): as the plan states.
  It means the measured counterfactual estimates of S2 and S6, not a certified RDMA or kTLS
  saving.
- **A premature or inconclusive result** (`spec.md`, Open questions; `design.md`, Docs, risks
  and ADRs): as the plan states. Inconclusive is a valid outcome, and the measurements may show
  the work is premature.
- **The commands and result schema** (`design.md`, Contracts, "Proposed"): as proposed.
  `make transport-cost` (local, no lab access), `make transport-profile` (the opt-in authorized
  RF2 campaign through `lab.sh`), `make rdma-inventory` (read-only), and the output directory's
  `schema_version` 1 files and exit codes.
- **Hardware and profile access** (`spec.md`, Open questions; `design.md`, RF2 profile and
  inventory and Lab): no recommendation in the plan. Settled 2026-11-13 (user's instruction): the
  plan's own procedure for gathering them (see Settled at the user's instruction of 2026-11-13,
  below).
- **Later transport security and library choices** (`spec.md`, Open questions and S8;
  `design.md`, Docs, risks and ADRs): no recommendation in the plan. Settled 2026-11-13 (user's
  instruction): none is chosen in this step (see Settled at the user's instruction of
  2026-11-13, below). The plan writes no RDMA, security or library ADR before the evidence it
  requires.

## Settled at the user's instruction of 2026-11-13

- **Hardware and profile access:** the plan's own procedure for gathering it. First the
  read-only `make rdma-inventory` on each physical host and guest, which changes nothing and
  records what it cannot inspect as unknown (S7); the RF2 profile campaign
  (`make transport-profile`) and any software RDMA setup only after explicit lab authorization,
  each as its own phase with its stated cleanup; real RDMA hardware measurements are later #65
  work. Recommended by Claude, no recommendation in the plan: this is evidence to gather, so the
  decision is the plan's procedure, not a guessed result or a guessed host.
- **Later transport security and library choices:** none is chosen in this step. S8 lists the
  decisions still needed once the measurements exist, and each (RDMA, transport security, an
  unsafe FFI library) is then decided in an ADR on that evidence. Recommended by Claude, no
  recommendation in the plan: the step measures and builds no transport, and the plan forbids
  such an ADR before its evidence.
