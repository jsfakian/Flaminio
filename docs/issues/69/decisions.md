# Issue #69 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.

Approved with the recommended answer to each open question (see Settled at approval). The plan's
review loop continues after approval; later fixes are recorded as changes made after approval.
Nothing is implemented. Approved before its plan review cleared (status: pending).

## Settled at approval

- **The reader identity URI and API extension** (`spec.md`, Open questions; `design.md`, Reader
  identity and authorization, Risks and ADRs; `adr-proposal.md`, Decision proposed): as
  proposed. `spiffe://<trust-domain>/flaminio/reader/<name>`, parsed identically in Go and Rust;
  exactly ListNodes, ListVolumes, GetVolume and node-scoped ListTombstones allowed, every other
  and future method denied; replica sessions refuse it; reader credentials are issued only after
  server support, treated as admin read access, with no admin fallback.
- **The initial CLI and resource limits** (`spec.md`, Open questions; `design.md`, Collection
  and derivation, "Proposed limits", and CLI and output contract): as proposed.
  `flaminio-meta status [--json]` with the reader identity; 4096 registered nodes, 4096 volume
  IDs, 64 MiB of collected and encoded data, a 30 s total deadline and four RPCs in flight,
  refused with ResourceExhausted rather than truncated; the `schema_version` 1 JSON contract.
- **The ADR's decision** (`adr-proposal.md`, Decision proposed; `design.md`, ADRs): accepted as
  proposed, with the plan.
- **Dashboard authorization and toolchain** (`spec.md`, Open questions; `design.md`, Docs):
  no recommendation in the plan. Settled 2026-11-13 (user's instruction): decided by #69's later
  dashboard stage within this plan's rules, the toolchain in an ADR; none in this step (see
  Settled at the user's instruction of 2026-11-13, below).

## Settled at the user's instruction of 2026-11-13

- **Dashboard authorization and toolchain:** neither is chosen in this step, which serves no
  page. #69's later dashboard stage decides both in its own plan, after #43's operator can hold
  the reader identity, within this plan's rules: the page's server holds only the reader
  identity, never an administrator's; browsers authenticate through Kubernetes, as the intent
  states; and the view is admin read access, never given to tenants. The web toolchain is chosen
  in an ADR beside ADR 0001, which names only Go and Rust. Recommended by Claude, no
  recommendation in the plan: nothing in this step would use either choice, and a new toolchain
  needs an ADR.
