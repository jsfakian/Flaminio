# Issue #81: review choices — draft

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.

Approved with the recommended answer to each open question (see Settled at approval). The plan's
review loop continues after approval; later fixes are recorded as changes made after approval.
Nothing is implemented.

## Settled at approval

- None to settle: the [spec](spec.md)'s Open questions section records none requiring a
  decision, and the [design](design.md)'s Risks (Open choice) states that no additional
  architectural choice is required.

## Choices

- **Review81-R1-C1:** after target-generation and live-request correlation, a higher authenticated
  metadata epoch deposes before chunk-payload validation. Use current safe geometry with only
  the epoch raised; malformed data never enters caches or node maps. This preserves #80's
  authoritative-epoch contract and avoids turning shape validation into suppression of fencing.
- **Review81-R1-N1:** keep the plan's choices links and supply this explicitly unapproved record.

Runtime regressions and checks named in the design remain future implementation work.
