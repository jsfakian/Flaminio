# Issue #70 — decisions

## Approvals

- **2026-11-13, the user:** accepted this plan (intent, spec, design) and its
  [ADR](adr-proposal.md), with the recommended answer to each open question (below). Given
  after the round-1 fixes, before an incremental review of them. Nothing is implemented.
- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.
- **After approval:** the fixes of review rounds 2 to 9 came after the approval. They are listed
  under Changed after approval. Settled 2026-11-13 (user's instruction): accepted (see Settled at
  the user's instruction of 2026-11-13).

## Changed after approval

The user's instruction of 2026-11-13 keeps the review loop running after approval. Changes made
since are recorded here. Settled 2026-11-13 (user's instruction): accepted (see Settled at the
user's instruction of 2026-11-13).
- **Round 2** (82b7bec, "Address the review of the design of #70, round 2") came after the
  approval. Besides fixes, it added the node-wide wait below to S6, S7, the design and the ADR's
  Consequences.
- **Round 3** ("Address the review of the design of #70, round 3") came after the approval. It
  takes the node-wide wait out of the accepted risk, states its full scope, and marks it in place.
- **Round 4** ("Address the review of the design of #70, round 4") came after the approval. It
  makes each step charge #72's ledger once #72 lands (spec S2, the design's Capacity charge, the
  intent, a marked note in the ADR), corrects the wait's scope and release paths, and moves the
  lab flag to #75's shared mechanism.
- **Round 5** ("Address the review of the design of #70, round 5") came after the approval. It
  changes the settled answer "Wait for #72? No": growth is now refused until it charges #72's
  ledger (replaced in round 6). It adds #73's quota charge under the same rule, cuts the release
  paths and the narrowing, narrows the wait to its one remaining case, and checks the lab flag
  through each instance.
- **Round 6** ("Address the review of the design of #70, round 6") came after the approval. It
  replaces round 5's refusal with an ordering prerequisite (this plan lands with or after #72, so
  growth always charges the ledger), checks the flag first again, refuses growth of an unassigned
  volume once #73's tenancy is active, names #73's `ExpandVolume` role row, and moves the lab
  probe to #75's per-run ids.
- **Round 7** ("Address the review of the design of #70, round 7") came after the approval. It
  makes every step compare #73's activation and owner records, places the `wrong-kind` check in
  the steps, and records the orchestrator's correction to #72.
- **Round 8** ("Address the review of the design of #70, round 8") came after the approval. It
  moves the race check before each attempt, as #73's rule, settles the outcome of an assignment
  during a call, and gives a context with no role #73's `no-role`.
- **Round 9** ("Address the review of the design of #70, round 9") came after the approval. A
  marked call compares `tenant/authorization` absent, as #73; the `no-role` check moves before
  any read.
- **The node-wide wait** (found after approval; the design's Risks, the one place it is stated).
  Settled 2026-11-13 (user's instruction): accepted as stated (see Settled at the user's
  instruction of 2026-11-13). Growth that does not fit #72's ledger is refused before commit, so
  one case is left: a node's physical chunk slots run out under its ledger. A chunk of the grown
  volume none of whose replicas accepts the session then waits with no deadline; for ext4 the node
  plugin's steps on the device (NodeStage's `blkid -p` and `mount`, NodeExpand's `resize2fs`,
  NodeUnstage's `umount`) wait with it under its lock, so the node's staging stops (with #15,
  other tenants' volumes too). The plan gives no release beyond the operator's investigation.
  Choices: accept it; a daemon status field checked before the node plugin's device steps (a
  contract change); or releasing the lock at the deadline and leaving the child, guarded per
  volume.
- **The changed answer on #72** (rounds 5 and 6). Settled 2026-11-13 (user's instruction):
  accepted (see Settled at the user's instruction of 2026-11-13): this plan lands with or after
  #72, so growth always charges its ledger, in place of "Expansion does not wait for #72".

## Settled at approval

With the recommendations, 2026-11-13; where each is in the plan: the spec's Settled questions,
the design's Risks, the ADR's Decisions settled at approval.

- **Wait for #72?** No. Expansion does not wait for #72's capacity reservation. The availability
  risk is accepted: a node out of chunk slots at the attach after a growth refuses the session,
  and its prefix chunks lose that member or stall at minISR, with no acknowledged write lost. #72
  later reserves the capacity. *Changed after approval (rounds 5, 6).* Settled 2026-11-13 (user's
  instruction): this plan lands with or after #72, so growth always charges its ledger (see
  Settled at the user's instruction of 2026-11-13).
- **Step size:** `growStep = 64` chunks per transaction.
- **Preferred node of suffix chunks:** the volume's last target.
- **Advertising while the flag is off:** the plugin always advertises offline expansion; a
  request while the metadata flag is off gets `FailedPrecondition` naming the flag.
- **Version check:** none automatic. The documented rollout order (every data plane node, then
  the flag) is the gate; a data plane rollback after a growth stays unsupported.

## Choices settled in plan review round 1

- **Narrowed step (R1-C1, R1-C2, R1-S3, R1-S11, R1-S12):** this plan is the offline raw block
  and ext4 step ([intent](intent.md#this-step)). Online resize is cut: a running target cannot
  take new chunks without a new Hello, which the core counts as a member failure. Its rules, the
  `resize` and `resize-status` socket lines and the `UPDATE_SIZE` profile are gone.
- **No pending growth (R1-C2, R1-S3, R1-C6, R1-C10, R1-S4, R1-S13):** growth commits in
  self-contained steps, each one transaction that appends chunks and raises the size. No growth
  record, generation, abort transition or Attach refusal exists; a failed request leaves a smaller
  complete volume. No header field is added, so older metadata writers lose nothing.
- **XFS cut (R1-S1, R1-C3, R1-C12):** XFS needs an XFS profile in #79's initialization contract
  and `xfsprogs` in the image; it is a later step. ext4 growth formats nothing.
- **No use-state protocol (R1-C3, R1-C7, R1-S6, R1-S9, R1-S15):** controller expansion does not
  need the volume offline, because a running target ignores chunks appended after its attach and
  the device takes the new size only when an attach creates it. `VolumeUseState`,
  `ReleaseVolumeUse`, the use fields and their reconciliation are gone, and so are the name and
  tag collisions with #79, #18 and #13.
- **No capability labels or mode (R1-C4, R1-C5, R1-S5, R1-S10, R1-C9):** one metadata flag,
  `--volume-expansion` (default off), gates growth, set after every data plane node is upgraded;
  the plugin advertises OFFLINE only; no automatic version check (settled above).
  `ExpandVolumeRequest` carries `id` and `size_bytes`; CSI alone validates the signed range
  with #80's helper; the `online` field is gone.
- **Drain during growth (R1-S7, R1-C2):** each step compares #78's placement gate, which drain
  marks and registrations advance; #78 is a prerequisite. Nothing is hidden from the drain's scan.
- **Slots exhausted (R1-S2):** the rule is the existing member-failure path, now stated and
  tested. Per-chunk refusal needs a protocol change and pre-creation needs a pending state, which
  this step cut. Expansion does not wait for #72 (settled above).
- **Replica size (R1-S8):** the known size rises only in the Fence arm, after chunk creation and
  the store fence succeed; a Hello below the fence gets `Stale` and changes nothing.
- **Simulator (R1-C8):** the core rule (a target applies views to its attached chunks) gets a
  simulator scenario and the injected bug `drop_grown_view`. Metadata growth changes no epoch,
  ISR, replica set or version of an existing chunk, so the Rust model's vectors do not change; its
  atomicity is proven on real etcd.
- **Legacy volumes (R1-C11):** they grow like any other; the migration is dropped; a test covers it.
- **Lab (R1-C13):** offline needs no 6.6 VM; the check runs on the 7.0 lab VMs through
  `scripts/lab.sh`, listed in Files touched.
- **Prerequisites (R1-S14):** #18, #77, #78, #80, #79 and #15 are declared with the commits read.
- **Decisions link (the code review's nit, R1-S16):** this file now exists.

## Choices settled in plan review round 2

- **Engine refresh (R2-C1, R2-S5):** the change to #80's refresh check is code in
  `flaminio-node/src/engine.rs`, now in Files touched, stated once in the design (Targets, Engine
  refresh): the attached geometry is kept and compared, never replaced; a larger same-epoch reply
  by whole chunks passes, anything else is refused. `a_malformed_refresh_is_refused` tests the
  refusals.
- **Resizer (R2-C2):** the sidecar takes the attacher's args, `securityContext` and socket mount,
  and the lab check waits for it to be Ready.
- **Replica ordering and slots (R2-C3, R2-S1):** slot exhaustion is refused by #80's preflight
  before the core, so the node's fence stays at the old epoch and, for that volume, it is an
  unreachable replica. The Fence arm's failure is left to store faults, tested by
  `a_failed_creation_keeps_the_old_size`. The full-replica test covers the local path.
- **A chunk with no slot on any replica (R2-S1):** the attach succeeds at the larger size, I/O to
  that chunk waits without a deadline, and NodeExpand's `resize2fs` waits with it, holding the
  node plugin's lock. Documented; round 3 corrected its filing: it was not accepted then (Changed
  after approval). Settled 2026-11-13 (user's instruction): accepted as stated in round 5's form
  (see Settled at the user's instruction of 2026-11-13).
- **Test gaps (R2-C4, R2-C5, R2-S2):** a `faultKV` wrapper for the KV outcomes; a named hook for
  `TestExpandSteps`; the missing ControllerExpand and NodeExpand cases, every NodeExpand refusal
  asserting that no tool runs.
- **Simulator (R2-C6):** the sweep's three changes are named, and `Report::suffix_writes` pairs
  with `Report::growths` so a post-growth target is shown to serve a suffix chunk.
- **Lab (R2-S3):** block first, its namespace always deleted; then ext4 and `csi_failover` with a
  6Gi and added-file check; what the start deletes is stated; the trap deletes the namespace.
- **Nits (code nits, R2-S4):** the ADR names the cut option without draft history; the Hello
  citation is split; `Options` is in `auth.go`.
- **Cross-plan:** each step will also charge #72's per-node capacity ledger (dependency only);
  the operator note may describe `--volume-expansion` with #75's `--qos`.

## Choices settled in plan review round 3

- **Accepted versus found later (R3-S1):** the risk the user accepted stays word for word (above;
  the design's Risks). The node-wide wait is listed apart (not accepted then; Settled 2026-11-13
  (user's instruction): accepted, see below), in the design's Risks, the spec's header, the ADR's
  Consequences (marked, with the accepted text restored) and here.
- **Where the wait starts (R3-S2):** for ext4 it starts at NodeStage's `blkid -p`, not mkfs, and
  the volume's NodeUnstage waits behind the same lock. The release path (free slots on the
  refusing nodes, or restart the data plane on the target's node; removed in round 5) goes into
  `README.md`'s upgrade note and `docs/followups.md`; `TestNodeStage` pins the wait.
- **Refresh order (R3-1):** #80's check runs on the raw reply, before any name enters the node
  table; the refusal test asserts no ListNodes call.
- **Lab (R3-2, R3-S3):** `csi_failover` runs in a subshell with `KEEP=1`, then the 6Gi and
  added-file check, then the cleanup; the block pod is in `flaminio-test`.
- **Flag (cross-plan, #75):** `--volume-expansion` follows `--qos`: a second rollout, nothing
  enforcing it, an older binary refusing the unknown flag, and `meta.sh` writing it only with
  `VOLUME_EXPANSION=1`.
- **Nits (code nits, R3-S4):** S9 says refusals name the flag; the simulator sentence says
  `replica/tests.rs` covers the Fence-arm creation; a waiting chunk waits until one of its
  replicas accepts the session.

## Choices settled in plan review round 4

- **Capacity charge (R4-S2):** the growth transaction is this plan's, so it owns #72's charge:
  once #72 lands, each step charges its chunks by #72's admission rule in the same transaction
  and is refused with `RESOURCE_EXHAUSTED` if it does not fit. Whichever of #70 and #72 lands
  second adds it, with `TestExpandChargesLedger`. The narrowing now waits for that charge, not
  for #72 alone, in the design, here, the README note and `docs/followups.md` (removed in round 5;
  round 6 makes #72 an ordering prerequisite instead).
- **Release paths (R4-S1, removed in round 5):** the restart release removes the pod first;
  freeing slots helps only within each refusing node's slot table;
  `a_suffix_chunk_without_a_slot_waits` now frees a slot and sees the waiting I/O complete.
- **Scope (R4-S4, R4-S5):** NodeStage's `mount` waits too when `blkid -p` passes; "has a slot"
  becomes "accepts the session" here and in the ADR.
- **Lab flag (R4 code F1–F3):** #75's design, section "Lab flags of the metadata service (S5,
  shared with #70)" (40cdf3f), is the one mechanism for both flags; whichever of #70 and #75
  lands first adds it, the other its flag and variable. `csi_expand` reads the flags file as
  `qos-check` does; `(KEEP=1 csi_failover) || die`; `scripts/tools/meta-flags-test.sh` covers the
  flag and the refusal.
- **Rollback (R4-S6):** an older metadata binary given the flag exits and restarts in a loop,
  serving nothing; the rollback turns the flag off first. The flag's usage says so, as `--qos`'s.
- **Spec header (R4-S3):** it lists every change made after approval.
- **Nit:** the target's reconnect "succeeds once a replica accepts the session".

## Choices settled in plan review round 5

- **The cause cut (R5-S3, R5-S4, coordinator):** growth is refused until it charges #72's ledger
  (step 1 of Metadata growth; replaced in round 6 by #72 as an ordering prerequisite), so a growth that does not fit is refused before commit. The wait
  is stated once, in the design's Risks, for the one case left, with no release beyond the
  operator's investigation. Release paths (a) and (b) and the narrowing are gone; R5-S3 and R5-S4
  are resolved by the cut.
- **Charges (R5-S1, R5-S2):** one rule, in the design's Charges: each step charges #72's ledger
  and #73's quota in its transaction; whichever plan lands second adds each charge and its test.
  #73's keys count in the 120 bound; "a ResourceQuota until #73" now says what holds.
- **Scope (R5-S6):** NodeUnstage's `umount` is in it.
- **Narrowing text (R5-S7):** resolved by the cut: nothing is left for an operator to wait for,
  so neither the usage nor `meta-flags` help carries it.
- **Lab flag (R5-C1, R5-S5):** #75's rule (4663835): `csi_expand` expands a probe volume through
  each cluster VM's own instance, then deletes it; no flags file is read. This needs the admin
  CLI's new `expand` command.
- **Tests and reads (R5-S8, R5-C2, R5-C3):** `TestExpandChargesLedger` gains slots alone,
  left-out nodes and a preferred node short of room; `TestExpandChargesQuota` is added; step 3
  reads the reports with the nodes, then the ledgers it compares; `TestControllerExpand` passes
  `ResourceExhausted` through.
- **Usage (R5-S9):** "remove it before a metadata rollback; a data plane rollback after a growth
  is unsupported".

## Choices settled in plan review round 6

- **#72 first (R6-B1):** of the two fixes offered, the simpler: this plan lands with or after #72,
  as #72 does for #13, so it is the second lander of #72's rule and adds the ledger charge, and
  growth never runs uncharged. Round 5's refusal is dropped, so the growth tests, the CLI test,
  `make csi-expand`, the advertising and the intent's Done-when work when this plan lands.
- **Flag first (R6-B2):** with the refusal gone, step 1 checks the flag, so a refusal while it is
  off names the flag, as the settled answer and S9 say.
- **Unassigned volumes (R6-S1):** once #73's tenancy is active, a volume with no owner record does
  not grow (`FailedPrecondition` `unassigned`, as #73's DeleteVolume); a system owner record
  charges nothing; the step compares `owners/<id>`. `TestExpandChargesQuota` covers both.
- **#73's role row (R6-S2):** stated once, in the design's Contracts: `| ExpandVolume |
  administrator name | csi-controller or system-provisioner; the method requires the one of the
  owner's kind; an unassigned volume is refused | – |`, added by whichever of #70 and #73 lands
  second, with `auth_test.go` cases. #73's plan must add the row.
- **Probe ids (R6-C1):** `csi-expand-probe-<vm>-<T>-<P>` per #75's rule (597b861; 9083e69 since round 7), deleted by its
  trap rules; the S9 test records w's probe deleted after a failed check.
- **Read order (R6-C2):** step 3 reads #73's owner record, policy and quota key after the
  ledgers.
- **Nit (R6-C3):** the round-3 and round-4 entries mark what round 5 removed.

## Choices settled in plan review round 7

- **#73's activation window (R7-S1):** every step's transaction compares `tenant/authorization`
  and `owners/<id>` at the revisions read, absent included, so an activation or an assignment
  between read and commit makes it read again; a call Authorize marked `inactiveRules` then
  answers `Aborted` (#73's race rule), committed steps kept. Stated once, in step 3 and the Quota
  bullet; `TestExpandChargesQuota` reaches it with `interleaveKV`. (Round 8 moved the check before
  each attempt and limited the read-again case to an activation; round 9 has a marked call
  compare `tenant/authorization` absent.)
- **Owner kind (R7-C1):** step 3 refuses the other role with `PermissionDenied` `wrong-kind`, from
  the owner record the transaction compares. A call authorized before activation cannot keep
  growing after it because of the race check, which answers `Aborted` (corrected in round 8).
  Tested in `TestExpandChargesQuota`.
- **#72's text (R7-C2):** #72's growth bullet and tests row still have the second lander remove
  `ExpandVolume`'s step-1 refusal; the orchestrator is correcting #72's sentence and tests row,
  as there is nothing to remove since #70's round 6.
- **Nits (R7-C3, R7-C4):** #75 is cited at 9083e69; the followups line names only #73's quota
  charge; the round-5 entry marks its refusal as replaced in round 6.

## Choices settled in plan review round 8

- **The race as #73's rule (R8-B1):** `s.active` is checked before each attempt, as #73 does; the
  step's transaction compares `tenant/authorization` and `owners/<id>`. The test calls the handler
  with the `inactiveRules` mark and activates tenancy in a hook after the step's read of
  `owners/<id>`, so the commit fails on the compare; without the compare it would commit
  uncharged and the test fails. (Round 9: the compare of a marked call is "absent", and a second
  hook, after the read of `volumes/v`, tests the earlier window.)
- **Assignment during a call (R8-B2):** one outcome: on an active store, a step that reads no owner
  record answers `unassigned`, even if the volume is assigned after the step's read; a later call
  charges the new owner (worded so in round 9). Spec S2, step 3 and the test say the same; the Quota bullet no
  longer claims an assignment makes the step read again.
- **No role (R8-C1):** a context with neither the mark nor a role answers `PermissionDenied`
  `no-role`, active or not, as #73; `wrong-kind` is only for the other role.

## Choices settled in plan review round 9

- **The marked call's compare (R9-B1):** a call marked `inactiveRules` compares
  `tenant/authorization` absent, as #73's race rule; a call with a role compares it at the
  revision read. An activation after the race check and before the step's read then fails the
  commit, and the next attempt answers `Aborted`; `TestExpandChargesQuota` reaches that window
  with a hook after the read of `volumes/v`.
- **`no-role` first (R9-C1):** step 1 refuses a context with neither the mark nor a role before
  any read, so even a volume already at the requested size answers `no-role`.
- **Nits:** `unassigned` ends the call ("a later call"); the spec header's Tests list stays
  "rounds 2 to 7".

## Settled at the user's instruction of 2026-11-13

The user decided on 2026-11-13: "For all open questions go with the recommendations."
- **The node-wide wait** (found after approval; the design's Risks): a node's physical chunk slots
  running out under its #72 ledger can leave a chunk of a grown volume unserved, and the node
  plugin's device steps wait with it, with no release beyond the operator's investigation.
  Answer: accepted as the design's Risks state it. Accepted under this instruction (the plan's
  current text; the alternatives it lists, a daemon status field or releasing the lock at the
  deadline, carry no recommendation).
- **The changed answer on #72** (rounds 5 and 6): in place of "Expansion does not wait for #72",
  this plan lands with or after #72 and growth always charges #72's ledger. Answer: accepted.
  Accepted under this instruction (the plan's current text).
- **The other changes made after approval** (rounds 2 to 9, listed under Changed after approval):
  accepted as the plan now states them. Accepted under this instruction (the plan's current text).
