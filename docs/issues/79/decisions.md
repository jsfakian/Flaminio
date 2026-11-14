# Issue #79: review choices — draft

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.

Approved with the recommended answer to each open question (see Settled at approval). The plan's
review loop continues after approval; later fixes are recorded as changes made after approval.
Nothing is implemented.

## Settled at approval

- **Ownership direction** ([ADR proposal](adr-proposal.md), Decision proposed and Options): the
  Proposed choice, persistent intent plus live kernel evidence, with its stated consequences
  (state loss can block operations until quiesced reconciliation; unprovable leaves may remain).
  The [ADR proposal](adr-proposal.md) is accepted with the plan.
- **Initialization authority** ([initialization contract](initialization-contract.md), the whole
  Proposed contract; [design](design.md), Ownership and crash boundaries): accepted as written,
  including the R2 scope amendment that adds permanent metadata use state, its RPCs and per-method
  authorization to #79, and its rule that an uncertain Start or an initialization daemon restart
  needs quiesced manual recovery, never another format.
- **Kubelet cleanup profile** ([kubelet cleanup contract](kubelet-cleanup-contract.md), the whole
  Proposed R2 contract; [design](design.md), Access and cleanup): accepted as written. The initial
  supported profile is upstream Kubernetes v1.35.0 with its path templates and deployment
  enforcement; other kubelet versions need a new pinned-source qualification.
- **Raw-block readonly mechanism** ([spec](spec.md), Open questions; [design](design.md), Access
  and cleanup; [ADR proposal](adr-proposal.md), Decision proposed): no recommendation in the plan.
  Settled 2026-11-13 (user's instruction): neither is chosen in this plan; a later plan chooses
  before S5's raw-block readonly is implemented, and it stays a release gate for #79 (see below). Until a tested mechanism is approved, the
  design's interim rule holds: readonly block publish returns `FailedPrecondition` before
  exposure, never a global `BLKROSET` on the shared device.
- **Not settled here:** the plan depends on #15's shared device-lifetime amendment (`8af7aeb`) and
  #83's fresh-process contract (`5a2fcd8`); each is accepted, or not, with its own issue's plan.

## Settled at the user's instruction of 2026-11-13

- **Raw-block readonly mechanism:** neither a per-publication device mechanism nor an explicit
  restriction is chosen in this plan; a later plan of #79, with its own proof (and an ADR if the
  mechanism needs one), chooses before S5's raw-block readonly enforcement is implemented, and
  the choice stays a release gate for #79. Until then the design's interim rule holds (readonly
  block publish returns `FailedPrecondition` before exposure, never a global `BLKROSET` on the
  shared ublk device), and the intent's and S5's promise stand. Recommended by Claude, no
  recommendation in the plan: deferred because the one choice available without a proven
  mechanism, an explicit restriction, would narrow S5's readonly for both block and filesystem
  and the issue's consistent block and filesystem handling.

## Choices resolved in R1

- **R1-S1/S6:** retain incarnation access/format history beyond path retirement; reject raw/fs
  profile conflicts and all other stages/effects before probe/format. Persist format-start and
  fail closed on uncertain completion rather than introducing automatic format repair.
- **R1-S2/S3:** depend on #15's concrete shared lifetime profile at 8af7aeb. Authoritative
  device-info/open/device-info compares all expected identity; detach is conditional at daemon
  execution. Legacy lifecycle effects fail closed, without changing old callers' wire formats.
- **R1-S4/S5:** normal unmount closes mounted-root FDs and uses a trusted pinned parent/path
  under exclusive cooperative namespace mutation. Preserve ambiguous leaves after interruption;
  persistent inode/type evidence cannot authorize an unlink without the original live witness.
- **R1-S7/S8/C3:** daemon-side ID grammar, effective readonly for both modes and the full built-
  image ext4-loop lifecycle are required, with adversarial cases mapped in design tests.

These are review fixes to draft contracts, not evidence of implementation or human acceptance.
Raw-block readonly mechanism/support: Settled 2026-11-13 (user's instruction): neither is chosen
in this plan; a later plan chooses, and it stays a release gate (see below).

## Choices resolved in R2

- **Review79-R2-C1:** replace R1's unspecified trusted history inventory/local profile authority
  with permanent metadata use state atomically created for new volumes. FreshFs and one-shot
  Start CAS are the only format authority; raw/unknown/legacy/import/restore never grant it.
  This explicitly amends intent's metadata exclusion and requires per-method auth, an executable
  model with caught mutants, and coordinated #76 fullidentity/#80 budget integration. No new
  steady-state metadata RPC/lock is introduced; shared #15 lifetime remains unchanged.
- **Review79-R2-C2:** replace vague cooperative ownership with upstream kubelet v1.35.0's verified
  executor/path/teardown contract, concrete admitted-writer restrictions and startup profile checks.
  Preserve normal successful parent-relative unmount; arbitrary trusted root mutation cannot be
  atomically fenced by Linux6.6 umount2 and is explicitly outside that deployment contract.
- **Review79-R2-S3:** fsync unbound operation intent before metadata/daemon attachment, persist
  returned epoch, then bind authoritative lifetime before probe/effects. New daemon operation
  wrappers make lost replies recoverable without adopting/detaching a different entry. An unknown
  Start outcome intentionally blocks automatic formatting and requires quiesced maintenance.

These supersede only the stated R1 assumptions. All architecture remains Proposed/unaccepted;
implementation/conformance remain unresolved, not approvals or test results. Raw-block readonly:
Settled 2026-11-13 (user's instruction): neither is chosen in this plan (see below).

## Choices resolved in R3

- **Review79-R3-C1:** require a named public-init admission bypass mutant and an independent
  event/history oracle; forbidden ordinary credential issuance fails even when another layer still
  blocks the resulting public attachment. Do not test a gate by reusing its own predicate.
- **Review79-R3-C2:** remove unused local authority_revision; no RPC returns it. Keep authoritative
  epoch/operation/nonce locally and keep conditional header-revision transactions at the service.
- **Review79-R3-S3:** compose required Proposed #83 fresh-process contract at5a2fcd8 exactly.
  First Reserve stores nonce and mints purpose2 FLCH only after the new epoch CAS; same Reserved
  op/node/epoch/nonce can recover credentials, changed daemon nonce cannot. Same-daemon plugin
  restart remains supported; init daemon restart requires quiesced manual recovery. Normal
  ReadyFs/Raw recovery obtains fresh Attach epoch/purpose1 FLCH. No FLCT/FLCI wire change.

These remain draft review corrections, not acceptance or proof that planned tests have run.
