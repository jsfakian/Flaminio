# Issue #62 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.

Approved with the recommended answer to each open question (see Settled at approval). The plan's
review loop continues after approval; later fixes are recorded as changes made after approval.
Nothing is implemented. Approved before its plan review cleared (status: pending). The R1
revisions below and both new appendices are **proposed and unapproved**; the dated approvals
above remain historical provenance, not approval of revised contracts.

## Settled at approval

- **The measurement boundaries** (`spec.md`, Open questions, S1 and S5; `design.md`, Classify
  and count at the transport boundary and Isolated topology and shared observation): as proposed. TX
  counts the engine's frame handoff, connect attempts included (`tx_basis=engine_handoff_attempt`),
  RX counts decoded receipt, and all counters are classified by the labels observed at report
  time (`classification_basis=report_time_labels`). Neither is billing or delivery evidence; a
  change needs explicit contract review. R1 supplies concrete composition with the actual
  owners #71/#47 (observation) and #90 (identity), preserving these boundaries.
- **Mixed foreground and copy traffic** (`spec.md`, Open questions and S8; `design.md`,
  Successful client denominator and immutable snapshots and Isolated topology and shared observation):
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

## Proposed R1 corrections after approval

These settle the draft's technical mechanism for fresh review, not human acceptance. The
original engine-handoff TX, decoded RX, report-time labels, frontend-success-offered client
denominator and measurement-only scope remain unchanged. No runtime tests or lab collection
ran, and dependency plans remain read-only.

- **R1-C1/SEC62-R1-02 — actual observation owner.** #47 now uses #71's separate `observe`
  socket, schema2, not root stats-v1. [Observer contract](observer-contract.md) specifies #62's
  schema3 traffic/nullable exact latency, strict Rust/Go fixtures, bounded exact refusal,
  decoder-first upgrade, telemetry-off/privsep rules and inherited capture/rate limits.
- **R1-C2/SEC62-R1-01 — isolated bounded topology.** The same appendix supplies one
  ListNodes-only observer worker/private channel, one outstanding attempt, bounded whole-call
  deadline and completion-based retry, bounded decoding/cache and atomic generation-checked
  last-good installation. Authority metadata never queues behind observation. #72 provides no
  node-label paging; oversized/failed observations remain stale/unknown with visible age.
- **R1-C3 — decoded refusal.** Actual outgoing net.rs converts Refused directly to Lost.
  [Counting contract](counting-contract.md) emits exactly one RX Other receipt before that
  conversion, retaining the same failure behavior. Real initial/established codec/session
  tests include same-instance old sessions and ended-instance suppression.
- **SEC62-R1-03 — instance isolation.** Volume/epoch cannot distinguish standalone current
  detach/reattach. A fresh immutable Arc marker travels through outgoing commands/sessions/
  receipts; compare it before flow lookup/addition. Report ordinal exhaustion invalidates only
  diagnostics. Same-volume/same-epoch tests hold queued old callbacks; attach/fencing authority
  does not change.
- **R1-C4/SEC62-R1-04 — honest supported environment.** Preflight exact binaries/profile/
  provenance before mutation. #83-active current VMs yield blocked/uncollected; an explicitly
  separately authorized pre-#83 VM run is exploratory only. Supported collection needs the
  equivalent separately authorized qualified disposable physical fixture. No fabricated
  physical contract or admission bypass, and no baseline claim when the fixture is absent.
- **R1-C5/SEC62-R1-05 — one identity owner.** #90 alone owns exact refs, store lifetime,
  incarnation/session/readiness and v4 ordinary/snapshot/root profiles. #62's whoever-lands-
  second adapter preserves that admission before physical-node grouping and uses actual codec
  profiles/lengths; #61 is a tracker wrapper and #72 capacity only.
- **R1-C6 — actual placement floor.** R−1 assumes R distinct known replica zones with a
  replica in the known target zone. #18 requested zone/bestEffort can fall back per chunk and
  admit zero crossings. Compare each actual placement, add the mixed same-zone/distinct-zone
  fallback fixture, and retain unknown-zone incompleteness. No later policy value is set.
