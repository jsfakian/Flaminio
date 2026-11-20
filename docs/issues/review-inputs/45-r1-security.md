# Plan review: issue #45, round 1, focus design-security

Scope: docs/issues/45/ (intent, spec, design, decisions, adr-proposal), checked against the code at
the reviewed commit and against the plans of #13, #17, #18, #72, #78 and #82.

### R1-S1 — High
**A failed node that is also marked draining is never rebuilt, and #13 marks every forgotten node draining for good.**
The spec says the rebuild "asks nothing for an active, missing or draining node" (S9). The design
derives lost replicas "on Failed nodes except draining sources" (design.md:97). Two cases leave a
dead node with that mark, so its chunks are never re-protected and S15/S16 give no reason:
- #13's `ForgetNode` writes `drain/<node>` "so placement and the drain's candidates skip it as a
  draining node", and refuses an undrain of a forgotten node (flaminio-13 design.md:162-168, 211).
  Forgetting is #13's step for a node "gone for good". It is also the only thing that clears a dead
  node's releases (#13 S2, S11, S19; see R1-S2). Once a dead node is forgotten, the rebuild drops
  its remaining chunks for good. #13 S19 says "its replicas are moved off as any other".
- A drain that stops leaves the mark: `Run` returns on any error and never clears it
  (control/drain/drain.go:171-189). #18 S13 keeps it after a drain whose chunks are all blocked
  ("undrain clears it"). If that node later fails, the rebuild ignores it.
Excluding draining sources buys no safety: a running drain and the rebuild cannot both record a
move, because `CheckMove` allows one per chunk (meta/domain/domain.go:287).
**Fix:** S9 and design: rebuild every lost replica on a Failed node whatever its drain mark, or at
least treat a forgotten node as failed. If a mark is kept as an exclusion, report it as an S15
reason. Tests: a failed node that is then forgotten, and one with a stale drain mark, both get moves.

### R1-S2 — Medium
**#13's move rules are not in the rebuild's reasons or behaviour, and a dead node's releases never clear.**
The design assumes #13, but uses only "the drop releases the source". Under #13:
- Every rebuild drop releases the failed source (#13 S1). A dead node never confirms, and release
  deletion is off by default (#13 S14), so moved chunks keep releases.
- S2's bound refuses a move of a chunk holding two releases (`holding 2 releases of chunk C`).
  So a chunk that has already moved, by an earlier drain or an earlier rebuild, cannot be rebuilt
  after a later failure until a node confirms or is forgotten.
- "No move back while released" makes releasing nodes ineligible (#18 Eligible). When only such
  nodes are left, #18 has the chunk wait, not block.
S15 lists none of these reasons. The design re-asks a refused move at every pass
(design.md:97-101), while #13 S16 says the drain never asks a move S2 refuses. The lab check "fl-b
back with its disk: no record names it" (spec.md:137) is false under #13: its releases name fl-b
until fl-b's sweep confirms them, and that needs release deletion on, which the lab plan never
turns on.
**Fix:** S15: add "release bound (#13 S2)" and "only releasing nodes left (waits, #18)". Don't ask
a move #13 refuses (as #13 S16). Runbook: forget a dead failed node once it is re-protected (with
R1-S1 fixed). Lab: release deletion on, and check "no replica or move names it, and no release
after its sweep". Tests for each case.

### R1-S3 — Medium
**The loop runs in every instance, so maintenance attaches can depose each other, and the metadata reads multiply.**
In #17, `AttachMaintenance` applies whenever the volume is not `Held()`, including over a standing
maintenance record. A lost compare is "decided on a new read" and applies again (flaminio-17
design.md:66, 72-76). So two rebuild loops that both read "no maintenance standing" both commit. The
second takes a new epoch and deposes the maintenance target the first one just started, maybe on
another node. S17's "a second request for a chunk is refused by the first" holds for `MoveReplica`
(`CheckMove`), not for maintenance attaches. Nothing in #17 backs the design's "Reask only under
#17's conditional hold/epoch rules" (design.md:106); #17 has no expected-epoch condition.
The load also multiplies: each `serve` re-lists nodes, volumes and every chunk record every 5 s
(design.md:91, 97), with no idle short-circuit when no node is Failed. Every instance re-asks
refused moves at every pass with no backoff (R1-S2, R1-S5).
**Fix:** keep a loop in every serve (Q4), but let it act only while it holds the `rebuild-observer`
ownership, or a lease of its own. Skip the scan when no node is Failed. Back off per chunk after a
refusal. Test: two loops never create two maintenance epochs for one need.

### R1-S4 — Medium
**`isr_minimum` judged from records misses unheld volumes, whose stuck maintenance targets hold #17's slots.**
S15 says a chunk is at the minimum when "the lost replica is in the ISR, which is no larger than
the minimum". On a volume no target ran on, the record's ISR still lists every dead member. Take
RF 3, minimum 2, two failed members: the record shows ISR 3 > 2, so the chunk is not "at the
minimum". Yet a new target stays in `Fencing` (fenced 1 < min; flaminio-repl/src/target.rs:2252)
and never removes them. RF 1 volumes on a failed node behave the same way. Such a maintenance
target always has work, so it never ends (#17 S11). It is never asked again while it is alive and
started (#17 S17). So it keeps one of the node's 2 slots (#17 S14) for good. Lowest redundancy
first (S10) asks these volumes first, so they take the slots before volumes that could progress.
That breaks S15's "blocked chunks do not stop the rest".
**Fix:** call a chunk blocked when its ISR members on non-failed nodes are fewer than the minimum
(and keep today's rule for a running target with a write waiting). Ask no maintenance attach for a
volume whose every pending rebuild move is blocked that way; report it. Tests: RF 3/min 2 with two
failures, and RF 1, unheld; other volumes still get maintenance.

### R1-S5 — Medium
**#72 contradicted: the heartbeat's alive value, its refusals, and filling disks.**
design.md:22 says "No #72 draft exists". #72 is approved:
- Its `NodeHeartbeat` writes the capacity report as the value of `alive/<node>` by `PutTTL`
  (flaminio-72 design.md:44-47). #45 replaces that write with a transactional TTL put, "same 10 s
  semantics", and specifies no value (design.md:38-41). Whichever lands second either erases the
  reports, after which every create and move is refused (#72 S1, S5), or skips the state compare.
- #72 refuses rebuild moves with `RESOURCE_EXHAUSTED` or `FAILED_PRECONDITION` (#72 S5 names
  rebuild moves). S15 has no such reason, and the loop does not move to the next destination as
  #18 S14 and #72 S8 do for the drain.
- Until #72 lands, an automatic rebuild can fill destinations. A full replica fails its session
  (#72 S9), so healthy chunks there lose ISR members. That can follow a wrong declaration, for
  example a node cut off from the metadata service only.
**Fix:** state that the heartbeat transaction writes #72's report value (or `1`) on the fresh
lease. Add "no room / no capacity report" to S15, with #18 S14's next-destination rule. Gate
`enabled` on #72, or say plainly why not.

### R1-S6 — Medium
**S7's conditional write depends on #78, which the plan does not declare.**
Only #78's `placement-gate` fences a Fail against a racing `MoveReplica` or create claim
(design.md:50, 81-85: "Do not create a second placement gate"). The spec and intent assume only
#13, #17 and #18 (spec.md:4). Without #78, `MoveReplica` compares only the chunk and header
(service.go:905-907), so a Fail committed after its read still lets a move onto the failed node
commit.
**Fix:** declare #78 (and #76, design.md:85) as prerequisites, or specify that `MoveReplica` and
the create claim also compare the destination's `node-state/<id>` revision. Add a race test.

### R1-S7 — Medium
**`SetNodeState.action` has FAIL as its zero value.**
`enum action {FAIL=0; ADMIT=1;}` (design.md:133). A request with the action unset, from an old or
buggy client, declares the node failed. `expected_state_revision` 0 means "absent", which matches
every legacy node without a record. So `{node: X}` alone fails a legacy node: a sticky state that
starts copies. The enums also fail `buf lint` STANDARD (buf.yaml): ENUM_ZERO_VALUE_SUFFIX,
ENUM_VALUE_PREFIX, ENUM_PASCAL_CASE for `action`, and unprefixed `NodeState` values. So `make lint`
fails.
**Fix:** `enum SetNodeStateAction { SET_NODE_STATE_ACTION_UNSPECIFIED = 0; ..._FAIL = 1;
..._ADMIT = 2; }`, rejecting 0 with InvalidArgument; name the states `NODE_STATE_*`. Service and
auth test: an unset action changes nothing.

### R1-S8 — Low
**The agent may heartbeat, and a heartbeat now keeps a node out of Missing.**
`NodeHeartbeat` allows the node's `agent` as well as `io` (auth.go:58-59, 73-74). Only the data
plane heartbeats (flaminio-node/src/engine.rs:726). Under #45, an agent heartbeating while the data
plane and store are down keeps the node Active, so its chunks are never rebuilt. #72 S10 already
limits reports to `io`.
**Fix:** S8: a heartbeat that sets state comes only from the node's `io` identity (and
administrators). Auth test.

### R1-S9 — Low
**The lab procedure fails the nodes it stops for single-node verification.**
The procedure sets the wait to 5 s, then later says "Verify each remaining node alone with the
other data plane stopped" (design.md:170-173), with the policy still on. A node stopped longer than
TTL plus 5 s becomes Failed, sticky until admitted, which starts rebuild requests mid-check.
**Fix:** run `rebuild-policy … off`, or restore the wait, before that phase. Add the case to
`rebuild-test.sh`.

### R1-S10 — Low
**A fresh lease on every heartbeat roughly triples etcd's heartbeat writes.**
Each heartbeat becomes a lease grant, a transaction and, later, the expiry revoke of the replaced
lease, where today it is a renew and a put (design.md:40-41; kv/etcd.go:120-150). Renewing only
after a committed transaction is as safe: a Fail needs `alive` absent, so a renew can never
re-publish an expired key.
**Fix:** renew the node's current lease after the transaction commits, and grant a new one only
when it has expired. Test both stores.

### R1-S11 — Low
**The mixed-version guard proves nothing about the other instances.**
The guard says an "old-server UNSPECIFIED response prevents enabling mixed-version controller"
(design.md:155). One answer comes from one instance, so it cannot show that every instance is new
(S19). Old instances place on failed nodes and accept them as move destinations.
**Fix:** state that S19 is an operator rule (as #18 Q5 does), or make enabling compare a version
key that every new instance has written.

### R1-S12 — Low
**What `enabled` turns on is unspecified.**
The design keeps "Enabling timeout failure is blocked by the S5 choice", "Until then timeout
failure remains disabled" and "shorten wait … only after resolving S5" (design.md:75, 140, 170).
Decisions settle S5 and allow automatic failure once the credit clock passes review. There is one
`enabled` flag, and the design never says whether it starts the loop, timeout failure, or both, nor
what holds timeout failure off until that review.
**Fix:** either two flags (rebuild loop; timeout failure), or one, documented with its gate. Update
the stale lines.

## Nits
- design.md:180: "fl-c tombstone confirmation follows #14's explicit operator procedure". #13 S13
  removes #14's step 2 (`vol-confirm`, `flaminio-meta confirm`); with #13 assumed, the data plane's
  sweep confirms.
- design.md:85: the refusal of a failed destination needs a fixed message prefix, so that #18's
  drain (its S7, S12, S14) can tell it from #18's own refusal and from #72's or #78's.

Reviewed commit: 9acc14576c7414a20a55df1b5b1247dcc465340c
