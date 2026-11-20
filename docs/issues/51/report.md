# Issue #51 — report

## Review rounds

### Plan review round 1 — c8908eb

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: design-code focus (51-r1-code.md, R1-C1..C13), design-security focus
(51-r1-security.md, R1-S1..S7).

- **R1-C1 — High:** the snapshot-capable bound used #80's original term, contradicting #13's
  clear bound, and its catalog allowance was not derived; a volume at #13's RF1 cap passed the
  check yet its full reply exceeded 4 MiB. Evidence: design.md Budget, decisions.md;
  origin/main `docs/issues/13/design.md:65-85`.
- **R1-C2 — Medium:** growth (#70) of a volume with snapshots was not bounded, and a chunk
  added after a cut had no defined snapshot content. Evidence: origin/main
  `docs/issues/70/spec.md:32-42`; design.md Budget ("before accepting the first snapshot").
- **R1-C3 — Medium:** #73's hand-offs not taken: no role rows for the three methods, no audit
  or follow-up, no quota charge. Evidence: #73 contracts.md:32-53, spec S4 and S7, decisions.
- **R1-C4 — Medium:** catalog deletion unspecified; a crash between the header's deletion and
  the catalog batches leaks records for good; operations not counted. Evidence:
  `meta/service/service.go:525-531, 575-577`; `meta/kv/kv.go:33`.
- **R1-C5 — Medium:** contracts.md called the bytes exact and accepted although #58 requires a
  coordinated review with #51 and #52 before implementation; #51 defined no completeness
  evidence for #52's `SourceProofV1`. Evidence: `flaminio-58/docs/issues/58/design.md:9`;
  `flaminio-52/docs/issues/52/decisions.md:58-62`; spec S22.
- **R1-C6 — Medium:** S18 and the design's frontier-recheck and stage-fencing rules had no
  injected bug; the early-publication checker was not one of the spec's checks. Evidence:
  spec.md Tests; design.md:109,121,154.
- **R1-C7 — Low:** S12's refusal had no wire reply; contracts misstated `Refused` as
  correlated by `req`. Evidence: `flaminio-net/src/frame.rs:110-113, 309-312`;
  `flaminio-node/src/replica/mod.rs:11-12`; `flaminio-repl/src/msg.rs:83-101`.
- **R1-C8 — Low:** CommitSnapshot's "wrong caller PermissionDenied" contradicts S3: a deposed
  target would not learn it is deposed. Evidence: `meta/service/service.go:794-800`.
- **R1-C9 — Low:** RequestSnapshot writes the header, against S2's "changes nothing else", and
  the catalog was not placed in #77's validated window. Evidence: design.md:21-27; origin/main
  `docs/issues/77/design.md`, Pin Attach.
- **R1-C10 — Low:** `snapshot-read` did not say how it finds chunks or what `--first-epoch`
  checks. Evidence: `flaminio-node/src/lib.rs:21`; `flaminio-io/src/cli.rs:41-48`; #30 S5.
- **R1-C11 — Low:** the encrypted-volume refusal was in the design only; the "incompatible
  profile" refusal had no source. Evidence: design.md:42-43; contracts.md RequestSnapshot.
- **R1-C12 — Low:** the test plan named no tests or files, the lab check no script or target,
  and the commands omitted `make proto meta-vectors hygiene`. Evidence: design.md test table.
- **R1-C13 — Low:** spec.md, design.md and adr-proposal.md called settled choices open.
  Evidence: spec.md:3, design.md:170-171, adr-proposal.md:43.
- **R1-S1 — High:** under #73 the three methods had no role, so any Admin SAN, `audit-reader`
  and `deletion-override` included, could burn a tenant volume's 63 lifetime snapshots.
  Evidence: origin/main `docs/issues/73/contracts.md`, Methods and roles; `auth.go:57`.
- **R1-S2 — High:** retained layers break #72's capacity bound (one chunk size per copy), so
  other volumes on the same disks can fail writes with OutOfSpace. Evidence: `store.rs:1765,
  1790, 1806`; `state.rs:226, 504`; origin/main `docs/issues/72/spec.md:15,36`.
- **R1-S3 — Medium:** #50's refusal of encrypted snapshots was in the design only, with no test.
  Evidence: `flaminio-50/docs/issues/50/contracts.md:166-170`; spec S2.
- **R1-S4 — Medium:** #73's audit hand-off not taken. Evidence: batch-2026-11-12.md:222; #73
  spec S7.
- **R1-S5 — Medium:** frame tag 0x47 (71) collides with #13's `NOT_HELD`. Evidence: origin/main
  `docs/issues/13/design.md:940`, `decisions.md:70`; `frame.rs:130-148`.
- **R1-S6 — Low:** cleanup of snapshot records after a partial DeleteVolume unspecified; reads
  not limited to the current incarnation. Evidence: `service.go:527-533, 575-576`.
- **R1-S7 — Low:** `snapshot-read --first-epoch` not checked against the store, so an earlier
  volume's layers could be read as this one's. Evidence: `replica/mod.rs:193-211`.

Changes/resolution:
- R1-C1: `B_s(N,R) = 4160 + 12549 + 415 + N × (288 + 264 × (R+1) + 132)`, from #13's bound; the
  catalog allowance derived (63 maximal entries of 199 bytes, field 30's 12; contracts, Size),
  #50's 415 kept; caps 4406/3446/2830/2400/2084; checked at every request; #80's fixtures gain the
  catalog, #13's worst chunk and #50's descriptor; whichever of #13, #80, #51 lands second
  applies it (design, Budget; decisions).
- R1-C2: every #70 step of a volume with snapshots checks `B_s` (whichever of #70, #51 lands
  second); a chunk added at generation g is zeros in every snapshot at or below g (S2, S19,
  design, with tests).
- R1-C3, R1-S1, R1-S4, R1-S2 (one fix): #73 rows (RequestSnapshot administrator name, once active
  `cluster-admin`; ListSnapshots an open read; CommitSnapshot the target's data plane), insecure
  behaviour stated, role tests; audit as a `docs/followups.md` entry, issue to be filed; the
  retention charge: a volume with k snapshots charged (1 + k) times by #72's ledger and #73's
  quota, the request charging in its own transaction and refused without room or quota, the
  multiplier in #72's `VolumeCharges` and `MoveReplica`, #13's `ConfirmRelease` and #70's growth
  (whichever lands second applies each). S2 narrowed: copies on more than 114 nodes refused (the
  120-operation bound). The page pool stays uncharged: named, not accepted (spec S2, S4, S6, S24;
  contracts; design, Roles, audit and charges; intent, Out of scope).
- R1-C4, R1-S6 (one fix): the catalog goes after the header's transaction in `deleteChunks`'
  batches conditional on the header being absent; the header-absent branch and the creation's
  claim clear `snapshots/<volume>/`; nothing joins the header's transaction; reads list the
  current incarnation only (S6, contracts Deletion, design Catalog, `TestDeleteClearsCatalog`).
- R1-C5: contracts.md marks the shared bytes pending #58's coordinated review (deferred, not
  settled here); completeness evidence defined (contracts, Completeness evidence; S22).
- R1-C6: mutations `join_before_frontier`, `stale_stage_reply`, and a fourth check,
  publication, for `early_publish` (spec Tests, design table, ADR Confirmation).
- R1-C7: h < t ends the session with `Refused`, no store effect, the target fails the replica
  (S12, contracts, design, `write_below_top_ends_session`).
- R1-C8: only Authorize answers PermissionDenied; another node's `io` or a deposed target gets
  `applied=false` with the current state (S4, contracts, design).
- R1-C9: counters stay in the header, the effect stated (ChangeIsr and MoveReplica retry, #77's
  pinned Attach may answer `Aborted`: named, not accepted), the catalog listed inside #77's
  window (S2, contracts, design, `TestSnapshotRaces`).
- R1-C10, R1-S7 (one fix): the node's `volume_uuid`; a chunk fenced below `--first-epoch`
  refused, #30's rule (contracts, design, `snapshot-read-test.sh`). S7's "above E" is not taken:
  it would refuse the volume's own chunks fenced at E.
- R1-C11, R1-S3 (one fix): refused at request by #50's field 20 or header mark, with a test;
  the "incompatible profile" refusal dropped (S2, contracts, design).
- R1-S5: `Layers` 0x48 (72), `Changes` 0x49 (73), with a decode test (contracts).
- R1-C12: a named test, with its file, per rule and per mutation; `make snapshot-check`
  (`scripts/checks/snapshot.sh`, script test under `make lint`); `make proto meta-vectors` and
  the full pre-PR set (design).
- R1-C13 and the security Nit on design.md's Lab section: reworded as settled (spec, design,
  ADR proposal).
- Nits: #76's creation and #73's deletion ownership named; `<first_epoch>` as 20 zero-padded
  decimal digits; the `snapshot-v2` gate names its callers as #50's does.

Nits left: none.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 2 — da886c5

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: design-code focus (51-r2-code.md), design-security focus (51-r2-security.md).

- **R2-C1 — Medium:** the request's retention charge races MoveReplica: a move to a node with no
  copy of the volume commits between the request's chunk List and its transaction, and the
  destination is charged (1 + k) while the header says k + 1, so the tombstone releases one chunk
  size more than was charged. Evidence: MoveReplica compares the header but never writes it
  (`service.go:908`); #72's charge in it writes only `capacity/<to>`; the request compared only
  the header, the record and its own D ledgers.
- **R2-C2 — Medium:** round 1's 114-node narrowing of S2 is not shown necessary and conflicts
  with #72's admission. Evidence: a Pending request retains nothing (S12), so it can be charged in
  batches, as #72's InitCapacity and #70's steps are; #72 admits creation on 116 nodes
  (`MaxPlacementNodes`, `TestCreateAt116Nodes`); D ≤ 114 holds only with #73's four compares;
  the "no valid report" refusal (a down node, a released copy awaiting ForgetNode) was not stated
  in S2.
- **R2-C3 — Medium:** #73's AssignOwner is missing from the (1 + k) multiplier sites, so a
  volume snapshotted before activation is assigned at its size only. Evidence: #73's owner
  `bytes` is "a tenant's size at its claim or assignment plus every growth step's" (origin/main
  `docs/issues/73/contracts.md:154`); the request charges quota only for a tenant owner once
  active; the plan did not say what a request does for an unassigned volume once active.
- **R2-C4 — Low:** the lab check's invocation was misdescribed and its `VOL` ambiguous. Evidence:
  `resync.sh` has no lab.sh subcommand or make target and runs from the host
  (`resync.sh:6`), generating its own volume name; the design's check took `VOL=` from the caller
  yet its cleanup deletes the volume; the new script tests were not in `make lint`'s explicit
  list (`Makefile:92-94`).
- **R2-S1 — Medium:** the page pool can be bounded with one more multiplier, yet the plan left it
  a named risk, real for chunks below 32 MiB. Evidence: `pool_pairs = ceil(N/E)×2 +
  chunk_slots×2` (format §2.1), one page table per (slot, layer) (§5.4); chunks down to 1 MiB
  (`domain.go:180`); the design said "a layer takes none" of the slots, against #72's premise
  that the pool covers every slot's page tables (origin/main `docs/issues/72/spec.md:27-28`).
- **R2-S2 — Low:** the quota multiplier misses AssignOwner, so snapshots taken while a volume is
  unassigned (before activation, or once active without an owner record) never count against the
  tenant's quota. Evidence: as R2-C3; contracts, Retention charge; #73's contracts:153-155.
- **R2-S3 — Low:** if #51 lands before #50, nobody adds the encrypted-volume refusal. Evidence:
  the refusal relies on #50's field 20 and header mark; #50's clear plan names no RequestSnapshot
  check (`flaminio-50/docs/issues/50/contracts.md:166-170`); no "lands second" clause.
- **Nit (code):** decisions.md's snapshot-read reason was wrong: no chunk is fenced at E (`Start`
  sets the epoch to the first epoch, every Attach adds one, `domain.go:211-219`), so the two
  phrasings are equivalent.
- **Nit (code):** #80's fixtures omitted CommitSnapshot's response (a Volume and a Snapshot of up
  to 198 bytes).
- **Nit (security):** "whichever of three or more plans lands second" leaves the third plan's part
  without an owner.
- **Nit (security):** the Roles section did not say which of #73's Authorize classes the methods
  join (`origin/main docs/issues/73/design.md:45-53`).

Changes/resolution:
- R2-C1, R2-C2 (one fix, the reviewer's): a request checks room before any write, then marks in
  one transaction (record `CHARGING`, count, ordinal, a new header cursor `snapshot_charge_next`
  = 0, and the quota), then charges one chunk size and one slot per named copy in batches by chunk
  index, each comparing the header, its chunk records and its ledgers (2 + b + d ≤ 120) and
  advancing the cursor; the last makes the record `PENDING`. Every charging site uses m(j) = 1 +
  `snapshot_count`, less one for a chunk at or after the cursor, so a racing move is charged once.
  A `CHARGING` snapshot is never cut (CommitSnapshot `applied=false`, the target skips it); a
  batch without room keeps earlier batches (as #70's steps) and the volume's next request
  completes it, as after a crash. Round 1's 114-node refusal is withdrawn (S2, contracts,
  design, decisions). The no-valid-report refusal is stated in S2 and tested. Tests:
  `TestSnapshotRaces` gains the `interleaveKV` move hook; `TestRequestSnapshotCharges` gains
  300 nodes in several batches, a batch without room, completion by a later request;
  `TestCommitSnapshot` refuses a Charging record; the meta vectors gain the charge.
- R2-C3, R2-S2 (one fix, the reviewer's first option): #73's AssignOwner of a tenant charges
  (1 + `snapshot_count`) times the size, and #73's owner `bytes` says so; the last of #51 and #73
  to land applies it, tested in `TestRequestSnapshotCharges`. This covers snapshots taken before
  activation and on unassigned volumes; refusing unassigned volumes would not cover the first.
- R2-C4: the check runs from the host as `resync.sh` does; the `lab.sh snapshot-check`
  subcommand and the make target are named as new; the check generates its volume name, takes no
  `VOL` and refuses before any write if that volume exists; `snapshot-check-test.sh` tests both;
  both new script tests join `make lint`'s explicit list.
- R2-S1 (the reviewer's fix): m(j) applies to slots as to bytes at every site (request,
  `VolumeCharges`, `MoveReplica`, `ConfirmRelease`, growth, tombstone); the design states
  pages ≤ 2L + N/E ≤ 2S + N/E within `pool_pairs`. Round 1's page-pool risk is withdrawn and its
  "charging pages" follow-up (to be filed) dropped; the ADR proposal says so. Tests: slots alone
  refuse with 1 MiB chunks; `layers_within_slots_fit_pool`.
- R2-S3: whichever of #50 and #51 lands second adds the refusal and its two test cases
  (contracts, design). #50 is not edited.
- Nits: all four fixed (the decisions reason corrected; CommitSnapshot's response joins #80's
  fixtures; each part's owner is the last to land of the plans it names; RequestSnapshot joins
  #73's existing methods, CommitSnapshot its node rule, ListSnapshots the open reads).
- Risk named, not accepted: a request whose batches stop leaves a `CHARGING` snapshot, counted and
  not cut, until the volume's next request completes it.

Nits left: none.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 3 — 4d45c35

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: one fresh plan-reviewer, design-code and design-security focuses together.

- **R3-C1 — Medium:** `snapshot_charge_next` = 0 cannot be told from absent as the design
  specified the field, so at cursor 0 R2-C1's mis-charge comes back: a move or growth charges
  1 + k_new and the batch adds one more, and a deletion's tombstone releases 1 + k_new for copies
  charged k_new. Evidence: contracts.md:15-16, 77-78 (present only while charging, written as 0);
  design.md:22-23 ("default0/omitted"); the header's optional fields are `omitempty` scalars
  (`meta/domain/domain.go:47-53`); no test covers cursor 0.
- **R3-B2 — Medium:** on an active store the mark does not compare an unassigned volume's
  absent `owners/<id>`, so an `AssignOwner` racing the mark charges (1 + k) × size while the
  mark raises the count to k + 1 with no quota charge: one snapshot outside the quota for good.
  Evidence: contracts.md:102-105 (compares for a tenant and under `inactiveRules` only);
  #73 design:173-178 (`AssignOwner` never writes the header); #70 design:96-100 (the step
  compares `owners/<id>`, absent included).
- **R3-C3 — Low:** a charge batch that finds a node without a valid report or a readable ledger
  had no stated outcome, and the resulting stall (the snapshot `CHARGING` until the node reports
  again or `ForgetNode`, the volume unable to take a snapshot meanwhile) was not named. Evidence:
  contracts.md:81-85 (only "a batch without room" answered); spec S2 and the design's named risk
  cover only no room or a crash.
- **Nit (contracts.md:48-59):** "refused before any write" conflicted with completing a
  `CHARGING` snapshot before the request's own checks.
- **Nit (design.md:250, S24):** `TestSnapshotCommands` should print no Pending or Charging
  snapshot as ready.

Changes/resolution:
- R3-C1: `snapshot_charge_next` is a `*uint32` with `omitempty` (contracts, Metadata and
  lifecycle; design, Catalog): the mark's cursor of 0 is written, only nil is absent, m(*j*)
  unchanged. Of the reviewer's two options, the pointer keeps the field a chunk index, so no
  site's comparison shifts. `TestRequestSnapshotCharges` gains a stop right after the mark
  followed by a MoveReplica, and a deletion with the snapshot `CHARGING` at cursor 0 and at a
  middle cursor: the ledgers equal Σ m(*j*) and the tombstone releases exactly what was charged.
- R3-B2: once #73 is active the mark always compares `owners/<id>` at the revision read, absent
  included, and `tenant/authorization`, as #70's step does: 4 compares for an unassigned volume
  or a system owner, 6 for a tenant, 3 under `inactiveRules` (contracts, Transactions; design,
  Roles, audit and charges). `TestRequestSnapshotCharges` gains an `interleaveKV` hook after the
  mark's read of `owners/<id>` that assigns the volume: the mark reads again and charges the
  quota once. #73 is not edited.
- R3-C3: a batch that finds a named copy's node without a valid report, or its ledger marked by
  InitCapacity, answers `FailedPrecondition` naming the node, earlier batches kept, the snapshot
  `CHARGING` (contracts, Retention charge; design). S2's sentence names the case. Tests: a
  report expiring after the check and a ledger marked by InitCapacity between batches. Risk
  named, not accepted: the snapshot stays `CHARGING` until the node reports again or
  `ForgetNode` removes its copy (or InitCapacity's run ends); meanwhile every request of the
  volume answers `FailedPrecondition`, and its count and quota stay held (design, risks;
  decisions.md).
- Nit (contracts): completion runs only after the request's own checks admit it; a refused
  request writes nothing; a request naming the `CHARGING` snapshot is checked as an answer only
  (id and name, lifecycle, `expected_first_epoch`) and completes it, with a test. A rule
  narrowed, shown necessary by this Nit: S2's "completed by the volume's next request" now
  reads "by a later request of the volume that is not refused (the same name's included)", since
  it and S2's "refused, before any write" could not both hold (decisions.md, round 3).
- Nit (design S24): reworded as the reviewer asked.

Nits left: none.

Status: needs_incremental_review. The plan was approved on 2026-11-13; these fixes were made after approval.

### Plan review round 4 — 30e0643

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: one fresh plan-reviewer, design-code and design-security focuses together.

Reviewed commit 50df1c3 (diff 4d45c35..30e0643). R3-C1, R3-B2, R3-C3 and both Nits fixed.
Nothing of Low or worse. Two Nits, both wording, fixed in a450dc6: the design's Charging risk now
says "a later request that is not refused" (design.md:292-293), and the down-node risk says the
copy is moved off before `ForgetNode` removes the released copy (design.md:294-295,
decisions.md:209; #13's `ForgetNode` removes released copies only).

Status: clear. Approval remains separate; no approval is inferred.
