# Issue #81: review choices — draft

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Spec:** approved by Ioannis Sfakianakis, 2026-11-20, of the published clear snapshot (`docs/issues/publication-2026-11-20.json`, content_sha256=`c333817de44a2d9115d9da47a5bd2c4209440a212a2728d48c09c9f19a59668f`).
- **Design:** approved by Ioannis Sfakianakis, 2026-11-20, of the same published clear snapshot.

Approved with the recommended answer to each open question (see Settled at approval). The 2026-11-20 approval covers the published clear post-review text (including every plan-review fix recorded after the earlier 2026-11-13 approval, where present). Nothing was implemented at approval; current implementation and validation are recorded in [report.md](report.md).

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

Runtime regressions and checks named in the design are recorded in [report.md](report.md).

## Implementation choices — 2026-11-27

- The real Rust-client/Go-service fixture drives `flaminio-repl::Target` directly, as the
  approved design requires. `flaminio-metaclient` therefore gains a path **dev-dependency**
  on `flaminio-repl`, with the corresponding workspace lockfile update. This adds no runtime
  dependency or protocol change; the manifest and lockfile changes support the specified
  interoperation test.
