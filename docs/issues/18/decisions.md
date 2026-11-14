# Issue #18 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.
- Plan review round 1 (2026-11-14) fixes, made after approval: see below.
- Plan review round 2 (2026-11-14) fixes, made after approval: see below.
- Plan review round 3 (2026-11-14) fixes, made after approval: see below.
- Plan review round 4 (2026-11-14) fixes, made after approval: see below.
- Plan review round 5 (2026-11-14) fixes, made after approval: see below.
- Plan review round 6 (2026-11-14) fixes, made after approval: see below.

Approved with the recommended answer to each open question (see Settled at approval). The plan's
review loop continues after approval; later fixes are recorded as changes made after approval.
Nothing is implemented. Approved before its plan review cleared (status: pending).

## Settled at approval

- **Q1. What a `strict` drain does with a chunk it cannot move:** (b), skip it, move the rest,
  report it, and fail once only blocked chunks are left; also for a chunk with no eligible node
  at all, which changes today's result for that case. `spec.md`, Open questions Q1 and S11–S13;
  `design.md`, Risks (Open, Q1).
- **Q2. A `bestEffort` move named by hand:** (a), the service accepts it as today; only the
  drain prefers the higher level. `spec.md`, Open questions Q2 and S6; `design.md`, Risks
  (Open, Q2).
- **Q3. Volumes that exist already:** (a) now, they read as `node`, `strict`; (b), an
  administrator's command to set a volume's level and policy, later, if such volumes exist
  outside the lab. `spec.md`, Open questions Q3 and S3; `design.md`, Risks (Open, Q3).
- **Q4. The rule in the Rust model, its vectors and the simulator:** Go only, with the property
  test as the injected bug's check. `spec.md`, Open questions Q4; `design.md`, Risks (Open, Q4)
  and ADRs.
- **Q5. Mixed versions of the metadata service:** (a), accepted: every instance runs the new
  version before a `rack` or `zone` volume is created or moved (`docs/lab.md`). `spec.md`, Open
  questions Q5; `design.md`, Risks (Open, Q5). Changed after approval (R1-S2, below): no
  rollback below this version once a `rack` or `zone` volume exists.

## Changed after approval: plan review round 1 (2026-11-14)

Made after approval, in round 1 of this batch's plan review (reviewed commit 1287453), each with
its finding. No rule is narrowed beyond what its finding shows; none is approved again here.

- **#13's released nodes in the drain's choice** (R1-C1, High; R1-S4, Medium; the
  orchestrator's decision, #13's hand-off in its decisions, "Hand-offs to #45 and #18"). Due in
  whichever of #13 and #18 lands second. `choose` follows: a chunk at #13's bound waits first;
  eligible nodes exclude the chunk's replicas and its `GetReleased()`; the domain filter runs on
  what is left; with no node left, a chunk that a releasing candidate would pass waits, with
  #13's reason `only nodes releasing it are left (N)` in `Progress.Held`, and is not counted as
  blocked; otherwise it is blocked. Spec: Terms (Eligible, Blocked chunk), Tests; design: the
  drain, Tests (two drain tests), Lab (`DELETE_RELEASED=1` once #13 lands, the deletions by #13's
  sweeps instead of `vol-confirm`). Reason: without it the drain proposes a releasing node at
  every step until its time-out; with a naive exclusion a chunk #13 says waits ends the drain.
- **The collision refusal is told by a fixed text; other refusals follow #72's rule** (R1-C2,
  High; R1-S3, Medium; the orchestrator's decision). The refusal keeps `FailedPrecondition`
  (S7, unchanged in class) and its message starts with the fixed text `failure domain
  collision: `; the drain tells a collision by the code and that text. Chosen over #13's
  `MoveReplicaResponse.reason` (field 3) because the clear plans that share this code on
  `MoveReplica`, #72 and #78, identify their refusals by stated texts, because `reason` rides on
  `applied=false`, which S7's failed precondition excludes, and because it would tie #18 to
  #13's landing order: the simplest safe option, with no new field or dependency. A collision
  blocks that chunk only; any other `FailedPrecondition` and `ResourceExhausted` (#72's
  destination without a report, a ledger or room; #78's draining destination) take that
  destination out of the step for every chunk, `choose` runs again within the same rules, and
  with none left the chunk is blocked, not a failed step (S11, S14). `Run`: `BlockedError` when
  `Remaining > 0` and only blocked, unrefused chunks are left, checked before #78's
  `CheckDrain`, which alone then certifies success. S15 and the intent no longer say a draining
  or dead destination named by hand is accepted: #78 refuses a draining one and #72 a dead one
  or one without a capacity report; #18 adds neither check. Contracts name the other causes of
  `FailedPrecondition`. The combined compare count is stated: at most 9 compares, 2 operations,
  under 120. Spec: S7, S13, S14, S15, Tests; intent: Out of scope; design: Accepting a move, the
  drain, Contracts, Tests (an interceptor's `FailedPrecondition` without the text: not refused).
- **`Volume` fields 18 and 19** (R1-C3, Medium; R1-S5, Medium; the orchestrator's decision).
  `failure_domain = 18`, `failure_domain_policy = 19`, in the proto and, should `pb.rs` ever name
  them, its tags. 9 and 10 are #79's, 11–14 #70's, 15 #13's, 16–17 #17's. Design: Contracts,
  Tests (Targets).
- **#17's drain** (R1-C4, Medium; the orchestrator's decision). Whichever of #17 and #18 lands
  second: `choose` is #17's seam; a volume not held, with no move pending and a blocked chunk,
  is #17's `waiting` with why `no destination (LEVEL/POLICY)`, #18's own reason, not #13's;
  `Run` holds #17's `Drainer` and checks `BlockedError` after `Drainer.Step`, before #78's
  `CheckDrain`; `Progress.String()` puts #18's part right after the counts, not relative to the
  `Unattached` #17 removes. Spec: S12, Tests; design: the drain, Tests.
- **A stored word the drain does not know blocks that volume's chunks** (R1-C5, Low; R1-S6,
  Low). The step no longer fails: the volume's chunks on the node without a pending move are
  blocked, with the words as read (shown with `%q`), the other volumes move, and `BlockedError`
  names it. The reviewer's first option, which S11 already asks for. Spec: S11, S12, Tests;
  design: the drain, Tests.
- **The fall-back level's test** (R1-C6, Low): a `zone`/`bestEffort` volume placed at `rack`
  reports `zone`, `bestEffort` after create, get and attach. Spec and design: Tests.
- **`Run`'s condition, worded** (R1-C7, Nit): a volume waiting for a target keeps the drain
  going only through a pending move; one whose chunks are all blocked ends it, as `TestDrain`'s
  `v2`. S13's "or volumes wait for a target" said the same ambiguously and is worded likewise (a
  clarification, not a change of rule). Spec: S13; design: the drain, Tests.
- **Unknown domains fail closed under `strict`** (R1-S1, High; the reviewer's fix). At `rack` or
  `zone`, for a chunk with a staying replica, a `strict` move is refused when the destination
  has no label at that level or an empty one, or when a staying replica has none, has no node
  record, or has one that does not parse. An unlabelled node is so no domain of its own under
  `strict`: a new node, which registers before its labels arrive, takes no `strict` replica
  until it is labelled (by a move or a drain; at creation too only since round 2, R2-B1, and for
  chunks of two replicas or more), and an empty label no longer reads as a fresh zone. A chunk
  with one replica still takes any destination, and `bestEffort` keeps unlabelled nodes as one
  shared domain: the finding shows neither unsafe, so neither is narrowed. The drain's `strict`
  choice uses the same rule (`domain.Strict`). The lab gives fl-c the zone `lab-c` for step 3.
  Intent: What we want 2; spec: Terms (Unknown at a level), the invariant, S5, S7, S8, S10, Tests;
  design: Accepting a move, the drain, Tests, Lab, Risks.
- **No rollback below this version once a `rack` or `zone` volume exists** (R1-S2, Medium;
  changes Q5). A rollback is a mixed version: an N-1 instance's attach, from an agent's restart
  or a CSI publish, would drop the level and policy for good. Of the reviewer's two options this
  is the one that keeps an N-1 instance from dropping them; the other only documents the loss,
  and its remedy, Q3(b), does not exist yet. (Round 2, R2-S1: it keeps them only as an operating
  rule; nothing enforces it.) Before such a volume exists, rollback stays as
  ARCHITECTURE §18 says. Stated in `docs/ARCHITECTURE.md` §18 and `docs/lab.md`. Spec: Q5, S16;
  design: Docs, Files touched, Risks.
- **Who may set a node's labels, stated in full** (R1-S7, Low; the reviewer's first option).
  Every administrator identity for any node (the CSI controller's `admin/csi-controller` as well
  as the operator's), the node's `agent`, and the node's `io` for its own node (`auth.go:71-72`),
  though the data plane sends none. This is the trust the approved plan already gave ("labels
  are trusted, as at creation"): creation trusts the same writers, and the change widens
  nothing. Refusing label changes from `io` is left to its own issue, not filed by this plan.
  Design: Risks.
- **Nits** (security review): the compare count is 7 at most, not "8 (5 replicas)": the chunk,
  the header, the destination and at most 4 staying replicas; the refusal formats every id and
  label value with `%q`, so a label holding `"` or a newline cannot forge the text the lab greps.
  Design: facts, Accepting a move, Tests.

## Changed after approval: plan review round 2 (2026-11-14)

Made after approval, in round 2 of this batch's plan review (reviewed commit 339601b), each with
its finding. None is approved again here.

- **Creation fails closed like moves** (R2-B1, Medium; the coordinator's decision, in place of
  the reviewer's rewording). Under `strict` at `rack` or `zone`, a volume of two replicas or more
  gets no replica on a node unknown at that level: `Place` leaves such nodes out first, and a
  topology short of labelled domains is refused as today, its message counting the nodes left
  out. This widens the intent's "Creation: `Place` and its rules stay as they are" by that one
  rule (intent, Out of scope; spec S5, S15). One-replica volumes keep today's placement, as S5
  lets a one-replica chunk move anywhere: nothing to spread, and the two stay consistent; so the
  claim "a new node takes no `strict` replica until labelled" holds for chunks of two replicas
  or more, and the docs say it so. No chunk of a volume that records `rack` or `zone` was placed
  before the rule: record and rule ship together, and a volume without the record reads as
  `node` (S3). A chunk has a replica on an unknown node only when that node loses its label (its
  `agent` or `io` may clear it) or its record after placement: that replica may move; the others
  cannot, by hand, by a drain or by #45's rebuild, until the node is labelled again or its
  replica moved first. Risks names it, with a node's power to cause it and the remedy, as a
  risk named and not accepted (round 3, R3-S1).
  `BlockedVolume` gains `Why`, printed by `String()` and `BlockedError`, so such a block names the
  replica (`replica "x" has no zone label`; `Strict` now names an unknown staying replica first).
  Hand-off to #45's S15, due in whichever lands second. Tests: `TestPlaceStrictLeavesOutUnlabelled`
  and a service test of a node registered with an address only.
- **A drain test for an unknown staying replica** (R2-C1, Medium): a node of the chunk set to the
  empty zone, and again with its record deleted, the other drained: no `MoveReplica` request,
  blocked and not refused, `Run` returns `BlockedError`. It fails for a drain that checks staying
  replicas with `Collision`.
- **The drain's marker is the service's constant** (R2-C2, Low): the drain matches
  `domain.ErrCollision.Error() + ": "` as the prefix of `status.Convert(err).Message()`, and one
  test relabels the destination through the real service just before its `MoveReplica`.
- **The last refusal is reported** (R2-C3, Low; the reviewer's per-volume option, one mechanism
  with R2-B1's reason): a chunk blocked by #72's rule has the last refusal's message as its
  volume's `Why`, in `String()` and `BlockedError`; the existing test asserts it.
- **The lab waits for fl-c's releases before step 4** (R2-C4, Low): once #13 lands, 60 s at most
  until `metactl releases fl-c` is empty, else exit 1 naming fl-c; otherwise the hand move gets
  `applied=false` and exit 0. Step 4's expected stderr now carries the volume's reason.
- **The no-rollback rule is documented, not enforced** (R2-S1, Low): an operating rule that
  nothing enforces, since an older instance cannot know it. `docs/ARCHITECTURE.md` §18 and
  `docs/lab.md` give the command to check whether it applies (`volumes`, then `get` each, kept
  when `failureDomain` is `rack` or `zone`), say a rollback anyway strips the level and policy of
  every volume attached meanwhile, silently and for good, and that recreating the volume is the
  only remedy (Q3(b) does not exist). §19's rollback row: for this release, creating the first
  `rack` or `zone` volume acts as the finalize step. Risks names the silent loss as a risk named,
  not accepted; Q5's accepted part stays the upgrade-time drop. No mechanism is added: none
  exists cheaply, since the older instance is the one that would have to refuse.
- **Nits:** the invariant says "a move of a chunk with a staying replica" (R2-N1), as S5 and
  `Strict` allow a one-replica chunk any destination. `Strict` returns the bare reason, and only
  the service builds the message, once, `fmt.Errorf("%w: volume %q chunk %d: %s",
  domain.ErrCollision, …)`, so the prefix comes once and first (R2-N2).

## Changed after approval: plan review round 3 (2026-11-14)

Made after approval, in round 3 of this batch's plan review (reviewed commit c6c7373), each with
its finding. None is approved again here.

- **A blocked chunk's reason, in one order** (R3-C1, Low; the reviewer's precedence): the first
  that applies of (1) the chunk's own refusal in the step, the last one; (2) words the drain
  does not know; (3) no candidate without a replica, `no eligible node`; (4) every candidate
  taken out by this step's refusals, the step's last such refusal; (5) `Strict`'s reason for the
  least loaded candidate left (under `strict` only). Each blocked chunk so gets exactly one
  reason, and the tests name the case they check; a test of case 4 is added (a later chunk whose
  only usable node an earlier refusal took out). Spec S12, Tests; design: the drain, Tests.
- **The property test fits the creation rule** (R3-C2, Low): volumes are created only where
  enough labelled domains exist, and the test checks then that no replica is on an unknown node;
  between moves it clears random nodes' labels to the empty value and restores some, so the
  "beside an unknown node" clause can fail; distinct domains are checked among labelled replicas.
  A second test-only switch, `skipStayingCheck`, is a service that forgets an unknown staying
  replica: only the "beside" clause catches it. The S5–S7 cases build an absent label, a deleted
  or an unparsable record by writing the store, as `RegisterNode` merges labels. Spec Tests;
  design: Accepting a move (the injected bugs), Tests.
- **The creation refusal names its level's label, and composes with #72** (R3-C3, Low):
  `domain.LeftOut` gives `; N nodes without a zone label left out`, `rack label` at `rack`;
  `createVolume` appends it last to a placement refusal, so with #72 it follows #72's own
  suffixes on either create error, room refusal included. `TestPlaceStrictLeavesOutUnlabelled`
  asserts both levels. #72's text needs the change, in whichever of #72 and #18 lands second.
  Spec S5, Tests; design: Creation, Contracts, Tests. (Narrowed in round 4, R4-3: the suffix
  goes on the refusal for too few labelled domains only, not on #72's room refusal.)
- **The creation change is documented where users see it** (R3-B1, Low):
  `CreateVolumeRequest.failure_domain_policy` and `prefer_node` comments, ARCHITECTURE §8.2's
  policy row, follow-ups §4 step 2, and a `docs/lab.md` upgrade note (label every storage node
  before upgrading where `rack` or `zone` `strict` classes are in use); Risks' Compatibility says
  such classes start refusing after the upgrade. Spec S16; design: Contracts, Docs, Risks.
- **The lost-label block is named, not accepted** (R3-S1, Low): listed under "Named, not
  accepted" in Risks, beside the rollback loss, and said so in the R2-B1 entry above.
- **`BlockedError`'s example quotes as #72 does** (R3-N1, Nit): a refusal's message is used
  verbatim, so #72's `node d has no current capacity report` appears unquoted; #18's own reasons
  keep `%q`.

## Changed after approval: plan review round 4 (2026-11-14)

Made after approval, in round 4 of this batch's plan review (reviewed commit 4048b5d), each with
its finding. None is approved again here.

- **Reasons 1 and 4 name the right thing** (R4-1, Low; the reviewer's ordering fix and the
  coordinator's wording). Reason 1, the chunk's own last refusal, applies only when `Strict`
  refuses no node still left to the chunk (Left: its candidates less those refused for it or
  taken out this step); otherwise reason 5 gives `Strict`'s reason, so a refusal does not hide
  it. A collision is not followed by a new choice, so nodes that pass can remain: then reason 1
  still applies. Reason 4 no longer repeats another chunk's message, which names that chunk: it
  reads `every candidate refused earlier in this step: <node>: <message>; …`, this chunk's own
  candidates with the refusal that took each out. Tests: a refused chunk whose other candidate
  `Strict` refuses (reason 5); the reason-4 test, for a later chunk of the same and of a later
  volume, asserts the exact text. Spec S12, Tests; design: the drain, Tests.
- **The property test reaches the unknown-node case for certain** (R4-2, Low): it clears only
  the label at a volume's level, on a node holding one of that volume's replicas (a label of the
  other level changes nothing for it); its budget is the fixed seeds 1–64, each a forced step
  and 40 random steps; the forced step clears the level's label of a chunk's first replica's
  node and moves its second replica by hand to a labelled node in a domain the chunk does not
  use, which the generator guarantees (one labelled domain more than the replicas). The check
  refuses it; `skipCollisionCheck` and `skipStayingCheck` each accept it on every seed. Spec
  Tests; design: Tests.
- **`LeftOut` is precise and shares `Place`'s predicate** (R4-3, Low; the coordinator's choice of
  no suffix on unrelated refusals, over the reviewer's "state that this is accepted"): one
  unexported predicate, `leftOut`, serves `Place`'s filter and `LeftOut`, with a table test;
  `Place`'s live-node check runs on all nodes, before the filter. `LeftOut` is non-empty only for
  the refusal too few labelled domains cause (some node left out, enough nodes in all, too few
  distinct domains among those kept). `createVolume` appends it only when `errors.Is(err,
  domain.ErrPlacement)` and it is non-empty, re-wrapping with `%w` so `errStatus` still maps it
  to `FailedPrecondition`. A refusal for too few live nodes, and #72's `ErrCapacity` room
  refusal, get no suffix; #72's short-domain refusal stays `ErrPlacement` and gets it after
  #72's own suffixes. The round-3 hand-off to #72 is corrected so. Spec S5, Tests; design:
  Creation, Contracts, Tests.
- **The property test's claim gives its reason** (R4-N1, Nit): clearing and restoring never put
  two labelled replicas of a chunk in one domain because, while a node is cleared, S5 refuses
  every move of its chunks' other replicas and every move to it; when its former label returns,
  no replica of those chunks has entered that domain. The argument rests on S5, and the design
  says so beside the claim.

## Changed after approval: plan review round 5 (2026-11-14)

Made after approval, in round 5 of this batch's plan review (reviewed commit b71f1c6), each with
its finding. None is approved again here.

- **Every seed admits a volume, and the test says the forced step ran** (R5-C1, Medium): the
  creation condition was a filter, so a seed's topology could admit no volume, and the injected
  bugs would pass on it. The generator now gives three nodes distinct labelled zones and racks
  first, so the seed's first volume (`strict`, 2 replicas, at the seed's level) always has
  replicas + 1 labelled domains and is created, and then draws the other nodes at random. The
  test asserts on every seed that the forced step ran on that volume's chunk 0 and was refused,
  and, under each injected switch, accepted with the "beside" clause failing. "Caught on every
  seed" (round 4's entry, spec Tests) is so true. Spec Tests; design: Tests.
- **`TestLeftOutPredicate` checks what it can see** (R5-C2, Low): "`Place` leaves out exactly
  the nodes `LeftOut` counts" could not be asserted, since `Place` returns only the placement
  and `LeftOut` is empty whenever `Place` succeeds. Three assertions per row instead: `leftOut`
  per node against the table; `LeftOut` non-empty exactly when `Place` returns `ErrPlacement`
  with some node left out, at least `replicas` nodes in all and fewer than `replicas` domains
  among those kept; and its count equal to the nodes `leftOut` names. Design: Tests.

## Changed after approval: plan review round 6 (2026-11-14)

Made after the approval of 2026-11-13, in round 6 of this batch's plan review of commit `1e5ec2d`
(reviewer: plan-reviewer (design-code and design-security, one agent)). No rule is narrowed.

- **R6-B1 (Low):** the forced step's destination: the chunk holds two of the first three nodes, so
  at least one of the three is outside it (not "always the third"). Design Tests.
- **R6-B2 (Low):** the forced step asserts the refusal itself: `FailedPrecondition`, the message
  starting with `domain.ErrCollision.Error() + ": "` and naming the cleared replica's node, so
  another refusal or a no-op cannot pass. Design Tests.
