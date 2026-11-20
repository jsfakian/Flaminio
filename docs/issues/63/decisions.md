# Issue #63 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.

Approved with the recommended answer to each open question (see Settled at approval). The plan's
review loop continues after approval; later fixes are recorded as changes made after approval.
Nothing is implemented. Approved before its plan review cleared (status: pending).

## Settled at approval

- **Shared guard tracking** (`spec.md`, opening paragraph and Open questions; `intent.md`,
  opening paragraphs; `design.md`, Lab, docs, risks and ADRs): as the intent recommends. The
  guard belongs in an issue of its own, which #48's step 3 and #63 depend on, and the intent
  moves there unchanged; that tracker action is not taken here.
- **The guard's process** (`design.md`, Process, security and Kubernetes contract, "Proposed"):
  as proposed. `flaminio-disruption-guard --namespace flaminio-system --meta ENDPOINTS --cert
  PATH --key PATH --ca PATH --trust-domain TD`, one controller Deployment and a leader Lease.
- **The lab check** (`design.md`, Lab, docs, risks and ADRs, "proposed"): as proposed,
  `make disruption-guard-test`, run only after explicit authorization and once the mechanism
  below is resolved.
- **The ADR's decision** (`adr-proposal.md`, Proposed decision and unresolved choice):
  accepted as proposed, with the plan: guard metadata stays read-only, admitted disruptions are
  serialized by Kubernetes' own accounting, identity is the Node UID, unknown state holds, and
  no pod matches more than one PDB. Its unresolved choice is the next item. Settled 2026-11-13
  (user's instruction): left to this ADR before implementation (see Settled at the user's
  instruction of 2026-11-13, below).
- **The fail-closed coverage and stale-permission mechanism for S1 and S4** (`spec.md`, S4 and
  Open questions; `design.md`, Required gate: coverage and stale permission;
  `adr-proposal.md`, Proposed decision and unresolved choice): no recommendation in the plan.
  Settled 2026-11-13 (user's instruction): neither option is chosen in this plan; the ADR
  chooses before implementation (see Settled at the user's instruction of 2026-11-13, below).
  The plan offers (a) narrowing the guard to an observed, best-effort contract with bounded
  staleness, or (b) a synchronous fail-closed eviction admission webhook with an enrollment
  gate, prefers neither, and requires the choice before implementation; the
  PDB marker candidate is not claimed as S1/S4 compliance.
- **Kubernetes version qualification** (`spec.md`, Open questions; `design.md`, The facts it
  starts from): no recommendation in the plan. Settled 2026-11-13 (user's instruction): the lab's
  k3s release (see Settled at the user's instruction of 2026-11-13, below). The plan pins
  qualification to the actual Kubernetes release against a real API server, but names no
  release.

## Settled at the user's instruction of 2026-11-13

- **The fail-closed coverage and stale-permission mechanism for S1 and S4:** neither option is
  chosen in this plan, as the batch's other plans leave a choice that needs its own evidence to
  the ADR or later plan that makes it. This issue's ADR (`adr-proposal.md`) chooses (a), the
  bounded best-effort contract with its exact failure window and bypasses, or (b), the
  synchronous fail-closed eviction admission with an enrollment gate, before S1 or S4 is
  implemented, with the exact API and race proof the design requires. Until then the intent's
  promise stands unnarrowed, the PDB marker candidate is not claimed as S1/S4 compliance, and
  nothing of S1 or S4 is built. If (b) is chosen, the intent, spec and ADR are updated first, as
  `design.md` (Required gate) says. Acknowledged writes rest on fencing and the ISR minimum under
  either option (the spec's invariant). Replaces an earlier answer of the same day that chose
  (a), at the user's instruction to keep #63 in line with the others.
- **Kubernetes version qualification:** the lab's k3s release, `K3S_VERSION` v1.36.5+k3s1
  (`scripts/lab.conf:114`): the real-API tests and the S8 lab check run against it, and no other
  release is claimed until the same tests pass on it. Recommended by Claude, no recommendation in
  the plan: it is the one real API server the project runs, and the plan requires qualification
  on an actual release rather than a named guess.
