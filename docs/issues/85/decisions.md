# Issue #85 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.

Approved with the recommended answer to each open question (see Settled at approval). The plan's
review loop continues after approval; later fixes are recorded as changes made after approval.
Nothing is implemented.

## Settled at approval

- None to settle: the [spec](spec.md)'s Open questions section records none, and the
  [design](design.md)'s ADRs section states that no new architectural choice is required.

## Choices

- Review85-R1-C1: fail-stop protection includes teardown/reaping while ownership is outstanding,
  including safe Bytes owner destructor panics. The protection is a technical lifetime rule,
  not a new accepted operational risk; existing fail-stop policy remains.
- Review85-R1-C2: only a Submitted flush group owner may consume a flush CQE; success is exactly0.
  Waiting/Covered/Done and duplicate/impossible completions cannot mutate outcomes.
