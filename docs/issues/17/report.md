# Issue #17 — report

## Review rounds

### Plan review round 1 — b904a18

Scope: correctness against code and security of the plan, approved 2026-11-13; fixes are
changes made after approval. No runtime implementation.
Reviewers: plan-reviewer (design-code), plan-reviewer (design-security).

- **R1-C1 — High, R1-S2 — Medium:** the maintenance start, at generation 0 with no registry
  entry, did not fit #15's attach path: #15's `MarkEpoch` goes by `(volume, generation)` to an
  entry's waiter and an unknown generation is dropped, so the target either never started or
  ran at an epoch whose run mark was not durable, against #15's S11 and #17's S13. Evidence:
  design.md:104-115, :353-354; flaminio-15 docs/issues/15/design.md:38-42, :538-560; #15
  spec.md S11.
- **R1-C2 — High, R1-S1 — High:** the chaos made maintenance attaches and releases only when
  the simulator's own record allowed them, and the controller calls the model synchronously, so
  `maintenance_ignores_held` and `release_any_node` could never trip the one new check;
  StartMaintenance's hold guard and Publish's re-hold had no injected bug, and the failovers
  never ran Publish or Restart. Evidence: design.md:46-53, :278-289; spec.md:131-134; ADR
  Confirmation; `sim/mod.rs:1319`; AGENTS.md Principles.
- **R1-C3 — High, R1-S3 — Medium:** an ask stood only if this run made it, and the ranking
  counted this run's asks, so two drains (or a drain and `maintain`, or #45) re-asked over each
  other's maintenance records every 5 s step, each a new epoch deposing the other's target, and
  neither completed. Evidence: design.md:140-147; spec.md S15; `main.go:436`; flaminio-45
  design.md:103-107.
- **R1-C4 — Medium, R1-S6 — Medium:** `Volume` fields 9 and 10 collided with #79's (and #18's)
  claims; with both landed, prost fails to decode `GetVolume`. Evidence: design.md:186,
  :218-219; `meta.proto:117-127`; #79 initialization-contract.md:45-46; flaminio-18
  design.md:115-117; flaminio-13 design.md:927-928.
- **R1-C5 — Medium:** S18's "completes when no chunk replica is left" and the deletion of the
  package `Step` contradicted #78, whose `Run` calls `Step` and completes only on `CheckDrain`,
  with `PendingCreations` and `Incoming`. Evidence: spec.md:111; design.md:135-136, :153-154;
  /tmp/flaminio-plans-2026-11-12/78/docs/issues/78/design.md:95-100.
- **R1-C6 — Medium, R1-S10 — Low, R1-S11 — Low, code Nit:** #17 redefined `Progress` without
  #13's `Held` and `Released`, used "held" and "release" for other things than #13 does in the
  same code and CLI, asked maintenance for volumes whose chunks only wait on #13's releases, and
  let `AttachMaintenance` and `StartMaintenance` name a node #13 has forgotten. Evidence:
  design.md:68-71, :86, :138-142, :153, :246-250; flaminio-13 design.md:162-170, :190-196,
  :225-230, :785-802, :957; flaminio-13 spec.md:140-147.
- **R1-C7 — Medium:** asking again after 5 minutes without a whole chunk moving restarted a
  copy that took longer (large `chunk_bytes`, a slow link, #75's throttle) forever. Evidence:
  design.md:148-152; docs/replication-protocol.md:304; `meta/domain/domain.go:180`.
- **R1-C8 — Medium, R1-S5 — Medium:** the design refused `Restart` on a maintenance record,
  against the Terms and S5 ("never refused") and the ADR; a wrong release then locked a pod's
  node out; the transition counts disagreed (ADR 3, spec 4, design 6); and S19's "mixed versions
  stay safe" was untrue (an older instance's `unless_target` leaves a released record released),
  with neither finding in decisions.md. Evidence: spec.md:8-9, :47-49, :115-118, :180; ADR
  0007:31-33, :40-42; design.md:46-53, :61-62, :340-346; `service.go:716`.
- **R1-C9 — Low, R1-S4 — Medium:** nothing ended a maintenance record, so the node listed and
  read it forever, and any later move started a target there with no new ask and no S16 check.
  Evidence: design.md:46-53, :96-103, :118-127, :297, :361-365; `domain.go:184`;
  `flaminio-metaclient/src/lib.rs:107-111`.
- **R1-C10 — Low:** "held with no progress" had no mechanism or test. Evidence: spec.md:109-110;
  design.md:139, :249-250, :303-306.
- **R1-C11 — Low:** lab step 4 ("again") had no setup, assumed fl-c was the maintenance node,
  and killed the only destination of some moves without restarting it. Evidence:
  design.md:146-147, :315-317; `drain.go:122`.
- **R1-C12 — Low:** ADR 0007 is the audit's number (0007-0011; #12 has 0012, #16 0013).
  Evidence: `docs/adr/0007-maintenance-attach-only-while-not-held.md:1`; design.md:5-6, :182,
  :389-393; decisions.md:33-36; spec.md:181.
- **R1-S7 — Low:** `Held()` failed open on a `hold` value it did not know. Evidence:
  design.md:39-42.
- **R1-S8 — Low:** any client token above the run mark ended a maintenance target, then failed
  its epoch check, so a stale token could stall maintenance. Evidence: design.md:125-126;
  flaminio-15 design.md:68-70.
- **R1-S9 — Low:** volume ids from `ListMaintenance` reached the engine, logs, the socket and
  the run mark's path unvalidated, and the list had no cap. Evidence: design.md:74-75, :210-211,
  :238-244; AGENTS.md.
- **Nits (both reviewers):** design.md:3 "There is no `decisions.md` yet"; spec.md:3 "Decisions
  and design come after approval"; design.md:392 "Approving this design accepts it", against
  decisions item 10.

Changes/resolution:
- R1-C1, R1-S2: the engine hands the `StartMaintenance` token to the main thread
  (`Output::Maintain`), which reserves a registry entry of kind maintenance with a generation
  (counted against `--max-maintenance` only), writes the run mark on `MarkEpoch` and sends
  `Input::Marked`, so before any `Hello`, as #15 says. S9 says so; engine tests of the mark. The
  registry now covers S9's "runs no target". S13's mutation becomes
  `sim::Bugs::maintenance_skips_run_mark` (the old `keeps_epoch` would be refused by the mark),
  with #15's check of two incarnations at one epoch.
- R1-C2, R1-S1: chaos makes maintenance attaches and unholds whatever the record says, unholds
  name any node, the `Maintainer`'s starts and ends reach the model late, and failovers use
  `Publish` and `Restart` as well as `Attach`. Checks at application, by transition:
  `maintenance over a held record` and `hold lost`. New injected bugs `start_ignores_hold`,
  `publish_keeps_unheld`, `end_any_hold`, with the spec's Tests and the ADR's Confirmation.
- R1-C3, R1-S3: any maintenance record stands while its node is eligible under S16, whoever
  asked; "fewest standing asks of this run" goes; the stall clock runs from when this run first
  saw the record; no ask once the node is undrained, and `Run` ends. S15, S17 changed; drain
  tests of two `Drainer`s and of one started over a standing record.
- R1-C4, R1-S6: `held = 16`, `maintenance = 17`, in Contracts and in `pb.rs`.
- R1-C5: completion is #78's `CheckDrain` once #78 lands; `Progress` and the line keep
  `PendingCreations` and `Incoming`; the package `Step` stays as one step of a new `Drainer`,
  and whichever of #17 and #78 lands second makes `Run` call `Drainer.Step`. S18 changed.
- R1-C6, R1-S10, R1-S11, code Nit: #17's release is renamed unhold (`UnholdVolume`,
  `flaminio-meta unhold`, hold value `unheld`, bug `unhold_any_node`); the drain's per-volume
  state is `in use`; #13's `Progress.Held` and `Released` kept; a maintenance attach only with a
  move pending, else `no move pending` with #13's reason; `AttachMaintenance` and
  `StartMaintenance` refuse a forgotten node with `FailedPrecondition`, comparing #13's mark,
  and so does `ListMaintenance` for its own `io` (whichever of #13 and #17 lands second).
- R1-C7: S17: a node that is alive and took an epoch since the drain first saw the record is
  not asked again; the 5 minutes stay for a node that took no epoch. The finding's first option
  (a cut) over scaling `Stall` by `chunk_bytes`; a residual case is in the Risks.
- R1-C8, R1-S5: S5 kept: `Restart` on a maintenance record naming its node is a client attach,
  which deposes the maintenance target; S4 amended. `ControllerPublishVolume` answers
  `Unavailable` when the answer's `Volume.held` is false, so S19 holds as stated (tested in
  `csi_test.go`). The ADR lists the four new transitions and the attach's three modes, the
  spec's count is corrected, and the accepted risk names the lockout when maintenance lands on
  another node after a wrong unhold. Findings 2 and 3 recorded in decisions.md.
- R1-C9, R1-S4: new `EndMaintenance(volume, node, epoch)`, that node's `io` only, applied only
  to a maintenance record naming it at that epoch (hold := unheld); sent when a target ends (no
  work, root, a client attach) and for a listed record without work. Chosen over a filtered
  `ListMaintenance`, since only it ends the standing right; R1-S4's further S16 check in
  `StartMaintenance` not taken. S11 and the ADR say so.
- R1-C10: the `Drainer` keeps, per volume, when its replicas on the node last went down, and
  the line says `no progress for D`; tested.
- R1-C11: step 4 is a second round on two new volumes after `undrain`; the node whose
  `maintenance` lists the target is killed once every move is requested, then started again.
- R1-C12: `git mv` to `docs/adr/0014-maintenance-attach-only-while-not-held.md`, title "0014.",
  every reference in docs/issues/17/ updated, README row added as Proposed.
- R1-S7: `Held()` is a target with a hold neither `unheld` nor `maintenance`; domain test.
- R1-S8: the socket ends a maintenance entry only for a verified token above its epoch, else
  `error: VOLUME: epoch: ...` and the target runs on (S12); test.
- R1-S9: entries failing `ValidID` dropped and logged; `ListMaintenance` at most 1024; tests.
- Nits: the three stale lines removed or corrected.

Commit: `c25679e`. Recorded in decisions.md as changes made after approval, plan review round 1
(2026-11-14).

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 2 — 937f1e1

Scope: correctness against code and security of the plan, approved 2026-11-13, incremental on
the diff b904a18..c25679e (937f1e1 adds only report.md); fixes are changes made after approval.
No runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Earlier findings: R1-C1 to R1-C12, R1-S1 to R1-S11 and both reports' Nits found fixed. The fixes
of R1-C1, R1-C2, R1-C7 and R1-C8 opened R2-B1, R2-C2, R2-C1 and R2-C4.

- **R2-C1 — High:** `catches_end_over_held` could not fail. The simulator's hold record took
  every applied end as "not held", just as `end_any_hold` does, and neither check looked at an
  end, so a late end over a client's record went unseen and a later maintenance attach deposed
  the client. Evidence: design.md:391-397, :407-408; spec.md:159-165; ADR 0014:97-104; AGENTS.md
  Principles.
- **R2-B1 — Medium:** a start that took its epoch and then failed (refused at the registry or
  the run mark, `AttachFailed`, a failed mark, a time-out) reached no one. The `Maintainer`
  either kept counting it or took an epoch at every scan. Meanwhile the drain never asked a node
  that "took an epoch" and stayed alive elsewhere, so it waited forever on #15's persistent
  triggers (a stopped device entry, an unwritable state directory, a malformed mark). Evidence:
  design.md:139-158, :215-223, :320-325, :510-512; spec.md:120-127; #15 design.md:228-229,
  :553-554, :569.
- **R2-C2 — Medium:** nothing ended a maintenance entry on `Detached` after `Attached`, so its
  `--max-maintenance` place leaked, and S12's client attach waited out its 30 s and failed. A
  start's wait did not stop on `Detached` either. Evidence: design.md:145, :154-156, :162-164,
  :169-170, :178-183; #15 design.md:39-40, :79-89, :226-233; `flaminio-io/src/ublk.rs:697`.
- **R2-C3 — Low:** with rollout step 2 no longer waiting for step 1, an unhold answered
  `Unimplemented` by an older instance counted as success and was lost, leaving the volume held
  for good. Evidence: design.md:120-121, :235-241; decisions.md:111-122.
- **R2-C4 — Low:** the reversed `Restart` row had no injected bug. Evidence: design.md:64,
  :80-84, :400-412; spec.md:162-165; ADR 0014:101-104.
- **R2-N1 — Nit:** the spec's list of rules changed after approval left out S2 (whose condition
  changed) and S20; decisions.md did not record S2's change. Evidence: spec.md:5-6.
- **R2-N2 — Nit:** S19's "changes nothing" holds only for a record naming the publishing node.
  Evidence: spec.md:140-142; design.md:118, :238.
- **R2-N3 — Nit:** the Terms called "a node's own restart of its target" a client attach, while
  S4 makes the maintenance node's restart a start. Evidence: spec.md:11-12, :48-50.
- **R2-N4 — Nit:** "timed from when this run first saw its record at its epoch" read as a clock
  restarting at each epoch, against the rule not to ask a copying node again. Evidence:
  design.md:193, :215-216, :221-223.

Changes/resolution:
- R2-C1: the record takes each request as the model should admit it: an end leaves it not held
  only at the node and epoch it last recorded as maintenance. Each check runs before the record
  takes the request, and `maintenance over a held record` covers `EndMaintenance`. The spec's
  Tests and the ADR's Confirmation say so.
- R2-B1: every failed start (including a `Detached` before `Attached`) goes back as
  `Input::MaintainFailed { volume, epoch }`. It reaches the `Maintainer` as `End::StartFailed`,
  which frees the start's place and backs the volume off for 5 minutes, and the engine sends
  `EndMaintenance` at that epoch. S17 and the drain: after `Stall` without a replica moved, a
  node that took no epoch in the sighting, or more than one since the later of the sighting's
  start and the last progress, is asked elsewhere; one epoch counts as a start. S11 and S17
  changed. Tests: a `Maintain` refused over a stopped device entry takes no second epoch before
  the back-off; a failed mark ends the record's maintenance; a node whose epoch rises at every
  step is asked elsewhere after `Stall`. The Risk that called the pass-over a mere saving is
  corrected.
- R2-C2: the router, routing a maintenance entry's `Detached` after the engine's lock is let
  go, removes the entry under the registry's lock, which frees its place, and wakes its
  waiters. The main thread's start wait ends at once on its generation's `Detached`. Tests: the
  entry and its place are gone after each kind of end, and a client attach over a start in
  flight reserves without waiting it out.
- R2-C3: every rollout step waits for all of the one before again. `ControllerUnpublishVolume`
  answers `Unimplemented` as `Unavailable`, so Kubernetes retries; `lab.sh vol detach` fails on
  a failed unhold. S19, the ADR's rollout consequence and `csi_test.go` say so. The publish
  check of `held` stays.
- R2-C4: `meta::Bugs::restart_keeps_maintenance` (`catches_restart_keeping_maintenance`, first
  seed to be recorded), in the design, the spec's Tests and the ADR's Confirmation (seven bugs).
- R2-N1: the spec lists the Terms, S2 and S20 as changed; decisions.md records S2's change (it
  matches the design's `Held()` row and S6).
- R2-N2: S19 says "not held and names the publishing node"; over another node, the old
  instance rewrites the header, which S6 counts as held.
- R2-N3: the Terms name the node agent's `only_if_target` restart; the maintenance node's own
  restart is a start (S4). S4 and the ADR say "the agent's restart".
- R2-N4: the drain's sighting (its node, epoch and time) is set at this run's first sight of a
  maintenance record naming that node. It is kept across new epochs and across the drain's own
  new ask of the same node, until the drain asks another node or the volume is held (design and
  S17).

Commit: `fdf6b6e`. Recorded in decisions.md as changes made after approval, plan review round 2
(2026-11-14), items 29–37.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 3 — a4c01a7

Scope: correctness against code and security of the plan, approved 2026-11-13, incremental on
the diff 937f1e1..fdf6b6e (a4c01a7 adds only report.md); fixes are changes made after approval.
No runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Earlier findings: R2-C1, R2-B1, R2-C2, R2-C3, R2-C4, R2-N1, R2-N2 and R2-N4 found fixed; R2-N3
fixed in the spec and the ADR but not in the design's Scenarios line (R3-N1). The fixes of
R2-B1, R2-C2 and R2-C3 opened R3-C1, R3-C3, R3-C2 and R3-B1.

- **R3-B1 — Medium:** a rollback of the metadata service alone blocked every Kubernetes publish
  and unpublish, and no order for rolling back was stated. In a mixed window the "retry" never
  reached a new instance while an old one was up: `metaCall` moves on only on `Unavailable`,
  `DeadlineExceeded` or `Canceled`, and stores any other answer's instance as `preferred`.
  Evidence: design.md:116-122, :257-270; ADR 0014:88-90; spec.md:144-146;
  `control/csi/meta.go:63-84`.
- **R3-C1 — Low:** a failed start after its reservation was ended and reported twice. The main
  thread's `Input::Detach` produced `Detached` (an S11 end with `on_ended` and `EndMaintenance`),
  and then `MaintainFailed` reported it again. The router's removal was not tied to a
  generation, so it could end a client's entry let in after it. Evidence: design.md:157-158,
  :160-166, :174-178, :194-200, :352-355; #15 design.md:87-89, :253.
- **R3-C2 — Low:** a socket's detach that reached the engine before the start's `Input::Attach`
  left an `Attaching` that nothing ended, and the client's attach then failed until a restart.
  Evidence: design.md:145-150, :158-162, :174-178, :194-199; `engine.rs:649-654`; #15
  design.md:253, :365-366.
- **R3-C3 — Low:** S17 counted the drain's own `AttachMaintenance`, and a start retried after a
  lost answer, as epochs taken by the node, so "more than one" could depose a healthy copying
  target. Evidence: `domain.go:217-218`; design.md:66, :170, :210-213, :227-229, :240-243;
  spec.md:124-131.
- **R3-C4 — Low:** the design and the spec put `hold lost` before the record takes the request.
  Read that way, every correct client attach trips it, and `catches_restart_keeping_maintenance`
  could not fail. Evidence: design.md:425-428; spec.md:168-171; ADR 0014:104-105.
- **R3-N1 — Nit:** the design's Scenarios line called "the maintenance node's own restart"
  taking a client epoch, against the Terms. Evidence: design.md:449-451; spec.md:13-14.
- **R3-N2 — Nit:** `on_ended` took no `now`, yet `StartFailed` starts a 5-minute back-off in a
  sans-I/O machine. Evidence: design.md:348-355.

Changes/resolution:
- R3-B1: the unhold and the publish's attach go through a `metaCall` variant. An
  `Unimplemented`, or an answer without `held`, moves to the next instance within the same call
  and leaves `preferred` unchanged; only when every instance answers as old is the call
  `Unavailable`. The rollback order (the CSI controller and `lab.sh` before the metadata
  service, since an older service fails every publish and unpublish) is stated in the design's
  Rollout, S19, ADR 0014 and the planned `docs/lab.md` text. New `csi_test.go` case: two
  instances, the first old; a publish and an unpublish both succeed through the second in one
  call, and `preferred` is unchanged.
- R3-C1: the main thread is a failed start's one ender and reporter. The router ends, on
  `Detached`, only a running entry, and only the entry of that generation. The engine ends a
  maintenance attach that never reached `Attached` without S11's end. `on_ended` ignores a
  `(K, epoch)` it no longer holds. Test: after a failed mark, one place freed, one
  `EndMaintenance`, no "ended by root" line.
- R3-C2: on every failed start, a `Detached` before `Attached` included, the main thread sends
  `Input::Detach` with the generation after its own `Input::Attach`. Test: the socket's detach
  ordered before the start's attach; the client attach succeeds, and no `Attaching` remains.
- R3-C3: the drain counts no epochs (the finding's first option, which excludes both the
  drain's own re-ask and a retried start). A failed start is seen as the sighting's node ending
  its maintenance record while a move is still pending, and the drain asks elsewhere at once.
  The only epoch compared is whether the node took any since the sighting began or since the
  drain's own last ask of it. The `Maintainer` resends a failed start's `EndMaintenance` while
  the volume is still listed at that epoch. S17 changed. Tests: a re-ask after an end at no
  work, and a start retried after a lost answer, each copying past `Stall`, are not asked
  elsewhere; a failed start is asked elsewhere at once. Residual, in the Risks: a crash loop
  that keeps the node's heartbeat is not asked elsewhere.
- R3-C4: `maintenance over a held record` runs before the record takes the request, and `hold
  lost` after it, in the design and the spec, as in the ADR.
- R3-N1: the Scenarios line now reads "the agent's `only_if_target` restart on the maintenance
  node".
- R3-N2: `on_ended(now, K, Epoch, End)`; the back-off is timed from `now`.

Commit: `9e3a5e2`. Recorded in decisions.md as changes made after approval, plan review round 3
(2026-11-14), items 38–44.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 4 — 958436b

Scope: correctness against code and security of the plan, approved 2026-11-13, incremental on
the diff a4c01a7..9e3a5e2 (958436b adds only report.md); fixes are changes made after approval.
No runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Earlier findings: R3-B1, R3-C1, R3-C2, R3-C3, R3-C4, R3-N1 and R3-N2 found fixed. Their fixes
opened R4-C1, R4-C3 (R3-C1), R4-C2, R4-C4 (R3-C3) and R4-N1 (R3-B1).

- **R4-C1 — Low:** nothing said who marks a maintenance entry running. A `Detached` routed after
  its `Attached`, but before the main thread took `Attached`, found a starting entry and was
  ended by nobody, so the entry's place leaked and S12's client attach failed until a restart.
  Evidence: design.md:164-165, :189-194, :503-508; #15 design.md:163-166.
- **R4-C2 — Low:** the "failed start, asked elsewhere at once" rule needed a previous read
  showing the node's maintenance record. A start that fails within one drain step, the usual
  case, was missed: the drain asked the same node again and moved on only after `Stall`. The
  test scripted the intermediate read. Evidence: design.md:166-181, :233-234, :243-249,
  :262-267, :532-533; spec.md:126-127; `main.go:436`.
- **R4-C3 — Low:** step 5 sent `Input::Detach` "with the generation" even for a refusal before
  the entry, which has neither an entry nor a generation. `Engine::detach` works by volume
  alone, so it could end a client's target. Evidence: design.md:153-157, :166-171;
  `engine.rs:1856-1860`; #15 design.md:248-253.
- **R4-C4 — Low:** the sighting's clock was kept across the drain's own re-ask, so the Stall
  rule could fire at the next step, before the re-asked node started, and a lower-ranked
  candidate's attach could depose a start just begun. Evidence: design.md:229-233, :268-270,
  :534-535; spec.md:128-131.
- **R4-N1 — Nit:** the test's "`preferred` is unchanged" could not fail: `metaCall` stores the
  instance that answered, and the old instance was the default index 0. Evidence:
  design.md:443-444; `control/csi/meta.go:82`.
- **R4-N2 — Nit:** a test line said the router's removal "of a later generation's entry leaves
  a client's entry alone", the opposite of the rule. Evidence: design.md:500-501.

Changes/resolution:
- R4-C1: the router marks the entry running as it routes its `Attached`, under the registry's
  lock. A `Detached` routed first is seen first by the main thread, which ends the entry (step
  5); one routed after finds the entry running, and the router ends it. Test: a `Detached`
  routed right after `Attached`, before the main thread takes it; the entry and its place are
  gone.
- R4-C2: the drain's own applied `AttachMaintenance` counts as a read of that node's
  maintenance record with a move pending (stated where the sighting is defined). So a start
  that fails before the next step is a failed start there, asked elsewhere at once. Test: the
  start fails between the ask and the next step, with no read scripted between them.
- R4-C3: of the two fixes, the cut. The main thread sends `Input::Detach` only for an entry it
  reserved, with that entry's generation; a refusal before the entry sends nothing. Every
  `Input::Detach` left names an entry its sender holds, so no engine generation check is added.
  Test: a refusal leaves a client's target running.
- R4-C4: the drain's own applied ask sets the sighting's time together with its epoch, so
  `Stall` waits for the re-asked node; the node's own new epoch still resets neither. S17
  changed. R2-N4's reason for keeping the time is gone now that failed starts are seen
  directly. Test: a re-ask with both clocks long past `Stall`, the node not yet started at the
  next step, is not asked elsewhere before `Stall` from that ask.
- R4-N1: the variant never stores the old instance as `preferred` (it stores the instance that
  answers as new, as `metaCall` does). The test starts with `preferred` at the old instance.
  Round 3's "leaving `preferred` unchanged" is corrected in decisions.md.
- R4-N2: the line now reads "the router's removal on the maintenance generation's `Detached`
  leaves a later client entry alone".

ADR 0014 is unchanged this round: none of these findings touches its text.

Commit: `517c8d6`. Recorded in decisions.md as changes made after approval, plan review round 4
(2026-11-14), items 45–50.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 5 — 5455e49

Scope: correctness against code and security of the plan, approved 2026-11-13, incremental on
the diff 958436b..517c8d6 (5455e49 adds only report.md); fixes are changes made after approval.
No runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Earlier findings: R4-C4, R4-N1 and R4-N2 found fixed. R4-C1, R4-C2 and R4-C3 were fixed in
intent, but opened R5-3, R5-1 and R5-2.

- **R5-1 — Low:** read literally, the failed-start rule fired on a start that had not happened
  yet. One step after the drain's own ask (which now counts as a read), the record still names
  the node, not held, with a move pending; that is also what a maintenance record looks like.
  So the drain could ask elsewhere and depose a start just begun. Evidence: design.md:262-270,
  :179-180, :255-257.
- **R5-2 — Low:** round 4's claim that every `Input::Detach` names an entry its sender holds was
  false for root's `maintenance end` and S12's detach, which look the entry up first. Root's end
  can arrive after the entry ended at no work and a client attached, and `Engine::detach` acts
  by volume alone, so it would end the client's target. Evidence: decisions.md item 47;
  design.md:174-176, :222-225; `engine.rs:1856-1860`.
- **R5-3 — Low:** "one thread ends it" was not shown for a time-out, or for the waiter's order.
  A wait timing out after the router marked the entry running sent a detach to a running target
  (an S11 end) plus `MaintainFailed`. A waiter that kept only the last message would read an
  `Attached` then `Detached` as a failed start. Evidence: design.md:196-200, :202-205.

Changes/resolution:
- R5-1: the rule now says the record "is no longer a maintenance record": its maintenance ended
  (unheld, still naming the node), with the move still pending. A record still that node's
  maintenance record, at the same epoch or a newer one, stands. Test: an ask applied, the next
  step finds the record standing at the same epoch with the flag true from the ask, and nothing
  is asked.
- R5-2: root's `maintenance end` and the main thread's step 5 send the new
  `Input::DetachMaintenance { volume, generation }`. The engine applies it only to a
  `Kind::Maintenance` target or `Attaching` of that volume and generation, and leaves anything
  else alone. S12's client attach keeps #15's `Input::Detach`: it reserves only after the entry
  has ended, and its own attach follows on the one engine channel. Round 4's claim is corrected
  in decisions.md. Test: root's late end, delivered after the entry ended at no work and a
  client attached, leaves the client's target running.
- R5-3:
  - The waiter is an ordered queue.
  - At a time-out the main thread reads the entry's running mark under the registry's lock. If
    running, the start succeeded: no detach, no `MaintainFailed`, and its later end is a normal
    S11 end with the router's removal. If still starting, the main thread removes the entry in
    that same hold, so an `Attached` routed after finds no entry to mark.
  - Every removal is by generation and idempotent, and frees the place only when it removes an
    entry.
  - Test: a time-out concurrent with `Attached`.

ADR 0014 is unchanged this round: none of these findings touches its text.

Commit: `a77024c`. Recorded in decisions.md as changes made after approval, plan review round 5
(2026-11-14), items 51–53.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

Also 747b560 (orchestrator): decisions.md item 53 records the starting-case removal the design states.

### Plan review round 6 — ebd8e23

Scope: correctness against code and security of the plan, approved 2026-11-13, incremental on
the diff 5455e49..747b560 (ebd8e23 adds only report.md); fixes are changes made after approval.
No runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Earlier findings: R5-1 found fixed. R5-2 and R5-3 were found partly fixed, which opened R6-B1
(R5-2) and R6-B2 and R6-B3 (R5-3).

- **R6-B1 — Low:** S12's detach of a maintenance entry still used #15's by-volume
  `Input::Detach`. With two concurrent client attaches, the second's detach, looked up earlier
  and delivered later, ends the first client's target. Evidence: design.md:219-222, :242-244;
  `engine.rs:1856-1860`.
- **R6-B2 — Low:** the time-out removed the entry in the main thread before the engine had ended
  anything.
  - A client attach could then reserve and attach first, and be answered "already
    attaching".
  - If the engine's attach reached `Attached` meanwhile, the detach was sent before
    `MaintainFailed`, so the S11 end won and no back-off applied.

  Evidence: design.md:171-180; `engine.rs:640-653`.
- **R6-B3 — Low:** the design did not say what the main thread does at a time-out when the entry
  is already gone (it ran and ended during the wait); it could report a spurious failed start.
  Evidence: design.md:171-176.
- **R6-B4 — Low:** root's `maintenance end` could reach the engine before the start's own
  `Input::Attach`. It was left alone and lost, although root was answered `ok`. Evidence:
  design.md:219-222.

Changes/resolution: all four are orderings in one area, which rounds 4 to 6 each found anew.
By the orchestrator's direction, the prose of earlier rounds on the entry's start, failure and
end is replaced by one state machine in the design.
- **States:** `starting`, `running`, `ending`, `gone`.
- **Events:** the main thread's `Maintain`, `MarkEpoch` and time-out; the router's `Attached`
  and `Detached`; root's `maintenance end`; a client attach (S12).
- **Each cell:** the next state, who acts under the registry's lock, what is sent, and whether
  the place is freed.
- **Invariants, kept by every cell:** one end; one place freed; no client attach refused for
  the maintenance entry (S5, S12); no client's target ended by a maintenance event.
- **The rules beneath the table:**
  - the reserve and the `Input::Attach` share one registry hold;
  - every maintenance detach is `DetachMaintenance` with the entry's generation;
  - the engine's last output for a generation is one `Detached` (in place of `AttachFailed`
    too), with its one report just before (`StartFailed` if it never reached `Attached`, else
    the S11 end);
  - only the router removes an entry;
  - the main thread's part ends at `Input::Marked`;
  - `MaintainFailed` is sent only for a start refused before its entry exists.
- **Test:** `maintenance_entry_lifecycle`, table-driven, one case per row, with the invariants
  checked after each case and the orderings of rounds 2 to 6 as named cases.

Per finding, checked against the table:
- R6-B1: S12's row sends `DetachMaintenance` with the entry's generation. Two client attaches
  at once both detach that generation, which ends nothing of either. Test: the first client's
  target keeps running.
- R6-B2: the main thread removes nothing; the entry stands until the engine's `Detached`, so
  no client attaches early. After a time-out or failed mark no `Input::Marked` was sent, so no
  late `Attached` comes, and the engine's `StartFailed`, made before its `Detached` and the
  removal, holds the back-off. An `Attached` routed while `ending` leaves the entry `ending`,
  and its end is the S11 end with root's or the client's reason. Test: a time-out with the
  entry starting gives no late `Attached`, one back-off and no second epoch.
- R6-B3: the `gone` + time-out row: nothing sent, nothing reported (the engine has reported).
  Test case in the table.
- R6-B4: root's end cannot precede the start's `Input::Attach`, which is queued in the hold
  that made the entry visible; so it is applied, with nothing to remember. Applying was chosen
  over refusing: root's request is valid once the entry exists, and a refusal would make root
  retry a race it cannot see. Test: root's end sent at once after the reserve ends the start.

ADR 0014 is unchanged this round.

Commit: `fc6cac8`. Recorded in decisions.md as changes made after approval, plan review round 6
(2026-11-14), items 54–58.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 7 — 30cf82d

Scope: correctness against code and security of the plan, approved 2026-11-13, incremental on
the diff ebd8e23..fc6cac8 (30cf82d adds only report.md); fixes are changes made after approval.
No runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Earlier findings: R6-B1 to R6-B4 found fixed. The lifecycle table itself was found sound:
every cell covered, and every cell keeping the four invariants. Earlier rules (the run mark
before `Hello`, the `StartFailed` back-off, `EndMaintenance` at the epoch, the forgotten-node
refusals) were found still in place.

- **R7-B1 — Medium:** the table-driven test ran with the engine scripted, so its checks of the
  engine's rules only repeated the script. The rules in question:
  - `DetachMaintenance`'s generation filter;
  - one `Detached` per generation;
  - `StartFailed` before `Detached`;
  - a `Marked` after a drop starting nothing.

  Round 6 had deleted the R5-2 engine assertion, so an engine whose `DetachMaintenance` acts by
  volume passed every listed test. Three rows bundled two events each, and R6-B4's case had no
  hook to put root's lookup between a reserve and a send. Evidence: design.md:178-191, :222,
  :225, :228, :575-597; `engine.rs:1856-1861`.
- **R7-B2 — Low:** root's `maintenance end`, new in round 6, waits for the entry to be gone for
  "#15's 30 s". Neither root's answer past that, the entry's state, nor S12's client attach's
  answer was stated, and the socket line did not list it. Evidence: design.md:215, :240, :285,
  :479, :482-483.
- **R7-N1 — Nit:** "two client attaches at once both detach G" contradicted the table's `ending
  | a client attach` row, which sends nothing. Evidence: design.md:241-242;
  decisions.md:346-347.
- **R7-N2 — Nit:** the `starting | Detached{G}` row named only a refused attach. It is also
  reached when the engine drops G after `Marked`, and when a running target's `Detached` is
  routed ahead of its `Attached`, where the report was the S11 end. The `gone` rows left out
  the main thread taking `Detached{G}` from its waiter. Evidence: design.md:214, :225; #15
  design.md:31, :39; `ublk.rs:582-600`.
- **R7-N3 — Nit:** item 54 named only items 52 and 53, though it also reverses items 30, 39, 40
  and 45. Evidence: decisions.md:343-345.

Changes/resolution:
- R7-B1: new `maintenance_generation_end` in `flaminio-node/tests/engine.rs`, with the real
  engine and a fake metadata client.
  - It covers each way a generation ends, on its own: a refused attach (a bad token, another
    epoch, the local fence, a record no longer a maintenance record naming the node);
    `DetachMaintenance` before the `GetVolume` answer, between `MarkEpoch` and `Marked`, and
    after `Attached`; no work; deposed; the volume gone.
  - For each it checks: one report, first, then one `Detached` as the last output; `StartFailed`
    exactly when `Attached` never came, with its log line; no `AttachFailed`; `EndMaintenance`
    as S11 says; the 5-minute back-off.
  - The R5-2 assertion is restored: a `DetachMaintenance` of another generation, over a
    client's target, or after the end, emits nothing and leaves the target running; a `Marked`
    after a drop sends no `Hello`.
  - `maintenance_entry_lifecycle` keeps the node's half (what is sent, the state, the place
    count, the socket's answer, who waits). The bundled rows are split so each row is one event
    and one case.
  - R6-B4's case gets a hook: each time the starting thread lets the registry's lock go,
    root's lookup runs, and whenever it sees the entry the fake channel already holds
    `Input::Attach{G}`.
  - Each case names the rule it fails without.
- R7-B2: past 30 s, root's end and a client attach over the entry each answer `error: VOLUME:
  the maintenance target did not end within 30 s`, as #15's detach does. The client reserves
  nothing, and the entry stays `ending` until the router removes it on `Detached{G}`. This is
  added to the socket line, to the table as two rows, and to the client-attach invariant (the
  bound is #15's; kubelet retries the stage). Test: a scripted engine withholds `Detached{G}`.
- R7-N1: "the first detaches G, the second waits for gone", in the design's invariant and in
  item 55.
- R7-N2: the row names its other causes, and the engine's report as `StartFailed` or the S11
  end as the case was; the `gone` rows add the main thread taking `Detached{G}` from its waiter
  (one row per event).
- R7-N3: item 54 lists what it reverses: items 30, 39, 40, 45, 52 and 53.

Commit: `7d5d9a0`. Recorded in decisions.md as changes made after approval, plan review round 7
(2026-11-14), items 59–63.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 8 — 24df440

Scope: correctness against code and security of the plan, approved 2026-11-13, incremental on
the diff 30cf82d..7d5d9a0 (24df440 adds only report.md); fixes are changes made after approval.
No runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Earlier findings: R7-B1, R7-B2 and R7-N1 to R7-N3 found fixed. The table and the invariant text
agree. R7-B1 left R8-L1 and R8-L3, and R7-B2 left R8-L2.

- **R8-L1 — Low:** "the report comes first in the output sequence" could not fail in the engine
  test: the report to the `Maintainer` is an in-engine call, and `Detached{G}` an output of the
  same locked step. "Exactly one report" was hidden by `on_ended` ignoring a repeat. Evidence:
  design.md `maintenance_generation_end`, :472.
- **R8-L2 — Low:** spec S5 ("never refused or made to wait for maintenance") and S12 ("not
  refused") were not amended, though the design now refuses a client attach after 30 s. The
  30-s error's CSI code was unnamed, although #15 maps attach failures to specific codes, not
  `Internal`. Evidence: spec.md S5, S12; design.md:227, :244-249; #15 design.md:397-402.
- **R8-L3 — Low:** the R6-B4 hook had no placement rule: a later refactor adding a lock hold
  without calling it would pass, and how production code carries it was unsaid. Evidence:
  design.md "the hold"; #15 design.md:753.
- **R8-L4 — Low:** an `on_ended` repeat is a `Maintainer` rule, yet it was listed in the engine
  test, which cannot call `on_ended`. Evidence: design.md `maintenance_generation_end`.
- **R8-N1 — Nit:** the 30-s rows did not say which wins when the entry goes `gone` as the wait
  expires. Evidence: design.md:226-227.

Changes/resolution:
- R8-L1: the rule now reads: in the same step that emits `Detached{G}`, and not after it, the
  engine reports, so the `Maintainer`'s back-off already holds when `Detached{G}` is taken.
  - The test takes `Detached{G}`, turns the engine, and finds the `Maintainer` already acting on
    the report: no `StartMaintenance` after `StartFailed`; the place free after an S11 end.
  - It counts the report in the log lines and in the `EndMaintenance` calls on the fake
    metadata client.
- R8-L2: S5 and S12 are narrowed to what holds, and the narrowing and its reason are recorded
  in decisions.md (item 65).
  - The metadata service never refuses or delays a client attach.
  - On the node, the client's attach waits for the maintenance end, at most 30 s (#15's bound
    for a detach); past that it is refused with nothing reserved, and kubelet retries.
  - The CSI node plugin (`control/csi/node.go`, `device`) maps the exact prefix to
    `Unavailable`, not `Internal`. A `csi_test.go` case checks it, including that the same words
    about another volume are not taken for it.
  - ADR 0014's Decision and the design's invariant are amended to match. S19 needs no change.
- R8-L3: the registry's mutex is private to its module, and every hold goes through
  `Registry::lock()`, whose `RegistryGuard` calls a `cfg(test)` release hook in its `Drop` (the
  node tests are unit tests of `node.rs`). A later hold cannot skip the hook, because locking the
  mutex otherwise would not compile. The test runs root's lookup from the hook at every release,
  and checks that the hook ran during the start.
- R8-L4: the check moves to `maintenance.rs` unit tests. An `on_ended` repeated for a
  `(K, epoch)`, or for one the `Maintainer` no longer holds, changes nothing: one place freed,
  one `EndMaintenance`, one back-off.
- R8-N1: the wait reads the state under the registry's lock; gone wins, so root is answered
  `ok` and the client reserves.

Commit: `2ad19b9`. Recorded in decisions.md as changes made after approval, plan review round 8
(2026-11-14), items 64–68.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 9 — a28a267

Scope: correctness against code and security of the plan, approved 2026-11-13, incremental on
the diff 24df440..2ad19b9 (a28a267 adds only report.md); fixes are changes made after approval.
No runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Earlier findings: R8-L4 found fixed. R8-L1, R8-L2 and R8-L3 were found partly fixed, and R8-N1
fixed in text with no test; these are R9-B1 to R9-B5.

- **R9-B1 — Medium:** the `RegistryGuard` hook, as designed, deadlocks or recurses. Its `Drop`
  ran root's lookup while the inner `MutexGuard` was still held, and the lookup's own guard would
  call the hook again. It also did not fit #15's `Mutex<Registry>`, and "would not compile
  outside the module" held only for a registry module the plans do not have. Evidence:
  design.md:182-187, :623-628, :419; #15 design.md:61.
- **R9-B2 — Medium:** the round-8 narrowing was not carried to every live promise: the spec's
  Tests ("no client attach refused (S12)"), the node test's invariant, and the design's
  `Restart` bullet ("none is refused for maintenance"). Evidence: spec.md:196;
  design.md:82-83, :612.
- **R9-B3 — Low:** the plugin does not see `error: <volume>: ...`: `ControlSocket.command`
  hands it `data plane: <volume>: ...`. The design also did not say that the wait is cut by the
  attach's own time limit. Evidence: design.md:134-137, :552-555; `control/csi/dataplane.go:83`,
  `:107`; `node.go:70-76`.
- **R9-B4 — Low:** "takes `Detached{G}`, then turns the engine" passes an engine that reports at
  the head of the next step; how log lines are captured was unsaid. Evidence:
  design.md:644-650.
- **R9-B5 — Low:** "gone wins" at the 30-s expiry had no test. Evidence: design.md:250-251,
  :632-635; decisions.md item 68.

Changes/resolution:
- R9-B1: the hook is cut, not repaired (the orchestrator's direction).
  - One new method of #15's registry, `Registry::reserve_maintenance(volume, token,
    &attach_tx)`, is the only way a maintenance entry is inserted. It sends the
    `Input::Attach` before it returns, within the caller's one hold of `Mutex<Registry>`.
  - The entries stay a private field, so `control.rs` reaches them only through the registry's
    methods, and #15's `reserve` makes device entries only. No new module.
  - The test checks the outcome: when `reserve_maintenance` returns `Ok(G)`, the channel already
    holds `Input::Attach{G}` (`try_recv`, no step between). The compiler enforces that no other
    path inserts (privacy), and after every lifecycle case each maintenance entry's attach must
    have come first among its generation's inputs.
- R9-B2: the spec's Tests and the node test's invariant say "no client attach refused or made
  to wait except by the 30-s bound", and the `Restart` bullet says the metadata service refuses
  none and the device waits at most 30 s. A grep for other such promises finds only history.
- R9-B3: the plugin matches `data plane: <volume>: the maintenance target did not end within
  30 s` and maps it to `Unavailable`. The wait is cut by the attach's own time limit
  (`within`): a caller with less than 30 s left gets #15's time-out answer, `DeadlineExceeded`
  in the plugin. This is stated in the plugin bullet, the table row and the socket section. The
  plugin test feeds the real socket text through `ControlSocket.command`, and covers the
  shorter limit.
- R9-B4: straight after the `handle` or `turn` whose outputs hold `Detached{G}`, before any
  further input or turn, the engine test reads the new read-only `Engine::maintenance()` (as
  `holding()` is): the volume backed off after `StartFailed`, the place free after any end. An
  engine that reports at the head of the next step, named as the injected late reporter, fails
  it. Log lines are counted by a recording log sink the test installs.
- R9-B5: new node test case. The test holds the registry's lock and, in that hold, removes the
  entry as the router does on `Detached{G}` and moves the fake clock past 30 s. Then root is
  answered `ok`, the client reserves, and the place is freed once; a wait that decides on the
  clock first fails it.

ADR 0014 is unchanged this round.

Commit: `7daf8ea`. Recorded in decisions.md as changes made after approval, plan review round 9
(2026-11-14), items 69–73.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 10 — 9a54f04

Scope: correctness against code and security of the plan, approved 2026-11-13, incremental on
the diff a28a267..7daf8ea (9a54f04 adds only report.md); fixes are changes made after approval.
No runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Earlier findings: R9-B1 and R9-B2 found fixed. R9-B3 and R9-B4 were found partly fixed, and
R9-B5 fixed in intent with wiring gaps; R9-B1 left a residual (R10-1).

- **R10-1 — Medium:** "the only way a maintenance entry is inserted" was not the compiler's
  where the main thread lives. Rust privacy is per module, #15 puts the registry in `Node` in
  `ublk.rs` beside the main thread, and the single-threaded backstop check also passes a
  reserve and a send in two holds. Evidence: design.md:189-192, :638-643; #15 design.md:13, :61.
- **R10-2 — Medium:** the `DeadlineExceeded` path rested on #15's text for an attach never
  begun ("before it could begin"), which is wrong for a wait cut mid-way. The plugin test named
  no input string. Evidence: design.md:142-145, :247, :272, :567-569; #15 design.md:92, :399;
  `dataplane.go:106-107`.
- **R10-3 — Low:** the "recording log sink the test installs" does not exist: the engine logs
  with `eprintln!`. Evidence: design.md:665-666; `engine.rs:830`, `:860`, `:1343`.
- **R10-4 — Low:** the "gone wins" test did not say how a blocked waiter sees the fake clock or
  is woken, nor that both waiters must be blocked before the test takes the lock. Evidence:
  design.md:651-655.
- **R10-5 — Low:** `try_recv` was named on a channel the real node owns. Evidence:
  design.md:638; `node.rs:288`.

Changes/resolution:
- R10-1: `Registry` now lives in `mod registry` in `ublk.rs`, beside #15's `Node`.
  - It has a private field and a private maintenance-entry constructor called only by
    `reserve_maintenance`, and `pub(super)` methods: #15's `reserve` (device entries only),
    `reserve_maintenance`, the lookups, the state's reads and marks, and `remove_generation`.
  - It is listed in Files touched, and noted in the Risks as a change to #15's layout that
    whichever of #15 and #17 lands second makes.
  - The backstop check is cut: privacy now enforces the rule.
- R10-2: the wait cut by the attach's own limit answers `error: VOLUME: the attach's time ran
  out waiting for the maintenance target to end`. The plugin sees it as `data plane: VOLUME:
  the attach's time ran out waiting for the maintenance target to end` and maps that exact
  prefix to `DeadlineExceeded`. The text is named in the plugin bullet, the table row, the
  socket section and the plugin test's input (fed through `ControlSocket.command`).
- R10-3: log-line counting is cut. The engine test counts `EndMaintenance` calls on the fake
  metadata client and reads `Engine::maintenance()`'s state (one place freed, one back-off).
- R10-4:
  - Each 30-s wait is a condvar wait on the registry's lock, with its deadline taken from the
    node's injected clock. The fake clock's advance and every removal notify that condvar, and
    the waiter reads the state before the clock.
  - The test waits until both waiters are blocked (a `cfg(test)` count, raised under the lock
    just before the wait, reaching 2). Then, in one hold, it calls the router's own
    `Registry::remove_generation(G)`, which frees the place and wakes the waiters, and moves
    the clock past 30 s.
- R10-5: the node under test is built with its own `mpsc::channel::<Input>()`, and the test
  keeps the receiver for `try_recv`.

The spec and ADR 0014 are unchanged this round.

Commit: `55ae108`. Recorded in decisions.md as changes made after approval, plan review round
10 (2026-11-14), items 74–78.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 11 — 8a53a4b

Scope: correctness against code and security of the plan, approved 2026-11-13, incremental on
the diff 9a54f04..55ae108 (8a53a4b adds only report.md); fixes are changes made after approval.
No runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Earlier findings: R10-3 and R10-5 found fixed; R10-2 fixed in the text, with the files list
lacking the second mapping (R11-B3); R10-4 partly fixed (R11-B2); R10-1 not fixed as written
(R11-B1).

- **R11-B1 — Medium:** `pub(super)` on `ublk::registry`'s items reaches `ublk` only, yet the
  socket's threads in `control.rs` were said to reach those methods directly. `cli`,
  `control`, `node`, `proto`, `remote`, `replica`, `ublk` and `volume` are siblings under the
  crate root. Evidence: design.md:196-204, :512-516, :435-437, :816-820;
  `flaminio-io/src/main.rs:10-21`.
- **R11-B2 — Low:** the fake clock's wake of the condvar was not wired: the injected clock holds
  no reference to the registry's condvar, `wait_timeout` takes a real duration, and the wait was
  not stated as a loop. Evidence: design.md:668-677.
- **R11-B3 — Low:** the files list named only the 30-s mapping for `control/csi/node.go`, not
  the `DeadlineExceeded` prefix. Evidence: design.md:430, :142-147, :583-585.

Changes/resolution:
- R11-B1: `Node` wrapper methods, as the orchestrator directed.
  - `Node`, the main thread and the router, all in `ublk.rs`, call the registry's `pub(super)`
    methods.
  - `control.rs` and `node.rs` reach the registry only through `Node`'s `pub(crate)` methods,
    whose only maintenance insert is `Node::reserve_maintenance`.
  - The registry's methods are not made `pub(crate)`, since any module could then insert.
  - The module tree (`main.rs:10-21`) is stated, and the choice is named in the lifecycle rule,
    Contracts, Files touched, the `reserve_maintenance` test case and the Risks.
- R11-B2:
  - The 30-s wait is a loop on the registry's condvar, under its lock: read the entry's state
    (gone: done); then read the injected clock (past 30 s, or the attach's own limit if sooner:
    the error); else `wait_timeout` for a bounded real slice (at most 100 ms, or the time left
    on the injected clock), and loop.
  - Every removal calls `notify_all`, and so does the tests' fake clock's `advance`. The slice
    bounds a wake that never comes.
  - The "gone wins" test builds the node with that fake clock.
- R11-B3: the `control/csi/node.go` line names both exact prefixes: `data plane: <volume>: the
  maintenance target did not end within 30 s` to `Unavailable`, and `data plane: <volume>: the
  attach's time ran out waiting for the maintenance target to end` to `DeadlineExceeded`.

The spec and ADR 0014 are unchanged this round.

Commit: `c65179e`. Recorded in decisions.md as changes made after approval, plan review round
11 (2026-11-14), items 79–81.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 12 — 62a3ee9

Scope: correctness against code and security of the plan, approved 2026-11-13, incremental on
the diff 8a53a4b..c65179e (62a3ee9 adds only report.md); fixes are changes made after approval.
No runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Earlier findings: R11-B3 found fixed; R11-B1 fixed in substance, with how `control.rs` obtains
a `Node` open (R12-B2); R11-B2 partly fixed, the clock itself undefined (R12-B1).

- **R12-B1 — Low:** "the node's injected clock" did not exist. `flaminio-io` reads
  `Instant::now()` directly, #15 adds no clock to `Node`, and the design said neither which type
  is injected, who owns it, nor how its `advance` reaches the condvar. Evidence:
  design.md:234-237, :690-697; `ublk.rs:583`, `:739`, `:802`; `control.rs:130`, `:205`.
- **R12-B2 — Low:** how `control.rs`'s threads obtain a `Node` was not said: `control.rs` takes
  an `mpsc::Sender<Control>` today, and #15 builds `Node` in `serve_node` without passing it to
  the socket. The 30-s wait was listed among the registry's `pub(super)` methods, though its
  condvar pairs with the `Mutex<Registry>` inside `Node`. Evidence: design.md:196-206, :199,
  :537; `control.rs:41`, `:69`, `:118`.

Changes/resolution:
- R12-B1:
  - A small `Clock` trait in `ublk.rs`, with `now() -> Instant`. `Node` holds an
    `Arc<dyn Clock>`, the system clock (`Instant::now`) in production. Only the maintenance
    waits read it: the 30-s wait, and the main thread's wait for a maintenance start.
  - The tests' fake clock implements it; its `advance` calls `notify_all` on the condvar `Node`
    owns beside its `Mutex<Registry>`, taken through a `cfg(test)` accessor.
  - #15 adds no clock, so this is #17's. It is in Contracts, Files touched and the Risks.
- R12-B2:
  - `control::spawn` takes an `Arc<Node>` in place of today's `attached: Attached`
    (`control.rs:41-45`, an `Arc<Mutex<..>>` that #15's registry replaces), keeping its `ctl`
    and `engine` senders. This matches today's code with the least change: the socket's threads
    already touch shared state, and #15's socket reserves entries itself.
  - A `Control` message per wait was rejected, as it would block the main thread, which takes
    attaches one at a time, for up to 30 s.
  - The 30-s wait is now a `Node` method, locking `Node`'s `Mutex<Registry>`, reading the
    entry's state through the registry's methods, and waiting on `Node`'s condvar.
  - `Node::remove_generation` removes under that lock, then calls `notify_all`; the test uses
    its held-guard form.
  - `node.rs` only parses `--max-maintenance` into `NodeConfig`.

The spec and ADR 0014 are unchanged this round.

Commit: `c19d1d4`. Recorded in decisions.md as changes made after approval, plan review round
12 (2026-11-14), items 82–83.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 13 — 3d16fc1

Scope: correctness against code and security of the plan, approved 2026-11-13, incremental on
the diff 62a3ee9..c19d1d4 (3d16fc1 adds only report.md); fixes are changes made after approval.
No runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Earlier findings: R12-B1 and R12-B2 found fixed in substance. No new security finding: the wait
holds the registry lock only inside `wait_timeout`, the removal and `notify_all` happen under
the one lock, and the 100-ms slice bounds a missed wake.

- **R13-L1 — Low:** the fake clock and `Node`'s condvar were built in the wrong order. The clock
  exists before `Node`, so it cannot take a clone of `Node`'s condvar at construction, and
  Contracts gave a plain `Condvar` where a clone needs an `Arc<Condvar>`. Evidence: design.md
  "The clock", Contracts.
- **R13-L2 — Low:** "only the maintenance waits read the clock" included the main thread's
  wait for a maintenance start, which waits on the entry's waiter with `METADATA_LIMIT`, not on
  `Node`'s condvar, and reads `Instant::now()` today; a fake clock could not wake it. Evidence:
  design.md "The clock"; `ublk.rs:858`.
- **R13-L3 — Low:** the replacement of `Attached` was not traced through today's other users.
  The design did not say that `Node` must exist before `control::spawn` and the startup attach
  wait, nor what replaces the startup wait's check. Evidence: design.md:208-212;
  `ublk.rs:682`, `:722`, `:751-752`, `:838`, `:939`; `control.rs:72`, `:192`.

Changes/resolution:
- R13-L1: `Node`'s condvar is an `Arc<Condvar>` in Contracts, with a `cfg(test)` accessor that
  clones it. The fake clock holds a `OnceLock<Arc<Condvar>>` slot that the test fills from that
  accessor after building `Node`; its `advance` calls `notify_all` on it.
- R13-L2: a cut. The main thread's maintenance-start wait no longer reads the clock: it stays on
  real time with `METADATA_LIMIT`, as today (`ublk.rs:858`). Only the 30-s wait reads the
  `Clock`. The lifecycle test's two time-out cases wait out `METADATA_LIMIT` on real time.
- R13-L3: `Node` is built first in `serve_node`, where `attached` is made today
  (`ublk.rs:722`), and handed to `control::spawn` and to the startup attach's retry thread
  (`:749`). Each other use is #15's to replace, with #15's lines cited:
  - the type and its making (`:682`, `:722`): the registry in `Node` (#15 :14-18, :61);
  - the attach and device paths (`:838`, `:939`): #15's main thread and device thread (#15
    :79-89, :226-233);
  - `control.rs`'s `serve` and `detach` (`:72`, `:192`): #15's `detach` over entries (#15
    :365-374);
  - the startup retry's check (`:752`): #15's "its retry stops once that volume has an entry"
    (#15 :71, :726). #17 counts a device entry only there, so that a maintenance entry of the
    volume does not end the retry, whose attach goes through S12.

The overlong line left by round 12 (design.md:216) is rewrapped in passing. The spec and ADR
0014 are unchanged this round.

Commit: `958b9f2`. Recorded in decisions.md as changes made after approval, plan review round
13 (2026-11-14), items 84–86.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 14 — 324191e

Scope: correctness against code and security of the plan, approved 2026-11-13, incremental on
the diff 3d16fc1..958b9f2 (324191e adds only report.md); fixes are changes made after approval.
No runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Earlier findings: R13-L1 and R13-L2 found fixed; R13-L3 fixed in substance, its `ublk.rs`,
`control.rs` and #15 citations verified, with one Low left (R14-L2). No new security finding.

- **R14-L1 — Low:** the two time-out cases waited out `METADATA_LIMIT` in real time, 30 s each,
  in `make test`. Evidence: design.md Tests (~:707-711); decisions.md item 85.
- **R14-L2 — Low:** the startup retry's path through S12 was wrong or unspecified. The startup
  attach and its retries go to the main thread over `ctl_tx`, while S12's end-and-wait runs on
  the client's socket thread and must not run on the main thread. So the startup attach's
  behaviour with a maintenance entry present was undefined. Evidence: design.md:224-226, :287,
  :290; `ublk.rs:745-767`.

Changes/resolution:
- R14-L1: `Node` carries `start_limit: Duration`: `METADATA_LIMIT` in production, set by a
  `cfg(test)` constructor argument. The two time-out cases build `Node` with about 200 ms.
  Production is unchanged.
- R14-L2: S5 is kept rather than adding the reviewer's refusal, as the orchestrator preferred,
  since it works with today's code and #15's design.
  - The startup attempts run on the startup thread (`ublk.rs:749`), not the main thread, and #15
    gives the startup attach the same check and reservation as the socket's (#15 :71), made
    before the request is queued to the main thread.
  - The startup thread therefore runs S12's step through the same `Node` methods as the socket's
    thread: `DetachMaintenance{G}`, a wait for the entry to be gone (30 s or its own deadline),
    then the reserve and the attach queued to the main thread.
  - A wait that runs out fails that attempt, and the retry tries again 5 s later, up to
    `STARTUP_ATTACH_LIMIT`. The first attempt, sent before the socket opens, meets an empty
    registry.
  - This is stated in the design's startup paragraph and S12's ending bullet, and the startup
    thread is named among the users of `Node`'s wrappers.
  - New test: a startup attach with a maintenance entry present ends the maintenance target and
    attaches, the retry not stopping on the maintenance entry and the main thread never blocked
    by the wait.

The spec and ADR 0014 are unchanged this round.

Commit: `85c678e`. Recorded in decisions.md as changes made after approval, plan review round
14 (2026-11-14), items 87–88.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 15 — a09eb34

Scope: correctness against code and security of the plan, approved 2026-11-13, incremental on
the diff 324191e..85c678e (a09eb34 adds only report.md); fixes are changes made after approval.
No runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Earlier findings: R14-L1 found fixed; R14-L2 fixed in substance, with one Low left (R15-C1). No
new security finding: the startup thread uses the same token check and epoch rule as the
socket, and a refused attempt is bounded by `STARTUP_ATTACH_LIMIT` and the 30-s wait.

- **R15-C1 — Low:** the startup thread's S12 step had no defined place in the retry loop, and no
  test seam.
  - Today's loop receives the main thread's reply on `answer`, but a failed S12 wait never
    reaches the main thread, so it produces no reply there.
  - `done()` was not re-checked after the wait of up to 30 s.
  - The retry is a closure with a fixed 5-s sleep, which a test cannot drive.
  - The case of the startup retry and a socket client at once over one entry was not named.

  Evidence: design.md:226-236 and its Tests case; `ublk.rs:745-767`.

Changes/resolution:
- R15-C1, as the reviewer recommended:
  - The startup attempt is a named function, `startup_attempt(&Node, volume, token, deadline)
    -> Result<Receiver<String>, String>`. It checks the token; if the volume has a maintenance
    entry, it sends `DetachMaintenance{G}` and waits for the entry to be gone; it re-checks
    `done()` after the wait and stops without reserving if done; else it reserves and queues
    its attach as #15 does, and returns the reply's receiver.
  - Every failed attempt returns `Err(answer)`, one that never reached the main thread
    included. The loop at `ublk.rs:745-767` treats it like a failed reply: logged, then the
    same `done()`, sleep and `done()` path.
  - The interval between attempts is a parameter: 5 s in production, milliseconds in tests.
  - The first attempt still runs before the control socket opens.
  - Tests added: a wait that runs out (`Err`, logged, the next attempt after the interval); a
    device entry made during the wait (the retry ends, no reserve); and the startup retry and a
    socket client attach at once over one maintenance entry (one `DetachMaintenance{G}`, one
    reserve; the other refused, or for the startup thread `Err` and a stop at `done()`).

The spec and ADR 0014 are unchanged this round.

Commit: `db43ba2`. Recorded in decisions.md as a change made after approval, plan review round
15 (2026-11-14), item 89.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 16 — 98bfbe2

Scope: correctness against code and security of the plan, approved 2026-11-13, incremental on
the diff a09eb34..db43ba2 (98bfbe2 adds only report.md); fixes are changes made after approval.
No runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Earlier findings: R15-C1 found fixed in substance, with one Low left in the same area (R16-C1).
No new security finding.

- **R16-C1 — Low:** the first attempt's hand-off to the retry thread, and the concurrent test's
  outcomes, were underspecified.
  - The first attempt returns a `Result`, but the retry loop started from `answer.recv()`, so a
    failed first attempt would be lost.
  - The concurrent test was racy and named neither the refusal's text nor both winners. When
    the socket client wins, the startup retry stops at the `done()` after the sleep.

  Evidence: design.md:229-250, :739-751; `ublk.rs:745-767`.

Changes/resolution:
- R16-C1, as the reviewer words it:
  - **The hand-off.** The retry thread's loop starts from the first attempt's `Result`. `Ok(rx)`
    goes to `rx.recv()`; an `Err(answer)` is logged and goes down the same `done()`, sleep and
    `done()` path. Test: a failed first attempt is logged and retried after the interval.
  - **`done()`.** It counts a running device entry of the volume, as today's `attached` is set
    only once the device runs (`ublk.rs:752`). So another client's attach still starting, which
    may yet fail, does not end the retry; this is what makes the socket-first outcome below end
    at the `done()` after the interval.
  - **The concurrent test.** It lines both runners up with the `cfg(test)` waiter count (both
    blocked in the 30-s wait, the count at 2) and removes the entry. A `cfg(test)` gate, after
    each runner's wait and before its reserve, is opened for the chosen winner first. Both
    orders run, and each winner is asserted:
    - socket first: the startup attempt's reserve is refused with #15's answer over a starting
      entry, `error: VOLUME is starting here` (#15 design.md:78), the "already attaching" case.
      The startup retry logs it, sleeps the interval, and ends at the `done()` after it, with
      no further attempt;
    - startup first: the socket client's reserve is refused with the same answer.

    In both, one `DetachMaintenance{G}` is sent and one reserve made. The refusal is a conflict
    between two clients, as on #15, consistent with S5 as narrowed.

The spec and ADR 0014 are unchanged this round.

Commit: `71d614d`. Recorded in decisions.md as a change made after approval, plan review round
16 (2026-11-14), item 90.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 17 — f28981a

Scope: correctness against code and security of the plan, approved 2026-11-13, incremental on
the diff 98bfbe2..71d614d (f28981a adds only report.md); fixes are changes made after approval.
No runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Earlier findings: R16-C1 found fixed, with the gaps below. No new security finding: the gate
and the waiter count are `cfg(test)` only, and the retry stays bounded by
`STARTUP_ATTACH_LIMIT` and the 30-s wait.

- **R17-L1 — Low:** `done()` counting only a running device entry silently departed from #15,
  whose rule "its retry stops once that volume has an entry" (any state) #15 also tests; if
  both landed, one of the two tests would fail. Evidence: design.md:222-227; #15
  design.md:71, :726; `ublk.rs:752`, `:1044`.
- **R17-L2 — Low:** the concurrent test's refusal text depends on the winner's entry still being
  `starting`; if the fake attach completes first, the answer is `... is running here`. The test
  must hold the winner's metadata phase unanswered until the loser's reserve is refused.
  Evidence: design.md concurrent test.
- **R17-L3 — Low:** the gate's position relative to `startup_attempt`'s `done()` re-check was
  unspecified. Placed before `done()`, the startup runner could stop at `done()` without
  reserving, and the test would not see the refusal. Evidence: design.md:234-239 and the test.

Changes/resolution:
- R17-L1: the orchestrator decided, under the user's rule (where plans conflict, follow the
  approved, review-clear one, here #15), to follow #15's rule exactly and cut the departure.
  - `done()` stops the retry once the volume has any device entry, starting, running or
    stopping, read from the registry (#15 design.md:71, :726).
  - Round 13's "a device entry" wording is restored. decisions.md records that round 16's
    "running only" is reverted, and why.
  - A maintenance entry still does not stop the retry.
- R17-L2: the test holds the winner's metadata phase unanswered, so its entry stays `starting`,
  until the loser's reserve is refused with `error: VOLUME is starting here`; then it releases
  it.
  - Socket wins: the startup attempt's refused reserve returns `Err`, and the retry's next
    `done()` sees the socket client's device entry and ends at once, with no further attempt.
  - Startup wins: the socket client gets the same text.
- R17-L3: the startup runner's `cfg(test)` gate sits between its `done()` re-check and its
  reserve, which is stated in `startup_attempt` and in the test, so the refusal case is
  exercised. The socket runner's gate stays after its wait, before its reserve. The case where
  the startup runner stops at `done()`, a device entry made before its re-check, is a separate
  test case.

The spec and ADR 0014 are unchanged this round.

Commit: `f802039`. Recorded in decisions.md as changes made after approval, plan review round
17 (2026-11-14), items 91–93.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 18 — c3e6ac5

Scope: correctness against code and security of the plan, approved 2026-11-13, incremental on
the diff f28981a..f802039 (c3e6ac5 adds only report.md); fixes are changes made after approval.
No runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

Earlier findings: R17-L1, R17-L2 and R17-L3 found fixed. No new security finding: the gates and
counts are `cfg(test)` only, and the retry stays bounded by `STARTUP_ATTACH_LIMIT`.

- **R18-L1 — Low:** the socket-first order did not say how the test knows the startup runner is
  parked at its gate before it opens the socket runner's gate. Otherwise the socket client could
  reserve first, and the startup runner would stop at `done()` with no reserve, never showing
  the refusal. Evidence: design.md:762-764.
- **R18-L2 — Low:** decisions.md item 90 still stated the old gate position and the old end of
  the retry ("at the `done()` after the interval"); items 91-93 supersede both, but item 90 did
  not say so. Evidence: decisions.md item 90.
- **R18-L3 — Low:** under the any-state rule, in socket-first the retry ends at the first
  `done()` after the `Err`, while the socket client's entry is still `starting`. The text said
  "device entry", which misleads about what the test asserts, and did not name the accepted
  cost if that held attach then fails. Evidence: design.md:768-770.

Changes/resolution:
- R18-L1: each runner raises a `cfg(test)` "reached gate" signal when it parks at its gate. In
  the socket-first order the test waits for the startup runner's signal, so that its `done()`
  re-check has passed, before it opens the socket gate.
- R18-L2: item 90 is marked in place: its gate position superseded by item 93, its "ends at the
  `done()` after the interval" by items 92 and 94, and its `done()` point withdrawn by item 91.
- R18-L3: the socket-first text now reads that the retry ends at the first `done()` after the
  `Err`, while the socket client's entry is still `starting` (held), with no further attempt. It
  notes that if that held attach failed instead, the startup `--attach` would have ended and
  nothing would retry it: #15's rule, and #15's accepted cost. No test is added beyond #15's.

The spec and ADR 0014 are unchanged this round.

Commit: `d95e16e`. Recorded in decisions.md as changes made after approval, plan review round
18 (2026-11-14), items 94–95.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 19 — a3a3bcd

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

- All three round-18 findings fixed (the "reached gate" signal; item 90's marks; the socket-first
  wording).
- **R19-L1 — Low:** the text did not say how the test knows the winner has reserved before it
  opens the loser's gate, in either order. Evidence: design.md:757–767.

Changes/resolution: R19-L1: the test waits until the fake metadata client has received the
winner's held request before opening the loser's gate; decisions.md item 96, round 19, after
approval.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 20 — a308666

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

- R19-L1 fixed in intent, both orders.
- **R20-L1 — Low:** the sentence named "the fake metadata client", which is only in the
  engine-side tests, not in this node test. Evidence: design.md:765, decisions item 96.

Changes/resolution: the test waits, with a bounded timeout, until the registry shows the winner's
device entry `starting` (read as the test already reads entry states and the place count). The
reviewer's suggestion (the receiver holding `Input::Attach{G}`) holds only for a maintenance
reserve; here both winners reserve device entries, so the registry's state is the signal for
both orders. Item 96 restated, round 20, after approval.

Status: needs_incremental_review. Approval remains separate; no approval is inferred.

### Plan review round 21 — 43d1ec2807f31d46aded1c494e5d5d730979476f

Scope: correctness against code and security of the draft plan; no runtime implementation.
Reviewers: plan-reviewer (design-code and design-security, one agent).

No comments from the requested review scopes, including Nits.

Status: clear. Approval remains separate; no approval is inferred.
