# #13 plan review, round 3 (incremental), focuses: design-code and design-security

Scope: `git diff 93ed53b..4cf21c5 -- docs/issues/13/ ':!docs/issues/13/report.md'` in
`/home/jsfakian/Documents/src/flaminio-13` (commits 0f05607, 4cf21c5), and the code and plans the
changed lines depend on: `auth.go`, `service.go`, `domain.go`, `kv/etcd.go`, `drain.go`,
`meta.proto`, `meta.rs`, `lab.sh`, and the plans of #80, #72, #45 and #18.

## Earlier findings

- R2-B1 (High): fixed for a node gone for good. S2 is restored, the bound of two releases is
  sized against #80, and `ForgetNode` ends a release without its node. The other half of R2-B1,
  release deletion off (the default), is narrowed to chunks moved twice. That half survives
  unnamed as a durability residual: R3-B1.
- R2-C1 (Low): fixed. design.md:935-938 has the 17th holder's two confirmations, no slot kept,
  and the `InitCapacity` rerun.
- R2-N1 (Nit): fixed. The replay case can now be built: once confirmed, the release no longer
  blocks the move back.

### R3-B1 — Medium

**Summary.** "The durability residual" is not the only one. With release deletion off, which
is the default (S14), two routine drains of live nodes leave a chunk at the bound. Take RF 2 on
{A, B}:
- A is drained to C, and A is released.
- Later C is drained to D. That move is allowed, one release beside it, and C is released.
- The chunk is now on {B, D} and holds {A, C}, both live and not deleting.
- B dies. The rebuild's move B→E is refused (`holding 2 releases`).

Forgetting B does not help, because B is a replica, not a releaser. Forgetting A or C would
retire a healthy node for good. The chunk stays at one replica until A or C restarts with
`DELETE_RELEASED=1`.

The plan does name "a chunk moves twice, then a drain or #45's rebuild waits", but only as a
move cost. It calls only the two-dead-nodes case "the durability residual", and its wording
offers forgetting as the cure for live nodes:
- spec S14: "deletes or is forgotten";
- design.md:760-762: "deletes … or is forgotten";
- design.md:1167-1169: "by design (release deletion off) … until one is forgotten".

The cure it names for #45 does not exist in #45's model either:
- design.md:1178 and decisions.md:175-176 say "#45 does, when it declares a node failed for
  good".
- But #45's Failed is automatic after the wait, and reversible: Admit, and S14 ("a failed node
  that returns holds what the records place on it").
- Forgetting is irreversible. So #45 could forget only on an administrator's declaration, which
  it does not have.

**Evidence.** spec.md:98-101; design.md:760-762, 1167-1169, 1172-1179; decisions.md:162-176;
report.md round 2 ("The residual, bounded and not accepted: … One dead node no longer blocks a
rebuild"); #45 spec.md:12-13, 79-84.

**Fix.**
- Name a second durability residual beside the first, for the user. With release deletion off,
  a chunk drained off two live nodes is not rebuilt after a loss until one of them runs with
  release deletion on. That cure is open once every instance is new (S14). Keep the residual
  "not accepted".
- In S14, design.md:760-762 and 1167-1169, offer forgetting only for nodes gone for good. Name
  release deletion on as the cure for live ones.
- Reword the #45 clause: #45 could close the residual only by adding an administrator's
  "failed for good" declaration that forgets. That is a change to #45, to record for its review.

### R3-B2 — Medium

**Summary.** A tombstone written after the forgetting still names the forgotten node, so its
id stays taken.
- The mark stops new releases and fence holds. But `DeleteVolume`'s tombstone also names every
  replica and move endpoint (`replicaNodes`), and the forgotten node keeps its replicas until a
  drain or rebuild moves them off (design.md:197-198).
- Some volumes are deleted while it still holds them:
  - any volume deleted before that move ends;
  - an RF 1 volume whose only replica was on the dead node, which no move can rebuild.
- Their tombstones name the node, and nothing confirms them.
- `ForgetNode` clears only the tombstones that exist when it runs (step 5). Its `pending` names
  only volumes being created or deleted at that moment, so nothing tells the administrator to
  run it again.

This contradicts:
- the spec's invariant ("a tombstone names every node a record names, until that node is
  forgotten");
- S5 ("… or is forgotten");
- the revised accepted risk ("the risk stands only until an administrator forgets it").

No test covers it.

**Evidence.** spec.md:21, 44-46; decisions.md:496-499; design.md:139-141 (Deletion), 173-175
(steps 5-6), 197-198, 952-955 (`TestForgetNodeClears`); `meta/service/service.go:92-115`
(`replicaNodes`: replicas and move endpoints), 566-567 (the tombstone written).

**Fix.**
- `DeleteVolume` reads the mark and leaves forgotten nodes off the tombstone. Its tombstone
  transaction compares the mark (two compares).
- This is safe: no move or placement can bring the id back to a forgotten node, so what is left
  on it is of the kind "copies no record names".
- Add the case to `TestForgetNodeClears`: a volume with a replica on the node, deleted after
  the forgetting, writes no tombstone naming it.

### R3-C1 — Low

**Summary.** `cmd_drain_test`'s guard checks only the caller's environment. That does not make
the nodes delete:
- fl-a and fl-b are restarted with `node_env` (`cmd_node up a`, `up b`, lab.sh:1297-1298).
- fl-c is started only if inactive (`ensure_node_c`, lab.sh:1243-1253). A running fl-c keeps
  the setting it was started with, and `node_env` defaults it to 0 under the plan.
- So after a run with fl-c deletion-off, the move back leaves fl-c holding a release of every
  chunk of vol1.
- The next `DELETE_RELEASED=1 make drain-test` first drains fl-b into fl-c. Its only candidate,
  fl-c, releases each chunk, and fl-a holds a replica. So it waits for good (`only nodes
  releasing it are left (fl-c)`), the hang the guard was added to prevent.

Lab check 1's "after the move back, the same for fl-c" also fails then. The plan's own run is
safe only because rollout step 4 restarted every node.

**Evidence.** design.md:779-784; design.md Lab, check 1 (1083-1090); `scripts/lab.sh:677-680`,
781-786, 1240-1253, 1297-1298, 1303.

**Fix.**
- With `DELETE_RELEASED=1`, `cmd_drain_test` restarts fl-c's node through `node_env` before
  the first drain. fl-a and fl-b are already restarted.
- Or it dies before each drain unless `metactl releases` is empty for the destination.
- Extend the `lab-deletion-test.sh` case to match.

### R3-C2 — Low

**Summary.** The revision adds rules that #45 and #18 must carry, and records neither hand-off,
unlike #80's ("#80's text needs the change (not made here)").
- **#45.** design.md:1175 says "#45's rebuild waits on it with its reason". But #45's approved
  S15 has no "holding 2 releases" or "only nodes releasing it are left". Its eligible
  destinations (as #18) do not exclude released nodes. So its requests to released nodes are
  refused every pass, with no category in its report.
- **#18.** design.md:748 now makes the drain's `leastLoaded` exclude `GetReleased()`. #18
  replaces `leastLoaded` with `choose`, whose eligible set excludes only replicas. Whichever
  lands second drops one rule or the other.

**Evidence.** design.md:748, 1175; #45 spec.md:18, 85-97; #18 design.md:76-80.

**Fix.** Record in decisions.md, as for #80, the changes due in whichever plan lands second:
- #45's S15 gains the two reasons, and released nodes are not eligible;
- #18's `choose` excludes the chunk's `GetReleased()`.

### R3-C3 — Low

**Summary.** The two race tests name no interleaving, so they can pass without the mechanism
they test. And step 4's read is not stated.
- **`TestForgetRacesAMoveBack`.** Only one move needs `MoveReplica`'s mark compare: one that
  read the mark before the mark was written, and the chunk after step 3 cleared the release.
  A hook at any other read leaves the release in place, and S2 alone refuses the move.
- **`TestForgetRacesAConfirmation`.** The argument "step 4, after every step 3, takes out the
  fence hold it added" holds only if step 4 reads the headers after step 3. Step 3's single
  `List` of `volumes/` is the only header read the design names. Suppose a confirmation that
  makes the node a fence holder commits between that `List` and step 4. A step 4 working from
  that `List` leaves the node in `fenced`, and so on every later tombstone of the volume.

**Evidence.** design.md:167-172 (steps 3-4), 182-184, 192-195, 956-959;
`meta/service/service_test.go:619-649` (`interleaveKV` runs its hook after the read it
intercepts).

**Fix.**
- Step 4 lists `volumes/` again after step 3, or reads each header it compares.
- `TestForgetRacesAMoveBack`: run `ForgetNode` in a hook on `MoveReplica`'s read of
  `forgotten`, with `MoveReplica` reading the mark before the chunk. It must fail with the
  compare removed.
- `TestForgetRacesAConfirmation`: run `ConfirmRelease` (the node not yet a holder) in a hook on
  `ForgetNode`'s `chunks/` `List`, then check `fenced`.

### R3-C4 — Low

**Summary.** Placement does not compare the mark. `createVolume` reads the nodes, `drain/`
included, before its claim, and its claim compares only the volume and tombstone keys. So a
creation that read the nodes before `ForgetNode`'s mark can place chunks on the node after it.
S19 says "from then on … no volume is placed on it", and `TestForgetNodeRefusals` covers only
the sequential case.

This is liveness only:
- it is a new id, so no release is at stake;
- it needs an alive node;
- it is today's race with a drain's mark.

The spec still states more than the design gives.

**Evidence.** spec.md:139-141 (S19); design.md:187; `meta/service/service.go:386-414`.

**Fix.** Either compare the `forgotten` key in the claim's transaction (one more compare,
retried as #72's 32 attempts are), or state the race in S19 and design.md:187.

### Nits

- **R3-N1.** design.md:195, "or fails its chunk compares and reads the mark". Per
  `ConfirmRelease`'s step 5 (design.md:123-126), a failed compare reads again from step 2, not
  the mark (step 1). A chunk the run rewrote then gives `Aborted`, not `FailedPrecondition`.
  Reword: "or fails its chunk compares: `Aborted`".
- **R3-N2.** design.md:190-191, `RegisterNode` "its transaction comparing the mark beside the
  node record". `RegisterNode` returns before any transaction when the record is unchanged
  (`service.go:201-203`), which is the usual case for a restarting node. Say that the mark is
  checked before that return, and test an unchanged re-registration.

## The brief's questions (no finding unless listed above)

- **`auth.go`:** `ForgetNode` joins the administrator-only case (`auth.go:64`). No node identity
  passes, the forgotten node's own included.
- **Where the mark is compared:**
  - `MoveReplica`: chunk, header and mark, 3 compares (4 with #72's ledger).
  - `ChangeIsr`'s drop: header, 64 chunks and the mark, 66.
  - `DrainNode` undraining.
  - `RegisterNode`: see R3-N2.
  - `ListReleases`: the mark is read.
  - `ConfirmRelease`: the mark is read, not compared. 120 compares is right; the argument needs
    R3-C3's step-4 read.
  - Placement: no (R3-C4).
- **A move back:** none commits.
  - Before the mark, S2 refuses it, and step 3 clears a release only after step 2's mark.
  - After the mark, `CheckMove` refuses it, and the compare closes the window.
  - A forgotten node's target can still `ChangeIsr`, but it can add neither a release nor a move
    to itself.
- **Transaction sizes:** all conditional and within the 120 that `etcd.go:91` enforces (`kv.go:33`; compares/ops):
  - mark: 1/2;
  - releases: 119/118;
  - fence hold: 1/1;
  - tombstone: 1/1.
- **The simulator:** `scenario_move_back_after_forgetting` passes without the bug. The move back
  at 2 s is refused, node 1 deletes at 5.1 s, its confirmation is refused, and T2 reads node 2's
  copy. With `forget_without_mark` the move back commits, and node 1 deletes the only replica on
  its 1.1 s read, so the read is not linearizable. `Config::releases`: RF 2 on three nodes never
  reaches two releases, and RF 1 does, as stated.
- **The bound:** `538 + 262R` against `552 + 264R`, and `669 + 262R` against `684 + 264R`. Room
  for five names at rest. The caps 4419, 3457, 2838, 2408, 2090 are all correct, from #80's
  `2048 + N(288 + 264(R+1))` (#80 design.md:30) with 4160 and +132. Field 3 of
  `MoveReplicaResponse` is free (`meta.proto:247-251`), and `metactl move` prints it through
  protojson.

Reviewed commit: cd3e193bc975d2bc93114d1c0a65db1e2d415e9a
