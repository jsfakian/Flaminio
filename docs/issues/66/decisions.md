# Issue #66 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Spec:** approved by Ioannis Sfakianakis, 2026-11-20, of the published clear snapshot (`docs/issues/publication-2026-11-20.json`, content_sha256=`1bf59f0f754e7fc62bab4c5db0dcdb6d0d941ee86c83c9200511f1c9f7f74b22`).
- **Design:** approved by Ioannis Sfakianakis, 2026-11-20, of the same published clear snapshot.

Approved with the recommended answer to each open question (see Settled at approval). The 2026-11-20 approval covers the published clear post-review text (including every plan-review fix recorded after the earlier 2026-11-13 approval, where present). Nothing is implemented yet. Earlier approval preceded plan-review clearance; the 2026-11-20 lines cover the cleared text.

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


## Review corrections — 2026-11-20 (proposed, unapproved)

Historical approvals and settlements above remain unchanged. These corrections address the
actual first plan review and claim neither fresh approval nor implementation evidence.

- **R1-C1:** preserve #84's sole ublk descriptor classifier, registration/flags/op/range
  precedence and configured max_io in both modes; typed refusals retain EOPNOTSUPP, EINVAL and
  EIO rather than collapsing unsupported requests to an engine error. Whoever lands second
  moves/reuses the existing classifier, with no second decoder or dependency edit.
- **R1-C2:** keep #15/#48's immutable device lifetime tuple separate from fresh routing Binding;
  kept entries have no held fd, resuming acquires one only at the specified ready step, and
  device-info/conditional detach retain the original tuple. #17 maintenance entries get no
  client binding/frontend/device slot. Exact Rust signatures still use the approved prototype.
- **R1-C3:** current Open questions now records the settled after-#15/test-only decisions;
  historical approval and later survey/ADR decisions remain intact.
- **SEC66-R1-01:** reserve event descriptors, bytes and a separate terminal slot before
  admission; route without waiting under the engine lock. Local attempts retain credits
  through disk ownership; retry/consumer exhaustion latches binding failure and detaches,
  retaining owners until drain or abort. Finite mailbox capacity does not assert a bound on
  all retries over time. This proposed exhaustion behavior makes resource failure explicit;
  no event or borrowed owner is silently lost.
- **SEC66-R1-02:** frontend PublishedBatch and enclosing queue owners enforce armed custom
  Drop-before-fields abort on panic/hard task cancellation while published I/O may reference
  memory. Normal cancellation drains. Prepare before eager publication; retain protection
  through completion/cleanup. #85's separate device-ring guard is not claimed to provide it.

Focused future classifier, recovery/maintenance, mailbox and ownership subprocess fixtures are
specified in the design. No runtime, prototype, profiler, lab or implementation tests ran during
these document corrections; current review revisions remain unapproved.
