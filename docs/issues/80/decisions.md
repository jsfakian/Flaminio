# Issue #80 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.

Approved with the recommended answer to each open question (see Settled at approval). The plan's
review loop continues after approval; later fixes are recorded as changes made after approval.
Nothing is implemented.

## Settled at approval

- **Geometry and metadata-budget policy** ([spec](spec.md), Open questions, first item;
  [ADR proposal](adr-proposal.md), Decision and Options; [design](design.md), Admission and
  metadata bounds and Risks, Open policy): the Proposed policy. Whole chunks only, power-of-two
  chunk sizes from 1 MiB through 1 GiB, at most 65,536 chunks, and the conservative admission
  bound `2048 + N*(288 + 264*(R+1)) <= 4,194,304`, giving effective N caps of
  5137/3881/3119/2607/2239 for RF 1 to 5. It accepts refusing some formerly accepted creates and
  stored volumes. Partial chunks, pagination and higher limits are deferred to a separate design.
  The [ADR proposal](adr-proposal.md) is accepted with the plan.
- **CSI compatibility contract** ([contracts](contracts.md), the whole draft contract;
  [design](design.md), CSI selection and retries): accepted as written. The administrator-only,
  read-only `CheckCreateVolume` decides CSI retries, with no scalar `GetVolume` fallback.
- **Migration of pre-existing unsupported volumes** ([spec](spec.md), Open questions, second
  item; [design](design.md), Risks; [ADR proposal](adr-proposal.md), Consequences): no
  recommendation in the plan. Settled 2026-11-13 (user's instruction): no migration path in this
  plan; a separate design defines one if it is needed (see below). Until one is defined, such volumes are refused before
  any target or replica mutation, with no automatic resize, rewrite or deletion (S8).

## Settled at the user's instruction of 2026-11-13

- **Migration of pre-existing unsupported volumes:** this plan defines none. Such volumes stay
  refused before any target or replica mutation, with no automatic resize, rewrite or deletion
  (S8), and a migration path, if one is needed, is a separate design, as higher limits are.
  Recommended by Claude, no recommendation in the plan: the refusal already keeps such a volume's
  data unchanged, any automatic migration would rewrite or drop data the plan cannot prove safe,
  and deferring it weakens no rule.

## Choices

- Review80-R1-C1: metadata alone evaluates full non-capacity identity and CSI range compatibility
  via read-only CheckCreateVolume. No scalar Get override; direct #76 creation stays strict.
- Review80-R1-C2: #86 owns bounded conservative uncertainty/Fenced framing, a required dependency
  before #80's full transport guarantee. No uncertainty truncation or silent reply-limit increase.
- Review80-R1-C3: both administrative and CSI quantities use checked arithmetic before RPC.
- Review80-R1-C4: validate every later view before runtime map/core mutation; higher epoch still
  deposes using safe current geometry. Growth requires its separate authorized transition.
