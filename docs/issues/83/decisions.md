# Issue #83: draft review choices

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.

Approved with the recommended answer to each open question (see Settled at approval). The plan's
review loop continues after approval; later fixes are recorded as changes made after approval.
Nothing is implemented.

## Settled at approval

- **Clock profile, fixed timing and attestation** ([spec](spec.md), Open questions, first item;
  [ADR proposal](adr-proposal.md), Decision; [design](design.md), Profile admission and Elapsed
  clock and observer): the Proposed profile, physical-boottime-v1. Qualified physical Linux hosts
  with `CLOCK_BOOTTIME` as elapsed authority; fixed cluster-wide L = 5 s and rho = 100,000 ppm;
  per-read error and resolution of at most 1 us, E = 3 us, and the takeover wait
  `ceil((L+E)*(1+rho))+E` (5,500,007 us at defaults); explicit operator attestation in the clock
  contract file, with no bypass. Virtual or unknown-provenance hosts, current lab VMs included,
  are refused until a separately qualified profile with an enforceable hypervisor clock and
  rollback contract exists. Quarantine stays sticky until operator restart. The
  [ADR proposal](adr-proposal.md) is accepted with the plan.
- **What local probes cannot prove** ([spec](spec.md), Open questions, second item;
  [design](design.md), Docs, risks and ADRs): as the plan states, the relative-rate bound and the
  absence of coordinated memory, disk or time rollback are not established by software. Hardware
  qualification and administrative exclusion of rollback stay deployment obligations under the
  operator's attestation.
- **Fresh-process handoff** ([fresh-process contract](fresh-process-contract.md), the whole
  Proposed contract; [design](design.md), Quarantine and completion ordering): accepted as
  written. An agent or administrator Attach under `only_if_target`, bound by the signed `FLCH`
  envelope to a 32-byte daemon challenge; node and io roles cannot mint it. A daemon restart
  during private initialization stays blocked for quiesced manual recovery.

## Choices

- **Review83-R1-C1:** existing agent/admin role issues new-epoch Attach, with signed FLCH binding
  its exact FLCT token digest to a32-byte unpredictable daemon challenge. Every target-start
  entry enforces it; no new permission for node/io. #15 u64 INSTANCE is not a nonce. #79 FLCI
  purpose remains separate; private initialization cannot restart under a new process at its
  old reserved epoch. Keep uncertain work/manual recovery rather than reopen format authority.
- **Review83-R1-C2:** qualify absolute read error/resolution<=1 us, elapsed-difference E=3 us,
  conservative wait ceil((L+E)*(1+rho))+E. Wider suspend-detection tolerance is not clock accuracy.
  Add independent worst-error phase tests and a caught ignore_observation_error mutation.
- **Review83-R1-N1:** supply this unapproved choices record for the existing links.

- **Review83-R2-N1:** retain the complete clock-error test description on its Markdown table row.
