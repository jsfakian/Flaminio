# Issue #82: draft review choices

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Spec:** approved by Ioannis Sfakianakis, 2026-11-20, of the published clear snapshot (`docs/issues/publication-2026-11-20.json`, content_sha256=`08b66fb0be95bd9b0b19078ba2b2f6490686cbb440d89df9d58184a0d771504c`).
- **Design:** approved by Ioannis Sfakianakis, 2026-11-20, of the same published clear snapshot.

Approved with the recommended answer to each open question (see Settled at approval). The 2026-11-20 approval covers the published clear post-review text (including every plan-review fix recorded after the earlier 2026-11-13 approval, where present). Nothing is implemented yet.

## Settled at approval

- **First shutdown authority and return policy** ([spec](spec.md), Open questions, first item;
  [manual-lockout contract](manual-lockout-contract.md), the whole Proposed profile;
  [design](design.md), Risks, Proposed first profile and Return trade-off;
  [ADR proposal](adr-proposal.md), Decision proposed for review): the Proposed choice,
  manual-lockout-v1. An administrator's signed physical-lockout assertion, bound to the exact
  cluster, Node UID and inventoried machine, held until terminal old-UID retirement. No automatic
  same-identity return or `Clear`: the machine returns only with a fresh node name and PKI and a
  clean OS, keeping replica disks and keys offline until #13/#14/#61 obligations are resolved.
  Observe-only stays the default; other providers stay unsupported. The
  [ADR proposal](adr-proposal.md) is accepted with the plan.
- **Scoped partition recovery** ([spec](spec.md), Open questions, second item;
  [design](design.md), Scoped recovery and takeover): not included. It stays unavailable and is
  reported so, with no implicit opt-in; adding it later needs a design addendum first.
- **Proposed ADR 0007** ([spec](spec.md), Open questions, third item; [design](design.md),
  ADRs): as the plan states, it is context, not authority for #82. This plan neither accepts nor
  relies on it; any contradiction with an accepted ADR goes through explicit supersession.

## Choices

- **Review82-R1-C1:** define one complete first production authority rather than a nil/fake-only
  interface. An administrator signs a bounded exact-cluster/UID/physical-machine lockout assertion;
  the physical lock persists until terminal UID retirement. Software verifies attribution and
  protocol, not actual isolation. The threat model explicitly trusts the lockout administrator.
- Same-identity automatic return is unsupported. Retire old UID and use fresh node name/PKI/clean
  OS, preserve disks/keys, and account for #13/#14/#61 obligations before disk readmission. This
  avoids treating Kubernetes UID replacement as revoking old metadata identities or tokens.
- Keep default observe-only, unavailable scoped recovery and unchanged replication authority.

- **Review82-R2-N1:** authority configuration belongs to `flaminio-csi failover` and its
  failover deployment; the node daemon has no such flag.

- **Review82-R3-N1:** use the existing Deployment name `flaminio-failover` from driver.yaml.
