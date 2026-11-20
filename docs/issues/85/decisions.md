# Issue #85 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Spec:** approved by Ioannis Sfakianakis, 2026-11-20, of the published clear snapshot (`docs/issues/publication-2026-11-20.json`, content_sha256=`71becde5d3aa4b06c4755891c5309e6f9eb44bb0094608cdcf1ade0a79fa6254`).
- **Design:** approved by Ioannis Sfakianakis, 2026-11-20, of the same published clear snapshot.

Approved with the recommended answer to each open question (see Settled at approval). The 2026-11-20 approval covers the published clear post-review text (including every plan-review fix recorded after the earlier 2026-11-13 approval, where present). Nothing is implemented yet.

## Settled at approval

- None to settle: the [spec](spec.md)'s Open questions section records none, and the
  [design](design.md)'s ADRs section states that no new architectural choice is required.

## Choices

- Review85-R1-C1: fail-stop protection includes teardown/reaping while ownership is outstanding,
  including safe Bytes owner destructor panics. The protection is a technical lifetime rule,
  not a new accepted operational risk; existing fail-stop policy remains.
- Review85-R1-C2: only a Submitted flush group owner may consume a flush CQE; success is exactly0.
  Waiting/Covered/Done and duplicate/impossible completions cannot mutate outcomes.
