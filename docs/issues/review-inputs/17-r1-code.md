# Plan review: issue #17, round 1, focus: design-code

Scope: docs/issues/17/ (intent, spec, design, decisions) and docs/adr/0007-maintenance-attach-only-while-not-held.md
on issue/17-drain-unattached-volumes. Cross-checked against #15, #13, #78, #72 and the batch manifest.

### R1-C1 — High
**A maintenance start's run mark (#15 S11) is left unspecified. Read literally, every start hangs.**
- The design starts a maintenance target "on `Input::Attach`'s path (#15 design, S11) ... an epoch
  already run or at or below the fence refused" (design.md:104-108). It also says a maintenance
  target "takes no registry entry, device id, record or slot, and routes no `Attached`,
  `AttachFailed`, `Detached` or `Deposed`" (design.md:114-115).
- On that path in #15, after `GetVolume` the engine holds the target unstarted and emits
  `Output::MarkEpoch { volume, generation, epoch }`. The main thread then writes and fsyncs the
  durable mark, and sends `Input::Marked`. Only then does the target start (#15 design.md:541-548).
  `MarkEpoch` goes by `(volume, generation)` to the registry entry's waiter (#15 design.md:39), and
  an unknown generation is dropped (#15 design.md:42).
- Generation 0 has no entry. So either `Marked` never comes and the target never starts, or the
  start skips #15 spec S11. That rule says the run mark is "durable before that target's first
  `Hello`", and an attach is refused "at or below the volume's run mark" (#15 spec.md:182-190).
- #17's S13 test "after a restart ... an old token refused" (design.md:300) depends on that mark.
  So does the S13 claim "never under an epoch it had".
Fix: say who writes a maintenance target's mark and how. The engine thread must not fsync under
its lock, so name the thread that does: for example, a mark writer the Maintainer's start waits on
before `Input::Marked`. The other way is to exempt maintenance epochs from the mark in #15's S11,
with the argument: `StartMaintenance` always takes a new epoch, and its token never leaves the
data plane. Add `MarkEpoch` to the list of outputs above, and an engine test that the mark is
durable (or deliberately unwritten) before the maintenance target's first `Hello`.

### R1-C2 — High
**As designed, the simulator cannot catch two of the three injected bugs.**
- The chaos makes "maintenance attaches when the simulator's own record of holds says not held"
  (design.md:279-280). The controller calls the model synchronously (`self.meta.handle(...)`,
  `sim/mod.rs:1319`). So when a maintenance attach reaches the model, the shadow record and the
  model agree that the volume is not held.
- `meta::Bugs::maintenance_ignores_held` therefore never meets a held record. Releases go to the
  shadow's target ("a live target's included"), and the next maintenance attach waits until the
  shadow says not held. So `meta::Bugs::release_any_node` never produces a maintenance epoch over a
  record the shadow holds.
- The only new check is "maintenance epoch E follows a held record" (design.md:281-282). Neither
  bug can trip it, so `catches_maintenance_over_held` and `catches_release_for_another_node`
  (design.md:285-289; spec.md:131-134; ADR Confirmation) would fail in `make sim`. ADR 0007's main
  rule would then have no working simulator check.
Fix: make maintenance attaches and releases without looking at the shadow. Either decide them at
random, or decide them from a stale snapshot taken earlier and deliver them later through `META`,
as a drain's read races a publish. Releases should name random nodes, not only the target. Keep
the check against the shadow at the moment of application. Optionally add a direct check that
the model's `held()` equals the shadow's after every transition.

### R1-C3 — High
**Standing asks are per run, so two drains (or a drain and `maintain`, or #45) override each other forever.**
- An ask stands only "while the record is a maintenance record naming the asked node". Any
  not-held volume with "no ask of this run standing" is asked again at once (design.md:140-144).
  The ranking starts with "fewest standing asks of this run" (design.md:145-147).
- Two `flaminio-meta drain` runs on nodes P and Q, sharing volume V, have different ask counts, so
  they can pick different nodes R and S.
- Each step (5 s, `main.go:436`), each drain finds the other's maintenance record and re-asks its
  own node. Each `AttachMaintenance` is a new epoch, which deposes the other's target before it
  has fenced, reconciled and copied a chunk. Neither drain completes.
- The same happens with an administrator's `flaminio-meta maintain`, and with #45, which plans to
  call `AttachMaintenance` and to "reconstruct standing asks/progress from records after restart"
  (#45 design.md:103-107).
- A drain rerun after its 30-minute default timeout deposes every running maintenance target and
  restarts its chunk copies.
- No test covers either case (design.md:303-306).
Fix: count as standing any maintenance record that names a node that is alive and eligible under
S16, whoever asked for it. Base the Stall clock on the record's epoch, not on an in-memory ask.
Add drain tests: two Drainers on one volume, and a Drainer started over an existing maintenance
record. Neither may re-ask while the replicas go down.

### R1-C4 — Medium
**`Volume` field numbers 9 and 10 are taken. Use 16 and 17.**
- Today `Volume` uses 1-8 (`api/proto/flaminio/meta/v1/meta.proto:117-127`).
- #17 claims `held = 9`, `maintenance = 10` (design.md:186), and the Rust `pb.rs` tags "Volume's 9
  and 10" (design.md:218-219).
- Numbers other plans claim:
  - 9-10: #79 `use_profile`, `initialization` (initialization-contract.md:45-46). #18 also claims
    them (`failure_domain`, `failure_domain_policy`, #18 design.md:115-117), and must move too.
  - 11-14: #70, as #75 counts them (#75 design.md:175).
  - 15: #13 `fenced` (#13 design.md:899, :927).
  - 20-21: #50 (contracts.md:38).
  - 30-31: #51 (contracts.md:10).
  - 40: #75 (design.md:171).
  - 40-41: #54 (contracts.md:16).
- The lowest numbers nobody claims are 16 and 17.
Fix: `bool held = 16; bool maintenance = 17;` in Contracts and in the Rust client's tags. The
orchestrator should give #18 numbers other than 16-17.

### R1-C5 — Medium
**The completion rule contradicts #78.**
- Spec S18 says "The drain completes when no chunk replica is left on the node, as today"
  (spec.md:111). The design says "The drain completes at `Remaining == 0`, as today"
  (design.md:153-154), and that `Run` keeps its signature.
- Under #78, `drain.Run` keeps `DrainNode`'s generation and calls `CheckDrain` every iteration. It
  "succeeds only on `complete=true`; old `Remaining==0` alone never succeeds". #78 also adds
  `Progress.PendingCreations` and `Incoming` (#78 design.md:95-100).
- #17 deletes the package function `Step`, which #78's `Run` calls ("runs ordinary `Step`").
Fix: in S18 and the design's `Progress`, say that completion is #78's `CheckDrain` once #78 lands,
whichever of the two lands second. Keep `PendingCreations` and `Incoming` in `Progress` and in the
CLI line. Say that #78's `Run` calls `Drainer.Step`.

### R1-C6 — Medium
**The drain and the new calls ignore what #13 adds.**
- **`Progress`:** #17 redefines `Progress` as `Remaining`, `Moving` and `Volumes` (design.md:153).
  Its CLI line has no room for #13's `Progress.Held` (reasons a chunk waits) or `Progress.Released`
  (#13 design.md:796-801), or for the `MoveReplicaResponse.reason` (field 3) that `Held` shows. #13
  "Held" (a chunk held back by releases) and #17 "held" (a client holds the volume) then share one
  drain line (design.md:247-250).
- **Chunks waiting under #13:** these chunks have no move: they hold 2 releases, or only releasing
  nodes are left (#13 design.md:789-795). If all of a volume's remaining chunks wait like this, it
  has no work, yet #17 still asks for a maintenance attach (design.md:138-142). The node takes no
  epoch (S9). The Stall rule then moves the ask to another candidate every 5 minutes, each time
  with a new epoch and a header write. The drain reports "maintenance on NODE" where the real
  reason is #13's wait.
- **Forgotten nodes:** #13's `ForgetNode` writes a drain marker, refuses a forgotten `to` in
  `MoveReplica`, and refuses the forgotten node's own calls (#13 design.md:162-170, :190-196,
  :225-230). `AttachMaintenance` checks only that the node is registered (design.md:68-71), and a
  forgotten node stays registered. So an administrator can name a forgotten node. A data plane
  forgotten while running can still `ListMaintenance`, and `StartMaintenance` on a record from
  before it was forgotten, and so become a target of a volume. That contradicts "retires the id for
  good".
Fix:
- Ask for a maintenance attach only when the volume has a pending move. Show #13's reason when it
  has none.
- Keep #13's `Held` and `Released` in `Progress` and in the line, and rename #17's per-volume
  state (for example `client`).
- `AttachMaintenance` and `StartMaintenance` read and compare #13's mark, and refuse a forgotten
  node with `FailedPrecondition`. `ListMaintenance` by a forgotten `io` gets `FailedPrecondition`.
  This lands with whichever of #13 and #17 comes second.
- Add a drain test of a volume whose chunks all wait on releases, and service tests for a
  forgotten node.

### R1-C7 — Medium
**Asking again on a stall can restart a slow move forever.**
- S17 asks again when the volume's replicas on the draining node "have not gone down for
  `Stall`". If the node took an epoch, it asks again, another candidate first (design.md:148-152).
  The drain counts progress only when a whole chunk completes.
- A target that dies mid-move leaves the destination "outside the ISR (it resyncs it in full)"
  (docs/replication-protocol.md:304). So each new ask restarts that chunk's copy from zero.
- A chunk whose copy takes longer than 5 minutes therefore never moves. That happens with large
  chunks (`ChunkBytes` has no upper bound, `meta/domain/domain.go:180`), with slow links, or with
  a background I/O throttle (#75).
- With one candidate, as for RF 1 (spec Open question 6), the drain re-asks the same node.
Fix:
- Do not re-ask a node that is alive and took an epoch since the ask.
- Otherwise measure progress by something a copy changes, or scale `Stall` with `chunk_bytes`.
- Add a drain test: a stall with an epoch taken on an alive node, with a fake clock, must not
  depose it while the chunk is still being copied.

### R1-C8 — Medium
**Finding 2 against the spec (a restart refused on a maintenance record) is not in the spec, the ADR or decisions.md.**
- The Terms make "a node's own restart of its target" a client attach (spec.md:8-9). S5 says a
  client attach "is never refused" (spec.md:47-49), and so does ADR 0007 ("it is never refused",
  :31-33).
- The design refuses `Restart` (`only_if_target`) on a maintenance record (design.md:50, :61-62,
  :340-342). decisions.md settles only finding 1 (item 11).
- The ADR lists three new transitions (:40-42), the spec says "four" (spec.md:180), and the
  design's table has six (design.md:46-53).
- The cost is understated. The ADR's accepted risk is a release recorded for a volume still in
  use. Suppose the drain then names the client's own node as maintenance node (it holds a
  replica). The pod's next `NodeStageVolume` gets `FailedPrecondition` on every retry: Kubernetes
  does not publish again, and nothing clears the maintenance record. That is a lockout, not "a
  failover" (ADR Consequences).
- Finding 3 also leaves S19's "Mixed versions stay safe" (spec.md:115-118) untrue as written.
Fix:
- Amend the Terms, S4 and S5: a restart is a client attach only on a client record.
- List `Restart`'s refusal (and `Publish`'s new epoch on a released record) in ADR 0007's Decision
  and in the transition count.
- Correct S19 with the rollout order, and record findings 2 and 3 in decisions.md.
- Alternatively, let `Restart` on a maintenance record naming the node take a new client epoch:
  S5 wins, and only `StartMaintenance` keeps the maintenance kind.

### R1-C9 — Low
**A maintenance record is never cleared.**
- No transition ends `hold = maintenance` except a client attach (design.md:46-53).
- After its target ends at no work, `ListMaintenance(node)` lists the volume forever. Every 60 s
  the scan reads the whole volume again (design.md:100-103), up to 2^20 chunks
  (`domain.go:184`). That read runs on the data plane's single, serial metadata thread
  (`flaminio-metaclient/src/lib.rs:107-111`), ahead of the targets' `ChangeIsr`.
- Any later move on the volume (another drain, #45, #72's rebalancing) starts a target on that
  node without an administrator's ask. The node rule is not checked again: S16 is applied only by
  the drain, at ask time.
Fix: either a conditional `EndMaintenance` transition (to `released`, at the target's epoch) when
the target ends at no work, or `ListMaintenance` that returns only records with work. Or accept
this and say so in the Risks.

### R1-C10 — Low
**"Held with no progress" has no mechanism or test.**
- S18 reports "waiting, and why (... held with no progress)" (spec.md:109-110), and the CLI has
  `held by NODE, no progress for 5m` (design.md:249-250).
- The `Drainer` tracks only its own asks. For a held volume it does "nothing. State `held`"
  (design.md:139). The drain tests list "held and not held volumes" only (design.md:303-306).
Fix: say that the `Drainer` keeps a per-volume progress clock for held volumes too, and add a
test.

### R1-C11 — Low
**Lab step 4 cannot be run as written.**
- "Again" is not defined. After step 3, fl-b holds nothing and both volumes are attached
  (held) on fl-a (design.md:315-317).
- Killing fl-c's data plane assumes fl-c is the maintenance node. The ranking ("most chunks of the
  volume, then name", design.md:146-147) may pick fl-a.
- fl-c is also the destination of fl-b's moves. While its data plane is down, those moves cannot
  finish, and today's drain stops at "no node to move to" (`drain.go:122`). So the drain completes
  only once fl-c restarts, and the step never restarts it.
Fix: state the setup (undrain fl-b and move the replicas back, or use new volumes, and release
them). Kill the node that `maintenance` lists on its own socket, and restart it, or make sure it
is not the only destination.

### R1-C12 — Low
**The ADR number must be 0014.**
- 0007-0011 are the audit's (`docs/adr/0007-takeover-readiness-and-clock-contract.md` and the
  others on audit/2026-11-12-correctness). #12 has 0012 and #16 has 0013.
- Evidence: `docs/adr/0007-maintenance-attach-only-while-not-held.md:1`; design.md:5-6, :182,
  :389-393; decisions.md:33-36; spec.md:181.
Fix:
- Rename the file to `0014-maintenance-attach-only-while-not-held.md` and its title to "0014.",
  and update every mention.
- Add the `docs/adr/README.md` row as Proposed now. The design is approved, so "when the design is
  approved" (design.md:182) no longer applies.

## Nits
- design.md:3: "There is no `decisions.md` yet." It exists. Remove the sentence.
- "release" names two unrelated things in one proto and one CLI:
  - #17's `ReleaseVolume` and `flaminio-meta release <volume> <node>` record the end of a client's
    hold.
  - #13's `ListReleases`, `ConfirmRelease`, `ChunkMeta.released` and `flaminio-meta releases <node>`
    concern copies to delete.
  Consider a distinct name for #17's call and command (for example `ReleaseHold` / `unhold`).

Reviewed commit: b904a18423c06700e895e54084c7cb746ed8d9bd
