# Issue #72 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.

Approved with the recommended answer to each open question (see Settled at approval). The
plan's review loop continues after approval; later fixes are recorded as changes made after
approval. Nothing is implemented. Approved before its plan review cleared (status:
addressing_round_2).

- **After approval:** the design review's round-3 fixes, 2026-11-13, came after this approval
  (see Choices settled by the design review, round 3, below).
- **After approval:** the design review's round-4 fixes, 2026-11-13, came after this approval
  too (see Choices settled by the design review, round 4, below).
- **After approval:** the design review's round-5 fixes, 2026-11-13, came after this approval
  too (see Choices settled by the design review, round 5, below).
- **After approval:** the design review's round-6 fixes, 2026-11-13, came after this approval
  too (see Choices settled by the design review, round 6, below).

## Settled at approval

- **Q1, fixed watermarks** (spec.md, Open questions): kept fixed at soft 80 % and hard 95 % in
  this step, until the configurable per-class policy lands.
- **Q3, old metadata-service instances** (spec.md, Open questions): no automatic exclusion; an
  upgrade runs every instance on the new binary, then `capacity-init --all`, whose rows name the
  placements the ledger did not charge.
- **Q4, 116 nodes per create** (spec.md, Open questions): accepted in this step.
- **Q5, capacity in `ListNodes`** (spec.md, Open questions): accepted; every authenticated
  identity sees it, as it sees addresses.
- **Copies no record names** (design.md, Risks): recommended handling taken: delete them before
  `capacity-init`.
- **Per-node capacity ledger charged in the placement transaction** (adr-proposal.md, Proposed):
  accepted with the plan, as proposed.
- **Capacity contracts** (capacity-contracts.md, "All of it remains Proposed"): accepted as
  written.

## Choices settled by the design review, round 1

- **The step (R1-C12):** this plan is the issue's step 1 with the admission half of step 2: a
  per-node ledger, creates refused from soft, moves admitted to hard, on one store per node.
  Device identity and routing, the class operation, grants, local enforcement, the reserve
  certificate, balancing, locality and metrics are cut, not guarded. Every finding on cut text
  is resolved by the cut (report, round 1).
- **No lock (R1-C3, R1-S1, R1-C18):** each placement compares and writes the ledgers it touches
  in its own transaction; a conflict rereads and retries, as every metadata write does.
- **Identity stays #61's (R1-C4, R1-S2, R1-S3, R1-C5):** no `ReplicaRef`, no Hello v2, no ISR
  counting change. Replicas and ISRs stay node lists; `CheckIsrChange` and the vectors are
  unchanged, so no simulator check is owed.
- **Tags (R1-C6, R1-S6):** only `NodeHeartbeatRequest.capacity = 72` and `Node.capacity = 72`,
  plus new messages. No device class field on `CreateVolumeRequest` or `Volume` in this step.
- **No `Volume` or `ChunkMeta` field (R1-C7, R1-S7):** #80's bound is untouched.
- **One admission rule (R1-C9, nit on thresholds):** per node, the ledger and the physical use,
  each plus the charge, within `floor(T × w / 1,000,000)`, and the same for slots; equality
  admitted; worked examples in the spec (restated in round 2, R2-S3).
- **Totals from the store (R1-S8, revised for R2-S6):** only the node's data plane identity may
  send a report; an administrator's or agent's report is refused. The CLI's typed figures are a
  test input that works only with the node's own `io` certificate.
- **Freshness without clocks (R1-S9):** the report is the liveness record's value, so it expires
  with liveness; no new write path.
- **Out-of-space behaviour unchanged (R1-C8, R1-S4, R1-C11):** no `NoSpace` frame, no grants; a
  full replica fails its session as today.
- **A completed move's source (found while fixing R1-C12; replaced in round 2, R2-S3, and
  again in round 3, R3-S1):** see round 3's released copies.
- **Old metadata-service instances (R1-S5):** not excluded by a mechanism in this step. Their
  placements are not refused, but cannot corrupt durability: a node that runs out fails its
  replica as today. The upgrade replaces every instance before `capacity-init`, whose rows name a
  recorded charge above the ledger. Open as Q3.
- **Drain near capacity (S8):** a destination refused for capacity leaves the step's candidates.
- **Lab (R1-C20):** `make capacity-check` on one disk per node, fl-a and fl-b only, metadata-only
  volumes; `scripts/lab.sh meta up` initializes ledgers (round 2).

## Choices settled by the design review, round 2

- **A completed move's source stays charged (R2-S3, R2-C1, R2-S1):** `ChangeIsr` writes no
  ledger, so a double release cannot happen. Q2 is closed: what remains, copies no record names
  from before this change, is a risk with a recommended handling. The record of that copy was
  #72's own `left` in round 2; round 3 replaces it with #13's `released`.
- **Claim charges (R2-S2):** the claim writes its per-node charge to `capacity-claims/<volume>`;
  the last header write deletes it, and deleting a creating volume moves it into the tombstone.
  #76 must keep it (or the same charges from its sealed plan).
- **Exact initialization (R2-C2, R2-S4, R2-S5):** `InitCapacity` marks, snapshots once, and writes
  comparing the mark, which any concurrent confirmation rewrites; `all` recomputes every node, the
  way out of a leaked charge, an older binary's rewrite or an unreadable ledger. Confirmations
  compare the ledger's revision in every state, 0 when absent.
- **Full zones (R2-S7):** nodes without room still count for failure domains; a full zone
  refuses with `RESOURCE_EXHAUSTED` instead of `bestEffort` falling back.
- **Create retries (R2-C4):** the chunk scan runs once per create; each attempt rereads only
  nodes, reports and ledgers.
- **116 nodes (R2-C nit 1):** a margin under the 118 a claim fits, chosen for #76 and #78.
- **Warnings (R2-S nit):** an invalid report warns once per change of state, per instance.

## Choices settled by the design review, round 3 (after the approval)

- **#13's released copies (R3-S1, R3-C2, R3-S6):** the copy a completed move leaves is #13's
  `released` entry, charged until the node confirms deleting it; a move to a released node is
  refused, as #13's S2 says. #72's `left`, `CompletedMove` and free move back are cut. This plan
  is implemented with or after #13's metadata rules, and needs #13's `ConfirmRelease` to apply
  #72's release rule in its own transaction, so it takes 118 chunks at most. A replaced disk's or
  a hand-deleted released copy is released when #13's sweep confirms it absent, which it does only
  with release deletion on (corrected in round 4, R4-S3).
- **Initialization writes only under its own mark (R3-C1, R3-S3):** any rewrite of the mark (a
  release, another call) leaves the node marked for a rerun; the CLI names the nodes still marked
  (R3-S4; exit 2 since round 4); its answer names the volumes holding a claim (R3-C2).
- **Q3's procedure, corrected after approval (R3-S2):** a tombstone an older binary rewrote
  keeps its pending nodes blocked; run `capacity-init --all` again once it is gone. The answer
  settled at approval stands, with this second run.
- **A nil room map has no room (R3-S7).**
- **`meta up` runs `capacity-init` once, for the whole cluster, visibly (R3-C5).**
- **Misreports (R3-S5):** no bound on a node's figures in this step; Risks states what an
  inflated or a zero report does.

## Choices settled by the design review, round 4 (after the approval)

- **The change to #13 is a prerequisite (R4-2, R4-S1):** spec S11 lists all of it and the places
  in #13's text; #72 lands only with or after it. #13's plan is not edited here.
- **The mark keeps the earlier ledger (R4-1, R4-S2):** `prev` in the mark; releases subtract from
  it; a blocked node is restored from it. The CLI exits 2 for nodes still marked, naming for each
  whether to rerun or to clear its blocker. (Round 5 makes every write compare this call's mark.)
- **Hand deletion frees no capacity while #13's release deletion is off (R4-S3).**
- **Fence holders are charged one slot (R4-S4):** in `VolumeCharges`, tombstones and
  initialization; `ConfirmRelease` keeps one slot when it adds the node to `Fenced`.
- **Parallelizable only in part (R4-3):** the Go domain and service units follow #13's rules.

## Choices settled by the design review, round 5 (after the approval)

- **The one rule for initialization and releases (R5-B1, R5-B2):** stated once, in the contracts:
  every write `InitCapacity` makes, restores included, compares this call's own mark, with no
  retry; every release on a mark rewrites it, a mark without `prev` included. So a release during
  a call leaves the node marked for a rerun, and none is lost.
- **`ConfirmDeletion`'s callers stay today's (R5-B3):** whichever identity `auth.go` allows for
  the node (its agent today, its data plane after #13's S12) or an administrator; S11 does not
  depend on #13's S12.
- **A restored blocked node exits 2 (R5-B4)** with a line saying to run `--all` once its blocker
  clears.
- **`previous_*` and `had_ledger` count a mark's `prev` (R5 nit 3).**
- **Growth (cross-plan with #70):** each growth step charges this ledger at soft in the
  transaction that exposes the new chunks; #70 adds it and its test (since #70's round 6, #70
  lands with or after #72; this replaced "whichever of #70 and #72 lands second").

## Choices settled by the design review, round 6 (after the approval)

- **CLI lines (R6-B1, R6-B2):** a blocked target whose restore raced a release (it keeps `prev`)
  gets `raced a release: run it again`; `blocked_by` is set only for the call's targets, so a
  node the call did not touch never makes it exit 2.
- **Growth's design side (R6-B3):** once #70 has landed, its `ExpandVolume` steps charge the
  ledger by #70's "Charges"; #70 adds it with `TestExpandChargesLedger`. Growth leaves the intent's open list. (Changed after #70's round 6:
  #70 lands with or after #72 and adds the charge itself; it has no step-1 refusal to remove.)
