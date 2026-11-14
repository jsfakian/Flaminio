# Report: #13

## Review rounds

### Spec loop (2026-11-05, after the rebase onto #14, #30, #11)

Five earlier rounds (2026-10-30) are summarised in `decisions.md` under Review.

**Round 1** (full plan): 2 Medium, 3 Low, Nits.
- Medium: the Go agent daemon contradicted ADR 0001 (node agent in Rust). Fixed, by the
  user's decision: the sweep moves into `flaminio-io node`, as the node's `io` identity; the
  `sweep` certificate, the flock and `delete-chunks` go.
- Medium: a target cut off from the metadata service keeps writing to the source; deleting
  the copy stalled it. Fixed: a chunk is deleted only once quiet for the grace.
- Low: S13's script tests named; S14's safety precondition (release deletion off until every
  metadata instance is new); `replication-protocol.md` §2, §3, §7 in the design's docs.
- S5 resolved as kept (reason in `decisions.md`). Nits fixed.

**Round 2** (incremental, sonnet): 1 Medium, 3 Low, Nits.
- Medium: the sweep's calls share the metadata client's one queue with `ChangeIsr`. Fixed: at
  most one sweep call outstanding, reads per volume.
- Low: which messages count as activity (heartbeats do not), the read's age; the start
  option's restart cost and an accepted risk; `O_EXCL` only for block devices. Nits fixed.

**Round 3** (incremental, sonnet): 2 Low, Nits.
- Low: S13's wait for the sweeps restored; the quiet test covers each message, table driven.

**Round 4** (incremental, sonnet): clear. Nit noted: a lab line's shorter wording.

### Spec loop, reopened (2026-11-05, the `NotHeld` protocol change)

The user approved the spec, then reopened it the same day for a protocol change closing the
idle-chunk gap: a replica answers a request for a chunk it no longer holds with `NotHeld`, read
from the record on demand; the target accepts it only as one of its open proposals. The quiet
rule and the 10 s grace went.

**Round 1** (full plan): 2 High, 4 Medium, 3 Low, Nits.
- High: a request answered `NotHeld` was lost unless a write (only `fail_peer` requeues); the
  simulator check could not catch its injected bugs. Fixed: the request fails on that member,
  the session kept; fault injection and an adoption invariant.
- Medium: `readVolume` reads header and chunks apart (argued as safe as a refresh); a drop can
  carry a pending removal (accept only the target's own proposal); confirming bumps no version;
  the on-demand read's time budget (join a read in flight, a short cache).
- Low: replicas only, not `moving`; a stale `Hello`'s leak accepted; held requests' bounds.

**Round 2** (incremental, sonnet): 3 Low. The kept proposal defined; the simulator's "not its
proposal" bug paired; a lab check restored.

**Round 3** (incremental, sonnet): 1 Medium, 2 Low. The kept proposal outlived known outcomes
(a forged rejected proposal); the target could refuse its own committed one; the second
invariant detected nothing. Fixed: open proposals, closed by a reply, a drop or a newer view.

**Round 4** (incremental, sonnet): 1 Medium, 3 Low. An honest late `NotHeld` was refused
(fixed: a removed sender's reply fails the request only); a reply closes only its own
request's proposals; the version+2 shape dropped; a cap of 4.

**Round 5** (incremental, sonnet): 1 Low. Identical resends kept once under the cap.

**Round 6** (incremental, sonnet): clear. Nit fixed: an identical resend keeps its first place.
Nits noted: an identical record sent in two requests is closed by either's rejection (liveness
only, a failed session as today); "as now held" could read "as the view now holds it".

### Design loop (2026-11-05)

**Round 1** (full plan, code and security): 2 High, 2 Medium, 9 Low (two found by both), Nits.
- High: a `NotHeld` record still naming its sender let a liar drop an honest member (fixed:
  the record must not name the sender); S9 and S10 had no simulator check (fixed: the core
  decides what a `Hello` creates, two injected bugs, two scripted scenarios).
- Medium: a stale `Hello` set a volume's shape (fixed: only on a fence raise); `NotHeld` and
  the forger would move the hinted mutation seeds (fixed: the forger only in a test config, an
  explicit rerun and re-search step).
- Low: `accept_not_held` cut (authentication is always on, by construction); lookup bounds per
  volume, a remembered `NotFound`, only chunks not naming the node cached; `BeginResync`'s
  unnamed copy an accepted risk; the harness's per-node delete removed; a safer hand cleanup;
  corrupt fence-only state refused at open; a test for `NotHeld` with release deletion off.
- Spec changed: the invariant, S9, S17, S18 and their tests.

**Round 2** (incremental, sonnet): security 1 Medium; code 1 Medium, 4 Low.
- Medium: a liar could quote an open drop that also removes another member (fixed: the record
  must keep every other ISR member; a joiner may be added). A scripted scenario could not run
  (fixed: the scripted `Move` does the whole move at the metadata model).
- Low: relay delays from their own rng and a full `make sim`; the restart scenario's timings;
  pruned lookup caches; the forger test-only.
- Spec changed: S18 and its tests and simulator bullet.

**Round 3** (incremental, sonnet): security clear (the joiner allowance confirmed safe); code
2 Low, wording: the scripted `Move`'s copy at RF 1; the pending-removal argument.

**Round 4** (incremental, sonnet): clear, both reviewers. Nits noted: `AssertStale`'s phrasing;
the order of the review entries in `decisions.md`.

### Plan review round 1 — 7defc59

Scope: correctness against code and security of the plan (approved 2026-11-13); no runtime
implementation.
Reviewers: plan-reviewer (design-code), plan-reviewer (design-security).

- **R1-C1 — High** (also **R1-S1 — High**): S2 (no move back while released) had no simulator
  check and no injected bug; the simulator deleted a source in the step that committed its drop,
  so no release was ever pending and the model's refusal never ran; S7's read-then-delete ran
  only in `sweep.rs`. Evidence: spec.md:28-29, 86-87, 143; design.md:106-110, 457, 486-488,
  499-507; `sim/mod.rs:769-794`, `1260-1285`; AGENTS.md Principles.
- **R1-C2 — Medium** (also **R1-S2 — Low**): #13 did not carry #72's prerequisite change to
  `ConfirmRelease` (1 to 118 chunks, the node's ledger compared and written); with the ledger, a
  119-chunk batch needs 121 compares against `kv.MaxTxnOps` 120. Evidence: design.md:72, 76-81,
  142, 628, 674; decisions.md:248; #72 spec.md:137-150, capacity-contracts.md:37;
  `meta/kv/kv.go:33`.
- **R1-C3 — Medium:** `Volume.fenced = 9` collided with #17 (`held = 9`) and #18
  (`failure_domain = 9`). Evidence: design.md:615; #17 design.md:186; #18 design.md:115.
- **R1-C4 — Medium:** S9 (a `Hello` at the fence creates nothing) left a deleted named chunk
  with no `Fenced` row, which #86's required-row rule turns into a failed session; the two plans
  defined `Request::Hello` differently. Evidence: #86 spec.md:80-82, design.md:138-161,
  decisions.md:55-57; #13 spec.md:64-66, design.md:231-235, 446-448, 509-519.
- **R1-C5 — Medium:** `released` and `fenced` were unbounded in every `GetVolume` and `Attach`;
  #80 (approved) requires bounded shapes and fixtures. Evidence: #80 contracts.md:46-49,
  decisions.md:16-19; #13 design.md:57-59, 614-615.
- **R1-C6 — Low:** the sweep's and lookups' new `GetVolume` calls were outside #75's
  `enforces_qos`, which #75 sets only at `engine.rs:668, 1782`. Evidence: #75 design.md:66-69,
  133; #13 design.md:140, 150, 172, 339.
- **R1-S3 — Low:** no named test bound a confirmation to the caller's own released copies.
  Evidence: design.md:73-75, 673-677, 683-685; `auth.go`.
- **R1-C7 — Nit** (also **R1-S N2**): `node.sh list` with `VOLUME` unset would exit in
  `volume_id`. Evidence: design.md:219; `scripts/guest/node.sh:102-105`.
- **R1-C8 — Nit:** the chunk range was checked before `readHeader`. Evidence: design.md:72;
  `service.go:775-778`.
- **R1-C9 — Nit** (also **R1-S N1**): the term "fence holder" said "with no chunk", but a node
  joins `Fenced` at its first confirmation. Evidence: spec.md:10; design.md:76-78.
- **R1-C10 — Nit:** `end_chaos` did not clear the new `one_way` set. Evidence: design.md:490-492;
  `sim/mod.rs:1332-1341`.

Changes/resolution:
- R1-C1, R1-S1: S7's decision moves into `flaminio-repl` (`release::may_delete`, sans I/O),
  called by the node's sweep and the simulator. The simulator's sources keep their copy until a
  modelled sweep reads the model (0–5 s after the release) and deletes 0–6 s after its read, as
  `may_delete` allows, then confirms. New injected bug `move_while_released` (the model's
  `MoveReplica` ignores `released`), caught by the scripted `scenario_move_back_before_deletion`
  (RF 1) and sought by `catches_move_while_released` on `Config::releases` (RF 1 or 2), under
  the hint rule. Spec: the tests' Simulator bullet.
- R1-C2, R1-S2: #72's S11 applied: 1 to 118 chunks, batches of 118, the ledger
  `/flaminio/v1/capacity/<node>` compared and written by #72's release rule, a moved header or
  ledger retried; tests 119 refused and 118 accepted on etcd, and an engine test confirming 119
  chunks in two batches. #13's read bound unchanged. Spec unchanged.
- R1-C3: `Volume.fenced` is field 15, the lowest `Volume` number no plan of the batch claims
  (9-10 #17, #18, #79; 11-14 #70's per #75; 20-21 #50; 30-31 #51; 40 #75; 40-41 #54).
- R1-C4: one `Request::Hello { chunks, shape }`, `chunks` as #86 defines it. The `Fenced` reply
  has a row per chunk held; a named chunk without one is not held there: the target gives it
  nothing and opens the session, and #86's validator reads a missing required row so. A target
  test and an engine test of a reconnect naming a deleted chunk. Spec: S9, the S17/S18 tests.
- R1-C5: a chunk holds one release at most and no move while it does (S2 now refuses any move
  of such a chunk; the drain waits on it, S16), which fits #80's per-chunk term; `fenced` holds
  16 at most (a 17th confirmer is confirmed, not named), adding 2112 B to #80's constant
  (`B = 4160 + N × (288 + 264(R+1))`, caps 5134/3879/3117/2605/2238). #80's fixtures and
  constant change with whichever lands second. Spec: the term, S2, S3, S11, S16. Two costs are
  named for review, not accepted: a chunk whose released node never deletes does not move, and a
  17th fence holder's slot is unnamed.
- R1-C6: one constructor, `get_volume_call`, builds all four data-plane `GetVolume` requests, so
  #75's flag covers them; with #75, an engine test covers the sweep and lookups.
- R1-S3: `TestConfirmRelease` gains a chunk released to another node, to nobody, a mixed batch
  (nothing written, `fenced` unchanged, no log line) and an out-of-range chunk, in memory and on
  etcd.
- R1-C7, R1-S N2: `volume_id` only when `VOLUME` is set. R1-C8: the range check after
  `readHeader`. R1-C9, R1-S N1: the term reworded. R1-C10: `end_chaos` clears `one_way`.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 2 — 93ed53b

Scope: correctness against code and security of the plan (approved 2026-11-13), incremental on
round 1's fix (7defc59..551dc8e); no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Earlier findings: R1-C1 to R1-C10, R1-S1 to R1-S3 and the R1-S nits fixed; R1-C5's fix opened
R2-B1.

- **R2-B1 — High:** round 1's narrowed S2 refused every move of a chunk holding a release, and
  only the released node ended one, so with release deletion off (the default) every moved
  chunk, and with a node gone for good every chunk moved off it, could never move again:
  drains and #45's rebuild stopped, and `Progress.Released` did not say why. Evidence:
  spec.md:29-31, 78-81, 83-85, 90-95, 101-102; design.md:79-82, 658-664, 1007-1010;
  decisions.md:79-84, 413; `control/drain/drain.go:124`; #45 spec.md:60-62, 75-77, 85-97.
- **R2-C1 — Low:** the 17th fence holder's effect on #72's ledger had no test; an implementation
  keying the kept slot on "not in `Fenced`" would creep the ledger up. Evidence:
  decisions.md:86-90; design.md:96-101, 811-812; #72 design.md:174.
- **R2-N1 — Nit:** the S3 case "after a move back and a new drop, the new release stays" could not
  be built under the narrowed S2. Evidence: design.md:804.

Changes/resolution, in two commits (0f05607, then 4cf21c5 on the orchestrator's decision about
the bound):
- R2-B1 (the orchestrator's decisions):
  - **S2:** it refuses again only a move back to a node that releases the chunk.
  - **The bound, first commit:** worked out from #80's per-chunk term as it stands. A chunk in a
    move takes `538 + 262R` B against `552 + 264R`, leaving 16–24 B, under the 131 B one release
    needs. So the term admits one release a chunk and no move beside it, and "a move elsewhere
    while releases are pending" could not hold.
  - **The bound, second commit:** the orchestrator took the 132 B option under the user's
    instruction "otherwise figure it out": the batch never accepts a durability loss on the
    user's behalf, and a smaller per-volume cap costs capacity, not data.
    - #80's per-chunk term becomes `288 + 264(R+1) + 132`. A chunk then holds at most two
      releases at rest and one during a move.
    - A move of a chunk holding two is refused, with `reason` `holding 2 releases of chunk C`
      (`MoveReplicaResponse.reason`, field 3), shown with both nodes in the drain's
      `Progress.Held`.
    - The drain again offers no released node, and says when only released nodes are left.
    - #80's caps, with round 1's `fenced` constant (4160): 4419, 3457, 2838, 2408, 2090 for RF 1
      to 5, from 5137, 3881, 3119, 2607, 2239. Whichever of #13 and #80 lands second changes the
      constant, the per-chunk term and the fixtures.
  - **Forgetting a node (S19):** `ForgetNode`, administrators only, marks the node (key
    `forgotten`, for good, with `drain/<node>`).
    - After the mark, `CheckMove`, placement and `RegisterNode` refuse it, its `io`'s
      `ListReleases`/`ConfirmRelease` are refused, and a drop no longer releases it.
    - The RPC then clears it from every `released` (batches of 118), `fenced` and tombstone,
      naming volumes being created or deleted for a later run.
    - Tests: service tests for each refusal, the clearing, a racing move back and a racing
      confirmation; the vectors' `forget` op; the model's `forget` with the injected bug
      `forget_without_mark`, caught by `scenario_move_back_after_forgetting`.
  - **With release deletion off:** a chunk moves twice, then waits with its reason, and never
    moves back to a node holding its release. The plan and the `docs/lab.md` changes say a drain
    needs release deletion on wherever releases are held. `make drain-test` requires
    `DELETE_RELEASED=1`.
  - **The residual, bounded and not accepted:** a chunk rebuilt off two dead nodes, neither yet
    forgotten, is not rebuilt after a third loss until one of them is forgotten. One dead node no
    longer blocks a rebuild.
  - **Consistency:** S2, S3, S5, S11, S12, S14, S16, `Config::releases` and the tests are aligned.
- R2-C1: `TestFenceHoldersAtMost16` gains, with #72 (whichever lands second), two confirmations
  by the 17th node each crediting n chunk sizes and n slots with no slot kept, and an
  `InitCapacity` rerun leaving that ledger unchanged.
- R2-N1: the case now confirms the release, moves the chunk back and drops it again, then replays
  the first confirmation at its old revision: `Aborted`, the new release stays.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 3 — cd3e193

Scope: correctness against code and security of the plan (approved 2026-11-13), incremental on
round 2's fixes (93ed53b..4cf21c5); no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Earlier findings: R2-B1 fixed for a node gone for good (its release-deletion-off half is
R3-B1); R2-C1 and R2-N1 fixed.

- **R3-B1 — Medium:** with release deletion off (the default), two routine drains of live
  nodes leave a chunk at the bound, and a later loss of its replicas is not rebuilt. Forgetting
  a healthy releaser would retire it for good. The plan named only the two-dead-nodes residual,
  offered forgetting as the cure for live nodes, and gave #45 a cure its reversible Failed does
  not have. Evidence: spec.md:98-101; design.md:760-762, 1167-1169, 1172-1179;
  decisions.md:162-176; #45 spec.md:12-13, 79-84.
- **R3-B2 — Medium:** a tombstone written after the forgetting still named the forgotten node,
  through its replicas and move endpoints (`replicaNodes`), so its id stayed taken. That
  contradicted the invariant, S5 and the accepted risk, and no test covered it. Evidence:
  spec.md:21, 44-46; decisions.md:496-499; design.md:139-141, 173-175, 197-198, 952-955;
  `service.go:92-115, 566-567`.
- **R3-C1 — Low:** `cmd_drain_test`'s guard did not make a running fl-c delete, so a fl-c left
  holding releases by an earlier run made the first drain wait for good. Evidence:
  design.md:779-784, 1083-1090; `lab.sh:677-680, 781-786, 1240-1253, 1297-1298, 1303`.
- **R3-C2 — Low:** the hand-offs to #45 (S15's reasons, released nodes not eligible) and #18
  (`choose` excluding `GetReleased()`) were not recorded. Evidence: design.md:748, 1175;
  #45 spec.md:18, 85-97; #18 design.md:76-80.
- **R3-C3 — Low:** the race tests named no interleaving, and step 4's header read was not stated.
  Evidence: design.md:167-172, 182-184, 192-195, 956-959; `service_test.go:619-649`.
- **R3-C4 — Low:** placement did not compare the mark, so S19's "no volume is placed on it" did
  not hold against a race. Evidence: spec.md:139-141; design.md:187; `service.go:386-414`.
- **R3-N1 — Nit:** a confirmation that fails its chunk compares gets `Aborted`, not a mark
  refusal. Evidence: design.md:195, 123-126.
- **R3-N2 — Nit:** `RegisterNode` returns before any transaction when its record is unchanged.
  Evidence: design.md:190-191; `service.go:201-203`.

Changes/resolution:
- R3-B1:
  - A second durability residual is named beside the first, both not accepted. With release
    deletion off, a chunk drained off two live nodes is not rebuilt after a loss until one of
    them runs with release deletion on, a cure open once every metadata instance is new (S14).
  - S2, S14, S19, the design's drain and security lines and the `docs/lab.md` changes offer
    forgetting only for a node gone for good, and name release deletion as the cure for a live
    one.
  - The #45 clause is now a change #45 needs: an administrator's "failed for good" declaration
    that forgets.
- R3-B2: `DeleteVolume` reads the mark and leaves forgotten nodes off the tombstone, its
  transaction comparing the header and the mark; a tombstone left with no node is not written.
  S19 says so. `TestForgetNodeClears` deletes a volume with a replica on the node after the
  forgetting, and `TestForgetRacesADeletion` races the two.
- R3-C1: with `DELETE_RELEASED=1`, `cmd_drain_test` restarts fl-c's node through `node_env` and
  waits, 60 s at most, for `metactl releases fl-c` to be empty before the first drain;
  `lab-deletion-test.sh`'s case checks it.
- R3-C2: decisions.md records both hand-offs, due in whichever plan lands second, as #80's is;
  the design's drain section notes #18's.
- R3-C3: step 4 lists `volumes/` again after every step 3. Each race test now names the
  interleaving its compare closes, and fails without it:
  - `TestForgetRacesAMoveBack`: `ForgetNode` runs in a hook on `MoveReplica`'s first read, of
    the mark;
  - `TestForgetRacesAConfirmation`: a confirmation that read the mark before step 2 commits in a
    hook on step 3's `chunks/` `List`;
  - `TestForgetRacesAPlacement` and `TestForgetRacesADeletion`.
- R3-C4: `createVolume` reads the mark before the nodes and its claim compares it; a failed claim
  places again, up to 32 attempts.
- R3-N1: reworded to `Aborted`. R3-N2: the mark is checked before the no-change return, and an
  unchanged re-registration is tested.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 4 — e350277

Scope: correctness against code and security of the plan (approved 2026-11-13), incremental on
round 3's fix (cd3e193..a081bd7); no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Earlier findings: R3-B1, R3-B2, R3-C1, R3-C2, R3-C3, R3-N1 and R3-N2 fixed. R3-C4 was fixed in
mechanism, but its test and its compare budget opened R4-C2 and R4-C3. R3-B2 made `pending` for
a volume being deleted moot (R4-C1).

- **R4-C1 — Medium:** since a tombstone written after the mark never names the node, a volume
  being deleted has nothing for a later run to clear. Yet S19, the design, the proto comment and
  the `docs/lab.md` line still promised that run, and `TestForgetNodeClears` expected it. An
  abandoned deletion would sit in `pending` for good. Evidence: spec.md:143-145, 156;
  design.md:166-174, 179-181, 197-204, 883, 981-982, 1182; `service.go:566-578`.
- **R4-C2 — Low:** `TestForgetRacesAPlacement` hooked where the mark's `drain/` marker already
  hid the node, so it passed without the claim's compare. `TestForgetRacesADeletion` depended on
  an unstated fixture and read position. Evidence: design.md:164-166, 191-204, 994-999;
  spec.md:157; `service.go:239-260, 399, 528-543`.
- **R4-C3 — Low:** with #72, #76 and #78 all landed, the claim's mark compare would bring it to
  121 compares. #78's gate was not bumped by the forgetting, and the hand-offs to #72 were
  unrecorded. Evidence: design.md:191-204; decisions.md:222-229, 236-239; #72
  capacity-contracts.md:42-45, design.md:56-73; #76 design.md:42; #78 design.md:40-43, 53-60;
  `etcd.go:91-93`.
- **R4-B1 — Low:** residual (2)'s cure said "every metadata instance", not S14's "every
  metadata instance and node". Evidence: design.md:778, 1234; decisions.md:206, 410-411;
  spec.md:95-96; design.md:242-243, 1114.
- **R4-N1 — Nit:** `MoveReplica`'s `forgotten` `Get` is not its first read; read before the
  loop, the mark would be stale on every retry. Evidence: design.md:986; `service.go:870`.
- **R4-N2 — Nit:** the confirmation race proved step 4's second listing only if step 3 lists
  `volumes/` before `chunks/`. Evidence: design.md:1004-1008.

Changes/resolution:
- R4-C1: steps 3 and 4 skip a volume being deleted without naming it, and `pending` names only
  volumes being created. S19, the design, the proto comment and the `docs/lab.md` line match.
  `TestForgetNodeClears` now checks that a volume being deleted at the run is not named, and
  that its tombstone, once written, does not name the node.
- R4-C2:
  - `TestForgetRacesAPlacement` hooks on `createVolume`'s `List` of `drain/` with `PreferNode`
    set to the node, and asserts no chunk names it; it fails without the claim's compare.
  - `TestForgetRacesADeletion` uses a node that is a replica only. `DeleteVolume` reads the mark
    after its deleting mark and before listing the chunks, as the design now states.
- R4-C3:
  - Once #78 lands, `ForgetNode`'s mark ensures and writes #78's gate with its drain marker, and
    #13's own mark compare in the claim and `MoveReplica` goes.
  - Until then the claim stays within 120 with nothing else, because #13's compare holds the
    gate's place: 2 + 1 + 116 = 119 in #72's claim, 3 + 1 + 116 = 120 in #76's admission.
  - Hand-offs are recorded for #72 (the claim's retry is already there; `VolumeCharges` and
    tombstone `charges` leave forgotten nodes out) and for #78 (`ForgetNode` writes its gate).
- R4-B1: "once every metadata instance and node is new (S14)" in the design's drain line, Risks
  and decisions; the older release deletion entry is aligned.
- R4-N1: the `forgotten` `Get` is the first read of each attempt, inside the loop, for
  `MoveReplica`, `createVolume` and `DeleteVolume`.
- R4-N2: step 3 reads `volumes/`, then `chunks/`, and the confirmation race hooks on its
  `volumes/` `List`.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 5 — 1de5da6

Scope: correctness against code and security of the plan (approved 2026-11-13), incremental on
round 4's fix (e350277..3b2564a); no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Earlier findings: R4-C1, R4-C2, R4-C3, R4-B1 and R4-N2 fixed. R4-N1 was fixed for
`MoveReplica`, with a wording left in decisions.md (R5-N2). R4-C2's "#78 gate" variant of the
move-back test opened R5-C1, and R4-C3's hand-off opened R5-C2.

- **R5-C1 — Low:** with #78, `TestForgetRacesAMoveBack`'s hook on the move's `Get` of
  `forgotten` let the move see the node draining and be refused whatever the gate compare did.
  So that variant could not fail without the gate compare. Evidence: design.md:187-192,
  1001-1005; #78 design.md:55-58; `service_test.go:638-643`.
- **R5-C2 — Low:** under the #78 hand-off, the forget mark rewrote an existing drain marker. That
  moved #78's drain generation and failed a running drain of the node, though forgetting such a
  node is residual (1)'s cure. Evidence: design.md:164-165, 202-205; decisions.md:272-275; #78
  spec.md:10-11, design.md:60-62, 94-104.
- **R5-N1 — Nit:** "two `interleaveKV` hooks" is more than one wrapper gives; one wrapper with
  both would fire the second hook on `ForgetNode`'s own step-2 read. Evidence: design.md:1006;
  `service_test.go:615-636`.
- **R5-N2 — Nit:** `createVolume` has no retry loop today, and the design did not say it adds one
  or what it covers. decisions.md said `DeleteVolume` reads the mark "likewise", first in each
  attempt, against the design. Evidence: `service.go:372-473`; decisions.md:290-292;
  design.md:209-210.

Changes/resolution:
- R5-C1: `TestForgetRacesAMoveBack` hooks `ForgetNode` on the move's header `Get`
  (`readHeader`), the first read after every destination check. Without #78 only the mark
  compare stops the move, and with #78 only the gate compare does; the test fails with that
  compare removed. Its fixture is stated: the node released by the chunk, not a fence holder,
  with no drain marker.
- R5-C2: the forget mark writes `drain/<node>` only when it is absent, comparing its revision,
  and with #78 writes the gate only with a new marker (3 compares, 3 writes). An existing marker
  is kept with its generation (2 compares, 1 write). decisions.md's #78 hand-off line says so,
  and `TestForgetKeepsARunningDrain` checks it.
- R5-N1: the confirmation test uses two stacked `interleaveKV` wrappers, one hook each: the outer
  on the `Get` of `forgotten`, the inner on the `List` of `volumes/`.
- R5-N2: the design says `createVolume` gains a retry loop of up to 32 attempts. Each attempt
  reads the mark first, then the tombstone check, `nodes()`, the candidates, `Place` and the
  claim; the chunk scan is read once before the loop. decisions.md now places `DeleteVolume`'s
  mark read after its deleting mark and before the chunks' listing.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 6 — b7f686c

Scope: correctness against code and security of the plan (approved 2026-11-13), incremental on
round 5's fix (1de5da6..9f3aa0a); no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Earlier findings: R5-C1, R5-C2, R5-N1 and R5-N2 fixed; the loop R5-N2 named opened R6-L1 and
R6-L2.

- **R6-L1 — Low:** the new `createVolume` retry loop had no test of its own.
  `TestForgetRacesAPlacement` still read "the claim fails", which no longer says whether
  `CreateVolume` succeeds or fails. Nothing covered exhaustion, nor a taken id or a live
  tombstone returning `ErrExists` without retrying. Evidence: design.md:197-205, 1027-1031.
- **R6-L2 — Low:** "a claim that fails with both keys still absent" did not say how that is
  known, nor its order against today's deleting-header branch. Evidence: design.md:201-204;
  `service.go:424`.
- **R6-N1 — Nit:** step 2 compared `drain/<node>`'s revision without saying it reads it first.
  Evidence: design.md:164-168.

Changes/resolution:
- R6-L1:
  - `TestForgetRacesAPlacement`: the first claim fails, and the second attempt skips the node and
    claims. `CreateVolume` succeeds with exactly two claims, and no chunk names the node.
  - New `TestCreateVolumeRetries`:
    - a mark that moves before every claim: `Aborted` after 32 claims;
    - a taken id, or a tombstone written between the check and the claim: `ErrExists` after one
      claim;
    - a tombstone already there: `ErrExists` before any claim;
    - an interrupted deletion: `Aborted`, as today.
- R6-L2: after a failed claim, the volume and tombstone keys are read again, a fresh `Get` each.
  The deleting-header branch comes first, as today. Then either key present is `ErrExists` at
  once; both absent retries.
- R6-N1: step 2 reads the mark and `drain/<node>` (with #78, the gate) before its transaction
  compares them.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 7 — 37808a0

Scope: correctness against code and security of the plan (approved 2026-11-13), incremental on
round 6's fix (b7f686c..8586a66); no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Earlier findings: R6-L1 was fixed in intent, with one counting problem (R7-L1). R6-L2 is fixed.
R6-N1 is fixed, with a wording gap (R7-N1).

- **R7-L1 — Low:** `TestForgetRacesAPlacement`'s "exactly two claims" cannot be counted with
  `interleaveKV.txns`, which counts every write of its key. A successful creation then counts
  three: two claims and the last header write. Evidence: design.md `TestForgetRacesAPlacement`;
  `service_test.go:662-666`.
- **R7-N1 — Nit:** step 2 reads the gate with #78, but its `Txn` listed only the mark and
  `drain/<node>` compares. Evidence: design.md step 2.

Changes/resolution:
- R7-L1: a new test helper, `claimCounter`, counts claims: the `TxnRev` calls whose compares hold
  the volume key at revision 0 and whose ops put it, whatever their result. The tests' claim
  counts all use it: "exactly two claims" in `TestForgetRacesAPlacement`, and "32 claims" and
  "after one claim" in `TestCreateVolumeRetries`.
- R7-N1: step 2's `Txn` compares the gate too, with #78.

Commits: dd3dbbd (the fixes), e566ab1 (a reflow of one test line, no content change).

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 8 — 0a89e15

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

- Both round-7 findings fixed (R7-L1 `claimCounter`; R7-N1 the gate in step 2's compares).
- **R8-N1 — Nit:** step 2's wording "(0 if absent) (with #78, the gate), putting…" is awkward and
  the round-7 reflow left a stub line. Evidence: design.md:164–168.

Changes/resolution: R8-N1 reworded to "the mark's revision (0 if absent), `drain/<node>`'s (0 if
absent) and, with #78, the gate's", and the stub line removed; recorded in decisions.md as round
8, after approval.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 9 — 612979176e245b00b74d58aa94c41b2d2989c8d8

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

No comments from the requested review scopes, including Nits.

Status: clear. Approval remains separate; no approval is inferred.
