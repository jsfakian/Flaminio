# Issue #17 plan review, round 1, design-security

Scope: docs/issues/17/{intent,spec,design,decisions}.md and docs/adr/0007-maintenance-attach-only-while-not-held.md
on issue/17-drain-unattached-volumes. Checked against the code in that worktree, #15's plan
(flaminio-15), #13's plan (flaminio-13), #74's plan, and #18/#79/#45 where they touch the same contracts.

### R1-S1 — High
**Two of the invariant's four guards have no injected bug, and the chaos never runs one of them.**
"A maintenance attach never takes a volume from a client" rests on four guards: AttachMaintenance's
not-held check, Release's node check, StartMaintenance's `target = node, hold = maintenance`, and
Publish marking a released record held again (S1, "takes a new epoch, even to the node the record
names"). The plan injects bugs only for the first two, plus `keeps_epoch`.
- If StartMaintenance skips its hold check, the node's io bumps the epoch of a volume a client holds on
  that node and deposes the pod's target. The Maintainer's list can be stale, so that call does happen.
  The check `maintenance epoch E follows a held record` catches this only if a "maintenance epoch"
  means an epoch taken by a maintenance transition. If it means one whose resulting `hold` is
  maintenance, the check misses it: the hold stays `client`. The design does not say which.
- Publish doing nothing on a released record that names its node is exactly the unsafe behaviour
  the design itself finds in an old instance (Risks, finding 3). But the simulator's failovers stay
  plain `MetaRequest::Attach` (`sim/mod.rs:1319`; design "Failovers stay client attaches"), so the
  Publish and Restart rows never run in the simulator.

Evidence: design.md:46-53 (table), :278-289 (chaos, checks, three mutations); spec.md:131-134;
ADR 0007 Confirmation; AGENTS.md Principles ("a simulator check and an injected bug" per new safety rule).

Fix:
- Have the chaos's client attaches use Publish and Restart, as the CSI controller and the agent do.
- Define the check by transition: no AttachMaintenance or StartMaintenance commits while the
  simulator's own hold record says held.
- Add `meta::Bugs::start_ignores_hold` and `meta::Bugs::publish_keeps_released`, each with its first
  seed recorded, to the design, the spec's Tests section and ADR 0007's Confirmation.

### R1-S2 — Medium
**The maintenance start does not fit #15's attach path. Either it never starts, or it runs at an epoch
not made durable.**
- The design starts a maintenance target "on `Input::Attach`'s path (#15 design, S11)", at generation 0,
  with "no registry entry", and lists the outputs it does not route. It never mentions the run mark.
- In #15's approved design, after `GetVolume` the engine "holds the target unstarted and emits
  `Output::MarkEpoch { volume, generation, epoch }`". That output goes "by `(volume, generation)`
  to the registry entry's waiter". The main thread makes the mark durable, then sends
  `Input::Marked`; "only then does the engine start the target". An unknown generation is
  "dropped and logged".
- So generation 0 with no entry never gets `Marked`, and the maintenance target never sends a
  `Hello`. If the implementer skips the mark instead, that breaks #15 S11 ("a target never runs at an
  epoch that is not durable here").
- The design's "an epoch already run ... refused" is the run mark. It lives in the registry, which the
  design itself puts out of the engine's reach (Risks, "Against #15's design"). Its facts about #15
  (design.md:29-32, "refuses an epoch already run") predate #15's change after approval (R1-S2:
  run mark replaces `ran_epochs`).

Evidence: design.md:104-108, :111-115, :353-354; flaminio-15 docs/issues/15/design.md:38-42, :538-560.

Fix: specify the maintenance target's mark. Either:
- the engine emits `MarkEpoch` with generation 0, and the main thread writes the mark with no entry
  and sends `Marked` back, refusing an epoch at or below the mark; or
- the plan argues why a maintenance epoch needs no durable mark (its token never outlives the
  process), drops the "already run" claim, and keeps an engine-side set of maintenance epochs run.

Add an engine test that the maintenance target's first `Hello` follows the mark, or the stated
alternative.

### R1-S3 — Medium
**Two drains, or a drain and a rebuild controller, depose each other's maintenance targets on every
step.**
- An ask "stands" only if this run made it: "not held, no ask of this run standing: AttachMaintenance".
- Ranking starts with "fewest standing asks of this run", so two runs choose differently.
- Take an RF 2 volume with replicas on two nodes drained at once (two `flaminio-meta drain`
  processes, a plausible case). Drain A asks X. Drain B sees no ask of its own and asks Y. A then
  sees a record naming Y and asks X again.
- Each `AttachMaintenance` takes an epoch and deposes the running target. That happens every 5 s
  step (`main.go:436`), against the node's 5 s scan, so the target may never start. Neither drain
  completes, and epochs churn.
- The same happens with a drain process still running after `undrain`, and with #45's planned
  rebuild controller, which already "reconstruct[s] standing asks ... from records"
  (flaminio-45 design.md:103-105).

Evidence: design.md:140-147; spec.md S15 ("unless one of its own is making progress").

Fix:
- Let any maintenance record stand, whoever asked for it, while its node is an eligible candidate.
- Re-ask only on S17's conditions (node dead, or no progress for `Stall`), timed from when this run
  first saw that record.
- Stop asking once the drained node is no longer draining.
- Test: two `Drainer`s on one in-memory service sharing a volume take at most one maintenance epoch
  per `Stall`.

### R1-S4 — Medium
**A maintenance record never ends.**
- Only a client attach clears `hold = maintenance` (table). The target ends at "no work", but the
  record keeps naming the node, and S11 and the tests restart it "once a move is requested".
- So one drain's choice becomes a standing right. Any later move on that volume (another drain, a
  manual `flaminio-meta move`, #45) makes that node take an epoch and a token. S16 (alive, not
  draining, stores the volume or is about to) is never checked again, and intent §5 ("the node already
  stores the volume, or is about to") does not hold over time.
- The cost also grows with every volume ever maintained: `ListMaintenance` reads every header per
  node every 5 s. Each idle entry costs a full `GetVolume`, chunks included, per node every 60 s,
  forever, and all of them at once after each data-plane restart. The design's Risks names the range
  read only.

Evidence: design.md:46-53, :96-103, :118-127, :297, :361-365.

Fix: end the record when the target ends at no work. A conditional `EndMaintenance(volume, node,
epoch)` by that node's io, applied only at its epoch and kind, sets hold := released. Add it to ADR
0007 while it is still Proposed. Additionally (or at least), have StartMaintenance enforce S16's rule
at token issuance: the node is in some chunk's `replicas` or `moving_to`, and is not draining.

### R1-S5 — Medium
**The approved spec and ADR 0007 still state two safety rules that the design's findings 2 and 3
break.**
- **Restart refused.** The Terms make "a node's own restart of its target" a client attach. S5 says a
  client attach "is never refused", and the ADR says it "always commits, whatever the record says".
  The design refuses `only_if_target` on a maintenance record (FailedPrecondition).
- **Mixed versions.** S19 says "Mixed versions stay safe ... never the reverse". Design finding 3 shows
  the reverse: an old instance's `unless_target` leaves a released record released while a client
  uses it (`service.go:716`). After that, a maintenance attach between publish and stage moves the
  target away, and the pod's `only_if_target` is refused for good.
- Neither finding is in decisions.md, and the second is "not enforced by code".

Evidence: spec.md:8-9, :47-49, :115-118; ADR 0007:31-33; design.md:61-62, :340-346.

Fix:
- Amend S4/S5 (a node's restart is a client attach only on a client or released record), S19 and the
  ADR's Decision, or record both findings as changes after approval.
- Enforce finding 3 at its only caller: `ControllerPublishVolume` fails as Unavailable unless the
  answer's `Volume.held` is true. An old instance never sets it, and the retry is safe. Test it in
  `csi_test.go`.

### R1-S6 — Medium
**`Volume` field numbers 9 and 10 collide with #18 and #79.**
- #17: `bool held = 9; bool maintenance = 10`.
- #18: `string failure_domain = 9; string failure_domain_policy = 10`.
- #79: `use_profile = 9`, `initialization = 10`.
- #13's design records the clash ("9 and 10 are #17's, #18's and #79's") and takes 15.

The wire types differ. When both plans land, prost fails to decode `GetVolume` on the data plane,
which breaks targets' polls. Go keeps the field as unknown, so `held` reads false.

Evidence: design.md:186; flaminio-18 docs/issues/18/design.md:115-117; /tmp/flaminio-plans-2026-11-12/79/docs/issues/79/initialization-contract.md:46;
flaminio-13 docs/issues/13/design.md:927-928.

Fix: take numbers no plan of the batch uses (16 and 17 are free by #13's list), in Contracts and in
`pb.rs`.

### R1-S7 — Low
**An unknown `hold` value fails open.**
- `Held()` is `Hold == "client"`, or `Hold == ""` with a target. Any other string counts as not held,
  so a maintenance attach can take the volume.
- Say a later version adds a client-like hold value and is rolled back, or a record is damaged.
  The volume is then free to maintenance: the unsafe direction S6 and S19 rule out.

Evidence: design.md:39-42.

Fix: `Held()` = target != "" && hold ∉ {released, maintenance}. Add a `domain_test.go` case with an
unknown value.

### R1-S8 — Low
**A stale client token ends a running maintenance target.**
- S12's path: `Input::Attach` "ends it, then goes on". #15's socket checks a token only against the run
  mark before `Input::Attach`.
- A client token at an epoch above the mark but below the maintenance target's passes that check
  (a token taken but never run, a retried lab `node.sh attach`). It kills the target, then fails
  `claims.epoch == volume.epoch`.
- The maintenance target then restarts at a new epoch. Repeated, maintenance never progresses.

Evidence: design.md:125-126; flaminio-15 design.md:68-70.

Fix: end a maintenance target only for a verified token whose epoch is above the target's own epoch.
Otherwise answer `error: VOLUME: epoch: ...` and keep the target. Add an engine test.

### R1-S9 — Low
**Volume ids from `ListMaintenance` are not validated, and the list is unbounded.**
- The design checks only the socket's input names ("names checked as #15's").
- Entries now come from the metadata service into the engine's start path, into log lines and into
  the `maintenance` socket line (`ok limit N [VOLUME STATE EPOCH ...]`), and possibly into #15's
  run-mark path (`DIR/epochs/<volume>`).
- The response has no cap; #13's `ListReleases` caps at 1024. A large one fails the decode and stops
  maintenance on that node.

Evidence: design.md:74-75, :210-211, :238-244; AGENTS.md ("Ids reaching ... socket lines are
validated").

Fix: the Maintainer drops and logs any entry that fails `ValidID`. Cap the response, as #13 does,
and test both.

### R1-S10 — Low
**A forgotten node (#13 S19) can still be a maintenance node.**
- #13 refuses moves, placement, registration and releases for a forgotten node, and `MoveReplica`
  refuses a forgotten `to`.
- #17's AttachMaintenance copies MoveReplica's "registered" check (`service.go:870`), but neither it
  nor StartMaintenance refuses a forgotten node.
- With R1-S4, a standing record lets a returning forgotten node take an epoch and a token.

Evidence: design.md:69-71, :76-79; flaminio-13 spec.md:140-147, design.md:247.

Fix: whichever plan lands second refuses a forgotten node in both calls (FailedPrecondition), with an
`auth_test`/`service_test` case. Note it in both plans.

### R1-S11 — Low
**#17 and #13 use the same words for different things in the same drain code.**
- **CLI commands one letter apart.** #17's `flaminio-meta release <volume> <node>` records a client
  detach. That is the gate a maintenance attach depends on, and the ADR's "operator's mistake" risk.
  #13 adds `flaminio-meta releases <node>`, which lists copies to delete.
- **"held".** #13 adds `Progress.Held` (chunks waiting on releases) and a `Progress.Released` count
  to `control/drain/drain.go`. #17 rewrites `Progress` (`Unattached` goes, `Volumes []VolumeState`
  comes), and its drain line says `V: held, moved by NODE`.
- #17 does not mention #13's drain changes at all.

Evidence: design.md:86, :153-154, :246-250; flaminio-13 design.md:785-802, :957.

Fix: rename #17's terms (for example `hold`/`unhold`, or `detached`, and drain state `in use`), and
state how `Progress` merges with #13's fields.

## Nits
- design.md:3 "There is no `decisions.md` yet" and spec.md:3 "Decisions and design come after
  approval" are stale: decisions.md exists and records both approvals.
- design.md:392 "Approving this design accepts it" contradicts decisions.md item 10 and the line
  right after it (ADR 0007 stays Proposed until accepted in its own step).

Reviewed commit: b904a18423c06700e895e54084c7cb746ed8d9bd
