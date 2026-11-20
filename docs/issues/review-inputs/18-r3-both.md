# Issue #18 plan review: round 3 (incremental), design-code and design-security

Scope: `git diff 339601b..fc86228 -- docs/issues/18/ ':!docs/issues/18/report.md'`, checked against
the code in /home/jsfakian/Documents/src/flaminio-18 and the plans of #13, #45, #70 and #72.
HEAD (c6c7373) differs from fc86228 only in report.md.

## Earlier findings
- R2-B1: fixed. Creation now fails closed, Risks names the block, `BlockedVolume.Why` names the
  replica, and #45 gets a hand-off. Follow-ups: R3-C2, R3-C3, R3-B1, R3-S1.
- R2-C1: fixed (design.md:305-311).
- R2-C2: fixed. The drain matches the constant, and a real-refusal test exists (design.md:136-139, :312-317).
- R2-C3: fixed for the chunk whose own refusal empties its set (design.md:302). The rule for picking the reason is inconsistent: R3-C1.
- R2-C4: fixed. The 60 s wait is feasible: step 1 restarts fl-c with `DELETE_RELEASED=1`, and `releases <node>` prints `{}` when empty (13/design.md:264, :832-838).
- R2-S1: fixed (decisions.md:106, :157-165; design.md Docs and Risks).
- R2-N1: fixed (spec.md:29-35).
- R2-N2: fixed. `errStatus` sends `err.Error()` as the message (service.go:143-144), so the prefix comes first (design.md:135-141).

### R3-C1 — Low
**The cases for a blocked chunk's `Why` overlap. Applied in the order written, they contradict the
design's own tests, and they leave a chunk blocked by earlier refusals with no useful reason.**
- design.md:127-132 lists four cases: unknown words; no candidate without a replica (`no eligible
  node`); "otherwise, under `strict`", `Strict`'s reason for the least loaded candidate; and last,
  "a refused chunk".
- A collision refusal (S14) happens only under `strict`, and only for a node `Strict` allowed in
  the drain's view. So the "otherwise" case applies first, and `Strict` gives an empty reason for
  the node that was proposed. The test at :316 expects the service's message instead.
- Under #72's rule, the refused node leaves the step's candidates (:141). An empty set then reads
  `no eligible node`, or another node's `Strict` reason, before "the last refusal" (:302 expects
  the last refusal).
- A later chunk, or a chunk of a later volume, whose only nodes an earlier chunk's refusal took
  out gets `no eligible node`, though nodes exist and were refused. #72's S8 named the last refusal
  in this case (72/spec.md:123-125).

**Fix:** state the precedence:
1. A refusal of this chunk in this step: its message (the last one).
2. Unknown words.
3. No candidate at all: `no eligible node`.
4. All eligible nodes taken out by this step's refusals: the step's last refusal message.
5. Otherwise, `Strict`'s reason over the eligible nodes.

Add a test for case 4: two chunks, where the first chunk's #72-style refusal removes the only node
the second could use.

### R3-C2 — Low
**R2-B1 empties half of the invariant's property test, and the test's setup no longer fits the
creation rule.**
- The test creates `strict` `rack` and `zone` volumes on random topologies, "some unlabelled"
  (design.md:282-288). Since R2-B1, `Place` gives such volumes no replica on an unlabelled node
  (:97-101), and the test never changes a label.
- When the check is in place, no replica ever sits on an unknown node, so the clause "no accepted
  move … went … beside a node unlabelled" can never fail. A service that checks `to` but forgets
  the staying replicas passes the property test. Only the S5–S7 unit cases catch that bug.
- Random topologies with few labelled domains now fail creation (`ErrPlacement`). The test does
  not say whether it skips them or retries.
- The S5–S7 service case "a staying replica without the label" (:270) can no longer be built at
  creation. `RegisterNode` merges labels (service.go:197-199), so it can set an empty value but
  cannot remove the key. The test must write the record to the store directly.

**Fix:**
- The generator clears random nodes' labels to the empty value between moves. Keep the "beside"
  clause, and check distinct domains only among labelled replicas.
- The generator only creates volumes when there are enough labelled domains.
- After creation, assert that no chunk of a `strict` `rack` or `zone` volume with 2 or more
  replicas has a replica on an unknown node.
- Say how the S5–S7 cases produce an absent label: write the record to the store.

### R3-C3 — Low
**The creation refusal's new suffix is wrong at `rack`, and it does not compose with #72's errors.**
- design.md:99-100 and :233 fix the text as `; %d nodes without a zone label left out`. It has no
  "`rack` for `zone`" rule, unlike `Strict` (:72-73). For a `rack` volume, the message tells the
  operator to add a zone label, when the missing one is `flaminio.io/rack`. The test (:253-256)
  runs at both levels.
- The #72 seam (:102-103) gives only the order of the two filters. #72 appends its left-out
  nodes to "either create error" (72/capacity-contracts.md:113-114; 72/spec.md:99-100). #18's
  suffix lives inside `Place`'s `ErrPlacement`, so a `RESOURCE_EXHAUSTED` room refusal never
  mentions the unlabelled nodes. Example: a full z1, a z2 with room, and an unlabelled node with
  room. The error reads as a capacity problem when labelling the node would fix it.
- There is no leak: the suffix is a count, `CreateVolume` is administrator-only (auth.go:64), and
  today's messages already give node and domain counts.

**Fix:**
- Use "a rack label" at `rack`, and assert it in `TestPlaceStrictLeavesOutUnlabelled`.
- In the #72 bullet, append the suffix to either create error, after #72's own suffixes.

### R3-B1 — Low
**The creation change is not documented where API users and operators will see it.**
- Intent "What we want" 2 now covers creation, and item 6 asks that the API comments say so.
- Contracts change only the `MoveReplica` comment, and say "No other message or RPC changes"
  (design.md:210-219).
- `CreateVolumeRequest.failure_domain_policy` still reads only "refuses if the topology is short"
  (meta.proto:139-140). The same holds for ARCHITECTURE §8.2's row (:442) and followups §4 step 2.
- Risks' Compatibility bullet (:421-422) omits the upgrade effect. A `zone`/`strict` or
  `rack`/`strict` StorageClass whose creations counted unlabelled nodes as a domain is refused
  after the upgrade (`ResourceExhausted` through CSI, controller.go:248-249), with no prior warning.

**Fix:**
- Update the proto comment of `failure_domain_policy` (and `prefer_node`: an unlabelled preferred
  node is ignored under `strict`).
- Update ARCHITECTURE §8.2's row and followups §4 step 2.
- Add a Compatibility line and an upgrade note in `docs/lab.md`: label every storage node before
  upgrading if `rack` or `zone` `strict` classes are in use.

### R3-S1 — Low
**The lost-label block is not recorded as "named, not accepted" the way the rollback loss is.**
- design.md:449-451 "Named, not accepted" lists only the rollback.
- The lost-label bullet (:414-420) ends "named here, for the approver", and decisions.md:139 says
  only "Risks names it".
- So the approver's list of risks named as not accepted leaves out a node's ability to block
  every move and rebuild beside its replicas.

**Fix:**
- Add the lost-label block to the "Named, not accepted" bullet.
- Say "not accepted" in decisions.md's R2-B1 entry.

## Nits
- design.md:188: the `BlockedError` example writes `node "d" has no current capacity report`.
  #72's contract is `node <to> has no current capacity report`, without quotes
  (72/capacity-contracts.md:115). Copy #72's form, or note that #72's text is used verbatim.

Reviewed commit: c6c7373eed9151ddce4f33c2d66e5c7b27f9a6fa
