# Issue #13: Replicas left behind by moves — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-05; reopened the same day for the protocol
  change (user decision).
- **Spec (reopened, with `NotHeld`):** approved by Ioannis Sfakianakis, 2026-11-05.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.
- Plan review round 1 (2026-11-14) fixes, made after approval: see below.
- Plan review round 2 (2026-11-14) fixes, made after approval: see below.
- Plan review round 3 (2026-11-14) fixes, made after approval: see below.
- Plan review round 4 (2026-11-14) fixes, made after approval: see below.
- Plan review round 5 (2026-11-14) fixes, made after approval: see below.
- Plan review round 6 (2026-11-14) fixes, made after approval: see below.
- Plan review round 7 (2026-11-14) fixes, made after approval: see below.
- Plan review round 8 (2026-11-14) fix, made after approval: see below.

Approved with the recommended answer to each open question (see Settled at approval). The plan's
review loop continues after approval; later fixes are recorded as changes made after approval.
Nothing is implemented. Approved before its plan review cleared (status: pending).

Settled in plan review rounds 1–4. Reviewers treat these as settled. Facts corrected on
2026-11-05, after #14's step 2, #30, #10 and #11 landed (marked "corrected"). Entries replaced
after spec review round 1 of 2026-11-05, or by the protocol change, are marked "superseded",
with what replaces them.

## Settled at approval

- None: `spec.md` (Open questions: "None."), `design.md` and this file leave no open question,
  and no choice is labelled Proposed. The idle-chunk question of spec review 2 was already closed
  by the protocol change (Review, below).

## Changes after approval: plan review round 1 (2026-11-14)

Made after the approval of 2026-11-13, in round 1 of the batch's plan review (reviewed commit
7defc59; reviewers: plan-reviewer (design-code), plan-reviewer (design-security)), taking each
reviewer's recommended fix. Nothing here is accepted by the user yet; the costs named are for
review.

- **S2 gets its simulator check and injected bug** (R1-C1, R1-S1, High). The simulator deleted
  a source at the drop, so no release was ever pending and the model's refusal never ran. Now:
  S7's decision is a sans-I/O function in `flaminio-repl` (`release::may_delete`), which the
  node's sweep and the simulator both call; the simulator's sources keep their copy until a
  modelled sweep reads the model (0–5 s after the release) and deletes 0–6 s after its read, as
  `may_delete` allows, then confirms; random moves go on meanwhile, and already pick former
  sources. Injected bug `move_while_released` (the model's `MoveReplica` ignores `released`; the
  security review's `move_to_released`, renamed since S2 now refuses every move of the chunk),
  caught by the scripted `scenario_move_back_before_deletion` (RF 1: a move back inside the
  window, then a new target's raising `Hello` reads 0) and sought in random runs by
  `catches_move_while_released` on `Config::releases` (RF 1 or 2). That search follows the hint
  rule (Choices): none found within 4 000 000 seeds, the scripted scenario stays the catch and
  the PR says so. Spec: the tests' Simulator bullet. No rule narrowed.
- **#72's change to `ConfirmRelease`, as #72's S11 states it** (R1-C2 Medium, R1-S2 Low). 1 to
  118 chunks, and the sweep confirms in batches of 118. The transaction also compares the
  node's ledger `/flaminio/v1/capacity/<node>` (revision 0 if absent) and writes it by #72's
  release rule (n chunk sizes and n slots less, one slot fewer when it adds the node to
  `Fenced`; absent or unreadable, compared and not written): header, 118 chunks and the ledger
  make 120 compares and at most 120 writes, `kv.MaxTxnOps`. A failed compare with every chunk
  still at its listed revision (the header or the ledger moved) is retried. Tests: 119 refused
  and 118 accepted, on etcd too; an engine test confirms 119 released chunks in two batches.
  The ledger read is the service's work within the sweep's one call: #13's bound on a node's
  metadata reads (S6, S17) is unchanged. #72 lands with or after #13; until then no ledger
  exists, and the rule's absent case is all of it. The spec is unchanged (#72's S11 says so).
- **`Volume.fenced` is field 15** (R1-C3, Medium; the orchestrator's decision: #17 keeps 9, #13
  takes the lowest `Volume` number no plan of the batch claims). Claimed in the batch's plans:
  9 and 10 (#17 `held`, `maintenance`; #18 `failure_domain`, `failure_domain_policy`; #79
  `use_profile`, `initialization`), 11–14 (#70's, as #75's design lists them; #70's own design
  now adds no `Volume` field, but #75's text and the code review still count them taken), 20–21
  (#50), 30–31 (#51), 40 (#75) and 40–41 (#54). 15 is the lowest left. `ChunkMeta.released = 6`,
  the new messages and frame tag 71 stay (free, per the review).
- **One `Request::Hello`, and the reply to a `Hello` at the fence** (R1-C4, Medium; #86's
  required-row rule, which #86 leaves to #13). `Request::Hello { chunks, shape }`: `chunks` as
  #86 defines it (ascending, computed in `open_session`, kept in `Session.hello_chunks`),
  `shape` #13's. The `Fenced` reply has a row per chunk held; a named chunk without one is not
  held there: the target gives it nothing from the reply (no `c.fenced`, `first_fence` or
  uncertainty) and opens the session, and its first request there gets `NotHeld` (S17). #86's
  validator reads a required id with no row so, all its other checks kept. Not chosen: an
  explicit not-held row, a change to `Fenced`'s wire form, which #86 keeps. Tests: a target test
  and an engine test of a reconnect naming a deleted chunk, under #86's validator once both have
  landed. Spec: S9 (one sentence) and the S17/S18 tests bullet. #86's text needs the matching
  change (its validator's "all required ids present", and its simulator `Replica` creating
  every listed chunk it lacks), not made here.
- **Bounds on `released` and `fenced`** (R1-C5, Medium; #80's 4 MiB budget). R1-C5 shows the
  unbounded lists cannot hold within it (four RF 1 chunks moving with an earlier release
  overflow #80's largest volume), so two rules narrow:
  - **`released`: one release a chunk, and no move with it.** S2 refuses any move of a chunk
    holding a release, not only one back; the drain waits on such a chunk (S16), and
    `leastLoaded`'s exclusion of released nodes goes (a chunk without a release has none). It
    fits #80's per-chunk term as it stands: 2R + 1 names, against the 2R + 2 names and two move
    endpoints #80 counts. Cost: a chunk holding a release moves nowhere until its node deletes
    (release deletion off, the node down, or gone for good until forgetting a node, a
    follow-up). Not chosen: two releases a chunk, 132 B more a chunk in #80's term, its caps
    down 7–14 % (RF 1: 4422, not 5137). **Replaced in round 2** (R2-B1, below): S2 restored, the
    bound raised to two releases a chunk with #80's term +132 B, a reason given, and a node
    forgotten; that cost is replaced by a bounded one.
  - **`fenced`: 16 at most** (`MaxFenceHolders`). A confirmation that would add a 17th confirms
    and does not add it, so #72's rule keeps no slot for it. #80's constant gains 16 × 132 =
    2112 B: `B(N,R) = 4160 + N × (288 + 264 × (R+1))`, caps 5134, 3879, 3117, 2605, 2238 for RF 1
    to 5 (from 5137, 3881, 3119, 2607, 2239); round 2 adds 132 B to the per-chunk term, for the
    final caps below. Cost: the 17th node's fence-only slot is a copy no
    record names (S8 lists it; #30's drop takes it if the name returns), of the kind of the
    accepted risk "copies no record names", which does not name this case.
  - **#80's fixtures and admission** gain a full `fenced` and a release on every chunk, and the
    new constant; whichever of #80 and #13 lands second makes the change, which #80's text
    needs too (not made here).
  - Spec: the term, S2, S3, S11, S16 and the S16 tests. Intent: "Forgetting a node".
- **One `GetVolume` constructor** (R1-C6, Low; #75's `enforces_qos`). `get_volume_call` builds
  the attach's, the refresh's, the sweep's and the lookups' requests, so #75's flag, set there,
  covers all four whichever lands second; with #75, an engine test covers a limited volume's
  sweep and lookups.
- **`ConfirmRelease`'s abuse cases tested** (R1-S3, Low): a chunk released only to another
  node, one released to nobody, a batch of one released and one not (nothing written, `fenced`
  unchanged, no log line), and a chunk at or beyond the count, in memory and on etcd.
- **Nits.** R1-C7 and R1-S N2: `node.sh list` checks `VOLUME` with `volume_id` only when it is
  set. R1-C8: `ConfirmRelease` checks the chunk range after `readHeader`, as `ChangeIsr` does.
  R1-C9 and R1-S N1: the term "fence holder" now reads "a node the volume's record names as
  having confirmed a release of it, which keeps its fence once it has no chunk". R1-C10:
  `end_chaos` clears `one_way` as it clears `down_links`.

## Changes after approval: plan review round 2 (2026-11-14)

Made after the approval of 2026-11-13, in round 2 of the batch's plan review (reviewed commit
93ed53b; reviewer: plan-reviewer (design-code and design-security, one agent)). R2-B1 follows
the orchestrator's decision, which the user's instruction allows ("otherwise figure it out");
the others take the reviewer's fix. Nothing here is accepted by the user yet.

- **S2 restored, its bound worked out from #80, and forgetting a node** (R2-B1, High). Round 1's
  S2 refused every move of a chunk holding a release, and only the released node ended one: with
  release deletion off (the default) every moved chunk, and with a node gone for good every
  chunk moved off it, could never move again, so drains and #45's rebuild stopped. The
  orchestrator's decision, and why: the batch never accepts a durability loss on the user's
  behalf, so (1) S2 again refuses only a move back to a node that releases the chunk, bounded
  differently, and (2) forgetting a node comes into this plan, the reviewer's first option, so
  that a release can end without its node.
  - **The bound, worked out from #80's budget:** #80's per-chunk term, `288 + 264 × (R+1)`,
    holds a chunk in a move (R + 1 replicas, R + 1 in the ISR, two endpoints: `538 + 262R` B
    encoded) with 16 B (RF 1) to 24 B (RF 5) to spare, under the 131 B one release takes: as it
    stands, it admits no release beside a move, so one release a chunk and no move with it, and
    the decision's "a move elsewhere is allowed while releases are pending" cannot hold. The
    first round-2 commit said so and stopped at one.
  - **One move beside a release: taken, the orchestrator's decision** (second round-2 commit),
    made under the user's instruction "otherwise figure it out". Reason: the batch never
    accepts a durability loss on the user's behalf, and a smaller per-volume cap costs
    capacity, not data. #80's per-chunk term becomes `288 + 264 × (R+1) + 132`, which holds a
    chunk in a move beside one release (`669 + 262R` B against `684 + 264R`). So a chunk holds
    at most one release during a move and two at rest (each release is made by a drop, so a
    third would need a move beside two). A move of a chunk holding two is refused, `reason`
    `holding 2 releases of chunk C` (`MoveReplicaResponse.reason`, field 3), shown in the
    drain's `Progress.Held` with the two nodes; a move back to a node holding a release is
    refused by S2, `node T releases chunk C`, and the drain, which no longer offers released
    nodes, says `only nodes releasing it are left` when none other is free.
  - **#80's caps, final** (with round 1's `fenced` constant): `B(N,R) = 4160 + N × (288 + 264 ×
    (R+1) + 132)` ≤ 4 MiB, so N at most 4419, 3457, 2838, 2408, 2090 for RF 1 to 5 (#80's today:
    5137, 3881, 3119, 2607, 2239). Whichever of #13 and #80 lands second changes #80's constant
    (2048 to 4160), its per-chunk term (+132 B) and its worst-view fixtures (a full `fenced`,
    every chunk in a move beside one release); #80's text needs the change (not made here).
  - **Forgetting a node (S19):** `ForgetNode`, administrators only. It marks the node first (the
    key `forgotten`, for good, with `drain/<node>`). After the mark, `CheckMove`, placement and
    `RegisterNode` refuse it, its `io`'s `ListReleases` and `ConfirmRelease` are refused, and a
    drop no longer releases it. Then it takes the node out of every `released` (batches of 118,
    the header compared), `fenced` and tombstone, in conditional transactions within 120
    operations; a volume being created is named for a later run (round 4: a volume being
    deleted is left to its tombstone, R4-C1). Safe because the
    hazard S2 guards is a move back to the released node, and after the mark none commits
    (`MoveReplica` compares the mark). Tests: each refusal, the clearing, a racing move back and
    a racing confirmation; the vectors' `forget` op; the model's `forget`, with the injected bug
    `forget_without_mark` that `scenario_move_back_after_forgetting` catches. No node is
    forgotten until every metadata instance is new (S14).
  - **With release deletion off** (the default), a chunk moves twice, then a drain waits on it,
    giving its reason, and it never moves back to a node holding its release; `docs/lab.md`
    says a drain needs release deletion on wherever releases are held. `make drain-test`'s move
    back (`drain "$VM_C"`) moves chunks fl-b released, and those whose only free node is fl-b
    would wait for good (S2), so it now requires `DELETE_RELEASED=1` and dies without it; its
    lab check already ran with it.
  - **The remaining cost, bounded** (replacing round 1's "a chunk whose released node never
    deletes cannot be moved"): a chunk never moves back to a node that releases it, and a chunk
    holding two releases moves again only once one of those nodes deletes (a sweep, with release
    deletion on) or is forgotten; with deletion off there, after two moves. **The durability
    residual:** a chunk rebuilt off two dead nodes, neither yet forgotten, holds both releases,
    so a third loss of its replicas is not rebuilt until an administrator forgets one of them.
    One dead node no longer blocks a rebuild. Not accepted: named for the user and the
    orchestrator. #45 closes it for any number of dead nodes if it forgets a node when it
    declares it failed for good (#45 takes that in its own review). **Changed in round 3**
    (R3-B1, below): a second residual named beside it, forgetting offered for nodes gone for
    good only, and the #45 clause made a change #45 needs.
  - **Consistency checked:** S3 (unchanged; a forgotten node's confirmation is refused by S19),
    S5 (a forgotten node no longer keeps the id taken), S11 (a node gone for good, until
    forgotten), S14 (forgetting only once every instance is new; a drain with deletion off), S16
    (the drain waits at the bound, or with only released nodes left, with its reason). Spec: the
    invariant, a term, S2, S5, S11, S12, S14, S16, S19 (new) and the tests. Intent: what we want
    8; "Forgetting a node" leaves Out of scope.
- **`TestFenceHoldersAtMost16` checks the ledger** (R2-C1, Low), with #72, in whichever of #13
  and #72 lands second: two confirmations by the 17th node each credit n chunk sizes and n
  slots, no slot kept, and a rerun of #72's `InitCapacity` leaves that ledger as it is.
- **The S3 replay case can be built** (R2-N1, Nit): the release confirmed, the chunk moved back
  and dropped again, then the first confirmation replayed at its old revision: `Aborted`, and
  the new release stays.

## Changes after approval: plan review round 3 (2026-11-14)

Made after the approval of 2026-11-13, in round 3 of the batch's plan review (reviewed commit
cd3e193; reviewer: plan-reviewer (design-code and design-security, one agent)), taking the
reviewer's recommended fix for each finding. Nothing here is accepted by the user yet.

- **A second durability residual, and no forgetting of live nodes** (R3-B1, Medium). With
  release deletion off (the default), two routine drains of live nodes leave a chunk at the
  bound (RF 2 on {A, B}: A drained to C, C to D; the chunk on {B, D} holds {A, C}); if B then
  dies, its rebuild is refused, and forgetting A or C would retire a healthy node for good. Two
  residuals are now named, both not accepted, for the user: (1) a chunk rebuilt off two dead
  nodes, neither forgotten, is not rebuilt after a third loss until one is forgotten; (2) with
  release deletion off, a chunk drained off two live nodes is not rebuilt after a loss until one
  of them runs with release deletion on, a cure open once every metadata instance and node is
  new (S14; "and node" added in round 4, R4-B1).
  S14, S2 and S19, the design's drain and security lines and `docs/lab.md` offer forgetting only
  for a node gone for good, and name release deletion on as the cure for a live one. The #45
  clause is now a change #45 needs: #45's Failed is automatic and reversible, so it could close
  residual (1) only by adding an administrator's "failed for good" declaration that forgets.
- **No tombstone written after the mark names a forgotten node** (R3-B2, Medium).
  `DeleteVolume` reads the mark and leaves forgotten nodes off the tombstone (replicas and move
  endpoints as well as releases and fence holds); its tombstone transaction compares the header
  and the mark; a tombstone left with no node is not written. Safe: no move or placement brings
  the id back to a forgotten node, so what is left on it is of the kind "copies no record
  names". The invariant, S5 (now also S19's "no tombstone written names it") and the accepted
  risk "a node gone for good" agree. Tests: `TestForgetNodeClears` deletes a volume with a
  replica on the node after the forgetting; `TestForgetRacesADeletion`.
- **`make drain-test` cleans fl-c first** (R3-C1, Low). A running fl-c keeps the setting it was
  started with (`ensure_node_c` starts it only when inactive). Before the first drain,
  `cmd_drain_test` restarts fl-c's node through `node_env` and waits, 60 s at most, until
  `metactl releases fl-c` is empty; `lab-deletion-test.sh`'s case checks it.
- **Hand-offs to #45 and #18** (R3-C2, Low), due in whichever plan lands second, as #80's is:
  - **#45:** its S15 gains the reasons `holding 2 releases of chunk C` and `only nodes
    releasing it are left`, and a chunk's released nodes are not eligible destinations; and,
    for residual (1), an administrator's "failed for good" declaration that forgets (R3-B1).
    #45's text needs the change (not made here).
  - **#18:** `choose`, which replaces the drain's `leastLoaded`, excludes the chunk's
    `GetReleased()` with its replicas. #18's text needs the change (not made here).
  - **#72 and #78:** round 4 adds theirs (R4-C3, below).
- **Race tests name their interleavings; step 4 lists again** (R3-C3, Low). `ForgetNode`'s
  step 4 lists `volumes/` again after every step 3. `TestForgetRacesAMoveBack`: `ForgetNode` in
  a hook on `MoveReplica`'s read of `forgotten`, which `MoveReplica` reads before the chunk;
  it fails with the compare removed. `TestForgetRacesAConfirmation`: a `ConfirmRelease` that
  read the mark before step 2 commits in a hook on `ForgetNode`'s `chunks/` `List`; it fails if
  step 4 works from step 3's listing. Each race test fails without the mechanism it tests.
- **Placement compares the mark** (R3-C4, Low). `createVolume` reads the mark before the nodes,
  skips a node the mark names, and its claim compares the mark; a claim failing with the volume
  and tombstone keys still absent places again, up to 32 attempts (#72's loop), then `Aborted`.
  S19's "no volume is placed on it" then holds against a race. Test:
  `TestForgetRacesAPlacement`.
- **Nits.** R3-N1: a confirmation that fails its chunk compares after a run rewrote them gets
  `Aborted` (step 5 reads again from step 2), not `FailedPrecondition`. R3-N2: `RegisterNode`
  checks the mark before its no-change return, the usual path of a restarting node; the test
  covers an unchanged re-registration.

## Changes after approval: plan review round 4 (2026-11-14)

Made after the approval of 2026-11-13, in round 4 of the batch's plan review (reviewed commit
e350277; reviewer: plan-reviewer (design-code and design-security, one agent)), taking the
reviewer's recommended fix for each finding. Nothing here is accepted by the user yet.

- **A volume being deleted needs no later run** (R4-C1, Medium). Since round 3 a tombstone
  written after the mark never names the node, and a deletion's records go with it, so no later
  run has anything of it to clear, and an abandoned deletion would have been named in `pending`
  for good. `ForgetNode`'s steps 3 and 4 skip a `deleting` volume without naming it; `pending`
  names only volumes being created. S19, the design's step text, the proto comment and the
  `docs/lab.md` line say so; `TestForgetNodeClears`'s case is now a volume being deleted at the
  run, not named in `pending`, whose tombstone, once written, does not name the node.
- **Each race test sits in its compare's interleaving** (R4-C2, Low).
  `TestForgetRacesAPlacement` hooks on `createVolume`'s `List` of `drain/`, the last read of
  `nodes()`, so the creation read the nodes before the mark and its first placement
  (`PreferNode` the node) uses it; it asserts no chunk names the node, and fails without the
  claim's compare. `TestForgetRacesADeletion`: the node is a replica only, so `ForgetNode`
  writes nothing of the volume; `DeleteVolume` reads the mark after its deleting mark and
  before listing the chunks (the design says so).
- **The claim's 120, and the hand-offs to #72 and #78** (R4-C3, Low), due in whichever plan
  lands second, as #80's, #45's and #18's are:
  - **#78** (the reviewer's first option, "cut before adding"): `ForgetNode`'s mark ensures
    #78's placement gate, and writes the drain marker and the gate only when the marker is
    absent, comparing its absence, as #78's own mark does for a new marker; an existing
    `drain/<node>` is left alone, so a running drain's generation is unchanged (round 5,
    R5-C2). #78's gate compare in the claim and in `MoveReplica` then covers the forgetting, and
    #13's own mark compare in both goes. #78's text needs the change (not made here).
  - **Until #78 lands,** #13's mark compare stays, and nothing else is needed: it holds the
    place #78's gate compare would hold, and the two never stand together. With #72's
    `MaxPlacementNodes` 116, #72's claim carries 2 + 1 + 116 = 119 compares, and #76's
    admission, which omits the gate without #78 (#76's design), 3 + 1 + 116 = 120. Of the two
    ways offered, "#13 lands after #78" would bind the landing order and "`MaxPlacementNodes`
    115 until then" would cut placement for nothing; this needs neither.
  - **#72:** the claim already compares the mark and retries, up to 32 attempts, when #72
    lands after #13: #72's loop is that one, with its ledgers added (its text calls today's
    claim "never retried"). `VolumeCharges` for the tombstone leaves forgotten nodes out, and
    `ForgetNode`'s tombstone clearing drops the node's `charges` entry. #72's text needs the
    change (not made here).
- **Residual (2)'s cure has S14's precondition** (R4-B1, Low): "once every metadata instance
  and node is new (S14)", in the design's drain line and Risks and here; the older release
  deletion entry aligned too.
- **Nits.** R4-N1: the `forgotten` `Get` is the first read of each attempt, inside the loop
  (`MoveReplica` reads `nodes/<to>` once before it, `service.go:870`), so a retry reads the
  mark again; `createVolume` likewise, in the loop it gains (round 5, R5-N2), while
  `DeleteVolume` reads it in each attempt after its deleting mark and before listing the chunks.
  R4-N2: step 3 reads `volumes/`, then
  `chunks/`, and `TestForgetRacesAConfirmation` hooks on step 3's `volumes/` `List`.

## Changes after approval: plan review round 5 (2026-11-14)

Made after the approval of 2026-11-13, in round 5 of the batch's plan review (reviewed commit
1de5da6; reviewer: plan-reviewer (design-code and design-security, one agent)), taking the
reviewer's recommended fix for each finding. Nothing here is accepted by the user yet.

- **The move-back race hooks on the header read** (R5-C1, Low). With #78, a hook on the move's
  `Get` of `forgotten` let the move see the node draining (the mark's drain marker) and be
  refused whatever the gate compare did. `TestForgetRacesAMoveBack` now hooks `ForgetNode` on
  the move's header `Get` (`readHeader`), the first read after every destination check: without
  #78 only the mark compare stops the move, with #78 only the gate compare does, and the test
  fails with that compare removed. Its fixture is stated: the node released by the chunk, not a
  fence holder, with no drain marker.
- **Forgetting keeps a running drain** (R5-C2, Low). `ForgetNode`'s mark writes `drain/<node>`
  only when it is absent, comparing its revision; an existing marker is left as it is, its
  drain generation unchanged. With #78 it writes #78's gate only with a new marker (3 compares,
  3 writes), as #78's own mark does; with an existing one, 2 compares and 1 write. So forgetting
  a node whose drain is running, the cure for residual (1), does not fail that drain with "drain
  generation is no longer current". The #78 hand-off line above says so; new test
  `TestForgetKeepsARunningDrain`.
- **Nits.** R5-N1: `TestForgetRacesAConfirmation` uses two stacked `interleaveKV` wrappers, one
  hook each: the outer on the `Get` of `forgotten`, the inner on the `List` of `volumes/`.
  R5-N2: `createVolume` gains a retry loop, which it has none of today: up to 32 attempts, each
  reading the mark first, then the tombstone check, `nodes()`, the candidates, `Place` and the
  claim, the chunk scan read once before the loop; the round-4 "likewise" now places
  `DeleteVolume`'s mark read after its deleting mark and before listing the chunks.

## Changes after approval: plan review round 6 (2026-11-14)

Made after the approval of 2026-11-13, in round 6 of the batch's plan review (reviewed commit
b7f686c; reviewer: plan-reviewer (design-code and design-security, one agent)), taking the
reviewer's recommended fix for each finding. Nothing here is accepted by the user yet.

- **The new `createVolume` loop is tested** (R6-L1, Low). `TestForgetRacesAPlacement` now
  states its outcome: the first claim fails, the second attempt skips the node and claims, so
  `CreateVolume` succeeds with exactly two claims and no chunk naming the node. New
  `TestCreateVolumeRetries`: a mark that moves before every claim gives `Aborted` after 32
  claims; a taken id, or a tombstone written between the tombstone check and the claim, gives
  `ErrExists` after one claim; a tombstone already there, `ErrExists` before any claim; an
  interrupted deletion, `Aborted` as today.
- **How a failed claim is judged** (R6-L2, Low). After a failed claim the volume key and the
  tombstone key are read again, a fresh `Get` each. The deleting-header branch comes first, as
  today (`service.go:424`); then either key present is `ErrExists` at once; both absent means
  the mark (with #72, or a ledger) moved, and the next attempt places again.
- **Nit.** R6-N1: `ForgetNode`'s step 2 reads the mark and `drain/<node>` (with #78, the gate)
  before its transaction compares them.

## Changes after approval: plan review round 7 (2026-11-14)

Made after the approval of 2026-11-13, in round 7 of the batch's plan review (reviewed commit
37808a0; reviewer: plan-reviewer (design-code and design-security, one agent)), taking the
reviewer's recommended fix for each finding. Nothing here is accepted by the user yet.

- **Claims are counted by a helper of their own** (R7-L1, Low). `interleaveKV.txns` counts
  every write of its key, so a successful creation would count three (two claims and the last
  header write). New test helper `claimCounter`: it counts the `TxnRev` calls whose compares
  hold the volume key at revision 0 and whose ops put it, whatever their result.
  `TestForgetRacesAPlacement` ("exactly two claims") and `TestCreateVolumeRetries` ("32
  claims", "after one claim") use it.
- **Nit.** R7-N1: step 2's `Txn` compares the gate too, with #78, as its reads say.

## Changes after approval: plan review round 8 (2026-11-14)

Made after the approval of 2026-11-13, in round 8 of the batch's plan review (reviewed commit
0a89e15; reviewer: plan-reviewer (design-code and design-security, one agent)).

- **Nit.** R8-N1: step 2's compares read "the mark's revision, `drain/<node>`'s and, with #78,
  the gate's", and the stub line left by the round-7 reflow is gone. Wording only.

## Choices

- **Releases live in the chunk record** (`released`), written by the drop's own put: atomic, no
  extra etcd operation, no new key.
- **A move to a released node is refused as other moves are** (`Applied: false`), not with an
  error: the drain already handles it. Changed after approval (round 1, R1-C5) to any move of a
  chunk holding a release; restored in round 2 (R2-B1, below), with S2's bound beside it: a
  move of a chunk holding two releases is refused too, now with a `reason`.
- **The tombstone names released nodes and fence holders:** the id stays taken until they
  delete. Not for adoption, which #30's drop prevents wherever a new volume of the name goes,
  but because that drop almost never runs: Kubernetes names never repeat (the CSI controller
  creates with `Id: req.GetName()`, `control/csi/controller.go`). Without it, a release still
  held at the deletion has no deleter (the sweep's read gets `NotFound`), and fence-only slots
  leak out of the bounded `chunk_slots` (`flaminio-format/src/layout.rs`) until `create_chunk`
  fails with `OutOfSpace`. The cost, an id held until those nodes confirm, is small because ids
  are not reused (spec review 1: the S5 open question, resolved as "keep").
- **A confirmation is conditional on the revisions listed,** all or nothing: a late one never
  removes a newer release.
- **Superseded: "only the node's own agent confirms releases", "the agent is the only
  sweeper", "a new certificate component, `sweep`", "one agent process per node (`flock`) and
  one `delete-chunks` per volume".** Reason: ADR 0001: the sweep lives in the Rust node daemon
  (user decision, 2026-11-05).
- **The sweep runs in the node daemon** (`flaminio-io node`), on the engine's thread, through
  its metadata client: releases and tombstones (issue #14's step 1, taken in). One loop orders
  tombstone confirmations against release deletions. Administrators only see releases: a
  second confirmer is what lets a deletion hit a live or new chunk.
- **Its identity is the node's data plane's, `io`** (`.../node/<node>/io`, the daemon's
  `io.crt`). The release methods and the tombstone methods accept only the named node's `io`
  (administrators keep the tombstone methods, as today); `agent` loses the tombstone methods.
  The CSI node pod mounts only `agent.crt` and its key (`deploy/csi/driver.yaml`), so it cannot
  confirm, and an old CSI sweep is refused. No new certificate.
- **One sweeper per node without a lock:** one data plane opens a block device (`O_EXCL`,
  `flaminio-uring/src/uring.rs`). For a regular file `O_EXCL` without `O_CREAT` excludes
  nothing, so a file-backed store, used only in tests, has no such guard; the lab's nodes use
  block devices (`data_disk`, `scripts/guest/common.sh`). Corrected in spec review 2. One release
  deletion per volume at a time, and the same loop confirms only after it ends; a restart
  forgets both, so nothing can act after a later confirmation.
- **The reply is `NotHeld`** (renamed from `Released`, reopened spec review 1): "released" already
  names the record's state.
- **A replica answers `NotHeld` for a chunk it lacks** (spec S17), after reading the volume's
  record, when the record does not name it as a replica. Reason: the protocol change (user decision,
  2026-11-05).
  - **Replicas only, not `moving`** (reopened spec review 1, L1): a move back requested after the
    confirmation bumps no version (`MoveReplica`, `service.go`), and would leave a cut-off proposer
    with no answer; the target's own check (S18) is what keeps it safe.
  - **The reply carries the volume's epoch and the chunk's ISR version, replicas and ISR:**
    S18 compares all four with the target's open proposals.
  - **No memory of deletions:** a restart loses nothing. Not chosen: persisting the released
    versions (in the fence-only record, or a per-chunk record): a format change and state to
    clean up, while a read is an error path. Out-of-range requests are dropped unread.
  - **One such read per node in flight, besides the sweep's call:** a `ChangeIsr` waits behind at
    most those two. Waiting requests join their volume's read, holding only session, request, chunk
    and kind, never a write's data (the target resends it), within a node-wide bound, volumes served
    in turn; the rest are dropped as today (reopened spec review 1, L3). Its own slot, not the
    sweep's: the target's request timeout (3 s) is short.
  - **Time budget** (reopened spec review 1, M4): the metadata client allows 3 s per endpoint
    (`node.rs`), the same as the target's `request_timeout`, and shares its queue with the sweep,
    refreshes, `ChangeIsr` and heartbeats. A read slower than the timeout fails the session; the
    target resends on reconnect (0.5 s, `retransmit`, `target.rs`), and the resend is answered from
    the volume's last completed read if it is under 3 s old and gives `NotHeld`. Safe: S18 refuses
    anything but the target's own proposal. That read keeps only the requested chunks' parts
    (reopened spec review 2), and of those only the chunks whose record does not name the node: an
    answer that names it is not cached (design review 1, code 5). A volume gone (`NotFound`) gets
    no `NotHeld`: the request is dropped.
  - **Bounds per volume and per node** (design review 1, security F3): at most 256 waiters per
    volume within the node's 4096, so one session cannot starve other volumes; a volume found
    gone is remembered for 3 s, its requests dropped unread meanwhile, so it is not read every
    turn. The spec's "holding session, request, chunk and kind only" became "holding no payload",
    to keep its length; the design keeps the four fields.
- **A target accepts `NotHeld` only as one of its open proposals** (spec S18): the record equals
  one, epoch and ISR version included. Replaces "its view without the sender" (reopened spec
  review 1, M2): `send_meta` (`target.rs`) builds a drop's ISR as ISR minus pending removals plus
  the joiner minus the source, so a drop can carry a removal; relaxing the shape rule would let a
  liar quote a smaller ISR. Reason: the protocol change (user decision, 2026-11-05).
  - **The record does not name the sender** (design review 1, security F1, code 4). `send_meta`
    also opens proposals that remove other members or add a joiner (`target.rs:2433-2440`);
    quoting one of those, a member could have the target drop an honest, failed replica the
    service still counts in the ISR, which `apply_view` never undoes. Every honest `NotHeld`
    meets the rule (S17), so it costs nothing.
  - **And keeps every other ISR member** (design review 2, security N1): the record's ISR holds
    every member of the view's ISR but the sender; a joiner added is fine (the target already
    counts it for leases and writes). Otherwise the source could quote its own drop that also
    removes a failed member Z, uncommitted, and the target would acknowledge writes without Z
    while the service still counts Z. A drop carrying a pending removal is therefore refused.
    `send_meta` bundles the removal into the drop's `IsrChange`, so when the answer was lost both
    committed; the target learns them with its next view, and until then its session to the
    source fails on each `NotHeld`, as today, and reads of that source's chunks wait. Nothing is
    lost (design review 3, code). Leases: after adoption the ISR is the old one less the sender,
    plus at most the joiner, all already required.
  - **Open proposals** (reopened spec review 3; superseded: "the kept proposal", reopened spec
    review 2, which kept one record per chunk until the view reached it, and so both outlived known
    outcomes and refused a rebuilt proposal that committed). `send_meta` rebuilds proposals from
    chunk state on each send, and `meta_inflight` is cleared at `meta_timeout` (`target.rs`), so a
    first proposal lost after the timeout can be rebuilt differently at the same `expected_ver`, and
    either may have committed. Open: every record the target sent in a `ChangeIsr` for the chunk at
    its view's version, at most 4 distinct records per chunk, the oldest evicted (reopened spec
    review 4: the cap costs liveness only, a failed session as today). An identical record (same
    epoch, version, ISR and replicas) is kept once (reopened spec review 5): `send_meta` rebuilds
    and resends the same record after each `meta_timeout` (`target.rs`), and counting each resend
    would let four timeouts evict an earlier, distinct proposal that may have committed; a timeout
    closes none. Closed (reopened spec review 4, precise): a seen reply, applied or rejected, closes
    its own request's proposal for each chunk that request named, not other requests' (an earlier
    timed-out request at the same expected version may still commit, unseen); the view passing that
    version closes all the chunk's proposals; a dropped change (`send_meta` clearing
    `add_replica`/`drop_replica`, a move cancelled) closes its own. A reply is seen only for the
    current `meta_inflight` (`on_meta_inner`, `target.rs`): a timed-out request's reply is never
    seen, so its proposals stay open until the view moves on, which is fine. A rejected proposal
    whose reply is seen is closed (round 3). Nothing new resends until the outcome is known; today's
    resend after a timeout stays, and an identical resend adds no record. A committed proposal
    closed unseen (a dropped change that committed late) is refused: a failure, as today.
  - **A `NotHeld` from a sender the view no longer names** (reopened spec review 4): fails the
    request on that member, session kept, adopting nothing, whatever the record says: the view
    already dropped the sender (`Applied` or a refresh first), and `apply_view` only forgets writes'
    acknowledgements, so a read, digest or `Mapped` probe would otherwise hang or fail the session.
    Safe, since the record is ignored.
  - **Superseded: "Authentication off (`TrustDomain` empty, development): the target never
    accepts `NotHeld`"** (reopened spec review 2). Authentication is always on for a data plane,
    by construction: `flaminio-io node` refuses an empty `--trust-domain` (`node.rs:210-216`),
    `flaminio-meta serve` requires one unless `--insecure` (`main.go:111-113`), and the metadata
    client speaks only mutual TLS (`flaminio-metaclient/src/lib.rs:85-95`), so it cannot reach an
    `--insecure` service. No switch turns adoption off (design review 1, security F5, code 3).
  - **ISRVer:** only `ApplyIsrChange` bumps it (`meta/domain/domain.go`), only through
    `ChangeIsr`, which accepts only the header's target node as `io` at the header's epoch,
    with authentication on (`service.go`, `auth.go`; with `TrustDomain` empty the target check
    is skipped). `MoveReplica`, the drain and a confirmation (S3) bump nothing; an attach
    changes the epoch; a volume created again restarts versions at 0 above every earlier epoch
    (#30). So the epoch is part of the match.
  - **The split read** (reopened spec review 1, M1): `readVolume` reads the header, then the chunks,
    apart, so a record may pair epoch E with a later target's chunk. Chosen: no service change; S18
    is as safe as a refresh, which reads the same way and adopts any record, resting on fencing and
    leases (the `Attach` comment, `service.go`). With the proposal match, S18 adopts a subset of
    what a refresh would: a later target's change identical to the proposal. The simulator models
    the split read. Not chosen: one-revision reads, a service change for no gain over a refresh.
  - **Failing the request on that member** (reopened spec review 1, H1): reads and digests
    requeued, resync rounds aborted, writes per #11 S11. Today only `fail_peer` does that, and
    `apply_view`/`adopt_replicas` only forget writes' acknowledgements. When it applies is the
    entry above, on a `NotHeld` from a sender the view no longer names; it replaces the earlier
    "known records" entry.
- **A simulator check** (reopened spec review 1, H2; reopened spec review 3): honest replicas and
  the atomic metadata model never produce a bad record, so the check injects faults on the reply
  path and splits relayed reads. One forging node sends version+1 records with another ISR,
  another epoch, and rejected or dropped exact proposals, never an open one. One invariant: every
  record a target adopts from `NotHeld` was committed by the model at that version (at the target's
  epoch, or later with the target deposed). The injected bugs and the shape that trips each:
  ignoring the epoch (an identical record at another epoch); accepting any record at view+1 at its
  epoch, without the open-proposal match (version+1 with another ISR); leaving proposals open after
  a reply or a drop (the rejected or dropped exact proposal). Each adopts a record never committed.
  Not kept: a second invariant ("equals a proposal it sent"), since every uncommitted record a bug
  can adopt already breaks the first, and a committed one adopted without a match is true
  information (superseded: reopened spec review 2's two invariants).
- **Release deletion also waits for every node to be new;** `NotHeld` itself is sent whatever
  the option, so during the rollout an old target fails its session on it at once (the
  decoder's unknown frame type, `flaminio-net` `frame.rs`), as a dropped request's timeout
  would later.
- **No ADR is contradicted.** ADR 0005 (replica-confirmed leases): the target's ISR still comes
  only from committed records, here relayed; its consequence that only chunks losing a replica
  during a metadata outage wait is kept, which the protocol change restores. ADR 0001, 0002,
  0006: untouched.
- **The sweep has at most one metadata call outstanding,** enqueues none while one is
  unanswered, and reads per volume, not per chunk (spec review 2). It shares the metadata
  client's one thread and FIFO queue (`flaminio-metaclient/src/lib.rs`), each call run to the
  end (up to 3 s per endpoint), with `ChangeIsr` and heartbeats: unbounded, sweep calls could
  queue ahead of them. So a `ChangeIsr` waits behind at most one sweep call, as behind a
  target's refresh. Not chosen: a second client, more connections for no further gain.
- **Release deletion is off unless the node is started with it on,** turned on once every
  metadata instance and node is new (S14; aligned in round 4, R4-B1), and off before any
  downgrade: an old instance rewrites chunk records
  without their releases, so S2 no longer holds and the sweep's read no longer proves a copy
  unwanted (spec review 1). A start option is the simplest gate the daemon supports; the
  tombstone step needs none. Changing it restarts the node's data plane (replica service and
  targets) (spec review 2).
- **No release token:** the data plane re-reads the chunk record at deletion. Safe because a
  released chunk cannot move back (S2), a forgotten node takes no
  move (S19), and only this node's data plane confirms, after the delete.
- **A deletion re-checks per chunk** (read age, local view, fence not risen past
  the epoch read, nothing of the volume deleted here since) and deletes at the durable fence, not
  whatever the fence as `delete VOLUME` does (`Store::delete_volume(uuid, None)`): a deletion
  that outlives its read cannot touch a new volume's chunk. Corrected: `delete VOLUME` has
  deleted with `None`, not `u64::MAX`, since #30 (its D13).
- **Superseded: "the grace is 10 s per (volume, chunk)" and "a chunk is deleted only when quiet".**
  Both kept the copy for a proposer that had not learned of its drop; `NotHeld` tells it instead,
  whether it is slow, cut off, or the source restarted. A released chunk is deleted at the first
  sweep that lists it. Reason: the protocol change (user decision, 2026-11-05).
- **One read per volume covers its listed chunks;** a chunk reached when the read is over 5 s
  old waits for the next sweep's read (spec review 2).
- **No session rule in the data plane:** a target keeps its session to a node for the node's
  other chunks, so the rule would block every deletion.
- **A `Hello` at the fence creates no chunk;** only one that raises the fence, or a resync, does.
  Its reply has no row for a named chunk the node lacks, which the target takes as not held
  there (changed after approval, round 1, R1-C4, below).
- **The core decides what a `Hello` creates** (design review 1, code 1): `Request::Hello`
  carries the target's chunk list and the shape, and `Action::Fence` lists the chunks to create,
  only when the `Hello` raises the fence. So the simulator checks S9 and S10: its replicas
  create what the core lists, and two injected bugs (`hello_creates_at_fence`,
  `fence_lost_with_last_chunk`) are each caught by a scripted scenario (a reconnect after the
  copy's deletion; an old target after the source's restart). Not chosen: random runs only, as
  the windows are narrow. The scripted `Move` does the whole move at `META` in one step, the
  destination given a copy as a finished resync would: a target cut off from `META` one way
  could otherwise never carry out the move, and cutting it later means hitting a 500 µs window
  (design review 2, code 1).
- **A `Hello`'s creations and fence commit in one batch;** one too large for a batch is
  refused: no multi-batch path to get wrong.
- **A failed `Hello` restores the volume's whole core state** (fence, session, chunks, filters);
  missing chunks come from the store.
- **The node records a volume's shape from a `Hello`** even with no chunk of it: a resync after a
  restart can create the chunk. Only from a `Hello` the fence accepts (design review 1, security
  F2): a stale one, perhaps an earlier volume's of the name, would otherwise overwrite the live
  volume's sizes. A failed `Hello` restores the service's entry with the core state.
- **A disk breaking the fence-only rules is refused at open** (design review 1, security F8):
  two fence-only slots of a volume, one beside a chunk of its volume, state 4 or record 9
  without bit 2, a state-4 slot with sizes, index or layers, a record 9 with reserved bits.
- **A fence-only record keeps the fence after the last chunk** (a slot state, a journal record,
  incompatible bit 2): within one volume, an older target must stay refused after a restart.
  It is an allocated slot everywhere, hidden only from chunk lookups: no code that treats an
  empty slot as free has to learn about it. One per volume; the volume's next chunk takes it.
- **Its deletion is #30's `Store::delete_volume(uuid, below)`,** `below` an `Option<u64>`: the
  slot goes when `below` is `None` (`delete VOLUME`) or its fence is below it (#30's drop).
  Corrected: #30 made `below` optional (its D13).
- **Fence holders are listed in the volume's header**, added at the first confirmation, so the
  tombstone deletes their records. 16 at most (changed after approval, round 1, R1-C5, below).
- **"A volume created again starts above every earlier epoch" is issue #30's,** not #13's, and
  so is the vectors' first epoch, and `Store::delete_volume(uuid, below)`, to which #13 adds the
  fence-only slot.
- **No path for copies no record names:** they are deleted by hand once in the lab; this change
  makes none, but in its rollout window.
- **`ListReleases` scans every chunk record, at most 1024 per call, no cursor:** small for the
  lab; the rest come in later sweeps.
- **Superseded: "`delete-chunks` takes at most 119 chunks".** No control-socket command deletes
  releases. Reason: ADR 0001: the sweep lives in the Rust node daemon (user decision,
  2026-11-05). One `ConfirmRelease` takes at most 118 chunks: the header, 118 chunks and the
  node's ledger fill etcd's 120 (#72's S11; changed after approval, round 1, R1-C2, R1-S2,
  below; it was 119).
- **The drain still ends when nothing is placed on the node;** it waits instead of failing on a
  chunk at S2's bound, or with only released nodes left, saying why (changed after approval,
  rounds 1 and 2, R1-C5 and R2-B1), and shows the releases held.
- **Superseded: "the simulator only confirms the releases it makes; no new check".** It also answers
  `NotHeld` and checks the target's acceptance (above). Reason: the protocol change
  (user decision, 2026-11-05). Changed after approval (round 1, R1-C1, R1-S1, below): its
  sources no longer delete at the drop; a modelled sweep does, later, as S7 allows.
- **Issue #14's step 2 landed first** (PR #31); #13 removes `vol-confirm` (with
  `scripts/guest/confirm.sh`), `flaminio-meta confirm` and the stop for fl-c. Corrected: no
  `tombstones` call moves to `sweep`. The harness's wait lists as an administrator
  (`metactl`, `AS=admin`), which keeps `ListTombstones`; the only listing as `agent` is
  `confirm.sh`'s, which goes.
- **The hand cleanup** deletes a volume's copies on a node only when no record names the node for
  it: otherwise it could delete a live or released copy. It runs with no drain, move or attach in
  progress, re-reads the record (`metactl get`) just before each delete, maps hashes to names
  with `node.sh list NAME`, and treats a hash no listed name matches as possibly named
  (`ListVolumes` omits volumes being created or deleted): it stays, and no store is reformatted
  on that basis (design review 1, security F7).
- **The harness deletes no copy itself** (design review 1, security F6): after a deletion, its
  `node.sh delete` on every node could run once the id is free again, and `delete VOLUME`
  passes any fence. The sweeps delete and confirm; the harness starts stopped nodes and waits.
- **The forging node runs only in the tests' `forging` configuration** (design review 1, code 2):
  `Config::random` never forges, so `make sim`'s campaign and the hinted seeds keep their
  configurations. `NotHeld` still changes runs. Rule: right after the simulator change, rerun the
  hints (`catches_fast_reads_across_a_fence`, 103 223; `catches_keep_dropped_acks`, 1 183 196);
  one that no longer catches is searched again with `scripts/dev/mutation-search.sh` over its
  test's budget (4 000 000 and 20 000 seeds), its comment updated with the new hint and date;
  none found: a scripted scenario of the bug replaces the hint. The PR says which. Then the
  whole `make sim` runs, and any `mutation!` test needing a new budget or hint is reported
  (design review 2, code 2). The relay's delays draw from an `Rng` of their own, so runs with no
  lookup are unchanged; a raising `Hello` now creates chunks in random runs (a joiner after a
  failover), and a failure there is a real finding. The forger's fields and code are
  `#[cfg(test)]`, and `Config::random` leaves them `None` and 0 (design review 2, code 5).
- **The drain's progress counts releases held,** not the chunks waiting on them: one counter
  is enough to see why a drain waits.
- **Metadata and data-plane rollback is unsupported** once a release, a fence holder or a
  fence-only slot exists. Simpler than a two-step rollout.
- **The protocol document changes with it:** `docs/replication-protocol.md` §2 and §7 (the
  release at the drop, the move back refused while released, its bound, forgetting a node), §3
  (a `Hello` creates chunks only when it raises the fence; its reply's rows; the fence-only
  record).

## Accepted risks

- **An unnamed copy after a missed epoch** (reopened spec review 1, L2): a node unreachable for
  the whole of epoch E deletes its released copy and confirms with its fence still at E0 < E;
  E's target, its drop's answer lost, then opens a session at E, which raises the fence and
  creates every missing chunk its stale view names (`ReplicaService::hello`,
  `flaminio-node/src/replica/mod.rs`). The chunk is outside that target's ISR (leases: every ISR
  member confirmed E, so had a session at E), so no read returns its zeros; it is a leak,
  listed by S8 and deleted by hand. Not chosen: skipping chunks deleted at or below the epoch at
  a fence-raising `Hello`: the fence-only record keeps no per-chunk history, so it is not small.
- **A target accepts a membership change on a replica's word** (spec S18). Only a lying
  authenticated session holder can quote an open proposal, one whose outcome the target does not
  know (it can read the volume and guess a drop); if that proposal never committed, the target
  adopts an ISR the service never had, and acknowledges writes on fewer replicas, which narrows
  durability. Compare: an authenticated peer can already acknowledge writes it never stored. A
  rejected or dropped proposal is closed, so quoting one is refused (reopened spec review 3). A
  record still naming the sender, or removing another ISR member, is refused, so a liar can only
  have itself dropped: no worse than acknowledging writes it never stored (design review 1,
  security F1; design review 2, security N1). The simulator's forging
  node sends no open proposal that leaves it out. The protocol change (user decision,
  2026-11-05).
- **A `BeginResync` creates a chunk the record may no longer place** (design review 1, security
  F4; `replica.rs:259-261`): a source outside the ISR at its drop, the answer lost, its copy
  deleted, then resynced by the stale target, gets an empty chunk no record names. The target's
  join then fails its version check, the view moves on, and the copy is a leak S8 lists. Not
  chosen: checking the record at a `BeginResync`, which would put a metadata read in front of
  every move's resync, a common path, for a rare leak. Follow-up.
- **Source and target both cut off from the metadata service:** the source cannot read the
  record, so it drops the request as today, and the target's writes to that chunk wait until
  one of them reaches the service. A double failure; no acknowledged write is lost.
- **Copies no record names** (from before this change; releases erased by an old metadata
  instance in the rollout) hold space until deleted by hand; a later volume of the name drops
  them (#30). Follow-up: their deletion.
- **A node gone for good:** its releases stay, and a deletion of their volume leaves a tombstone
  nothing confirms; the id stays taken. Follow-up: forgetting a node. Changed after approval
  (round 2, R2-B1): forgetting a node is now in this plan (S19); the risk stands only until an
  administrator forgets it.
- **The service still accepts an administrator's tombstone confirmation;** no tool sends one
  (issue #14).
- **No node sweeps** from the metadata service's rollout until the new data plane runs: no
  volume deletion in the lab meanwhile.
- **Fence-only slots can leak** when an old instance rewrites a header, or a header does not
  parse, dropping `fenced`: harmless with #30 (stale), and bounded by one per volume and node.
- **The copy listing exposes name hashes** to root on the node and to the CSI node pod, which
  shares the socket's directory.
- **Root on any node can sign capability tokens** (every node holds the token key), and can
  `delete VOLUME` without a token: as before. Follow-up: a token for `delete VOLUME`.
- **Superseded: "a root caller of `delete-chunks` can skip the grace".** There is no such
  command. Reason: ADR 0001: the sweep lives in the Rust node daemon (user decision, 2026-11-05).
- **Nothing in the data plane checks the metadata instances' versions** before release
  deletion is turned on: only the operator does. Turned on early, an old instance's rewrite can
  let a move back commit, and at RF 1 the copy deleted may be the only one (spec S14). No version
  check is added (spec review 2).
- **Fence-only slots draw from `chunk_slots`,** one per volume and node until the tombstone: the
  last chunk's record reuses that chunk's slot, so it never fails; new chunks may find fewer.
- **Each chunk deletion stalls the node's I/O** by the settle of its commits in flight (a held
  group included, #10's S5) plus the deletion's own synchronous commit.
- **A target of a deleted volume still running** can create chunks on a node that never held it:
  the incarnation follow-up.
- **The listing reads every chunk record every 5 s per node:** an index by node is a follow-up.

## Review

- **The plan review ended at round 5** with only Lows and nits, all fixed in the last revision.
- **Spec review 1 (2026-11-05, after the rebase):** 2 Medium, 3 Low, nits; all addressed.
- **Spec review 2 (2026-11-05):** 1 Medium, 3 Low, nits; all addressed. The idle-chunk question was
  left open for the user. **Superseded:** closed by the protocol change (user decision,
  2026-11-05), which also closes the gap after a source restart.
- **Reopened spec review 1 (2026-11-05):** 2 High, 4 Medium, 3 Low, nits; all addressed. Choices:
  M1 by the refresh argument (no service change), L2 as an accepted risk.
- **Reopened spec review 2 (2026-11-05):** 3 Low, nits; all addressed.
- **Reopened spec review 3 (2026-11-05):** 1 Medium, 2 Low, nits; all addressed (open proposals,
  closed by a reply or a drop; one simulator invariant).
- **Reopened spec review 4 (2026-11-05):** 1 Medium, 3 Low, nits; all addressed (precise
  closing rules, a cap of 4, the dropped sender's late `NotHeld`, no version+2 shape).
- **Reopened spec review 5 (2026-11-05):** 1 Low, nits; all addressed (identical resends kept
  once).
- **Design review 3 (2026-11-05):** security clear; code 2 Low (wording: the scripted move's
  copy, the pending-removal argument), nits; all addressed, no spec change.
- **Design review 2 (2026-11-05):** security 1 Medium (N1); code 1 Medium and 4 Low; nits; all
  addressed. Spec changes: S18 (the
  record keeps every other ISR member), the tests' S17/S18 bullet (a drop with a pending removal
  refused) and simulator bullet (the forged shape, the bug). Also: lookup caches pruned after 3 s
  without waiters (code 4).
- **Design review 1 (2026-11-05, code and security):** 2 High, 2 Medium, 9 Low, nits; all
  addressed. Spec changes: S18 (the record must not name the sender; authentication always on),
  S9 (only an accepted `Hello` sets the shape), S17 (bounds per volume, a gone volume not reread
  for 3 s, the cache), the invariant, and the tests' S6, S17/S18 and simulator bullets.
- **Follow-up, not a mechanism here:** `ChangeIsr` resends can pile up in the metadata client's
  unbounded queue when the service is slow; that exists today (reopened spec review 2).
