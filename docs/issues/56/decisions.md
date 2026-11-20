# Issue #56 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Spec:** approved by Ioannis Sfakianakis, 2026-11-20, of the published clear snapshot (`docs/issues/publication-2026-11-20.json`, content_sha256=`0f36f079e0dd3bd47cb9720b40a3266d91d3ccd9c6cbc1db3322d73ae7606157`).
- **Design:** approved by Ioannis Sfakianakis, 2026-11-20, of the same published clear snapshot.

Approved with the recommended answer to each open question (see Settled at approval). The 2026-11-20 approval covers the published clear post-review text (including every plan-review fix recorded after the earlier 2026-11-13 approval, where present). Nothing is implemented yet. Earlier approval preceded plan-review clearance; the 2026-11-20 lines cover the cleared text.

## Settled at approval

- **The lab VMs' clock profile, unqualified by #83:** as the design proposes, the check first
  looks for a qualified #83 profile; on the current, unqualified VMs its results are exploratory
  only, with status Blocked for production qualification. [spec.md](spec.md), Open questions;
  [design.md](design.md), Lab history and faults.
- **Automatic node-loss recovery:** as the plan states, it uses #82's safe fencing or a confirmed
  power-off, never today's out-of-service inference; while #82's safe mode prevents automatic
  rescheduling, the check tests the data path and manual target behaviour and marks full VM and
  pod recovery Blocked. [spec.md](spec.md), Open questions; [design.md](design.md), Lab history
  and faults.
- **Two-node mutation witnesses not yet found:** as the plan states, they stay coverage gaps until
  a witness or a structural proof exists, never waived as impossible. [spec.md](spec.md), Open
  questions; [design.md](design.md), Simulator and mutation coverage.


## Changes after approval — review round 1, 2026-11-19

These revisions are **unapproved**; the historical approvals and settled answers above are
preserved, not extended to revised text. No implementation, lab result or new risk acceptance.

- **R1-C1/S1:** supersede the confirmed-power-off recovery alternative. Current #82 requires
  physical held lockout and terminal old-UID retirement; VMs have no supported provider.
  Power state confirms fault effect only. Legacy data-path checks conditionally stop/restore
  the unsafe failover Deployment and never authorize node-wide Kubernetes recovery.
- **R1-C2/S2:** current #83 refuses VM admission. Enforced VM runs block before mutation;
  explicitly legacy pre-#83 exploration stays Blocked for production qualification.
- **R1-C3/S3:** a separate FLEDGE01 coordinator history and adapter own operation envelopes,
  endpoint overlap, unknown/late replies, epoch deadlines and checkpoints. Reuse #49's value
  encoding and bounded register search without changing its v1 segment semantics.
- **R1-C4:** model-only labeled probes, explicit authority/crash controls and phase assertions
  prevent skipped operations or final healed convergence from proving fault-phase behavior.
- **R1-C5:** observe #77's actual public epoch/isr_ver/ISR tuple; no exposed etcd revision.
- **R1-C6/S4:** normal cleanup is bounded and observed; management loss needs the independent
  typed-ledger healer. Validate complete private inventory before privileged actions and
  preserve foreign/ambiguous state.

The concrete test-only history and healing contracts are in [lab-contracts.md](lab-contracts.md).
Clear dependencies remain unchanged; whichever implementation lands second integrates these
explicit seams. Mutation coverage gaps and current-environment Blocked results remain visible.


## Changes after approval — review round 2, 2026-11-19

These revisions remain **unapproved**; no approval or runtime result is inferred.

- **R2-C1:** checkpoints require a new read after the block is gated and drained, immediately
  followed by its checkpoint marker. Earlier workload reads cannot hide intervening acknowledged
  writes. The decoder checks the entire stream before accepting a cut.
- **R2-C2:** controller ownership absence is confirmed by GET and guarded by that response's
  resourceVersion in JSON Patch. Missing annotations maps are created without overwriting foreign
  entries; a concurrent owner annotation conflicts and causes refusal.
- **R2-S1:** a helper action is durable before launch; a private remote invocation claim gates
  device access and supports intent-only recovery. Signal only the pidfd-pinned, verified single
  helper process. Ambiguous or replaced units are preserved and cleanup blocks explicitly.


## Changes after approval — review round 3, 2026-11-19

This revision remains **unapproved**; no approval or runtime result is inferred.

- **R3-C1:** any controller-hold conflict aborts the attempt without retry or fault injection.
  Preserve ownerless foreign scale/spec changes; never reuse the old restoration snapshot
  against a newer resourceVersion. Explicit fakes cover both ownerless update cases.
