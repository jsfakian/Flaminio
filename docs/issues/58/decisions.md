# Issue #58 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Spec:** approved by Ioannis Sfakianakis, 2026-11-20, of the published clear snapshot (`docs/issues/publication-2026-11-20.json`, content_sha256=`b1057ca955bc1aefe1bcc4d4e1b195fa65b20a72c286241aef2a8d825f6db738`).
- **Design:** approved by Ioannis Sfakianakis, 2026-11-20, of the same published clear snapshot.

Approved with the recommended answer to each open question (see Settled at approval). The 2026-11-20 approval covers the published clear post-review text (including every plan-review fix recorded after the earlier 2026-11-13 approval, where present). Nothing is implemented yet. Earlier approval preceded plan-review clearance; the 2026-11-20 lines cover the cleared text.

## Settled at approval

- **Staging a ship:** as the proposal recommends, durable isolated staging roots (an inactive
  copy-on-write root per destination chunk and ship), never overwriting then rolling back the
  published point; this adds a mandatory isolated-root store feature. [spec.md](spec.md), Open
  questions; [design.md](design.md), The facts it starts from, and Isolated staging and commit;
  [adr-proposal.md](adr-proposal.md), Proposed decision and Options.
- **The pair and ship API:** as proposed, the local metadata messages and methods of
  [contracts.md](contracts.md) (`ConfigureDrPair` to `PromoteStandby`, with the receiver-only
  `AttachStandby` and `CommitShip`) and the dedicated DR-v1 foreign protocol, aligned with #51
  and #52 before coding. [spec.md](spec.md), Open questions; [design.md](design.md), Local
  authority and relationship, Foreign protocol, and Contracts and files touched;
  [contracts.md](contracts.md), Local metadata API.
- **Retained key boundaries:** as proposed, encrypted shipping and key transport are refused
  until #50's and #52's exported-key identity and lifetime contracts exist. [spec.md](spec.md),
  Open questions; [design.md](design.md), Docs, risks and ADRs;
  [adr-proposal.md](adr-proposal.md), Consequences.
- **The ship worker and diff reader:** as proposed, the ship controller reads through #52's
  shared snapshot reader and its scoped snapshot-read identity (canonical changed ranges, explicit
  zeros, the whole volume on a first ship), never an admin or target token.
  [spec.md](spec.md), Open questions; [design.md](design.md), Local authority and relationship,
  and Isolated staging and commit.
- **Promotion's divergence policy:** as proposed, promotion needs verified exclusion evidence or
  an explicit `--accept-possible-dual-primary` naming the expected source incarnation and point,
  recorded durably with the operator; it forks a new lineage, status always shows possible
  divergence, and there is no failback in this step. [spec.md](spec.md), Open questions;
  [design.md](design.md), Promotion and lineage; [contracts.md](contracts.md), Local metadata
  API.
- **Local format migration:** as proposed, no in-place migration of existing stores and no
  destructive reformat; destination staging needs the new mandatory feature, which old readers
  refuse. [spec.md](spec.md), Open questions; [contracts.md](contracts.md), Root and prepare
  authority.
- **The exact byte contracts** (the root format's feature bit, record tags and page layout, the
  snapshot reader's metadata, manifest canonicalization, prepared-proof batching and the full
  Begin and Status encodings, to settle with #51 and #52): no recommendation in the plan.
  Settled 2026-11-13 (user's instruction): see below. [contracts.md](contracts.md), formerly Root and prepare authority/Remaining gates
  (resolved by the concrete review appendices below);
  [design.md](design.md), header and Parallelizable?.

## Settled at the user's instruction of 2026-11-13

- **The exact byte contracts** (the root format's feature bit, record tags, page layout,
  padding, CRC, checkpoint root and bounds; the snapshot reader's metadata; manifest
  canonicalization; prepared-proof batching; the full Begin and Status encodings): the plan's
  own procedure, as one coordinated review with #51 and #52 before implementation. The format
  part is allocated in `docs/format/` and checked as the format spec requires, the reader's
  metadata and canonicalization are #52's shared reader contract, and the rest is written into
  `contracts.md`. No implementation starts, and destination staging stays refused, until that
  review has fixed them. Recommended by Claude, no recommendation in the plan: the plan forbids
  guessing these bytes and they are shared with #51 and #52, so the only safe answer is to fix
  them together, once, before any code.


## Changes after approval — review round1,2026-11-20

These revisions are **unapproved**. Historical approvals/settled answers above remain; no new
risk acceptance, runtime feature or accepted ADR modification is inferred.

- **R1-C1:** the coordinated draft resolution is now concrete issue-local metadata/lifecycle,
  root-format and wire/reader/manifest appendices. It replaces the earlier future-ABI gate;
  exact proposed allocations/bytes and cross-decode/crash/fuzz vectors are fixed here.
- **R1-C2:** complete source/destination operation records, named Rust worker/receiver/promoter,
  idempotent RPC/CLI flow and creation/deletion/recreation hooks close the lifecycle.
- **R1-C3:** trust the current local target's immutable-root barrier, as #51 trusts its cut;
  joiners hold every committed+pinned root/receipt. Metadata compares O(1) epoch/header/ship/
  relationship/pair records, not a misleading staged vector of chunk ISR observations.
- **R1-C4:** local backup identity/grants are separate from foreign shipper authority. Exact base/
  target grants and post-CAS destination-signed DR outcome preserve #52's close barriers; local
  cancellation makes no false remote-fencing claim.
- **R1-C5:** destination dr-roots-v1 persists after promotion; paired source dr-source-v1 guards
  growth. Header/client/root profile gates prevent legacy attach/rewrite, while unsupported
  legacy DeleteVolume/drain risk remains explicit. Enable rollout stops all old metadata first.
- **R1-C6:** use disjoint fresh DR destination resources on fl-c, preserving #54's restore disk/
  units/data. Active #83 VM admission blocks before mutation; legacy evidence is exploratory.
- **R1-S1:** one pair CAS is the revoke/config authority barrier. Every batch and publication
  compares the admitted generation; no unbounded epoch fanout is assumed atomic.
- **R1-S2:** FLDT and typed device-less receiver/promoter entries close ordinary/control/CSI/
  maintenance/ublk shortcuts; direct mode only with explicit slot/restart/status semantics.
- **R1-S3:** explicit before/after #73 roles, insecure refusal and #76 system-owner creation
  preserve authorization/owner/activation comparisons. New DR audit append is a named follow-up.
- **R1-S4:** finite waits/admission rates, global allocation reservation and paged disk-accounted
  manifests/proofs bound peer work while preserving last point and source reader pins.
- **R1-N1:** distinguish real historical acceptance from unapproved concrete review revisions.

The first promotion profile chooses explicit possible-dual-primary acknowledgement only; no
verified-exclusion provider is invented. Retained terminal records/roots and unsupported rollback
risks are named, not newly accepted. Every new safety rule has named model checks/caught mutants
in design.md. Dependencies remain read-only; whichever implementation lands second adds hooks.

The first increment deliberately retains aborted staging/charges/slots until whole-incarnation
deletion. It emits no individual RootDrop without a future reference-GC protocol. Promotion
checks local selected-point authority and remains possible after pair revocation.

A local CancelIncomingShip CAS fences an abandoned incoming operation and frees only its active
operation slot. It retains roots/charges; commit races preserve whichever decision won. New
ships allocate #51's monotonic reserved ordinal, never last-committed+1 after cancellation.


## Changes after approval — review round2,2026-11-20

These focused revisions remain **unapproved**; historical approvals/choices are preserved.

- **R2-C1:** worker-only BindDrManifest validates/stores complete bounded canonical manifest root
  and hash against authorized refs/geometry before Sending/Begin. Actual deterministic grant
  barriers, reconciled through progress, advance Closing→Closed with preserved terminal outcome.
- **R2-C3:** each DR grant claim compares the exact noncancelled outgoing and source guard and
  atomically publishes its pin/GrantRef. Cancellation freezes claims first, discovers both
  deterministic grant IDs across lost replies/crashes and closes through #52's barriers. Exact
  transaction bounds (including DR collection eligibility) and late_grant_after_cancel caught mutant are specified; EndDrSource cannot
  race a late claim or rely only on the caller's published reference list.
- **R2-C4:** deletion uses the existing REVOKED relationship enum with generation/fence change;
  Incoming/Outgoing cancellation stays separate. Replay/status/recreation fixtures use that enum.

R2-C2's immutable manifest-init/recovery ABI is supplied by root-format.md/wire-contracts.md;
Incoming now stores full bounded manifest bytes so pre-bind crashes recover without the sender.
No runtime/dependency/lab change or new approval is inferred.


## Changes after approval — review round3,2026-11-20

These focused revisions remain **unapproved**; historical approvals/choices are preserved.

- **R3-C1:** an already-CLOSED closure/progress/cancel retry returns the unchanged durable
  outcome and all-closed mask without requiring grant records eligible for collection. Missing
  completed grants are Internal only before durable closure. Lost-final-reply→collection→retry
  fixtures cover both completed and cancelled outcomes.
- **R3-N1:** identical SENDING manifest-bind retries retain UNSPECIFIED outcome; only completed
  Committed/Closing/Closed retries require COMPLETED. Cancelled outcomes still refuse.

No ABI, runtime, dependency or lab change and no new approval is inferred.
