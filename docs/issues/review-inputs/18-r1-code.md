# Issue #18 plan review: round 1, design-code

Scope: docs/issues/18/ (intent, spec, design, decisions) at the commit below, checked against the
code in /home/jsfakian/Documents/src/flaminio-18 and the plans of #13, #17, #72, #78 and #79.
The code facts the plan cites check out: service.go:337, :404, :414/:467, :541, :724, :870, :907,
:143; domain.go:37, :199, :259, :287, :311, :322; drain.go:74, :120, :122, :150, :181;
kv.go:18, :33; etcd.go:91; main.go:426, :436; lab.sh:609, :1279; pb.rs:56; Makefile:105;
domain_test.go:60. The cross-plan contracts do not.

### R1-C1 — High
**#13's hand-off is missing: `choose` does not exclude released nodes, and "only nodes releasing
it are left" would count as Blocked, which ends the drain.**
- #13 requires: "`choose`, which replaces the drain's `leastLoaded`, excludes the chunk's
  `GetReleased()` with its replicas. #18's text needs the change"
  (flaminio-13/docs/issues/13/decisions.md:235-236; design.md:792-795). Under #13's S16
  (spec.md:110-112, design.md:794-799), a chunk with only released nodes left **waits** and is
  reported in `Progress.Held`.
- #18 has eligible = "the candidates without a replica of the chunk", and "no node chosen: the chunk
  is counted [blocked]" (design.md:76-83). `Run` returns `BlockedError` once
  `Remaining == Blocked - Refused` (design.md:92-93). #18 never mentions #13's released nodes or
  `Held`.
- If #18 is built as written once #13 has landed: the drain's `load` does not count a released copy,
  so a released node is often the least loaded. #13's `CheckMove` answers `applied=false`, reason
  `node T releases chunk C` (#13 design.md:94-98). The chunk is then neither Moving nor Blocked, so
  `Run` asks for the same node at every step until its time-out, and never tries another allowed
  node. A chunk whose only allowed nodes are released ends the drain with `BlockedError`, where #13
  says it waits.
- The lab plan has the same gap. Step 3 drains fl-c back into fl-b (design.md:185-186), the move
  back that #13 makes `drain-test` refuse to run without `DELETE_RELEASED=1` (#13
  design.md:826). The deletions rely on `lab.sh vol-confirm` (design.md:193, :225), which #13
  removes (design.md:820).
- **Fix:** add to the drain section, whichever of #13 and #18 lands second, the order `choose`
  follows:
  1. A chunk at #13's bound waits before `choose`.
  2. The eligible nodes exclude the chunk's replicas and `GetReleased()`.
  3. The domain filter runs on what is left.
  4. If no node is left and a released candidate would pass the domain filter, the chunk goes in
     `Held` (`only nodes releasing it are left (N)`), not in `Blocked`. Otherwise it is Blocked.

  Add two drain tests: a released node is the least loaded, so the next allowed node is chosen;
  the only allowed node is released, so `Run` keeps going and `Held` is set. In the lab plan, step
  3 needs `DELETE_RELEASED=1` once #13 lands, and the deletions follow #13 (no `vol-confirm`).

### R1-C2 — High
**The drain takes any `FailedPrecondition` from `MoveReplica` as a collision, but #72 and #78 (both
review-clear) give the same code on the same RPC for other causes. S15 and the intent also keep
behaviour that #78 and #72 change.**
- #18 says: "`FailedPrecondition` only for a collision … The drain tells a refusal by the code
  alone" (design.md:124-126). The drain counts any `FailedPrecondition` as blocked and refused,
  tried again at the next step (design.md:84-85).
- #72's `MoveReplica` answers `FAILED_PRECONDITION` when `alive/<to>` has no valid report or
  `capacity/<to>` cannot be read (72/design.md:66-68). #72's drain, on `FAILED_PRECONDITION` or
  `RESOURCE_EXHAUSTED`, drops the destination for the step's later volumes, picks again, and with
  no node left fails the step with today's message (72/design.md:105-108).
- #78's S5 refuses a draining destination with `FailedPrecondition` "for every caller", and says
  #18's domain rules compose with that check (78/spec.md:49-53). Its ADR says "#18's draft
  permission for direct draining moves must be narrowed when combined" (adr-proposal.md:42-43).
  #18 still says "a destination named by hand that is a witness, dead or draining is accepted as
  today" (spec.md:99-100; intent.md:65-67). #78's `Run` succeeds only on `CheckDrain`'s
  `complete`, not on `Remaining == 0` (78/design.md:95-97), which #18's `Run` keeps
  (design.md:92).
- Combined as written:
  - A node without a valid report is counted "refused by the service". The drain asks for it again
    at every step, because its load does not change, and never tries another node. The drain runs
    to its time-out, and the progress lists the node as a domain block under the volume's
    level/policy.
  - With #72's rule applied to #18's refusal, a collision, which concerns one chunk only, removes
    the node for every later volume. "None left" then fails the step, which undoes Q1(b).
- **Fix:**
  - Make the collision refusal identifiable by more than its code, e.g. a `google.rpc.ErrorInfo`
    detail with reason `FAILURE_DOMAIN_COLLISION`. Say in Contracts that `FailedPrecondition` also
    carries #72's and #78's causes.
  - Drain: a collision blocks that chunk only. Any other `FailedPrecondition`, and
    `ResourceExhausted`, follow #72's drop-and-pick-again.
  - S15 and the intent: a draining destination is refused once #78 lands, and a dead one or one
    without a report once #72 lands.
  - `Run`: the `BlockedError` check sits beside #78's `CheckDrain`.
  - State the combined compare count of `MoveReplica`: chunk, header, #78's gate (or #13's mark),
    #72's ledger, the destination and at most 4 staying replicas, so at most 9, under 120. Also
    correct design.md:18: #18 alone compares at most 7 keys, not "8 (5 replicas)", since `from`
    is one of at most 5 replicas.
  - Add a test: an interceptor answers `FailedPrecondition` without the detail, and the chunk is
    not counted as refused.

### R1-C3 — Medium
**The `Volume` field numbers collide with #79's and must move to 18 and 19.**
- design.md:115-117 claims `failure_domain = 9` and `failure_domain_policy = 10`, and design.md:173
  repeats "fields 9 and 10".
- Numbers other plans hold:
  - #79: `Volume.use_profile=9`, `initialization=10` (79/initialization-contract.md:45-46)
  - #70 (counted by #75): 11–14
  - #13: 15 (flaminio-13 decisions.md:64-69)
  - #17: 16–17 (flaminio-17 design.md:530)
  - #50: 20–21 (contracts.md:37-38)
  - #51: 30–31 (contracts.md:10)
  - #75: 40 (design.md:171)
  - #54: 40–41 (contracts.md:16)
- meta.proto:117-127 ends at 8. No plan in the manifest claims 18 or 19 (searched every plan
  folder).
- **Fix:** `string failure_domain = 18; string failure_domain_policy = 19;` in Contracts. The Targets
  line becomes "fields 18 and 19".

### R1-C4 — Medium
**#18's drain does not compose with #17's.**
- #17 removes `Progress.Unattached`, adds `Volumes []VolumeState{ID, State, Node, Why, Since}`,
  and has `Run` hold a `Drainer` (flaminio-17 design.md:424-431, :469-473). #17 reads "not held,
  no move pending" as chunks waiting on #13's releases (design.md:432-433; spec.md:115-121).
- #18 anchors `Progress.String()` "before 'waiting for a target'" (design.md:89), adds a second
  per-volume list (`BlockedVolumes`, design.md:86-88), and does not say where `BlockedError` goes
  in a `Drainer`-based `Run`.
- With both plans, a volume whose chunks are all blocked has no move pending, so #17 reports it as
  `waiting`, with #13's reason, which is wrong.
- **Fix:** add a cross-plan paragraph, whichever lands second:
  - `BlockedError` is checked in `Run` after `Drainer.Step`, and before #78's `CheckDrain`.
  - A volume with only blocked chunks gets #17's state `waiting`, with why `no destination
    (zone/strict)` (or `BlockedVolumes` feeds `VolumeState.Why`).
  - `String()` places its text after the counts, not relative to the removed `Unattached`.

### R1-C5 — Low
**An unknown stored word stops the whole drain step, which contradicts S11 / Q1(b), and no test
covers it.**
- The design says "a word they refuse fails the step" (design.md:77-79). So one volume written by a
  later version stops every other volume's moves, against S11's "the step still requests every move
  it can, for that volume and the others" (spec.md:81-83).
- The service answers `Internal` for that volume only (design.md:40-42).
- The drain tests (design.md:162-167) do not include this case.
- **Fix:** count that volume's chunks on the node as blocked, with the words as read in
  `BlockedVolumes`, so the rest moves and `BlockedError` names it. Add a drain test. Or justify the
  step failure in the spec and test that.

### R1-C6 — Low
**S1's "not the level a `bestEffort` placement fell back to" has no test.**
- spec.md:34-36. The design's S1–S3 tests check that the asked level is recorded
  (design.md:136-144), but name no topology where the placement falls back.
- **Fix:** add to `placement_test.go`: a `zone`/`bestEffort` volume created where all nodes are in
  one zone (so it is placed at rack) reports `zone`, `bestEffort` after create, get and attach.

### R1-C7 — Nit
- design.md:93-94: "volumes waiting for a target … keep it going". An unattached volume keeps `Run`
  going only through a pending move. One whose chunks are all blocked ends it, like `TestDrain`'s
  `v2`.
- Reword this, so that no implementer adds `len(Unattached) > 0` to the condition for going on.

Reviewed commit: 12874537d333dc6a0fcd3a2ea4f5abcc86fee85d
