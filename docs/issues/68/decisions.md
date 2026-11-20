# Issue #68 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.

Approved with the recommended answer to each open question (see Settled at approval). The plan's
review loop continues after approval; later fixes are recorded as changes made after approval.
Nothing is implemented. Approved before its plan review cleared (status: pending).

## Settled at approval

- **Which step comes first** (`intent.md`, opening paragraph; `spec.md`, opening paragraph): as
  the planner recommends, the ADR and the protocol and simulator groundwork, before the store,
  fetch path, command and lab check.
- **Who coordinates hydration** (`spec.md`, Open questions; `design.md`, Fetch and writes;
  `adr-proposal.md`, Proposed decision and Options): as recommended. Only the target
  coordinates it; a separate bounded, read-only worker verifies repository objects; replicas
  and applications get neither credentials nor fetch capabilities.
- **The availability tradeoff** (`spec.md`, Open questions and S7; `design.md`, Fetch and
  writes, "Recommended availability policy"; `adr-proposal.md`, Proposed decision and Options):
  as recommended, explicit typed failure rather than an indefinite wait: retriable
  `BaseUnavailable` on outage, `BaseMissing`, and `BaseCorrupt` on failed authentication.
  Filesystem behavior under these errors is still evaluated before release.
- **The durable presence representation** (`spec.md`, Open questions; `design.md`,
  Representation and authority; `adr-proposal.md`, Proposed decision and Options): as
  recommended, an explicit `Remote` presence state in every durable replica and protocol copy,
  never a reinterpreted Unmapped. The concrete versioned format and its number belong to the
  later runtime design.
- **The ADR's decision** (`adr-proposal.md`, Proposed decision; `design.md`, ADRs): accepted as
  proposed, with the plan.
- **The credential distribution protocol** (`spec.md`, Open questions; `design.md`, Contracts
  and Risks; `adr-proposal.md`, Consequences): no recommendation in the plan. Settled 2026-11-13
  (user's instruction): through #52's worker input, within the settled rule that only the
  scoped read-only worker holds repository credentials; the runtime design fixes their delivery
  and revocation (see Settled at the user's instruction of 2026-11-13, below).
- **#52's, #54's and #51's repository and snapshot contracts, the worker's home among them**
  (`spec.md`, Open questions; `design.md`, Risks and ADRs; `intent.md`, Why 8): no
  recommendation in the plan. Settled 2026-11-13 (user's instruction): as those issues' own
  approved plans settle them (see Settled at the user's instruction of 2026-11-13, below).

## Settled at the user's instruction of 2026-11-13

- **The credential distribution protocol:** the read-only fetch worker takes the repository's
  read credentials through #52's approved worker input: private credential files named by path,
  opened no-follow, never on argv, in logs, in frames or in records. They never reach the data
  plane, replicas, targets or applications. The later runtime design fixes how those files reach
  the worker's host and how they are rotated and revoked, before any runtime support; this model
  increment carries no credential. Recommended by Claude, no recommendation in the plan: it reuses
  an approved contract instead of a second credential channel, and keeps credentials off the
  replication wire and out of the data plane, as the settled worker rule requires.
- **#52's, #54's and #51's repository and snapshot contracts, the worker's home among them:** as
  those issues' own approved plans settle them (#52's `flaminio-backup` worker, a process of its
  own with a distinct backup identity, and its repository format; #51's snapshot and diff
  contract; #54's restore), adopted unchanged; #68's runtime design starts only after them.
  Recommended by Claude, no recommendation in the plan: the plan defers these to those issues,
  whose plans were approved on 2026-11-13, and a second contract here could only conflict with
  theirs.

## Proposed review revisions, 2026-11-20 — unapproved

These clarify the model-only slice after the historical approvals above. They do not record
new human approval or change the accepted issue-local ADR's protocol/worker/format decisions.

- **R1-C1 — independent partial-write oracle.** A four-lane model block and immutable original
  mask/operands in client history let a separate checker apply a patch to its own previous
  register state. Recording the implementation's merged value would bless zero-filled
  neighbors. Nonzero and overlapping partial/full-write scenarios catch the zero-fill mutant;
  the exact transition and history rules are in [model checks](model-checks.md).
- **R1-C2 — compiling isolated facade.** Put new hydration target/replica/metadata/message
  types in their own facade, leaving existing production enums and exhaustive adapters
  unchanged. The current wire cannot represent presence, and ordinary write/hole conversion
  would lose it. Cover the ordinary workspace build and both simulator paths; runtime typed
  adapters, durable layout, worker delivery/revocation and live integration remain later work.
- **SEC68-R1-01 — independent destination/base pin monitor.** Keep immutable requested origin
  and event-derived successful terminal commits outside metadata actors; after every model
  transition compare their required exact pin with durable modeled state. Omitted acquisition
  and early release must fail paired clean/mutant cases even with grow-only repository data.
  Include restart, failed/lost completion and same-name recreation; #52's reader-grant pins
  do not substitute for this destination dependency. No GC or credential protocol is added.
