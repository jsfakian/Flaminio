# Issue #45 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.

Approved with the recommended answer to each open question (see Settled at approval). The plan's
review loop continues after approval; later fixes are recorded as changes made after approval.
Nothing is implemented. Approved before its plan review cleared (status: pending).
- Plan review round 1 (2026-11-14) fixes, made after approval: see below.
- Plan review round 2 (2026-11-14) fixes, made after approval: see below.
- Plan review round 3 (2026-11-14) fixes, made after approval: see below.
- Plan review round 4 (2026-11-14) fixes, made after approval: see below.
- Plan review round 5 (2026-11-14) fixes, made after approval: see below.
- Plan review round 6 (2026-11-14) fixes, made after approval: see below.
- Plan review round 7 (2026-11-16) fixes, made after approval: see below.
- Plan review round 8 (2026-11-16) fixes, made after approval: see below.
- Plan review round 9 (2026-11-16) fixes, made after approval: see below.
- Plan review round 10 (2026-11-16) fixes, made after approval: see below.
- Plan review round 11 (2026-11-16) fixes, made after approval: see below.
- Plan review round 12 (2026-11-16) fixes, made after approval: see below.
- Plan review round 13 (2026-11-16) fixes, made after approval: see below.

## Settled at approval

- **Q1. Disk identity:** (b), an issue of its own, which #61 and this issue's later steps depend
  on, before any failed node's disk is replaced. `spec.md`, Open questions Q1 and S14.
- **Q2. A failed node that is alive again:** (a), it stays failed until admitted. `spec.md`, Open
  questions Q2 and S6; the [ADR proposal](adr-proposal.md), Options.
- **Q3. Where "missing since" lives:** (a), a record in etcd, by the metadata service's clock.
  `spec.md`, Open questions Q3 and S2; `design.md`, State record, transitions and heartbeat
  ordering.
- **Q4. Where the rebuild runs:** (a), a loop in every `flaminio-meta serve`. `spec.md`, Open
  questions Q4 and S17; `design.md`, Destination admission and rebuild loop.
- **Q5. Volumes held by a target whose node is lost:** (a), reported: they wait for a failover.
  `spec.md`, Open questions Q5 and S15.
- **Q6. Chunks at their ISR minimum:** (a), reported; rebuilt with step 3; the check uses minimum
  1. `spec.md`, Open questions Q6 and S15.
- **Q7. A bound on moves asked at once:** (a), none, as the drain; the cap comes with step 4, from
  this step's numbers. `spec.md`, Open questions Q7 and S10.
- **Q8. The states in the Rust model and its vectors:** (a), S2 to S6 in both models; the
  rebuild's choices in Go only. `spec.md`, Open questions Q8; `design.md`, Files and tests.
- **Q9. Numbers, dependencies, ADRs:** the wait is 10 minutes by default (`design.md`: 600 s,
  accepted range 1 to 86400 s); of #44 this step needs the measurement only, so #44 is dropped
  from the dependencies; no ADR is contradicted. `spec.md`, Open questions Q9; `design.md`,
  Contracts and authorization.
- **The proposed outage-aware wait:** as the design states it: a persisted credit clock
  (`rebuild-clock`) fenced by one TTL observer (`rebuild-observer`), crediting only consecutive
  bounded successful observations. `design.md`, Outage-aware wait: proposed, not certified; the
  ADR proposal, Decision.
- **S5's availability authority:** no recommendation in the plan. Settled 2026-11-13 (user's
  instruction): the observed-opportunity definition (see below). The plan asks for
  an agreed observed-opportunity definition of S5, or a stronger availability authority, before
  automatic timeout failure is enabled; until then that failure stays disabled, as the plan
  states. `design.md`, Outage-aware wait (Unresolved S5 boundary) and Risks, ADRs and parallel
  work; the ADR proposal, Decision (Unresolved).
- **VM clock qualification under #83's proposed profile:** no recommendation in the plan.
  Settled 2026-11-13 (user's instruction): left to #83's own procedure (see below). `design.md`,
  Risks, ADRs and parallel work.

## Settled at the user's instruction of 2026-11-13

- **S5's availability authority:** the observed-opportunity definition. S5's wait counts only
  the credit of `rebuild-clock`: consecutive successful probes by the same live observer, each
  interval capped at 1 s, and none for a failed, late (> 2 s), restarted or ownership-changed
  probe or a wall jump. Automatic timeout failure is enabled on it once the credit clock passes
  its implementation review and tests (`design.md`, Files and tests, S2–S5). Recommended by
  Claude, no recommendation in the plan: it is the only defined option (the ADR proposal says
  not to invent a stronger authority), it is already designed, and by the invariant a wrong
  failure only costs copies, never an acknowledged write or a fence.
  **Changed after approval, in round 3 of the plan review, recorded in round 4; for the user to
  confirm:** a successful probe is no longer the service answering the observer, but a heartbeat
  of an active node that the owner saw commit, the sighting at most 5 s old. Each tick's interval
  is still capped at 1 s, and none is credited for a late (> 2 s), restarted or ownership-changed
  tick or a wall jump; but unobserved credit is now up to 6 s of each outage (the interval of the
  tick that sees the last heartbeat, at most 1 s, plus the 5 s after that sighting), not 1 s an
  interval. See "Changed after approval: plan review round 4", R4-1, and rounds 5 to 7.
- **VM clock qualification under #83's proposed profile:** #83's settled answer and procedure
  apply: current lab VMs are not a qualified profile, and qualifying a virtual environment needs
  #83's separately qualified profile with an enforceable hypervisor clock and rollback contract.
  #45 qualifies no clock of its own; its lab checks run only where #83's admission lets volumes
  attach, with no VM suspend or rollback. Recommended by Claude, no recommendation in the plan:
  #45's safety rests on no node clock (the credit clock adds credit only for heartbeats it saw
  commit, up to 6 s of each outage unobserved, and a wrong failure costs copies), and clock
  qualification is #83's evidence to gather, not a guess here. (Corrected in round 4: it said "at
  most 1 s per probe", the definition before round 3's change, below; in round 5, 5 s became 6 s.)

## Changed after approval: plan review round 1 (2026-11-14)

Reviewed commit 9acc145; findings of plan-reviewer (design-code, `R1-C*`) and plan-reviewer
(design-security, `R1-S*`). Each change below was made after approval, in round 1 of this batch's
review. Where a finding offered options, the simplest safe one was taken ("cut before adding").
No change narrows the intent; the two spec rules narrowed are named, with the finding that shows
the rule could not hold as stated.

- **#13's rules: releases, failed for good, and draining failed nodes** (R1-C1, High; R1-S1,
  High; R1-S2, Medium). Taken as #13's hand-offs state them.
  - The choice is #18's `choose` with #13's order: a chunk at #13's bound waits (`holding 2
    releases of chunk C`); the chunk's released nodes are not eligible; with only releasing nodes
    left it waits (`only nodes releasing it are left`). Both are S15 reasons. No move #13 refuses
    is asked (#13 S16); `Applied=false` is classified by #13's `reason`, not taken as a concurrent
    winner. Spec S9, S11, S15; design, The choice and A refused proposal.
  - **Failed for good:** a new `SetNodeState` action, `NODE_STATE_ACTION_FORGET`, and a
    `forget-node` CLI: refused unless the node is Failed, it runs #13's `ForgetNode`, whose mark
    transaction also compares the state revision read. Admit of a forgotten node is refused. The
    automatic, reversible Failed never forgets. The reviewer's example, taken as given: a separate
    RPC would duplicate `SetNodeState`'s checks. Spec Terms, S4, S6, S8; design, Administrator
    transitions and Contracts.
  - **Draining failed nodes** (spec S9, which now asks more): the rebuild re-protects a failed
    node whatever its drain mark. Of R1-S1's options the first, the cut: the exclusion
    bought no safety, since `CheckMove` admits one move per chunk, and a forgotten node is marked
    draining for good (#13) while a stopped drain leaves its mark (`drain.go:171-189`).
  - **The invariant and its simulator check** carve out failed for good, which is #13's forgetting
    and rewrites releases, fence holds and tombstones; its safety is #13's S19 and model.
  - **Lab and runbook:** release deletion on in the lab; fl-b back is checked as "no replica or
    move names it, and no release after its sweep"; the runbook forgets a dead failed node once it
    is re-protected. Tests: a rebuild beside a releasing node, each new reason, forget then
    rebuild, a stale drain mark.
- **#17's maintenance rules** (R1-C2, Medium; R1-S3, Medium; R1-S4, Medium). Taken as #17's
  S15–S17 state them.
  - The design's maintenance paragraph states each rule: any record stands, whoever asked; ask
    only while a move is pending; never ask again a node that is alive and took an epoch; a record
    ended with a move pending is a failed start, asked elsewhere at once; the rebuild's own
    applied ask counts as a read and resets the sighting's time and epoch; held is read from
    `held` (`EndMaintenance` clears a finished record to unheld, still naming its node); each
    answer's handling. Each has a test.
  - **The node is #17's S16, with no "also active"** (spec S12): a standing record on a Failed
    but alive node would otherwise be asked elsewhere, deposing a started target. Taken: #17's
    rule, which only widens the nodes S12 allowed.
  - "Release a hold" is now "unhold" (`UnholdVolume`) in the intent and Q5.
  - **One active loop** (R1-S3; spec S17): every `serve` runs the loop (Q4 (a) unchanged), and only
    the instance holding `rebuild-observer` acts; its `AttachMaintenance` compares that ownership,
    so concurrent or late loops cannot depose each other's maintenance targets. The same owner
    records Missing and timeout Failed (R1-C5). A step with no Failed node stops after the node
    listing. A refused chunk is not asked again for 30 s, in memory (spec S15 now says
    "considered again at every pass; asked again after a back-off": narrowed, since asking at
    every pass multiplies refused requests with no gain, R1-S3). Test: two loops, one
    maintenance epoch per need.
  - **At the minimum, unheld** (R1-S4; spec S9, S12, S15 narrowed): a chunk with fewer ISR members
    on alive nodes than the minimum is blocked; of a volume not held, it gets no move and no
    maintenance attach. The finding shows "its move is asked" cannot hold for such a volume: a new
    target stays in `Fencing`, never ends and keeps one of #17's two slots for good. Held volumes
    keep Q6's rule. "Alive" rather than the reviewer's "not failed": a new target fences with the
    members that answer, and a missing node does not answer, while a failed node back alive does.
    Tests: RF 3/min 2 with two failures, RF 1, unheld; other volumes still get maintenance.
- **#18's choice and refusals** (R1-C3, Medium). Spec S11 names #18's `choose` (`strict` and
  `bestEffort`); S15 uses #18's wording, "every one is refused by #18's S5", with
  `domain.Strict`'s reason. The design states #18 S14's two classes: a collision
  (`FailedPrecondition` with `failure domain collision: `) blocks the chunk for the pass, the
  destination kept; any other `FailedPrecondition` or `ResourceExhausted` takes the destination
  out of the pass and `choose` runs again. A refused chunk is blocked; the pass goes on. Tests: a
  collision refusal, a destination failed mid-pass.
- **#72** (R1-C4, Medium; R1-S5, Medium). The stale "No #72 draft" text is gone, and #72 is a
  dependency in the intent, spec and design (R1-S5's first option, the cut: the rebuild cannot fill destinations, and no
  "until #72" path remains). `MoveReplica` charges through #72's ledger; the rebuild keeps no
  accounting and skips a refused destination, as #72's S8 drain does; with none left, the chunk
  is blocked with the last refusal. The heartbeat transaction writes #72's value of
  `alive/<node>` (the report, or `1`). Test: a full destination skipped, the next chosen.
- **#78's gate** (R1-C7, Low; R1-S6, Medium). #78 is a dependency in the intent, spec and design;
  #76 is not (#78 works without
  it, its Q2). The cut: no fallback compare. The gate is bumped only into and out of Failed, never
  by Missing↔Active or heartbeats, as #78's rule says. Test: a Fail racing a move.
- **The enums** (R1-C6, Medium; R1-S7, Medium). buf STANDARD: `NodeState` values `NODE_STATE_*`,
  and `enum NodeStateAction { NODE_STATE_ACTION_UNSPECIFIED = 0; …_FAIL = 1; …_ADMIT = 2;
  …_FORGET = 3; }` (R1-C6's name; R1-S7's `SetNodeStateAction` passes lint too, and the shorter
  one was taken). An unset action is refused with `InvalidArgument`, never a failure; service and
  auth tests.
- **The mixed-version guard** (R1-C9, Low; R1-S11, Low). S19 is an operator's rule, as #18's Q5,
  in `docs/lab.md`; the test claim that an old server's answer prevents enabling is dropped. The
  cut: a per-instance version record would add a mechanism for a rollout rule.
- **What `enabled` gates** (R1-C5, Medium; R1-S12, Low). One flag, off by default: it gates
  timeout failure and the rebuild's requests. Missing is recorded whether or not it is on; the
  observer and its clock always run, the first owner creating the clock with its ownership.
  Timeout failure waits for the credit clock's implementation review (settled at the user's
  instruction, above) because ship-spec ships nothing unreviewed, and the flag stays off until an
  administrator turns it on. Of R1-S12's options, one flag: a second would add a state no rule
  needs. Test: policy off, Missing recorded,
  never Failed by timeout.
- **The policy in timeout Failed** (R1-C8, Low): its transaction compares the policy key's
  revision. Test: a policy change racing it.
- **The invariant for the service's writes** (R1-C10, Low): a service test on Mem and etcd that
  every `volumes/` and `chunks/` key keeps its revision across a heartbeat, Missing, timeout
  Failed, FAIL and ADMIT.
- **The probe** (R1-C11, Low): a linearizable etcd `Get` of `rebuild-observer` returning the
  owner's revision within 1 s, in a process whose gRPC server is serving. Not the reviewer's
  example of a loopback TLS `ListNodes`: the server's certificate is issued for server use only
  (`pki.Server`), so the probe would need a client identity of its own; the observer stopping
  when `Serve` returns gives the same evidence.
- **Small clusters** (R1-C12, Low): named in the ADR proposal's Consequences, the runbook and
  `docs/lab.md` (keep the policy off or the wait long where RF equals the data nodes). Test: two
  data nodes, one Failed and back, creation refused, then accepted after ADMIT.
- **A runnable lab plan** (R1-C13, Low): `make rebuild-test` and `scripts/lab.sh rebuild-test`,
  each step's `metactl` call, the `make help` text and the `docs/lab.md` entry.
- **The agent's heartbeat** (R1-S8, Low): `NodeHeartbeat` only by an administrator or the node's
  `io` (spec S8). Auth test.
- **The policy during single-node checks** (R1-S9, Low): the lab turns the policy off before it
  stops a node to verify the other; `rebuild-test.sh` covers it.
- **Heartbeat writes** (R1-S10, Low): the cached lease is renewed after a committed transaction,
  and a fresh one granted only when it expired. Tests on both stores.
- **Stale "blocked" and "unapproved" text** (code Nit): the design and the ADR proposal state the
  settled S5 rule and the settled questions instead.
- **#14's confirmation step** (security Nit 1): a deleted volume's tombstone is confirmed by the
  nodes' sweeps (#13 S13).
- **A fixed prefix for the failed-destination refusal** (security Nit 2): `node failed: `
  (`domain.ErrNodeFailed`), so #18's drain and the rebuild tell it from a collision.

## Changed after approval: plan review round 2 (2026-11-14)

Reviewed commit ba9d1d5; findings of plan-reviewer (design-code and design-security, one agent),
`R2-*`. Each change below was made after approval, in round 2 of this batch's review, taking the
reviewer's recommended fix. One spec rule is narrowed, named below with the finding that shows
it could not hold as stated.

- **FORGET is the only way to forget a node** (R2-B1, Medium; the coordinator's decision, #13
  being review-clear and not edited). The Failed check moves into #13's `ForgetNode` itself: its
  mark transaction compares `node-state/<id>` (Failed, at `expected_state_revision` through
  FORGET, at the revision read otherwise). #13's RPC therefore makes the same check, and `metactl
  forget NODE REASON` sends `SetNodeState` FORGET; #45's separate `forget-node` command goes.
  Spec S4; design, Administrator transitions, Contracts. Test: `ForgetNode` of an Active node is
  refused.
  **A change to #13, made by whichever of #13 and #45 lands second** (not made in #13's plan):
  - `ForgetNode`'s step 2 also reads and compares `node-state/<node>` and `alive/<node>`;
    refused with `FailedPrecondition`, `node N is not failed` or `node N is alive` (two more
    compares in the mark transaction);
  - `metactl forget NODE REASON` sends `SetNodeState` FORGET after reading the state revision,
    instead of `ForgetNode`; its usage names the two conditions;
  - #13's forget tests and fixtures first make the node Failed and not alive; new cases:
    `ForgetNode` of an Active node, and of a Failed node that heartbeats, refused;
  - #13's runbook line for `metactl forget` adds the #82 evidence rule below.
- **No forgetting of a live or merely partitioned node** (R2-S1, Medium). The mark transaction
  compares `alive/<id>` absent (`node N is alive` otherwise). The runbook, `docs/lab.md` and spec
  S4, S20 say that Failed, timeout or declared, is no evidence that a node is gone (a partition
  gives the same Failed), and that a node is forgotten only on evidence from outside the cluster
  bound to that machine, as #82's S1 and S2 require before a node-wide action. Test: FORGET of a
  Failed node that heartbeats is refused.
- **A new maintenance attach never picks a Failed node** (R2-C1, Medium). A standing record still
  stands by #17's S16 alone, a Failed node included, so a started target is not deposed; a new ask
  chooses among S16's nodes that are also Active, else `no maintenance node`. Spec S12. Test: a
  Failed, alive node holding the most chunks is not chosen for a new ask, while a standing record
  on it stands.
- **The probe shows that a heartbeat could commit** (R2-B2, Medium). The probe is now the tick's
  checkpoint, a conditional write: an interval is credited only by the checkpoint that closes it,
  and only if it commits within the bound; a refused checkpoint (NOSPACE, no quorum, a lost
  compare) adds zero, and no checkpoint is written while the serving certificate (the leaf
  loaded at start) is outside its validity. This replaces round 1's linearizable `Get`, whose
  "same evidence" claim did not hold for an expired certificate. Tests: a refused checkpoint and
  an expired serving certificate each add zero.
- **No maintenance target that can never end** (R2-S2, Low; spec S12 and S15 narrowed). The
  reviewer's first option: a volume not held with any pending move that cannot be carried out
  (below the minimum on alive nodes, or its destination lost) gets no maintenance attach, and its
  other chunks wait, reported with that reason. S15's "blocked chunks do not stop the rest" now
  holds across volumes but not within such a volume: the finding shows that an attach there keeps
  one of #17's two slots for good (#17 S11, S14, S17), which stops other volumes. The Risks line
  names the case left, a target already started whose work later becomes uncarriable. Tests: one
  carryable move beside another chunk's below the minimum; a move whose destination is lost.
- **The lab's checks under #13** (R2-C2, Low). fl-b back is checked as "no release, and only its
  fence-only record of each volume" (`node.sh list`), since a node that confirmed a release keeps
  the volume's fence (#13 S3, S10); spec Tests to match. Before step 1, `metactl nodes` must show
  every data node Active and alive, or the run fails before any change.
- **A forgotten destination's refusal** (R2-N1): `Applied=false` with #13's reason `node T is
  forgotten`, or, with #78, `FailedPrecondition` `is draining`; either takes the destination out
  of the pass. Removed from the `FailedPrecondition` list it was in.
- **Which refusals start the back-off** (R2-N2): a collision, no candidate left after refusals,
  and `Applied=false` with a release reason; not a destination refusal followed by an applied
  move, `Applied=false` without a reason, or `NotFound`. The 30 s include the rest of the pass, so
  S11's "for the pass" agrees (S11 now says "for the pass and the back-off after it").
- **The creation claim's compares** (R2-N3): no admission transaction compares a `node-state/`
  key; #78's gate, bumped by every write into or out of Failed, covers them with one key.
- **A non-owner asks nothing for how long** (R2-N4): for the rest of the step and every step it
  runs without the ownership; once it holds the ownership again, it rebuilds its sightings from
  the records and acts.

## Changed after approval: plan review round 3 (2026-11-14)

Reviewed commit 890f77b; findings of plan-reviewer (design-code and design-security, one agent),
`R3-*`. Each change below was made after approval, in round 3 of this batch's review, taking the
reviewer's recommended fix and preferring cuts. No spec rule is narrowed; S7 widens.

- **#13's tests under the change to #13** (R3-C1, Medium). Round 2's "#13's forget tests and
  fixtures first make the node Failed and not alive" is replaced, so that #13's race tests stay
  meaningful. The change to #13, still made by whichever of #13 and #45 lands second:
  - `TestForgetRacesAMoveBack` and `TestForgetRacesAPlacement`: the node is Active and alive when
    the move or the claim reads it. The hook (on the move's header `Get`, on the creation's `List`
    of `drain/`) commits FAIL, lets `alive/<node>` expire (Mem's clock), then runs `ForgetNode`.
    The move's or claim's commit then fails on #78's gate, bumped by FAIL and by the forgetting's
    drain marker; the move's retry is refused with `node failed: `, and the creation's second claim
    places elsewhere (exactly two claims). Each still fails with the gate compare removed.
  - `TestForgetNodeRefusals`: the node is made Failed and not alive first. A move to it expects
    `node failed: ` (not `node T is forgotten`), and the creation does not place on it (it is
    Failed and not alive). The mark's own refusals stay: `RegisterNode` (changed and unchanged),
    `DrainNode` undraining it, its `io`'s `ListReleases` and `ConfirmRelease`, and a drop of a
    move off it releasing nothing.
  - `TestForgetNodeClears`, `TestForgetKeepsARunningDrain`, `TestForgetRacesAConfirmation`,
    `TestForgetRacesADeletion`: the node is made Failed and not alive before `ForgetNode`; nothing
    else changes.
  - New: `ForgetNode` of an Active node, and of a Failed node that heartbeats, is refused, and
    writes nothing.
  - Unchanged from round 2: `ForgetNode`'s step 2 reads and compares `node-state/<node>` and
    `alive/<node>`; `metactl forget NODE REASON` sends `SetNodeState` FORGET; #13's runbook line
    gains the evidence rule.
- **The stuck unheld volume** (R3-B1, Low). A volume not held with a pending move whose
  destination is gone for good never gets a maintenance attach under #45: no move is withdrawn,
  and #13's forgetting does not clear `moving`. Named in Risks and the runbook as a risk not
  accepted, with the operator's step: attach the volume, or `flaminio-meta maintain` it and have
  root end the target once its other moves are done (#17 S14). Withdrawing such a move is a new
  transition (intent, Out of scope): a follow-up (spec S20). The runbook's forget condition is now
  "not alive and no chunk names it as a replica", since moves to a dead destination may remain.
- **The probe writes nothing** (R3-S1, R3-C2, R3-C4, all Low). The cut the coordinator asked
  for: no per-tick checkpoint. A tick credits its interval (capped at 1 s, none past 2 s) when its
  linearizable read of `alive/` shows a node's heartbeat committed within the last 5 s, by the
  owner's monotonic clock. A committed heartbeat is S5's opportunity itself, through whichever
  instance took it, and the heartbeats already write. So:
  - **R3-S1:** no certificate check and no wall clock remain. An owner whose certificate expired
    credits from heartbeats other instances take; if every instance's certificate expired, none
    commits. No owner-side condition stops credit while another instance could credit, so no
    rule for giving up the ownership is needed; an owner that cannot reach etcd loses its 5 s
    ownership as before.
  - **R3-C2:** "late" is a tick more than 2 s after the previous, credited zero before any write.
    The clock is written once a 5 s pass, only while some node is Missing; a write that commits
    after its context ends keeps its credit, which heartbeats already seen earned, and the next
    pass anchors on the reread. Tests for both.
  - **R3-C4:** #45 adds at most one etcd revision every 5 s, and only during a node's outage (120
    at most for a node failed by timeout at the default wait). Compaction is the one heartbeats
    already need: the runbook and `docs/lab.md` state it, and the lab's `etcd.sh`, which sets
    none, gets a follow-up (spec S20).
  The settled observed-opportunity definition keeps its numbers (1 s cap, 2 s late); what counts
  as a successful observation is now a heartbeat seen committed, which bounds unobserved credit at
  5 s an outage instead of 1 s an interval. A cluster with no other node heartbeating credits
  nothing, so it fails none by timeout (none could take the chunks).
- **A drain's maintenance asks** (R3-C3, Low). #45 applies "also Active" to every new maintenance
  ask under #17's S16, the drain's included (spec S7), with a drain test. **A change to #17, made
  by whichever of #17 and #45 lands second** (not made in #17's plan): the drain's node for a new
  `AttachMaintenance` is #17's S16 and also not Failed (`node-state/`), ranked as before; a
  standing record still stands by S16 alone; test: a drain passes over a Failed, alive node
  holding the most chunks, and a standing record on it stands.
- **#82 as an analogy** (R3-N1): spec S20 and the runbook say "by analogy with the standard #82
  sets for node-wide recovery", and that #82's evidence provider is not part of FORGET.
- **`--insecure`** (R3-N2): with the certificate check gone, an instance started `--insecure`
  credits from heartbeats like any other.

## Changed after approval: plan review round 4 (2026-11-14)

Reviewed commit d490961; findings of plan-reviewer (design-code and design-security, one agent),
`R4-*`. Each change below was made after approval, in round 4 of this batch's review.

- **The settled S5 definition changed, and round 3 said it had not** (R4-1, Medium). Round 3's
  "the settled observed-opportunity definition keeps its numbers" was wrong: the definition the
  user settled on 2026-11-13 credits consecutive successful probes of the service, 1 s an
  interval; round 3 credits a tick whose last seen heartbeat committed within 5 s, so up to 5 s of
  each outage counts. **The user-settled item changed; the user is to confirm it.** Kept as
  round 3's rule, not the stricter "credit an interval only when a heartbeat committed within it
  was seen": each node heartbeats every 3 s and a tick lasts 1 s, so with heartbeats spread
  evenly a tick sees none with chance (2/3)^N for N live nodes, two thirds with one (an edge
  site) and four ninths with two; the 10-minute wait would then stretch to about 30 or 18
  minutes, by cluster size alone. Corrected to match: the settled S5 item (a note appended to the
  user's text, which stays as given), the VM-clock paragraph ("at most 5 s of each outage", not
  "1 s per probe"), spec S5 (its gloss states the bound: S5 changed), the design and the ADR
  proposal. Round 3's "no spec rule is narrowed" stands for its own changes; this round changes
  S5's statement.
- **The last rise starts unset** (R4-2, Low). Both the newest revision and its rise start unset,
  and are unset again after a failed read, a late tick or an ownership change; a rise counts only
  when the owner itself saw the value grow, so an old key read once credits nothing. Tests: an old
  `alive/` key with no rise, at a new owner and after a late tick: zero.
- **Only active nodes' heartbeats count** (R4-3, Low). The tick counts only the keys of data nodes
  Active at the last pass: a Failed node's heartbeat says nothing of other nodes, and counting
  fewer only delays a failure. Test: a Failed node's heartbeat alone credits nothing.
- **#13's placement race and its forgotten refusals** (R4-4, Low). Two more items of the change
  to #13, made by whichever of #13 and #45 lands second:
  - `TestForgetRacesAPlacement`: `nodes()` reads `node-state/` after `alive/` and before
    `drain/`, so the hook on its `List` of `drain/` runs after the first attempt read the node
    Active and alive; that attempt places on it, its claim fails on #78's gate, and the second
    places elsewhere (exactly two claims). The design states the read order.
  - New `TestForgottenRefusedWithoutFailed`: the forgotten mark written directly (kv) for a node
    whose state is Active and alive, without a drain marker: a move to it answers `Applied=false`,
    `reason` `node T is forgotten`, and a creation leaves it out (an RF that needs it is refused as
    a short topology). This keeps #13's mark refusal on both paths covered, which the Failed
    check otherwise hides.
- **The tick reads revisions only, and its shared state** (R4-5, Low). A keys-only range read
  (`kv.ListRevs`, etcd `WithKeysOnly`), no values, so #72's reports are not read every second.
  The tick runs in the observer's goroutine and the 5 s pass in another; they share their state
  under `clockMu`, held for in-memory updates only.
- **The credit write's lost reply** (R4-6, Low). The pass writes persisted plus a snapshot A of
  the accrued credit; A is subtracted when the write commits, or when a reread after an unknown
  outcome finds exactly the intended value (sequence, credit, nonce); otherwise A stays and the
  next write goes against the reread sequence. Tests for both.
- **The drain's file** (R4-N1): `control/drain/drain.go` in design.md's file list, with the #17
  and #18 changes it carries.

## Changed after approval: plan review round 5 (2026-11-14)

Reviewed commit f902a18; findings of plan-reviewer (design-code and design-security, one agent),
`R5-*`. Each change below was made after approval, in round 5 of this batch's review, taking the
reviewer's recommended fix.

- **The credit write sets a total, so a late commit cannot double count** (R5-1, Medium). Round
  4's rule (subtract a snapshot A when the reread finds the intended value, else keep it) counted
  A twice when the lost write committed after the reread. Now the owner reads the clock's credit
  B0 on taking the ownership and keeps T, its monotone total credited since; every write puts
  B0 + T, compared by sequence. Any outcome is followed by a reread of the sequence only: a late
  commit stores an older total, which the next write replaces, and nothing is subtracted. Test:
  a write that times out, whose reread sees the old value, then commits late: persisted credit
  equals the true credit, once.
- **The bound is up to 6 s of each outage** (R5-2, Low): 5 s from the rise the owner saw, plus the
  one tick (1 s) it may take to see it. Stated the same in spec S5, the design, the ADR proposal,
  the settled-item note and the VM-clock paragraph; tested as stated (an outage beginning just
  after a heartbeat commits credits at most 6 s). The settled-item note gives the exact bound for
  the user's confirmation.

## Changed after approval: plan review round 6 (2026-11-14)

Reviewed commit 7f66c24; findings of plan-reviewer (design-code and design-security, one agent),
`R6-*`. Each change below was made after approval, in round 6 of this batch's review, taking the
reviewer's recommended fix.

- **B0 is tied to the ownership commit** (R6-1, Low). B0 is read after the ownership transaction
  commits, so every older owner's write has either committed into B0 or is refused by the
  ownership compare. T starts at zero, and B0 is reread, at each acquisition of the ownership
  (worded "ownership revision" until round 7, R7-B1). Nothing
  is credited or written until the B0 read succeeds. Tests: an old owner's write in flight at the
  takeover (refused, or already in B0; the stored credit never below the old owner's last
  committed value); a regained ownership rereads B0; nothing before the B0 read.
- **Tests that separate the rules** (R6-2, Low). New: a new owner's first write is exactly the
  previous owner's last stored credit plus its own T (it fails if B0 is read before the ownership
  commits, or if T is not reset). The late-commit test now asserts that the stored value after
  the late commit is the older total, then replaced; it is the one that fails under round 4's
  rule. The two lost-reply tests are marked as regressions, which round 4's rule passed too.
- **The pass rereads the whole clock, and decides on the stored value** (R6-3, Low). The reread is
  of the sequence and the stored credit; timeout Failed is decided against the stored clock (the
  value written, or as reread), never against the in-memory B0 + T. A lagging stored value only
  delays a failure.
- **The 6 s explained by the 1 s cap** (R6-N1): the interval of the tick that sees the last
  heartbeat, at most 1 s, plus the 5 s after that sighting (corrected in round 7, R7-N1: it
  said "at most six ticks", which assumes ticks 1 s apart). The bound is unchanged; spec S5,
  the design, the ADR proposal and the settled-item note give this explanation.

## Changed after approval: plan review round 7 (2026-11-16)

Reviewed commit 08d9550; findings of plan-reviewer (design-code and design-security, one agent),
`R7-*`. Each change below was made after approval, in round 7 of this batch's review, taking the
reviewer's recommended fix.

- **Acquisition, not renewal, resets T and reads B0** (R7-B1, Low). "Ownership revision" could
  mean each renewal. Now: an acquisition is the conditional write that creates `rebuild-observer`
  or replaces another owner's, with a new nonce; a renewal is a keepalive of the key's lease
  (`kv.Renew`), which puts nothing, so the key's revision and nonce stay those of the
  acquisition. T starts at zero and B0 is read at an acquisition only; the owner keeps both
  across a renewal. "Ownership revision" is replaced in the design, and annotated in round 6's
  entry above. Test: a renewal changes neither B0, T nor the key's revision, and credit and
  writes go on across it.
- **The reason for the 6 s** (R7-N1, Nit; the report's R7-B2). The bound holds because each
  tick credits its own monotonic interval, capped at 1 s: the sighting tick's interval, at most
  1 s, plus the 5 s after the sighting. "At most six ticks" is dropped (ticks may be closer than
  1 s after a late one). Same wording in spec S5, the design, the ADR proposal, the settled-item
  note and round 6's entry. The bound is unchanged.

## Changed after approval: plan review round 8 (2026-11-16)

Reviewed commit 26b8a54; findings of plan-reviewer (design-code and design-security, one agent),
`R8-*`. Each change below was made after approval, in round 8 of this batch's review.

- **`kv.Renew`, kept and fully specified** (R8-1, Medium). Checked first whether it could be cut.
  The heartbeat's lease renewal (round 1, R1-S10) exists only inside a transactional TTL put: it
  cannot renew without writing. Using it for the ownership is the "no new primitive" option, a
  conditional put by the same nonce at each renewal. Not taken, for two costs: a put at every
  renewal adds an etcd revision several times per 5 s TTL for as long as the service runs, which
  undoes round 3's R3-C4 (no revisions outside an outage); and it changes the key's revision at
  every renewal, so every transaction that compares the ownership could be refused by the owner's
  own renewal, needing a retry rule by nonce and its tests. One method costs less. What is shared
  is specified once: a committed transactional TTL put registers its lease under the key with
  its revision, for heartbeats and the acquisition alike. `Renew(ctx, key, rev) error`: extends
  the TTL only while the key is the entry written at `rev`, else `kv.ErrLost`; writes nothing,
  never grants; scoped to the acquisition in both stores (etcd: the registered lease, a gone
  lease is lost; Mem: only if the entry's `Rev` equals `rev`). Cadence: every 1 s tick, in the
  tick's goroutine, never behind a pass. Files: `meta/kv/{kv.go,etcd.go,mem.go}` and the kvtest
  suite, with four cases on both stores.
- **The renewal test can fail** (R8-2, Low): credit x, a renewal, credit y, the next write stores
  B0 + x + y, with exactly one B0 read and no gap in the writes; it fails if a renewal or a new
  key revision resets T or rereads B0.
- **A lost lease ends the ownership at once** (R8-3, Low): on `ErrLost`, on a refused ownership
  compare, or after 5 s with no renewal succeeding, the instance credits, writes and asks nothing
  more and drops B0 and T. Test: later ticks credit nothing and no write is sent.
- **The run-on line** (R8-N1): design.md's ownership paragraph is reflowed.

## Changed after approval: plan review round 9 (2026-11-16)

Reviewed commit c8d0892; findings of plan-reviewer (design-code and design-security, one agent),
`R9-*`. Each change below was made after approval, in round 9 of this batch's review, taking the
reviewer's recommended fix; R9-3's failed-reread rule is the coordinator's.

- **The etcd `Renew` checks the key's revision** (R9-1, Medium). As specified in round 8 it only
  kept the registered lease alive, and an overwritten or deleted key leaves its old lease alive,
  so a displaced owner renewed for ever and etcd disagreed with Mem. Now it first reads the key
  and requires `ModRevision == rev`, then `KeepAliveOnce`. Only a key gone or at another
  revision, or `rpctypes.ErrLeaseNotFound`, is `ErrLost`; every other error passes through. A
  displaced owner's leftover lease is dropped from the registry and left to expire, not revoked:
  nothing keeps it alive, and a revoke would delete a key still attached to it. The
  displaced-owner kvtest case runs on real etcd too, with a second `KV` on the same etcd.
- **The no-renewal bound is below the TTL** (R9-2, Low): 4 s against the 5 s TTL, measured from
  the send time of the last renewal that succeeded, so the old owner stops crediting at least 1 s
  before the key can expire and be acquired again. Test: a renewal answered late.
- **The reread after a refused transaction** (R9-3, Low). Another revision or none ends the
  ownership; a failed reread ends it too, fail closed (the coordinator's choice; the reviewer
  suggested leaving it to the next renewal), and the instance may acquire again; the
  acquisition's own revision leaves the ownership, B0 and T, the refusal being another
  compare's. Tests for each.
- **What the lease registration holds** (R9-4, Low): the lease and the revision of the put, for
  `PutTTL` as for a transactional TTL put; replaced under `e.mu` only by a committed put with a
  newer revision, so of overlapping puts the later commit wins; a refused transaction leaves the
  registration and revokes the lease it granted. Tests for each.

## Changed after approval: plan review round 10 (2026-11-16)

Reviewed commit db80c68; findings of plan-reviewer (design-code and design-security, one agent),
`R10-*`. Each change below was made after approval, in round 10 of this batch's review, taking
the reviewer's recommended fix.

- **A late-answer test that can happen** (R10-L1, Low). Renewals run under a 1 s context, so
  "answered 3 s late" could not occur. The test is now a renewal answered at the context limit,
  on a fake clock: the ownership ends 4 s after its send, 3 s after the answer. The send time is
  taken before `Renew` is called, so before its `Get`.
- **Where the 4 s bound is checked, and when the ownership starts** (R10-L2, Low). The bound is
  checked, under `clockMu`, before every credit, every use of B0 or T and every write or ask,
  not only when a renewal returns. The acquisition no longer starts the bound: it may put on a
  lease extended long before, so the ownership is held only from the first renewal that succeeds
  after the acquisition commits, and the bound runs from that renewal's send. Of the two options
  (count the acquisition only with its keepalive, or a fresh lease for it), the first, as that
  renewal: it reuses `Renew`, where a fresh lease would add a second lease path to the TTL put.
  Tests: a tick past send + 4 s credits nothing; an acquisition whose first renewal fails.
- **`ErrLost` drops only its own registration** (R10-L3, Low): under `e.mu`, only if it is still
  the same lease and revision the call started with. Test: a late `ErrLost` for an old revision
  leaves a newer registration.

## Changed after approval: plan review round 11 (2026-11-16)

Reviewed commit c9e3472; one finding of plan-reviewer (design-code and design-security, one
agent). The change below was made after approval, in round 11 of this batch's review, taking the
reviewer's recommended fix.

- **The ticks before the ownership is held** (R11-L1, Low). From the acquisition until the first
  renewal succeeds, ticks credit zero and still advance the tick time, the newest revision and
  its rise, so no later tick credits that span and none is dropped as late because of it. The
  first credit is delayed by the renewal's round trip, at most its 1 s context, plus one tick per
  failed renewal, which only delays a failure. Both are in the "first renewal fails" test.
  Round 12 added the tick after the success to this delay: see below, R12-L1.

## Changed after approval: plan review round 12 (2026-11-16)

Reviewed commit 1da70f8; findings of plan-reviewer (design-code and design-security, one agent),
`R12-*`. Each change below was made after approval, in round 12 of this batch's review, taking
the reviewer's recommended fix.

- **The first credit comes one tick after the success** (R12-L1, Low). The renewal runs after the
  tick's read, so the tick in which it first succeeds has credited zero already. The delay is
  stated as up to the next tick after the first renewal succeeds: the renewal's round trip, at
  most 1 s, plus that tick, plus one tick per failed renewal. Corrected in the design and in
  round 11's entry.
- **The "first renewal fails" test pins both properties** (R12-L2, Low): at least three failed
  renewals, so the failures span more than 2 s, with a heartbeat rise seen during them, then a
  success; the first credited tick is the one after the success and adds exactly its own 1 s:
  not zero, not the span before it, not dropped as late.

## Changed after approval: plan review round 13 (2026-11-16)

Reviewed commit bac60f2; findings of plan-reviewer (design-code and design-security, one agent),
`R13-*`. Each change below was made after approval, in round 13 of this batch's review.

- **The test pins the round trip** (R13-L1, Low): the "first renewal fails" test gains a variant
  in which the successful renewal is answered at its 1 s context limit; the first credit is
  still at the first tick after the answer, adding exactly its own interval.
- **Round 11's entry reworded** (R13-N1, Nit): the parenthetical inside the delay's terms is now a
  sentence of its own, "Round 12 added the tick after the success to this delay".
