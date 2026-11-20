# Issue #62 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.

Approved with the recommended answer to each open question (see Settled at approval). The plan's
review loop continues after approval; later fixes are recorded as changes made after approval.
Nothing is implemented. Approved before its plan review cleared (status: pending).

## Settled at approval

- **The measurement boundaries** (`spec.md`, Open questions, S1 and S5; `design.md`, Classify
  and count at the transport boundary and Topology cache and shared stats v1): as proposed. TX
  counts the engine's frame handoff, connect attempts included (`tx_basis=engine_handoff_attempt`),
  RX counts decoded receipt, and all counters are classified by the labels observed at report
  time (`classification_basis=report_time_labels`). Neither is billing or delivery evidence; a
  change needs explicit contract review with #47 and #72.
- **Mixed foreground and copy traffic** (`spec.md`, Open questions and S8; `design.md`,
  Successful client denominator and immutable snapshots and Topology cache and shared stats v1):
  as the plan states. The report gives each flow and its raw denominators, with no amortized
  bill and no single claimed minimum.
- **Values for a later cost or latency policy** (`spec.md`, Open questions; `design.md`, Files
  and every-rule tests): no recommendation in the plan. Settled 2026-11-13 (user's instruction):
  none is set in this step; the later policy steps set them from this step's lab baseline (see
  Settled at the user's instruction of 2026-11-13, below).

## Settled at the user's instruction of 2026-11-13

- **Values for a later cost or latency policy:** none is set in this step; the later steps (cost
  in placement, zone read and copy sources, provider defaults) set them in their own plans from
  the baseline this step's lab check records in `docs/benchmarks/` (intent, What we want 6).
  Recommended by Claude, no recommendation in the plan: this step only measures and changes no
  placement or source choice, so values set now would be guesses that nothing uses, and the
  plan's own procedure for getting them is the measured baseline.
