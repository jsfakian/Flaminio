# Issue #17 — decisions

## Approvals

- **Spec:** approved by Ioannis Sfakianakis, 2026-11-13.
- **Design:** approved by Ioannis Sfakianakis, 2026-11-13.
- Plan review round 1 (2026-11-14) fixes, made after approval: see below.
- Plan review round 2 (2026-11-14) fixes, made after approval: see below.
- Plan review round 3 (2026-11-14) fixes, made after approval: see below.
- Plan review round 4 (2026-11-14) fixes, made after approval: see below.
- Plan review round 5 (2026-11-14) fixes, made after approval: see below.
- Plan review round 6 (2026-11-14) fixes, made after approval: see below.
- Plan review round 7 (2026-11-14) fixes, made after approval: see below.
- Plan review round 8 (2026-11-14) fixes, made after approval: see below.
- Plan review round 9 (2026-11-14) fixes, made after approval: see below.
- Plan review round 10 (2026-11-14) fixes, made after approval: see below.
- Plan review round 11 (2026-11-14) fixes, made after approval: see below.
- Plan review round 12 (2026-11-14) fixes, made after approval: see below.
- Plan review round 13 (2026-11-14) fixes, made after approval: see below.
- Plan review round 14 (2026-11-14) fixes, made after approval: see below.
- Plan review round 15 (2026-11-14) fixes, made after approval: see below.
- Plan review round 16 (2026-11-14) fixes, made after approval: see below.
- Plan review round 17 (2026-11-14) fixes, made after approval: see below.
- Plan review round 18 (2026-11-14) fixes, made after approval: see below.
- Plan review round 19 (2026-11-14) fix, made after approval: see below.
- Plan review round 20 (2026-11-14) fix, made after approval: item 96 restated.

Approved with the recommended answer to each open question (see Settled at approval). The plan's
review loop continues after approval; later fixes are recorded as changes made after approval.
Nothing is implemented. Approved before its plan review cleared (status: pending).

## Settled at approval

1. **#15 first:** this is built after #15, not on today's frontend. `spec.md`, Open questions 1;
   `design.md`, Risks (the spec's open questions, 1).
2. **Recording held:** the record, with a release (S1, S2); not Kubernetes VolumeAttachments.
   `spec.md`, Open questions 2; `design.md`, Risks (2).
3. **Who starts the target on the node:** (a), the data plane itself, reading the metadata
   service as its own identity; no control-socket command. `spec.md`, Open questions 3;
   `design.md`, Risks (3).
4. **Existing volumes:** held. `spec.md`, Open questions 4 and S6; `design.md`, Risks (4).
5. **A forced detach of a lost node:** accepted: it releases the volume, and a maintenance attach
   then deposes a target that may still run. `spec.md`, Open questions 5; `design.md`, Risks (5).
6. **The node rule:** kept (a node holding a replica of the volume or the destination of one of
   its moves); widened only on need. `spec.md`, Open questions 6 and S16; `design.md`, Risks (6).
7. **The token:** the epoch's kind comes from the record at the token's epoch, not a new claim;
   no token format change. `spec.md`, Open questions 7; `design.md`, Risks (7).
8. **The operator:** dropped from the issue's dependencies, keeping #15. `spec.md`, Open
   questions 8; `design.md`, Risks (8).
9. **The numbers** ("to be approved"): at most 2 maintenance targets per node (S14,
   `--max-maintenance`'s default); 5 minutes without progress before the drain asks again (S17,
   `Stall`). `spec.md`, Open questions 9, S14 and S17; `design.md`, Risks (9).
10. **An ADR for the new transitions:** yes, ADR 0014
    (`docs/adr/0014-maintenance-attach-only-while-not-held.md`; numbered 0007 then, renumbered
    after approval, R1-C12 below), drafted as Proposed. It stays Proposed there; accepting it is
    a separate step. `spec.md`, Open questions 10; `design.md`, Risks (10) and ADRs.
11. **A `NotFound` answer reaching the target** ("Open: for maintenance targets only?"): as
    designed, it reaches the target whatever its kind. `design.md`, Risks, Findings against the
    spec, 1.

## Changes after approval: plan review round 1 (2026-11-14)

Round 1 of this batch's plan review, on commit b904a18: plan-reviewer (design-code), findings
R1-C1 to R1-C12 and two Nits; plan-reviewer (design-security), R1-S1 to R1-S11 and two Nits.
Every change below was made after approval, and is not approved itself. Each finding's
recommended fix was taken; where a finding offered options, the simplest safe one, as said. No
change narrows the intent. Where a spec rule changed, the finding shows the rule could not hold
as stated, or the change keeps a promise the spec already made.

12. **The maintenance start follows #15's attach path** (R1-C1, R1-S2). As designed it had no
    registry entry, so #15's `MarkEpoch`, routed by `(volume, generation)` to an entry, would
    have found none: the target never started, or ran at an epoch not durable here. Now the
    engine hands the `StartMaintenance` token to the main thread (`Output::Maintain`), which
    reserves a registry entry of kind maintenance with a generation (counted against
    `--max-maintenance` only), and writes the run mark on `MarkEpoch` before `Input::Marked`,
    so before any `Hello` (as #15, review-clear). S9 says so. The registry's one entry per
    volume now covers S9's "runs no target"; the old Risk "the registry is out of the engine's
    reach" goes.
13. **S13's injected bug** (follows from 12). With the run mark on the maintenance path,
    `maintenance::Bugs::keeps_epoch` would be refused by the mark and could no longer trip.
    It is replaced by `sim::Bugs::maintenance_skips_run_mark`, and #17's own check "epoch E
    started twice" by #15's (two target incarnations at one epoch), which covers maintenance
    starts.
14. **Simulator checks the bugs can trip** (R1-C2, R1-S1). The chaos made maintenance attaches
    and unholds only when its own record allowed them, so the model never met a held record.
    Now maintenance attaches and unholds come at random, whatever the record says, unholds
    name any node, the `Maintainer`'s starts and ends reach the model late (a stale view), and
    failovers use `Publish` and `Restart` as well as `Attach`. Checks at the moment of
    application: `maintenance over a held record` (an `AttachMaintenance` or a
    `StartMaintenance` applied while the record says held: the check is by transition, as
    R1-S1 asks) and `hold lost` (the model's `held()` against the simulator's record, after
    every transition). New injected bugs: `meta::Bugs::start_ignores_hold`,
    `publish_keeps_unheld` (Publish re-holding), `end_any_hold` (for 20's transition).
15. **Any maintenance record stands** (R1-C3, R1-S3). S15's "unless one of its own" made two
    drains, a drain and `maintain`, or a drain and #45 depose each other's targets every step,
    so the drain could not complete: S15 and S17 changed. A maintenance record stands while its
    node is eligible under S16, whoever asked; the ranking's "fewest standing asks of this run"
    goes; the stall clock runs from when this run first saw the record; a drain whose node is
    undrained asks nothing, and `Run` ends.
16. **No second ask to a node that took an epoch** (R1-C7). S17 changed: as stated, its stall
    rule restarted a chunk's copy whenever the copy took longer than 5 minutes, so such a chunk
    never moved. Of the finding's options, the first, a cut, was taken: scaling `Stall` with
    `chunk_bytes` adds a rate assumption, and nothing in the record changes during a copy. The
    5 minutes stay for a node that took no epoch. A residual case is in the design's Risks.
17. **Field numbers** (R1-C4, R1-S6): `Volume.held = 16`, `Volume.maintenance = 17`, in the
    proto and in `pb.rs` (the orchestrator's decision: #79 keeps 9–10, #13 holds 15).
18. **Completion as #78 says** (R1-C5). S18 changed: completion is #78's `CheckDrain` once #78
    lands, `Remaining == 0` until then; `Progress` keeps #78's `PendingCreations` and
    `Incoming`, and the line prints them. The package `Step` stays, with its signature (the
    orchestrator's decision: #78's `Run` calls it), as one step of a new `Drainer`; `Run`
    holds a `Drainer`, and whichever of #17 and #78 lands second makes `Run` call
    `Drainer.Step`.
19. **#13's words and forgotten nodes** (R1-C6, R1-S10, R1-S11, the code Nit on "release").
    #13 is review-clear, so its terms stay (the orchestrator's decision). #17's release is
    renamed unhold: `UnholdVolume`, `flaminio-meta unhold`, the hold value `unheld`, the bug
    `meta::Bugs::unhold_any_node` (was `release_any_node`). The drain's per-volume `held` is
    `in use`; #13's `Progress.Held` and `Released` stay. The drain asks a maintenance attach
    only for a volume with a move pending, and shows #13's reason otherwise (S15).
    `AttachMaintenance` and `StartMaintenance` refuse a forgotten node with
    `FailedPrecondition`, comparing #13's mark, and so does `ListMaintenance` for the forgotten
    node's own `io` (S3, S4); whichever of #13 and #17 lands second builds it.
20. **A maintenance record ends** (R1-C9, R1-S4). New `EndMaintenance(volume, node, epoch)`, by
    that node's `io` only, applied only to a maintenance record naming the node at that epoch:
    hold := unheld. Sent after a target ends (no work, root's end, a client attach) and for a
    listed record without work (S11). Chosen over a `ListMaintenance` that lists only records
    with work: only an end stops one drain's choice from becoming a standing right (R1-S4).
    The scan's 60-second and per-epoch pass-overs go. Not taken: R1-S4's further option, an S16
    check in `StartMaintenance` (the record now ends with its work). The ADR lists the new
    transition.
21. **A client attach is never refused** (R1-C8, R1-S5). S5's promise kept, as the orchestrator
    preferred: `Restart` on a maintenance record naming its node is a client attach (a new
    epoch, hold := client), which deposes the maintenance target. S4 amended, since its "keeps
    the kind" clause contradicted S5. The design's findings 2 and 3, settled in the design only,
    are recorded here. Finding 3 is now enforced in code (R1-S5): `ControllerPublishVolume`
    answers `Unavailable` when the answer's `Volume.held` is false, so S19's "mixed versions
    stay safe" holds as stated, and the rollout's step 2 no longer waits for every metadata
    instance. ADR 0014's Decision lists the transitions (four new, and the attach's three
    modes) and the spec's count is corrected; its accepted risk now says that maintenance on
    another node after a wrong unhold locks the pod's node out until an administrator attaches
    it again or Kubernetes publishes it again.
22. **No progress, for every volume** (R1-C10): the `Drainer` keeps, per volume, when its
    replicas on the node last went down, in use or not, and the line says so past `Stall`.
23. **Lab step 4** (R1-C11): a second round on two new volumes, fl-b undrained first; the node
    whose `maintenance` lists the target is killed once every move is requested, then started
    again.
24. **ADR 0014** (R1-C12): renamed from 0007 (0007–0011 are the audit's, 0012 #12's, 0013
    #16's), its README row added as Proposed (the orchestrator's decision; the one exception to
    "never touch docs/adr/"). Approving the plan does not accept it (the design's ADRs; the
    security Nit).
25. **An unknown hold value is held** (R1-S7): `Held()` is a target with a hold neither
    `unheld` nor `maintenance`, so it fails closed.
26. **A stale client token leaves the maintenance target running** (R1-S8): the socket ends a
    maintenance entry only for a verified token whose epoch is above the entry's (S12).
27. **Listed ids validated, the list capped** (R1-S9): an entry failing `ValidID` is dropped and
    logged; `ListMaintenance` answers at most 1024 entries, as #13's `ListReleases`.
28. **Stale lines** (both Nits on them): the design's "There is no `decisions.md` yet", the
    spec's "Decisions and design come after approval", and the design's "Approving this design
    accepts it".

## Changes after approval: plan review round 2 (2026-11-14)

Round 2 of this batch's plan review, incremental, on commit 937f1e1 (the diff b904a18..c25679e):
plan-reviewer (design-code and design-security, one agent), findings R2-C1 to R2-C4, R2-B1 and
the Nits R2-N1 to R2-N4. Every change below was made after approval, and is not approved
itself. Each finding's recommended fix was taken. No change narrows the intent.

29. **The simulator's end check can fail** (R2-C1). Its hold record took any applied end as
    "not held", the same as the bug `end_any_hold` did, and neither check looked at ends. Now
    the record takes each request as the model should admit it (an end leaves it not held only
    at the node and epoch it last recorded as maintenance), each check runs before the record
    takes the request, and `maintenance over a held record` covers `EndMaintenance`. The spec's
    Tests and the ADR's Confirmation say so.
30. **A failed start is reported and backed off** (R2-B1). A start that took its epoch and then
    failed (refused at the registry or the run mark, `AttachFailed`, a failed mark, a time-out,
    a `Detached` before `Attached`) reached nobody, so the `Maintainer` either kept counting it
    or took an epoch at every scan, while the drain kept waiting on a node that "took an
    epoch". Now the main thread sends `Input::MaintainFailed`, the `Maintainer` gets
    `End::StartFailed`, frees the start's place and backs the volume off for 5 minutes, and the
    engine sends `EndMaintenance` at that epoch. S17: a node that took no epoch, or more than one
    without a replica moved, is asked elsewhere after `Stall`; one epoch counts as a start, not
    as progress. S11 and S17 changed; engine and drain tests added; the Risk that called the
    pass-over a mere saving is corrected.
31. **The router ends a maintenance entry** (R2-C2). Nothing ended it after `Attached`, so its
    `--max-maintenance` place leaked and S12's client attach waited out its 30 s and failed.
    Now the router, routing the entry's `Detached`, removes it under the registry's lock and
    wakes its waiters; the main thread's start wait ends at once on its generation's
    `Detached`. Tests of every kind of end, and of a client attach over a start in flight.
32. **Rollout step 2 waits for step 1 again** (R2-C3), reversing that part of item 21; the
    publish check of `held` stays. `ControllerUnpublishVolume` answers `Unimplemented` as
    `Unavailable`, so Kubernetes retries instead of losing the unhold; `lab.sh vol detach` fails
    on a failed unhold. S19 and the ADR say so.
33. **An injected bug for the reversed `Restart` row** (R2-C4):
    `meta::Bugs::restart_keeps_maintenance` (`catches_restart_keeping_maintenance`), in the
    spec's Tests and the ADR's Confirmation (seven bugs).
34. **The rules changed after approval, listed** (R2-N1): the spec's list now names the Terms,
    S2 and S20. S2's change, made in round 1 and recorded now: "the kind is client" became "the
    volume is held", as the design's row (`Held()`) and S6 ("until a client attach or a
    release") already said; so an unhold applies to a record from before this change, and
    (R1-S7) to a hold value this version does not know. S20 gained "its end".
35. **S19's "changes nothing"** (R2-N2) is said only of a record naming the publishing node;
    over another node an older instance rewrites the header, held by S6.
36. **The Terms and S4 agree** (R2-N3): a client attach names the node agent's restart of its
    node's target (`only_if_target`); the maintenance node's own restart of a maintenance target
    is a start (S4). S4 and ADR 0014 say "the agent's restart".
37. **The stall clock's wording** (R2-N4): the drain's sighting (its node, epoch and time) is
    set at this run's first sight of a maintenance record naming that node and kept across new
    epochs and the drain's own new ask of the same node, until it asks another node or the
    volume is held. The design and S17 say so.

## Changes after approval: plan review round 3 (2026-11-14)

Round 3 of this batch's plan review, incremental, on commit a4c01a7 (the diff 937f1e1..fdf6b6e):
plan-reviewer (design-code and design-security, one agent), findings R3-B1, R3-C1 to R3-C4 and
the Nits R3-N1 and R3-N2. Every change below was made after approval, and is not approved
itself. Each finding's recommended fix was taken. No change narrows the intent.

38. **An older instance is passed over within the call; the rollback order** (R3-B1). The
    CSI controller's `metaCall` (`control/csi/meta.go:63-84`) moves on only on a transport
    error and keeps any other answer's instance as `preferred`, so Kubernetes' retries kept
    reaching the same old instance. The unhold and the publish's attach now go through a
    variant that moves to the next instance, in the same call, on `Unimplemented` or on an
    answer without `held`, leaving `preferred` unchanged; only when every instance answers as
    old is the call `Unavailable`. The rollback order (the CSI controller and `lab.sh` before
    the metadata service, since an older service fails every publish and unpublish) is stated
    in the design's Rollout, S19, ADR 0014 and the planned `docs/lab.md` text. Test: two
    instances, the first old, a publish and an unpublish both done through the second.
39. **A failed start is ended and reported once** (R3-C1). The main thread is a failed start's
    one ender and reporter; the router ends, on `Detached`, only a running entry, and only the
    entry of that generation, so it never ends a client's entry let in after it; the engine
    ends a maintenance attach that never reached `Attached` without S11's end (no log, no
    `on_ended`, no `EndMaintenance`); `on_ended` ignores a `(K, epoch)` it no longer holds.
    Test: after a failed mark, one place freed, one `EndMaintenance`, no "ended by root".
40. **No `Attaching` left behind** (R3-C2). On every failed start, a `Detached` before
    `Attached` included, the main thread sends `Input::Detach` with the generation after its
    own `Input::Attach`, so a socket's detach that reached the engine first leaves no
    `Attaching`. Test with that order: the client attach succeeds.
41. **S17 counts no epochs** (R3-C3). The drain's own re-ask of the same node, and a start
    retried after a lost answer, raised the epoch too, so "more than one" deposed a healthy
    copying target. Of the finding's options, the first was taken, as the orchestrator
    directed (it excludes both causes; the other would accept a lost answer's extra epoch). A
    failed start is now seen as the sighting's node ending its maintenance record while a
    move is still pending (its `EndMaintenance`, item 30; root's end looks the same), and the
    drain asks another node at once. The only epoch compared is whether the node took any
    since the sighting began or since the drain's own last ask of it. S17 changed. To make
    the signal reliable, the `Maintainer` sends a failed start's `EndMaintenance` again at each
    scan that still lists the volume at that epoch. Tests: a re-ask after an end at no work, and
    a retried start, each copying past `Stall`, not asked elsewhere; a failed start asked
    elsewhere at once. Left, in the Risks: a crash loop that keeps the node's heartbeat and
    never ends the record is not asked elsewhere.
42. **`hold lost` runs after the record takes the request** (R3-C4); `maintenance over a held
    record` runs before. The design and the spec now say so, as the ADR did.
43. **The Scenarios line follows the Terms** (R3-N1): "the agent's `only_if_target` restart
    on the maintenance node".
44. **`on_ended` takes `now`** (R3-N2), from which the 5-minute back-off is timed.

## Changes after approval: plan review round 4 (2026-11-14)

Round 4 of this batch's plan review, incremental, on commit 958436b (the diff a4c01a7..9e3a5e2):
plan-reviewer (design-code and design-security, one agent), findings R4-C1 to R4-C4 and the
Nits R4-N1 and R4-N2. Every change below was made after approval, and is not approved itself.
Each finding's recommended fix was taken, cutting where the finding allowed it. No change
narrows the intent.

45. **The router marks a maintenance entry running** (R4-C1), as it routes the entry's
    `Attached`, under the registry's lock. Nothing said who did, so a `Detached` routed between
    the router's `Attached` and the main thread's taking it found a starting entry, and nobody
    ended it. Now a `Detached` routed first is seen first by the main thread, which ends the
    entry; routed after, it finds the entry running, and the router ends it. Test of that
    order.
46. **A start that fails within one drain step is seen** (R4-C2). The drain's own applied
    `AttachMaintenance` counts as a read of the node's maintenance record with a move pending,
    so a start that fails before the drain's next step is a failed start at that step, asked
    elsewhere at once, not asked again of the same node until `Stall`. Test with no read
    scripted between the ask and the next step.
47. **No `Input::Detach` for a refusal before the entry** (R4-C3). Of the finding's two
    fixes, the cut was taken: the main thread sends `Input::Detach` only for an entry it
    reserved, with that entry's generation. A refusal has no generation, and the volume may be
    a client's (today's `Engine::detach` acts by volume alone, `engine.rs:1856-1860`); every
    `Input::Detach` left names an entry its sender holds, so no generation check in the engine
    is added. Test: a refusal leaves a client's target running.
48. **The drain's own ask sets the sighting's time with its epoch** (R4-C4). Kept across that
    ask (item 37, R2-N4), the time let `Stall` fire right after a re-ask, before the node could
    start, and a candidate ranked lower could depose a start just begun. With failed starts
    seen directly (item 46), R2-N4's reason for keeping it is gone. The node's own new epoch
    still resets neither. S17 changed; test of a re-ask with both clocks long past `Stall`.
49. **The test of `preferred`** (R4-N1): the variant never stores the old instance as
    `preferred` (it stores the instance that answers as new, as `metaCall` does); item 38's
    "leaving `preferred` unchanged" is corrected. The test starts with `preferred` at the old
    instance, so it can fail.
50. **The router's removal, as worded in a test** (R4-N2): "the router's removal on the
    maintenance generation's `Detached` leaves a later client entry alone".

## Changes after approval: plan review round 5 (2026-11-14)

Round 5 of this batch's plan review, incremental, on commit 5455e49 (the diff 958436b..517c8d6):
plan-reviewer (design-code and design-security, one agent), findings R5-1 to R5-3. Every
change below was made after approval, and is not approved itself. Each finding's recommended
fix was taken. No change narrows the intent.

51. **The failed-start rule, worded** (R5-1): the record "is no longer a maintenance record"
    (its maintenance ended: unheld, still naming the node), with the move still pending. Read
    literally, "names that node, not held" also matched a maintenance record the node had not
    yet started, the usual state one step after the drain's own ask (item 46), so the drain
    could ask elsewhere and depose a start just begun. A record still that node's maintenance
    record stands. Test: an ask applied, the next step finds the record standing at the same
    epoch, and nothing is asked.
52. **A maintenance detach touches only a maintenance target** (R5-2). Root's `maintenance
    end` and the main thread's step 5 send the new `Input::DetachMaintenance { volume,
    generation }`, which the engine applies only to a `Kind::Maintenance` target or
    `Attaching` of that generation. Item 47's claim that every `Input::Detach` names an entry
    its sender holds was wrong for root's end: looked up first, it can arrive after the entry
    ended at no work and a client attached, and `Engine::detach` acts by volume alone
    (`engine.rs:1856-1860`). S12's client attach keeps #15's `Input::Detach` (it reserves only
    after the entry ended, and its attach follows on the one engine channel). Test: root's late
    end leaves the client's target running.
53. **The waiter is an ordered queue; a time-out reads the running mark** (R5-3). The main
    thread takes the waiter's messages in the order the router routed them, so an `Attached`
    then `Detached` is never read as a `Detached` before `Attached`. At a time-out it reads the
    entry's running mark under the registry's lock: if running, the start succeeded (its
    `Attached` came with the time-out), so the main thread neither ends it nor reports
    `MaintainFailed`, and its later end is a running target's normal end, through S11 and the
    router. If still starting, the main thread removes the entry in that same lock hold, so an
    `Attached` routed afterwards finds no entry to mark, then reports the failed start. Every
    removal, the router's and the main thread's, is by generation and
    idempotent, freeing the place only when it removes an entry. Test: a time-out concurrent
    with `Attached`.

## Changes after approval: plan review round 6 (2026-11-14)

Round 6 of this batch's plan review, incremental, on commit ebd8e23 (the diff 5455e49..747b560):
plan-reviewer (design-code and design-security, one agent), findings R6-B1 to R6-B4, all
orderings between the main thread, the router, the engine, root's `maintenance end` and a
client attach. Every change below was made after approval, and is not approved itself. No
change narrows the intent.

54. **The maintenance entry's lifecycle, as one state machine** (R6-B1 to R6-B4; the
    orchestrator's direction). Rounds 4 to 6 each found a new ordering in the prose of items
    30, 31, 39, 40, 45, 47, 52 and 53, so that prose is replaced by one table in the design:
    the states `starting`, `running`, `ending` and `gone`; the events (the main thread's
    `Maintain`, `MarkEpoch` and time-out, the router's `Attached` and `Detached`, root's
    `maintenance end`, a client attach); and for each cell the next state, who acts under the
    registry's lock, what is sent, and whether the place is freed. Every cell keeps four
    invariants: one end, one place freed, no client attach refused for the maintenance entry
    (S5, S12), no client's target ended by a maintenance event. The rules beneath it:
    - the main thread reserves the entry and enqueues its `Input::Attach` in one hold of the
      registry's lock, so whoever sees the entry sees it with that attach already queued;
    - every maintenance detach, S12's included, is `DetachMaintenance` with the entry's
      generation;
    - the engine's last output for a generation is one `Detached` (an attach it refuses
      included, in place of `AttachFailed`), and the engine makes the generation's one report
      just before it: `StartFailed` if it never reached `Attached`, else the S11 end;
    - only the router removes an entry, on that `Detached`, so the place is freed once and a
      client never reserves while the engine still holds the generation;
    - the main thread's part ends at `Input::Marked`, so none of its failures can race
      `Attached`; `MaintainFailed` is left only for a start refused before its entry.

    This replaces, and where they differ reverses (listed in round 7, R7-N3):
    - item 30's `MaintainFailed` for every failed start: now only for a start refused before
      its entry;
    - item 39's main thread as the one reporter, and its dropped start with no log and no
      `on_ended`: now the engine reports, and logs `maintenance start failed`;
    - item 40's main-thread detach after a `Detached`: in `gone` the main thread sends nothing;
    - item 45's main thread ending the entry on an early `Detached`: only the router removes;
    - item 52's exception for S12: S12 detaches with `DetachMaintenance` too;
    - item 53's time-out rules: no time-out can now meet a running entry.

    Test: `maintenance_entry_lifecycle`, table-driven, one case per row, the invariants checked
    after each, with the orderings of rounds 2 to 6 as named cases (split in round 7 into the
    node's and the engine's halves, item 59).
55. **S12's detach is `DetachMaintenance` with the entry's generation** (R6-B1). Of two client
    attaches at once, the first detaches that generation and the second waits for the entry to
    be gone (the table's `ending` row; wording corrected in round 7, R7-N1); neither ends
    anything of either client's. Test: the first client's target keeps running.
56. **No main-thread removal; the report precedes the removal** (R6-B2). The entry stands
    until the engine's `Detached`, so a client cannot attach before the engine has dropped the
    start. After a time-out or a failed mark no `Input::Marked` was sent, so no late `Attached`
    comes, and the engine's `StartFailed`, made before its `Detached`, keeps the back-off. An
    `Attached` routed while `ending` (a detach queued after `Input::Marked`) leaves the entry
    `ending`, and its end is the S11 end with root's or the client's reason, since that start
    had run. Test: a time-out with the entry starting, no late `Attached`, one back-off, no
    second epoch.
57. **An entry gone at the time-out** (R6-B3) means the engine has already reported: the main
    thread sends and reports nothing. Test case in the table.
58. **Root's end before the start's attach** (R6-B4) cannot happen: the entry becomes visible
    only in the hold that queues its `Input::Attach`. Root's end is therefore applied, with
    nothing to remember. Applying was chosen over refusing: root's request is valid once the
    entry exists, and a refusal would make root retry a race it cannot see. Test: root's end
    sent at once after the reserve ends the start.

## Changes after approval: plan review round 7 (2026-11-14)

Round 7 of this batch's plan review, incremental, on commit 30cf82d (the diff ebd8e23..fc6cac8):
plan-reviewer (design-code and design-security, one agent), findings R7-B1 and R7-B2 and the
Nits R7-N1 to R7-N3. The reviewer found the table itself sound: every cell covered and keeping
the four invariants. Every change below was made after approval, and is not approved itself.
Each finding's recommended fix was taken. No change narrows the intent.

59. **The engine's half of the lifecycle gets its own test** (R7-B1). With the engine
    scripted, the node test only repeated the script for the engine's rules (the generation
    filter of `DetachMaintenance`, one `Detached` per generation, `StartFailed` before it), and
    round 6 had dropped the R5-2 engine assertion.
    - New `maintenance_generation_end` in `flaminio-node/tests/engine.rs`, with the real engine
      and a fake metadata client, covers each way a generation ends, on its own. For each it
      checks one report, then one `Detached` as the last output, `StartFailed` exactly when
      `Attached` never came, and `EndMaintenance` as S11 says.
    - It also restores the R5-2 assertion: a `DetachMaintenance` of another generation, over a
      client's target, or after the end, emits nothing; a `Marked` after a drop sends no
      `Hello`.
    - The node test keeps the node's side only. The table's bundled rows are split, so that
      each row is one event and one case: `MarkEpoch` apart from the time-out in `ending` and
      in `gone`, and a second `Detached` apart from an unknown generation's output.
    - R6-B4's case gets a hook: whenever root's lookup, run each time the starting thread lets
      the registry's lock go, sees the entry, the fake engine channel already holds its
      `Input::Attach`.

    Each case names the rule it fails without.
60. **The 30-s answer** (R7-B2). Past #15's 30 s, root's `maintenance end` answers `error:
    VOLUME: the maintenance target did not end within 30 s`, as #15's detach does, and a
    client attach over the entry answers the same and reserves nothing. The entry stays
    `ending` until the router removes it on `Detached`. The answer is added to the socket line
    and to the table as two rows. Test: a scripted engine withholds `Detached`.
61. **Two client attaches, as the table says** (R7-N1): the first detaches the generation, the
    second waits for gone; the design's invariant and item 55 now say so.
62. **The `starting | Detached` row's other causes** (R7-N2): besides an attach the engine
    refused, the engine dropping G after `Input::Marked`, and a running target's `Detached`
    routed ahead of its `Attached` (#15 routes them after the engine's lock is let go, on
    whichever thread took them). The row now gives the engine's report as `StartFailed` or the
    S11 end, as the case was. The `gone` rows add the main thread taking `Detached` from its
    waiter.
63. **Item 54 names what it reverses** (R7-N3): items 30, 39, 40, 45, 52 and 53.

## Changes after approval: plan review round 8 (2026-11-14)

Round 8 of this batch's plan review, incremental, on commit 24df440 (the diff 30cf82d..7d5d9a0):
plan-reviewer (design-code and design-security, one agent), findings R8-L1 to R8-L4 and the Nit
R8-N1. Every change below was made after approval, and is not approved itself. Each finding's
recommended fix was taken. One spec rule is narrowed, where the finding shows it cannot hold
(item 65).

64. **What the engine test can see of the report** (R8-L1). The report to the `Maintainer`
    is an in-engine call, and `Detached{G}` an output of the same step, so "the report comes
    first" could not be observed. The rule now reads: in the same step that emits
    `Detached{G}`, and not after it, the engine reports, so the `Maintainer`'s back-off already
    holds when `Detached{G}` is taken. The test takes `Detached{G}`, turns the engine, and finds
    the `Maintainer` already acting on the report. "Exactly one report" is counted in the log
    lines and the `EndMaintenance` calls on the fake metadata client, which `on_ended`'s
    tolerance of repeats cannot hide.
65. **S5 and S12 narrowed to what holds** (R8-L2). They said a client attach is never refused
    or made to wait for maintenance. On the maintenance node the client's device must wait for
    the maintenance target to end, and #15's 30-s bound for a detach caps that wait, so neither
    "never made to wait" nor "never refused" can hold there. Now:
    - the metadata service never refuses a client attach or makes it wait (S5, unchanged in
      substance);
    - on the node, the client's attach waits for the maintenance end, at most 30 s, and past
      that is refused with nothing reserved, `the maintenance target did not end within 30 s`.

    The CSI node plugin (`control/csi/node.go`, `device`) maps that exact prefix to
    `Unavailable`, not `Internal`, as #15's map does for an attach in progress, so kubelet
    retries; a `csi_test.go` case checks it, and that the words about another volume are not
    taken for it. The ADR's Decision and the design's invariant say so. S19 needs no change.
66. **Where the R6-B4 hook lives** (R8-L3). The registry's mutex is private to its module;
    every hold goes through `Registry::lock()`, whose `RegistryGuard` calls a `cfg(test)`
    release hook in its `Drop`. A later hold therefore cannot skip the hook: locking the mutex
    otherwise would not compile. The test runs root's lookup from the hook at every release,
    and checks that the hook ran during the start.
67. **The `on_ended` repeat is a `maintenance.rs` unit test** (R8-L4), not an engine test: the
    `Maintainer` is inside the engine, and the rule is its own.
68. **A 30-s wait that expires as the entry goes `gone`** (R8-N1) reads the state under the
    registry's lock: gone wins, so root is answered `ok` and the client reserves.

## Changes after approval: plan review round 9 (2026-11-14)

Round 9 of this batch's plan review, incremental, on commit a28a267 (the diff 24df440..2ad19b9):
plan-reviewer (design-code and design-security, one agent), findings R9-B1 to R9-B5. Every
change below was made after approval, and is not approved itself. No change narrows the intent.

69. **The lock-guard hook is cut; R6-B4 holds by construction** (R9-B1; the orchestrator's
    direction). Item 66's `RegistryGuard` hook, as designed, deadlocks (its `Drop` ran a lookup
    while the inner guard was still held) or recurses (the lookup's own guard calls the hook
    again), and did not fit #15's `Mutex<Registry>`; it is removed rather than repaired.
    - Now one new method of #15's registry, `Registry::reserve_maintenance(volume, token,
      &attach_tx)`, is the only way a maintenance entry is inserted, and it sends the
      `Input::Attach` before it returns, within the caller's one hold.
    - The registry's entries stay a private field, so `control.rs` reaches them only through
      its methods, and #15's `reserve` makes device entries only. No new module.
    - The test checks the outcome: when `reserve_maintenance` returns `Ok(G)`, the channel
      already holds `Input::Attach{G}`. No other insert path is enforced by the compiler
      (privacy) and by a check after every lifecycle case that each maintenance entry's attach
      came first among its generation's inputs.
70. **The narrowing carried to every live promise** (R9-B2): the spec's Tests and the node
    test's invariant now say "no client attach refused or made to wait except by the 30-s
    bound", and the design's `Restart` bullet says the metadata service refuses none, the
    device waiting at most 30 s. Item 21's wording is history and stays.
71. **What the plugin sees, and the attach's own limit** (R9-B3). `ControlSocket.command`
    turns `error: VOL: ...` into `data plane: VOL: ...` (`dataplane.go:83`), so the plugin
    matches `data plane: <volume>: the maintenance target did not end within 30 s`. The wait is
    cut by the attach's own time limit (`within`, `dataplane.go:107`): a caller with less than
    30 s left gets #15's time-out answer, `DeadlineExceeded` in the plugin. The plugin test
    feeds the real socket text through `command`.
72. **The same-step check can fail** (R9-B4). Straight after the `handle` or `turn` whose
    outputs hold `Detached{G}`, before any further input or turn, the engine test reads the new
    read-only `Engine::maintenance()` (as `holding()` is). An engine that reports at the head
    of the next step, named as the injected late reporter, fails it. Log lines are counted by a
    recording log sink the test installs.
73. **"Gone wins" at 30 s is tested** (R9-B5). The test holds the registry's lock and, in that
    hold, removes the entry as the router does and moves the fake clock past 30 s; root is
    then answered `ok`, the client reserves, and the place is freed once.

## Changes after approval: plan review round 10 (2026-11-14)

Round 10 of this batch's plan review, incremental, on commit 9a54f04 (the diff
a28a267..7daf8ea): plan-reviewer (design-code and design-security, one agent), findings R10-1
to R10-5. Every change below was made after approval, and is not approved itself. Cuts were
preferred. No change narrows the intent.

74. **`Registry` in a module of its own** (R10-1). Privacy is per module, and #15 puts the
    registry in `Node`, in `ublk.rs`, where the main thread also lives, so item 69's claim
    that only `reserve_maintenance` inserts a maintenance entry was not the compiler's.
    - `Registry` now lives in `mod registry` in `ublk.rs` beside `Node`: a private field, a
      private maintenance-entry constructor called only by `reserve_maintenance`, and
      `pub(super)` methods. It is listed in Files touched.
    - This changes #15's layout; whichever of #15 and #17 lands second makes it (the
      design's Risks).
    - The single-threaded backstop check after every case (each maintenance entry's attach
      first) is cut, since privacy now enforces the rule and that check could not fail
      against a reserve and a send in two holds.
75. **Its own text for a wait cut by the attach's time limit** (R10-2), correcting item 71.
    Not #15's "before it could begin", which is for an attach never begun: the answer is
    `error: VOLUME: the attach's time ran out waiting for the maintenance target to end`, seen
    by the plugin as `data plane: VOLUME: the attach's time ran out waiting for the maintenance
    target to end` and mapped by that exact prefix to `DeadlineExceeded`. It is named in the
    plugin bullet, the table row, the socket section and the plugin test's input.
76. **No log-line counting** (R10-3), correcting item 72: the engine logs with `eprintln!`,
    with no sink to install. The engine test counts `EndMaintenance` calls on the fake
    metadata client and reads `Engine::maintenance()`'s state (one place freed, one back-off).
77. **How the "gone wins" waiters see the clock and are woken** (R10-4).
    - Each 30-s wait is a condvar wait on the registry's lock, its deadline taken from the
      node's injected clock; the fake clock's advance and every removal notify it; the waiter
      reads the state before the clock.
    - The test waits until both waiters are blocked (a `cfg(test)` count, raised under the
      lock just before the wait, reaching 2), then in one hold calls the router's own removal,
      `Registry::remove_generation`, which wakes the waiters, and moves the clock past 30 s.
78. **The test's own channel** (R10-5): the node under test is built with its own
    `mpsc::channel::<Input>()`, and the test keeps the receiver for `try_recv`.

## Changes after approval: plan review round 11 (2026-11-14)

Round 11 of this batch's plan review, incremental, on commit 8a53a4b (the diff
9a54f04..55ae108): plan-reviewer (design-code and design-security, one agent), findings R11-B1
to R11-B3. Every change below was made after approval, and is not approved itself. No change
narrows the intent.

79. **`Node`'s wrappers, not `pub(crate)` registry methods** (R11-B1; the orchestrator's
    direction). `pub(super)` items of `ublk::registry` are visible in `ublk` only, while
    `control.rs` and `node.rs` are its sibling modules under the crate root (`main.rs:10-21`).
    - `Node`, the main thread and the router, all in `ublk.rs`, call the registry's
      `pub(super)` methods.
    - `control.rs` and `node.rs` reach the registry only through `Node`'s `pub(crate)`
      methods, whose only maintenance insert is `Node::reserve_maintenance`.
    - Making the registry's methods `pub(crate)` was rejected, since any module could then
      insert.
    - Named in the design's rule, Contracts, Files touched, the test and the Risks.
80. **The 30-s wait's loop and the fake clock's wake** (R11-B2).
    - The wait is a loop under the registry's lock: read the entry's state, then the injected
      clock, then `wait_timeout` for a bounded real slice (at most 100 ms, or the time left on
      the injected clock).
    - Every removal calls `notify_all`, and so does the tests' fake clock's `advance`.
81. **The second plugin mapping in the files list** (R11-B3): the `control/csi/node.go` line
    names both prefixes, the 30-s one to `Unavailable` and the attach's-time one to
    `DeadlineExceeded`.

## Changes after approval: plan review round 12 (2026-11-14)

Round 12 of this batch's plan review, incremental, on commit 62a3ee9: plan-reviewer
(design-code and design-security, one agent), findings R12-B1 and R12-B2. Every change below
was made after approval, and is not approved itself. No change narrows the intent.

82. **The clock, defined** (R12-B1). `flaminio-io` reads `Instant::now()` directly, and #15
    adds no clock, so "the node's injected clock" did not exist; this is #17's.
    - A small `Clock` trait in `ublk.rs`, with `now() -> Instant`. `Node` holds an
      `Arc<dyn Clock>`, the system clock in production. Only the maintenance waits read it.
    - The tests' fake clock implements it; its `advance` calls `notify_all` on the condvar
      `Node` owns beside its `Mutex<Registry>`, taken through a `cfg(test)` accessor.
    - Listed in Contracts, Files touched and the Risks.
83. **How `control.rs` reaches `Node`, and the wait as a `Node` method** (R12-B2).
    - `control::spawn` takes an `Arc<Node>` in place of today's `attached: Attached`
      (`control.rs:41-45`, an `Arc<Mutex<..>>`), keeping its `ctl` and `engine` senders. This
      matches today's code with the least change: the socket's threads already touch shared
      state, and #15's socket reserves entries itself.
    - A `Control` message per wait was rejected: it would block the main thread, which takes
      attaches one at a time, for up to 30 s.
    - The 30-s wait leaves the registry's `pub(super)` list and becomes a `Node` method,
      since its condvar pairs with `Node`'s `Mutex<Registry>`. `Node::remove_generation`
      removes under that lock and then calls `notify_all`; the test uses its held-guard form.
    - `node.rs` only parses `--max-maintenance` into `NodeConfig`.

## Changes after approval: plan review round 13 (2026-11-14)

Round 13 of this batch's plan review, incremental, on commit 3d16fc1 (the diff
62a3ee9..c19d1d4): plan-reviewer (design-code and design-security, one agent), findings R13-L1
to R13-L3. The reviewer found no new security issue. Every change below was made after
approval, and is not approved itself. Cuts were preferred. No change narrows the intent.

84. **The fake clock's slot, and an `Arc<Condvar>`** (R13-L1). The clock exists before
    `Node`, so it cannot take `Node`'s condvar when built. `Node`'s condvar is an
    `Arc<Condvar>` (Contracts), with a `cfg(test)` accessor that clones it. The fake clock
    holds a `OnceLock<Arc<Condvar>>` slot that the test fills after building `Node`.
85. **Only the 30-s wait reads the `Clock`** (R13-L2, a cut). The main thread's wait for a
    maintenance start stays on real time with `METADATA_LIMIT`, as today (`ublk.rs:858`): it
    waits on the entry's waiter, not on `Node`'s condvar, so a fake clock could not wake it.
    The lifecycle test's two time-out cases wait out `METADATA_LIMIT` on real time.
86. **Today's users of `Attached`, traced** (R13-L3). `Node` is built first in `serve_node`,
    where `attached` is made today (`ublk.rs:722`), and is handed to `control::spawn` and to
    the startup attach's retry thread. Each other use is #15's to replace, with #15's lines
    cited:
    - the type and its making (`:682`, `:722`): the registry in `Node`;
    - the attach and device paths (`:838`, `:939`): #15's main thread and device thread;
    - `control.rs`'s `serve` and `detach` (`:72`, `:192`): #15's `detach` over entries;
    - the startup retry's check (`:752`): #15's "stops once that volume has an entry". #17
      counts a device entry only there, so that a maintenance entry does not end the retry.

## Changes after approval: plan review round 14 (2026-11-14)

Round 14 of this batch's plan review, incremental, on commit 324191e (the diff
3d16fc1..958b9f2): plan-reviewer (design-code and design-security, one agent), findings R14-L1
and R14-L2. The reviewer found no new security issue. Every change below was made after
approval, and is not approved itself. No change narrows the intent or a spec rule.

87. **The main thread's start limit, a `Node` field** (R14-L1). The two time-out cases waited
    out `METADATA_LIMIT`, 30 s each, in `make test`. `Node` now carries `start_limit`:
    `METADATA_LIMIT` in production, set by a `cfg(test)` constructor argument. The two cases
    use about 200 ms; production is unchanged.
88. **The startup attach goes through S12; S5 kept** (R14-L2; the orchestrator preferred
    keeping S5 over the reviewer's refusal). The startup `--attach` and its retries are sent
    to the main thread over `ctl_tx` (`ublk.rs:745-767`), so "goes through S12" did not say
    who runs S12's end-and-wait, which must not run on the main thread. It works as follows,
    with today's code and #15's design:
    - The attempts run on the startup thread (`ublk.rs:749`), and #15 gives the startup attach
      the same check and reservation as the socket's (#15 :71), made before the request is
      queued to the main thread.
    - So the startup thread runs S12's step through the same `Node` methods the socket's
      thread uses: `DetachMaintenance{G}`, a wait for the entry to be gone, then the reserve
      and the queued attach.
    - A wait that runs out fails that attempt, and the retry tries again 5 s later, up to
      `STARTUP_ATTACH_LIMIT`. The first attempt meets an empty registry.

    The reviewer's refusal ("volume is in maintenance") is therefore not needed. Test: a
    startup attach with a maintenance entry present ends the maintenance target and attaches,
    without blocking the main thread.

## Changes after approval: plan review round 15 (2026-11-14)

Round 15 of this batch's plan review, incremental, on commit a09eb34 (the diff
324191e..85c678e): plan-reviewer (design-code and design-security, one agent), finding R15-C1.
The reviewer found no new security issue. The change below was made after approval, and is
not approved itself. It narrows nothing.

89. **The startup attempt, a named function with a test seam** (R15-C1). A failed S12 wait
    on the startup thread produced no reply on the loop's `answer`, so the loop at
    `ublk.rs:745-767` had no defined place for it, `done()` was not re-checked after the
    wait, and the fixed 5-s sleep inside a closure gave a test nothing to drive.
    - `startup_attempt(&Node, volume, token, deadline) -> Result<Receiver<String>, String>`
      returns `Err(answer)` for every failed attempt, one that never reached the main thread
      included. The loop treats it as a failed reply: logged, then the same `done()` and sleep
      path.
    - `done()` is re-checked after the wait, before the reserve.
    - The interval between attempts is a parameter: 5 s in production, milliseconds in tests.
    - The first attempt still runs before the control socket opens.
    - Tests added: a wait that runs out; a device entry made during the wait; and the startup
      retry and a socket client at once over one maintenance entry (one `DetachMaintenance`,
      one reserve, the other refused or stopping at `done()`).

## Changes after approval: plan review round 16 (2026-11-14)

Round 16 of this batch's plan review, incremental, on commit 98bfbe2 (the diff
a09eb34..db43ba2): plan-reviewer (design-code and design-security, one agent), finding R16-C1.
The reviewer found no new security issue. The change below was made after approval, and is
not approved itself. It narrows nothing.

90. **The first attempt's hand-off, and the concurrent test's outcomes** (R16-C1).
    - **The hand-off.** The retry thread's loop starts from the first attempt's `Result`, not
      from a receiver. `Ok(rx)` goes to `rx.recv()`; an `Err(answer)` is logged and goes down
      the same `done()`, sleep and `done()` path, so a failed first attempt is retried. Test:
      a failed first attempt is logged and retried after the interval.
    - **`done()`.** It counts a running device entry of the volume, as today's `attached` is
      set only once the device runs (`ublk.rs:752`). This refines item 86's "a device entry":
      another client's attach still starting, which may yet fail, does not end the retry.
      (Withdrawn by item 91.)
    - **The concurrent test, both orders, each winner asserted.** The test lines both runners
      up with the `cfg(test)` waiter count, removes the entry, and opens a `cfg(test)` gate
      (after each runner's wait, before its reserve) for the chosen winner first. (The gate's
      position is superseded by item 93: the startup runner's gate sits between its `done()`
      re-check and its reserve.)
      - Socket first: the startup attempt's reserve is refused with #15's answer over a
        starting entry, `error: VOLUME is starting here` (#15 design.md:78). This is the
        reviewer's "already attaching" case, named by #15's exact text. The startup retry ends
        at the `done()` after the interval. (Superseded by items 92 and 94: the retry ends at
        the first `done()` after the `Err`.)
      - Startup first: the socket client's reserve is refused with that same answer.

      The refusal is a conflict between two clients, as on #15, consistent with S5 as
      narrowed.

## Changes after approval: plan review round 17 (2026-11-14)

Round 17 of this batch's plan review, incremental, on commit f28981a (the diff
98bfbe2..71d614d): plan-reviewer (design-code and design-security, one agent), findings R17-L1
to R17-L3. The reviewer found no new security issue. Every change below was made after
approval, and is not approved itself. No change narrows the intent.

91. **`done()` follows #15's rule again; round 16's "running only" is reverted** (R17-L1;
    the orchestrator's decision).
    - Round 16 made the startup retry's `done()` count only a running device entry, citing
      today's `attached`. #15 replaces `attached` with "its retry stops once that volume has
      an entry", in any state, and tests it (#15 design.md:71, :726). So #17 contradicted
      #15's test, and one of the two would fail if both landed.
    - Under the user's rule (where plans conflict, follow the approved, review-clear one,
      here #15), the departure is cut: `done()` stops the retry once the volume has any
      device entry, starting, running or stopping, read from the registry. Round 13's "a
      device entry" wording (item 86) is restored; item 90's `done()` point is withdrawn.
    - A maintenance entry still does not stop the retry.
92. **The concurrent test's refusal text is held** (R17-L2). The answer is `error: VOLUME is
    starting here` only while the winner's entry is still `starting`. The test therefore holds
    the winner's metadata phase unanswered until the loser's reserve is refused, then
    releases it. Under item 91, when the socket client wins, the startup retry's next `done()`
    sees the socket client's device entry and ends at once. When the startup wins, the socket
    client gets the same text.
93. **The gate sits between the startup runner's `done()` re-check and its reserve**
    (R17-L3), so the refusal case is exercised. The case where the startup runner stops at
    `done()`, a device entry made before its re-check, is a separate test case. The socket
    runner's gate stays after its wait and before its reserve.

## Changes after approval: plan review round 18 (2026-11-14)

Round 18 of this batch's plan review, incremental, on commit c3e6ac5 (the diff
f28981a..f802039): plan-reviewer (design-code and design-security, one agent), findings R18-L1
to R18-L3. The reviewer found no new security issue. Every change below was made after
approval, and is not approved itself. No change narrows the intent.

94. **A "reached gate" signal; the socket-first wording** (R18-L1, R18-L3).
    - Each runner raises a `cfg(test)` "reached gate" signal when it parks at its gate. In the
      socket-first order the test waits for the startup runner's signal, so that its `done()`
      re-check has passed, before it opens the socket gate. Otherwise the socket client could
      reserve first, and the startup runner would stop at `done()` with no reserve, so the
      refusal would never be seen.
    - In that order the retry ends at the first `done()` after the `Err`, while the socket
      client's entry is still `starting` (held), not after the interval, nor on a device
      that runs.
    - If that held attach later failed, the startup `--attach` would have ended and nothing
      would retry it: the cost of #15's any-state rule, which #15 accepts. No test is needed
      beyond #15's.
95. **Item 90's superseded points marked** (R18-L2). Its gate position is marked superseded by
    item 93, its "ends at the `done()` after the interval" by items 92 and 94, and its
    `done()` point as withdrawn by item 91.
96. **The test waits for the winner's reserve** (R19-L1, round 19, after approval). Before it
    opens the loser's gate, the concurrent test waits, with a bounded timeout, until the
    registry shows the winner's device entry `starting`, so the winner has reserved; in both
    orders. Round 20 replaced "the fake metadata client has received the winner's held
    request": that client is only in the engine-side tests, not in this node test.
