# #13 plan review, round 4 (incremental), focuses: design-code and design-security

Scope: `git diff cd3e193..a081bd7 -- docs/issues/13/ ':!docs/issues/13/report.md'` in
`/home/jsfakian/Documents/src/flaminio-13`. HEAD (e350277) adds only report.md. I read what the
changed lines depend on:
- `service.go`: `createVolume`, `DeleteVolume`, `RegisterNode`, `MoveReplica`, `nodes()`;
- `kv.go:33`, `etcd.go:91-93`;
- `service_test.go:615-665` (`interleaveKV`);
- `lab.sh:529-535`, 677-690, 781-786, 1240-1305; `lib.sh:38-55`, 152-158;
- `lab-deletion-test.sh:209`;
- the plans of #72, #76 and #78, where the claim is shared.

## Earlier findings

- R3-B1: fixed.
  - The second residual is named.
  - S2, S14, S19 and the design's drain, lab and security lines offer forgetting only for a
    node gone for good.
  - The #45 clause is a change #45 needs.
  - One wording drifts: the cure's precondition (R4-B1).
- R3-B2: fixed. `DeleteVolume` reads and compares the mark (design.md:197-204), S19 says so,
  and there are two tests. It makes `pending` for a volume being deleted moot (R4-C1).
- R3-C1: fixed. fl-c is restarted through `node_env` before the first drain, with a 60 s wait
  for `releases fl-c`, and the script test covers it.
- R3-C2: fixed for #45 and #18 (decisions.md:222-229). The revision adds #72 hand-offs that
  are not recorded (R4-C3).
- R3-C3: fixed. Step 4 lists again. `TestForgetRacesAMoveBack` and
  `TestForgetRacesAConfirmation` name their interleavings, and each fails without its mechanism
  (see R4-N1 and R4-N2).
- R3-C4: fixed in mechanism. Its test is in the wrong interleaving (R4-C2), and its compare
  overflows the claim shared with #72, #76 and #78 (R4-C3).
- R3-N1: fixed (design.md:213-216).
- R3-N2: fixed (design.md:205-207, 975-976).

### R4-C1 — Medium

**Summary.** R3-B2's fix makes `pending` for a volume being deleted moot. The plan still says
such a volume keeps the node until a later run, and a test expects that run to clear it.
- A tombstone written after the mark never names the node (design.md:181, 197-204).
- A `deleting` volume's header is still in `volumes/` at step 3, so its tombstone is not
  written yet. When it is written, it is written after the mark and leaves the node off.
- The volume's records go with the deletion: the header with the tombstone, the chunk records
  by `deleteChunks` (`service.go:570-578`). So no later run has anything of it to clear.
- An abandoned deletion stays `deleting` and is named in `pending` on every rerun, for good.

Statements that are now wrong:
- S19: "A volume being created or deleted keeps it until a later run: the call names such
  volumes".
- design.md:179-180: "keeps the node until its deletion's tombstone exists …; the next run
  clears it". This is beside the new line 181, which contradicts it.
- The `pending` comment: "still naming it: run again".
- The `docs/lab.md` line: "run again while it names volumes as pending".

`TestForgetNodeClears` has the case "a volume being deleted named in `pending`, cleared by a
run after its tombstone is written". It cannot pass as stated: that tombstone does not name the
node, so the run clears nothing.

**Evidence.** spec.md:143-145, 156; design.md:166-174 (steps 3-4), 179-181, 197-204, 883,
981-982, 1182; `meta/service/service.go:566-578`.

**Fix.**
- Steps 3 and 4 skip a `deleting` volume without naming it. Its tombstone, written after the
  mark, leaves the node off, and its records go with it. `pending` then names only volumes being
  created.
- Reword S19, design.md:179-180, the proto comment and the `docs/lab.md` line to match.
- In `TestForgetNodeClears`, replace the case: a volume being deleted at the run is not named
  in `pending`, and its tombstone, once written, does not name the node.

### R4-C2 — Low

**Summary.** `TestForgetRacesAPlacement`'s hook is not in the interleaving the claim's mark
compare closes, so the property it checks holds without the compare.
- The mark's transaction also writes `drain/<node>` (design.md:164-166).
- `createVolume` reads the mark before the nodes (design.md:191), and `nodes()` lists `drain/`
  (`service.go:253-260`).
- So when `ForgetNode` runs in the hook on the `Get` of `forgotten`, the placement already
  sees the node draining and skips it (`service.go:399`).
- With the compare removed, the volume is still "placed without the node". Only an assertion
  on the retry itself could fail, and that checks the mechanism, not S19.

The spec's test line asks for "each in the interleaving its compare closes" (spec.md:157).
That interleaving is a creation that read the nodes, `drain/` included, before the mark.

`TestForgetRacesADeletion`'s power depends on things the plan leaves open: where
`DeleteVolume` reads the mark, and the fixture.
- Take a node that is a fence holder of the volume, and a mark read before the deleting mark.
- `ForgetNode`'s step 4 rewrites the header, so the deleting mark's compare fails
  (`service.go:536-543`).
- The retry reads the mark again, and the test passes with the tombstone's compare removed.

**Evidence.** design.md:164-166, 191-196, 197-204, 994-999; spec.md:157;
`meta/service/service.go:239-260, 399, 528-543`; `service_test.go:615-652` (the hook runs once,
after the read it intercepts).

**Fix.**
- `TestForgetRacesAPlacement`: hook on `createVolume`'s `List` of `drain/` (the last read of
  `nodes()`), or on its `chunks/` `List`. Make the first placement use the node (`PreferNode` =
  the node, or RF equal to the alive nodes), and assert that no chunk names it.
- `TestForgetRacesADeletion`: the node is a replica only, neither released nor fenced.
  `DeleteVolume` reads the mark after its deleting mark, before listing the chunks; state that
  in the design.

### R4-C3 — Low

**Summary.** The claim's new mark compare does not fit the claim budget that #72's contracts
fixed for #76 and #78. The revision's hand-offs to #72 are not recorded beside #45's and #18's.
- **The budget.** #72 sizes the claim as `2 + k` compares, k ≤ 116. Its contracts add "#76's
  admission (4 compares with #78's gate, 2 writes) still fits 116", so 4 + 116 = 120.
  - #13 adds the mark compare to the same claim (design.md:192-195: "#72's loop, which adds its
    ledgers to the same claim").
  - With all four plans landed, a creation over 116 nodes carries 121 compares.
  - `etcd.go:91-93` rejects it ("transaction too large"), so the creation fails with an error,
    not a refusal.
- **#78.** #78 closes the same race for drain marks with a placement gate. Its gate is
  compared by the claim and by `MoveReplica` (#78 design.md:40-43, 53-55). `ForgetNode`'s mark
  writes a drain marker, but does not bump that gate.
- **Unrecorded hand-offs to #72:**
  - design.md:202-204 says "#72's tombstone `charges` must leave it out too (a change for #72)".
    That is not in decisions.md's hand-off list.
  - #72 describes today's claim as "never retried" and adds the 32-attempt loop itself (#72
    design.md:10, 61-64). Under this plan, both the retry and the mark compare already exist
    when #72 lands.

**Evidence.** design.md:191-204; decisions.md:222-229, 236-239; #72 capacity-contracts.md:42-45,
#72 design.md:56-73; #76 design.md:42; #78 design.md:40-43, 53-60; `meta/kv/etcd.go:91-93`.

**Fix.** Record in decisions.md, as for #80, #45 and #18, the change due in whichever plan lands
second:
- **#72:** the claim already compares the mark and retries. `VolumeCharges` for the tombstone
  leaves forgotten nodes out.
- **The claim's 120:**
  - Either: once #78 lands, `ForgetNode`'s mark writes #78's gate with its drain marker, as
    #78's mark does. #78's gate compare then covers the forgetting, and #13's own mark compare
    in the claim and in `MoveReplica` goes.
  - Or: #72's `MaxPlacementNodes` becomes 115.

### R4-B1 — Low

**Summary.** Residual (2)'s cure has the wrong precondition. It is stated as "open once every
metadata instance is new (S14)". S14 turns release deletion on only once every metadata
instance **and node** is new, and so do the option's usage line and rollout step 4. The
operator-facing `docs/lab.md` line carries the shorter condition, which would let an operator
turn deletion on with old nodes still running.

**Evidence.**
- The residual's wording: design.md:778, 1234; decisions.md:206 (and the older
  decisions.md:410-411, which it copies).
- Against: spec.md:95-96; design.md:242-243 (`--delete-released` usage: "only once every
  metadata instance and node runs this version"); design.md:1114 (rollout step 4).

**Fix.** Write "once every metadata instance and node is new (S14)" in design.md:778, 1234 and
decisions.md:206. Align decisions.md:410-411 too.

### Nits

- **R4-N1.** The text gives the wrong read.
  - design.md:986 calls `MoveReplica`'s `Get` of `forgotten` "its first read". The first read
    is `nodes/<to>`, before the loop (`service.go:870`).
  - Read before the loop, the mark would be stale on every retry: the compare fails 32 times
    and the move ends `Aborted`, not refused.
  - Say "the first read of each attempt, inside the loop".
- **R4-N2.** `TestForgetRacesAConfirmation` hooks on step 3's `chunks/` `List`. It proves
  step 4's second listing only if step 3 lists `volumes/` before `chunks/`. That order is only
  implied by "as `ListReleases` reads them". Hook on step 3's `volumes/` `List`, which works in
  either order, or state the order.

## The brief's questions (no finding unless listed above)

- **A forgotten node's replicas after a deletion: left with no tombstone and no release.**
  - They are left: never deleted by the system.
  - This is stated: S19 says "its copies stay where they are" and "no tombstone written names
    it", and design.md:200-202 calls them "of the kind 'copies no record names'".
  - It is acceptable under S19:
    - its precondition is a node gone for good;
    - no move or placement reaches the forgotten id (`CheckMove`, the claim's compare);
    - a machine that returns registers under a new id, where #30's drop covers a later volume
      of the name.
  - The R3-B2 goal holds. An RF 1 volume whose only replica was on the node writes no tombstone,
    so its id is freed.
- **`createVolume`'s retry.**
  - It is bounded (32 attempts, then `Aborted`), and it is #72's loop (#72 design.md:61-64).
  - Only the mark drives it, which is administrator-only, written once per newly forgotten node
    (step 2's "unless already marked"), plus #72's ledgers later. No client can force retries.
  - The 120 budget: see R4-C3.
- **The race hooks:**
  - `TestForgetRacesAMoveBack` fails without the compare. Step 3 rewrites only the chunk, so the
    header compare passes, and only the mark compare stops the move.
  - `TestForgetRacesAConfirmation` fails without the second listing (R4-N2 aside).
  - `TestForgetRacesAPlacement` does not test the race (R4-C2).
  - `TestForgetRacesADeletion` fails without its compare only with an unstated fixture (R4-C2).
- **`cmd_drain_test`.**
  - `node.sh down`, then `ensure_node_c`, starts fl-c through `node_env`, because it is then
    inactive (`lab.sh:1240-1253`).
  - `metactl` quotes its arguments with `%q` (`lab.sh:534`).
  - `DELETE_RELEASED` is exactly `1` under the guard, so the unquoted `ENV_VARS`
    (`lib.sh:157`) carries nothing else on this path.
  - The 60 s wait matches the plan's lab waits (design.md:1107). `lab-deletion-test.sh`'s fake
    `sleep` (`:209`) makes the time-out case instant.
- **Transactions** (compares/ops), all conditional:
  - mark: 1/2;
  - step 3: 119/118;
  - step 4: 1/1, from the second listing;
  - tombstone clearing: 1/1;
  - `DeleteVolume`'s tombstone: 2/2 (3 ops with #72's claim key);
  - `RegisterNode`: 2/1 (3/2 for a new node with #72);
  - `MoveReplica`: 3/1 (4 with #72);
  - the claim: 3/1 alone, 119/118 with #72 at 116 nodes, and 121 compares with #76 and #78
    (R4-C3).
- **The two residuals** read the same in design.md:1224-1235, decisions.md:199-210 (with the
  round-2 entry's "Changed in round 3" pointer) and the `docs/lab.md` line, except for the
  precondition in R4-B1.

Reviewed commit: e3502774bf3896091c54419b2f39004ec91050b7
