# Issue #51 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Spec:** approved by Ioannis Sfakianakis, 2026-11-20, of the published clear snapshot (`docs/issues/publication-2026-11-20.json`, content_sha256=`f6877520beccac71dbcca353c98aa3cd0312f01f5d8a6f08fd07b4807a6593a6`).
- **Design:** approved by Ioannis Sfakianakis, 2026-11-20, of the same published clear snapshot.
- Plan review round 1 (2026-11-18) fixes, made after approval: see below.
- Plan review round 2 (2026-11-18) fixes, made after approval: see below.
- Plan review round 3 (2026-11-18) fixes, made after approval: see below.
- Plan review round 4 (2026-11-18): clear; two wording Nits fixed after approval, see below.

Approved with the recommended answer to each open question (see Settled at approval). The 2026-11-20 approval covers the published clear post-review text (including every plan-review fix recorded after the earlier 2026-11-13 approval, where present). Nothing is implemented yet. Earlier approval preceded plan-review clearance; the 2026-11-20 lines cover the cleared text.

## Settled at approval

- **Q1. Who makes the cut:** the target, by S3, after an administrator's request (S3, S7–S10).
  [spec.md](spec.md), Open questions.
- **Q2. A volume that is not attached:** the snapshot waits for the next attach (S7).
  [spec.md](spec.md), Open questions.
- **Q3. Catch-up without a format change:** S15's order, oldest generation first.
  [spec.md](spec.md), Open questions.
- **Q4. A cut during a catch-up:** it does not wait (S17); the catch-up gains a stage.
  [spec.md](spec.md), Open questions.
- **Q5. How a snapshot is read in this step:** per replica only (S19–S21). [spec.md](spec.md),
  Open questions.
- **Q6. Limits before deletion exists:** 63 snapshots in a volume's life (S2), and a snapshot
  goes with its volume (S6). [spec.md](spec.md), Open questions.
- **Q7. Upgrades:** nodes are upgraded together (S11). [spec.md](spec.md), Open questions.
- **Q8. A metadata outage during a cut:** accepted: writes stay held until the answer (S9).
  [spec.md](spec.md), Open questions.
- **The first snapshot's budget eligibility** (the extra S2 rule): as the design proposes, a
  volume whose full 63-entry catalog cannot fit #80's 4 MiB response budget
  (`32768+N*(288+264*(R+1))`) is refused its first snapshot; snapshot-free volumes keep their
  geometry. [design.md](design.md), Budget, rollout, files and tests;
  [adr-proposal.md](adr-proposal.md), Consequences and open choices. The bound was changed
  after approval in round 1 (R1-C1, R1-C2, below).
- **A historical mismatch below a newer top:** as the design proposes, it fails closed and
  blocks the join; nothing is rewritten. If the model shows that ordered catch-up can create
  it, Q3's no-format-change choice is revised before implementation. [design.md](design.md),
  Catch-up, sparse history and joins, and Lab, docs, risks and ADRs.
- **The exact catch-up stage invariant:** as the design and the ADR propose: each stage is
  durably acknowledged before the next, live writes reach a joiner only once every older stage
  is complete, and a join needs every committed cut verified. [design.md](design.md), Catch-up,
  sparse history and joins; [adr-proposal.md](adr-proposal.md), Decision proposed.

## Plan review round 1 (2026-11-18), changes made after approval

Reviewed commit c8908eb; findings R1-C1 to R1-C13 (code) and R1-S1 to R1-S7 (security). Each
change below was made after approval, in this round; nothing in it is implemented, measured or
approved anew.

- **The snapshot-capable bound starts from #13's** (R1-C1): `B_s(N,R) = 4160 + 12549 + 415 +
  N × (288 + 264 × (R+1) + 132) ≤ 4,194,304`, caps 4406, 3446, 2830, 2400, 2084 for RF 1 to 5.
  The catalog allowance is derived (contracts, Size: 63 maximal entries of 199 bytes and field
  30's 12), not a separate constant; #50's maximal descriptor (415) is kept in the constant. The
  bound is checked at every request, not only the first. Whichever of #13, #80 and #51 lands
  second applies the constant, the term and the worst-view fixtures. Supersedes the formula of
  the Settled entry above. [design.md](design.md), Budget.
- **Growth of a volume with snapshots** (R1-C2): every #70 step of such a volume checks `B_s` at
  the new size (whichever of #70 and #51 lands second adds it, with its test), and a chunk added
  at generation g is zeros in every snapshot at or below g (S19, with a store test).
- **Roles, audit and the charge, one fix** (R1-C3, R1-S1, R1-S4, R1-S2): #73's table gains
  RequestSnapshot (administrator name; once active `cluster-admin`, the simplest role that reads
  no owner record, as MoveReplica's), ListSnapshots (an open read) and CommitSnapshot (the
  target's data plane), with insecure-instance behaviour stated; whichever of #51 and #73 lands
  second adds them. Audit: a `docs/followups.md` entry, issue to be filed, not events (cut before
  adding). Retention: a volume with k snapshots is charged (1 + k) times by #72's ledger and #73's
  quota, the request charging in its own transaction and refused without room or quota, the
  multiplier applied in #72's `VolumeCharges` and `MoveReplica`, #13's `ConfirmRelease` and #70's
  growth (whichever lands second applies each part). Of R1-S2's options this is the charge: the
  other, accepting the gap, is not this plan's to accept. [spec.md](spec.md), S2, S4, S6;
  [contracts.md](contracts.md), Retention charge, Roles; [design.md](design.md), Roles, audit
  and charges.
- **A rule narrowed, shown necessary by R1-S2 and the 120-operation bound (withdrawn in round 2,
  R2-C2, below):** a request whose volume's named copies are on more than 114 nodes is refused
  (`FailedPrecondition`): the charge must be in the request's transaction (#72's invariant: every
  copy charged in the transaction that names it), and 114 ledgers plus the header, the record and
  #73's four compares reach 120. S2 gains this refusal; the user may lift it with a batched charge
  in a later plan.
- **The page pool is not charged** (R1-S2; withdrawn in round 2, R2-S1, below): named, not accepted.
  #72 counts no metadata pages, and sparse writes across many layers can fill the pool, refusing
  other volumes' map pages on that disk. A follow-up, to be filed; the lab measures it.
- **Catalog deletion** (R1-C4, R1-S6, one fix): the catalog goes after the header's transaction,
  as chunk records do, in `deleteChunks`' batches conditional on the header being absent; the
  header-absent branch and a creation's claim clear `snapshots/<volume>/` too. Of the two
  orders the reports offer, C4's: it reuses the existing crash path and adds nothing to #73's
  `finishDeletion`. Every read lists the current incarnation's prefix only.
- **Coordinated byte review** (R1-C5): the bytes #52 and #58 read are pending #58's coordinated
  review with #51 and #52 before implementation (deferred, not settled here); contracts.md's
  title no longer calls them exact or accepted. #51's completeness evidence for #52's
  `SourceProofV1` is defined: a #77-validated view with the snapshot committed and the replica in
  the chunk's ISR at its `isr_ver` (contracts, Completeness evidence; S22). #52 and #58 unchanged.
- **Simulator** (R1-C6): mutations `join_before_frontier` (S18) and `stale_stage_reply` (S15,
  S18), caught by agreement; a fourth check, publication, defined in the spec, catches
  `early_publish`. Eleven mutations, all in `flaminio-repl`.
- **A write below the top** (R1-C7): ends the session with `Refused` (no `req`, closes the
  connection), no store effect; the target takes the replica as failed. No new reply tag.
- **CommitSnapshot's caller** (R1-C8): only Authorize answers PermissionDenied (a non-`io`
  caller); another node's `io` or a deposed target gets `applied=false` and the current state.
- **The request's header write** (R1-C9): kept in the header (the charge's other transactions
  compare it), its effect stated: racing ChangeIsr and MoveReplica retry, #77's pinned Attach can
  answer `Aborted` after its commit and its caller attaches again (named, not accepted); the
  catalog is listed inside #77's header-to-header window. S2's "changes nothing else" now names
  the count.
- **`snapshot-read`** (R1-C10, R1-S7, one fix): the node's key derivation; a chunk fenced below
  `--first-epoch` refused (#30 S5's rule, as `Store::delete_volume` applies it). R1-S7's
  phrasing selects the same chunks: none is fenced at E itself, since `Start` sets the epoch to
  the first epoch and every Attach adds one (`domain.go:211-219`) (reason corrected in round 2,
  a code Nit).
- **Encrypted volumes** (R1-C11, R1-S3, one fix): refused at request by #50's field 20 or its
  header mark, in S2 and the contract, with a test; the "incompatible profile" refusal is dropped
  (the service does not know a target's version; the GetVolume gate covers it).
- **Wire tags** (R1-S5): `Layers` 0x48 (72) and `Changes` 0x49 (73); #13 holds 71, no clear plan
  holds 72 or 73 as a frame tag.
- **Tests and commands** (R1-C12): a test named, with its file, per rule and per mutation; the
  lab check `make snapshot-check` (`scripts/checks/snapshot.sh`, script test under `make lint`);
  `make proto meta-vectors` and the full pre-PR set.
- **Settled wording** (R1-C13 and the security Nit on design.md's Lab section): spec.md, design.md
  and adr-proposal.md no longer call settled choices open.
- **Nits:** the design's "#76/#77" ownership now names #76's creation and #73's deletion; the
  `<first_epoch>` key segment is 20 zero-padded decimal digits; the `snapshot-v2` gate names its
  callers as #50's gate does. None left.

## Plan review round 2 (2026-11-18), changes made after approval

Reviewed commit da886c5; findings R2-C1 to R2-C4 (code) and R2-S1 to R2-S3 (security), and four
Nits. Each change below was made after approval, in this round; nothing in it is implemented,
measured or approved anew.

- **The node charge after a header mark, in batches** (R2-C1, R2-C2, one fix, the reviewer's):
  a request checks room before any write, then marks (record `CHARGING`, count, ordinal, the
  header's new `snapshot_charge_next` = 0, the quota), then charges one chunk size and one slot
  per named copy in batches by chunk index, each comparing the header, its chunk records and
  ledgers and advancing the cursor; the last makes the record `PENDING`. Every charging site uses
  m(*j*): 1 + `snapshot_count`, less one for a chunk at or after the cursor. A move racing the
  request is charged exactly once (`TestSnapshotRaces`' new hook). A `CHARGING` snapshot is never
  cut (it retains nothing); a batch without room keeps earlier batches, as #70's steps do, and
  the volume's next request completes the charge, as after a crash. [contracts.md](contracts.md),
  Retention charge, Transactions; [design.md](design.md), Roles, audit and charges.
- **Narrowing withdrawn** (R2-C2): round 1's refusal of a volume on more than 114 nodes is
  removed. R1-S2 showed a charge is needed, not that it must fit one transaction; a Pending
  request retains nothing, so it may be charged in batches. S2 holds as written before round 1
  in that any volume can be snapshotted, consistent with #72's creation limit (116 nodes) and
  with volumes that growth and moves spread further.
- **A node without a valid report** (R2-C2): round 1's refusal of a copy on such a node (a down
  node, a released copy awaiting `ForgetNode`) is kept, since #72's `Room` needs the report, and
  is now stated in S2 and tested; it is round 1's charge made explicit, not a new narrowing.
- **The slot charge** (R2-S1, the reviewer's fix): the (1 + *k*) multiplier, m(*j*), applies to
  slots as to bytes at every site (request, `VolumeCharges`, `MoveReplica`, `ConfirmRelease`,
  growth, tombstone). The design states the arithmetic: pages ≤ 2L + N/E ≤ 2S + N/E, within
  `pool_pairs`. Round 1's named risk "the page pool is not charged" is **withdrawn**, and its
  `docs/followups.md` item ("charging pages", to be filed) is dropped. Tests: slots alone refuse
  with 1 MiB chunks; `layers_within_slots_fit_pool`.
- **#73's `AssignOwner`** (R2-C3, R2-S2, one fix, the reviewer's first option): an assignment of a
  tenant charges (1 + `snapshot_count`) times the size, and #73's owner `bytes` says so; this
  covers snapshots taken before activation and on an unassigned volume once active, which the
  request charges no quota (the other option, refusing an unassigned volume, would not cover the
  first). The last of #51 and #73 to land applies it, with its test in
  `TestRequestSnapshotCharges`.
- **Encrypted-volume refusal's owner** (R2-S3): whichever of #50 and #51 lands second adds the
  refusal and its two test cases. #50 is not edited.
- **The lab check** (R2-C4): run from the host as `resync.sh` is; the `lab.sh snapshot-check`
  subcommand and the make target are new; the check names its own volume and refuses one that
  exists, so cleanup never deletes another volume; both script tests join `make lint`'s list.
- **Nits:** the snapshot-read reason above corrected; CommitSnapshot's response joins #80's
  fixtures; "whichever of three or more lands second" replaced by each part's owner, the last to
  land of the plans it names; the Roles section says RequestSnapshot joins #73's existing methods
  (`inactiveRules`), CommitSnapshot its node rule, ListSnapshots the open reads. None left.
- **Risk named, not accepted:** a request whose batches stop (no room after its check, or a
  crash) leaves a `CHARGING` snapshot, counted and not cut, until the volume's next request
  completes it.

## Plan review round 3 (2026-11-18), changes made after approval

Reviewed commit 4d45c35; findings R3-C1, R3-B2 (Medium), R3-C3 (Low) and two Nits, one
reviewer (design-code and design-security). Each change below was made after approval, in this
round; nothing in it is implemented, measured or approved anew.

- **The cursor at 0** (R3-C1, the reviewer's first option): `snapshot_charge_next` is a
  `*uint32` with `omitempty`, so the mark's cursor of 0 is written and only nil is absent; m(*j*)
  is unchanged. Of the two options, the pointer keeps the field a chunk index, so no site's
  arithmetic changes (index + 1 would shift every comparison). Tests: a stop right after the mark
  followed by a MoveReplica, and a deletion with the snapshot `CHARGING` at cursor 0 and at a
  middle cursor, the ledgers equal to Σ m(*j*) and the tombstone releasing what was charged.
  [contracts.md](contracts.md), Metadata and lifecycle; [design.md](design.md), Catalog.
- **The mark and `AssignOwner`** (R3-B2, the reviewer's fix): once #73 is active the mark always
  compares `owners/<id>` at the revision read, absent included, and `tenant/authorization`, as
  #70's step does: 4 compares for an unassigned volume or a system owner, 6 for a tenant, 3 under
  `inactiveRules`. A racing `AssignOwner` makes the mark read again and charge the quota once
  (an `interleaveKV` hook in `TestRequestSnapshotCharges`). #73 is not edited.
  [contracts.md](contracts.md), Transactions.
- **A batch that finds a node down or a marked ledger** (R3-C3, the reviewer's fix): it answers
  `FailedPrecondition` naming the node, earlier batches kept, the snapshot `CHARGING`; S2's
  sentence names the case, with tests for a report expiring after the check and a ledger marked
  by InitCapacity between batches. [contracts.md](contracts.md), Retention charge;
  [spec.md](spec.md), S2.
- **Completion after the request's own checks** (the contracts Nit, as the reviewer and the
  orchestrator direct): a request completes a `CHARGING` snapshot only once its own checks admit
  it, so a refused request writes nothing; a request naming the `CHARGING` snapshot is checked
  as an answer only (id and name, lifecycle, `expected_first_epoch`), so it can always complete
  it. **A rule narrowed, shown necessary by the Nit:** S2's "completed by the volume's next
  request" now reads "by a later request of the volume that is not refused (the same name's
  included)"; S2's "refused, before any write" and completion before the checks cannot both
  hold, and the refusal is the rule kept.
- **Nit, design S24:** `TestSnapshotCommands` prints no Pending or Charging snapshot as ready.
  None left.
- **Risk named, not accepted** (R3-C3): a batch that finds a node gone down, or its ledger marked
  by InitCapacity, leaves the snapshot `CHARGING` until the node reports again or its copy is
  moved off and `ForgetNode` removes the released copy (#13), or InitCapacity's run ends; meanwhile
  every request of the volume answers `FailedPrecondition`, so the volume takes no snapshot, and its
  count and quota stay held.
  [design.md](design.md), Lab, docs, risks and ADRs.

## Changes after approval — plan review round 4 (2026-11-18)

Nothing of Low or worse; the plan clears. Both Nits were wording, fixed by the orchestrator: the
design's risk now says a later request that is not refused completes a Charging snapshot (as S2
does), and the down-node risk says the copy must be moved off before `ForgetNode` removes the
released copy (#13's `ForgetNode` removes released copies only).
