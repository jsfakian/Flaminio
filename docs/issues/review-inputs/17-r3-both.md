# Plan review: issue #17, round 3 (incremental), design-code + design-security

Scope: `git diff 937f1e1..fdf6b6e -- docs/issues/17/ docs/adr/ ':!docs/issues/17/report.md'`
on issue/17-drain-unattached-volumes (HEAD a4c01a7 adds only report.md). Checked against the
code in that worktree and #15's plan (flaminio-15).

## Earlier findings
- R2-C1: fixed. The record now ends maintenance only at the node and epoch it last recorded,
  and `maintenance over a held record` covers `EndMaintenance` before the record takes it.
  - `end_any_hold`: a late end from the `Maintainer` reaching a record that a `Publish` or
    `Restart` made client trips both checks.
  - `restart_keeps_maintenance`: the record says held while the model keeps `maintenance`, so
    `hold lost` trips.
  - Neither check trips without its bug: in a correct run the record and the model agree on
    every row.
  - One sentence still puts `hold lost` before the record takes the request (R3-C4).
- R2-B1: fixed. `MaintainFailed`, `End::StartFailed`, the 5-minute back-off, `EndMaintenance` at
  the start's epoch, S17's "more than one", and tests.
  - That `EndMaintenance` is conditional on target, hold and epoch (row at design.md:68, :79),
    so it cannot end a newer start's record.
  - A drain on a healthy node still finishes in the common case: `Step` requests every move at
    once (drain.go:107-140), so one target run moves them all.
  - The fix opens R3-C1 and R3-C3.
- R2-C2: fixed. The router ends the entry on its `Detached`, and the start's wait ends on it.
  The slot is freed once only if that removal checks the generation (R3-C1). A new ordering race
  is R3-C2.
- R2-C3: fixed. The rollout order is restored, and `Unimplemented` becomes `Unavailable`. This
  opens R3-B1.
- R2-C4: fixed. `restart_keeps_maintenance` is in the design, the spec and the ADR (seven bugs).
- R2-N1, R2-N2, R2-N4: fixed.
- R2-N3: fixed in the spec and the ADR, but the design's Scenarios line now contradicts the
  Terms (R3-N1).

### R3-B1 — Medium
**A rollback of the metadata service blocks every Kubernetes detach and publish. And the
"retry" never reaches a new instance while an old one is up.**
- **Unpublish.** `ControllerUnpublishVolume` now answers an older instance's `Unimplemented` as
  `Unavailable` (design.md:120-122; ADR 0014:88-90; spec.md:144-146).
- **Publish.** The answer without `held` is already `Unavailable` (design.md:116-119).
- **Rollback.** Roll the metadata service back alone, while the CSI controller stays new, and
  every unpublish and every publish fails.
  - The external-attacher retries with back-off, indefinitely.
  - The VolumeAttachment keeps its finalizer, so an RWO volume cannot attach elsewhere
    (Multi-Attach).
  - This holds for pods leaving a lost node too, after the attach/detach controller's forced
    detach.
  - Rollout (design.md:257-270) and the ADR give the upgrade order only, not the rollback order.
- **A mixed window.** The claim "Kubernetes retries until a new instance answers"
  (design.md:262-267) is contradicted by `metaCall`, `control/csi/meta.go:63-84`.
  - It moves to the next instance only on `Unavailable`, `DeadlineExceeded` or `Canceled`.
  - Any other answer, `Unimplemented` or an OK publish without `held`, stores that instance as
    `preferred` (`:82`).
  - Every retry then starts at the same old instance, and fails again until that instance goes
    down.

Fix:
- **Fail over within one call.** Make the unhold's call, and the publish's check of `held`, move
  to the next instance on `Unimplemented` or on an answer without `held`, and leave `preferred`
  unchanged.
- **State the rollback order** in Rollout, the ADR and `docs/lab.md`: roll back the CSI controller
  and `lab.sh` before the metadata service. Say that a service older than the controller fails
  every publish and unpublish.
- **Test** (`csi_test.go`): two instances, the first old. A publish and an unpublish both
  succeed, through the second.

### R3-C1 — Low
**A failed start after its reservation is ended and reported twice.**
- **The entry.** For `AttachFailed`, a failed mark or a time-out, the main thread sends
  `Input::Detach` "as #15's" (design.md:157-158), and #15's step also ends the entry (#15
  design.md:87-89).
  - #15's `Engine::detach` gives `Detached` even with no target (#15 design.md:253). The router
    then ends the entry again (design.md:174-178).
  - This is safe only if the router removes the entry of that generation, under the registry's
    lock. The design does not say so.
  - Removed by volume instead, the second removal would end a client's entry, which the first
    removal let in. That frees its `--max-volumes` slot while its device runs.
- **The Maintainer.** The engine cannot tell the main thread's `Input::Detach` from root's or a
  socket's (design.md:194-200).
  - So it runs S11's end: it logs "ended by root", calls `on_ended(K, E, Detached)` and sends
    `EndMaintenance`.
  - The main thread then sends `MaintainFailed`, which gives `on_ended(K, E, StartFailed)` and
    a second `EndMaintenance` (design.md:160-166, :352-355).
  - The same happens on a client attach over a start in flight.
- **Effect.** "Frees the start's place" twice, unless `on_ended` is idempotent per `(K, epoch)`.
  The `Maintainer` then over-starts. The registry refuses each extra start, and each refusal
  costs a volume an epoch and a 5-minute back-off.

Fix:
- Say that the router ends only the entry whose generation matches.
- Name one ender for a failed start, the main thread or the router.
- Have the engine end a maintenance attach that never reached `Attached` without S11's report.
  `MaintainFailed` is then the one report.
- Make `on_ended` ignore a `(K, epoch)` the `Maintainer` no longer holds.
- Test: after a failed mark, exactly one place is freed, one `EndMaintenance` is sent, and no
  "ended by root" line is logged.

### R3-C2 — Low
**A socket's detach that reaches the engine before the start's `Input::Attach` leaves an
`Attaching` that nobody ends. The client's attach then fails until a restart.**
- **The order.** The main thread reserves the entry, then sends `Input::Attach`
  (design.md:145-150). A client attach over a start in flight sends `Input::Detach` with that
  generation, "in any state" (design.md:194-199).
- **When the detach wins:**
  1. `Detach` with no target gives `Detached` (#15 design.md:253);
  2. the router ends the entry and wakes the main thread (design.md:174-178). The main thread
     treats this as "a `Detached` before `Attached`" and sends only `MaintainFailed`
     (design.md:158-162);
  3. the engine then takes the late `Input::Attach` and creates an `Attaching`. Its `MarkEpoch`
     is dropped (unknown generation);
  4. the client's own attach next gets "already attaching" (`engine.rs:649-654`), and `busy(K)`
     stays true.
- **#15 has no such race:** its `detach` of a starting entry answers "is attaching" (#15
  design.md:365-366).

Fix:
- On every failed start, a `Detached` before `Attached` included, the main thread sends
  `Input::Detach` with the generation after its `Input::Attach`. Alternatively, it reserves and
  sends `Input::Attach` under one hold of the registry's lock.
- Test: the socket's detach is ordered before the start's `Input::Attach`. The client attach
  then succeeds, and the engine holds no `Attaching`.

### R3-C3 — Low
**S17 counts the drain's own `AttachMaintenance`, and a retried start, as epochs that the node
took. So a healthy, copying target can be deposed.**
- **What the drain sees.** It sees only the record's epoch, and every attach bumps it
  (`domain.go:217-218`; table design.md:66).
- **The drain's own ask counts.** The sighting goes on across its own re-ask of the same node
  (design.md:210-213, :227-229). That ask's epoch, plus the node's one start, makes "more than
  one" (design.md:240-243; spec.md:124-131).
- **Healthy cases that trip it:**
  - The target ended at no work while a chunk waited on #13's releases. The drain re-asks the
    same node once that chunk can move.
  - A `StartMaintenance` whose answer was lost: "the next scan decides again" (design.md:170)
    and takes a second epoch.
- **What follows.** If `Stall` has passed since the last progress, the drain asks another node
  while the first node's one real start is copying. The copy starts over there.
  - This contradicts "a node that is alive and took one epoch is not asked again" (spec.md:129-131).
  - The parenthetical at design.md:228-229 names only the failed start. No test covers a healthy
    re-ask.
- Liveness holds: the new node's sighting is new.

Fix:
- **Count no epochs.** Detect a failed start as "the sighting's node ended the record while this
  run had a move pending". `EndMaintenance` after a failed start is exactly that signal.
- Or: restart the epoch baseline at the drain's own ask, and accept a lost answer's extra epoch
  as a stated cost.
- Test: a node re-asked after a no-work end, copying past `Stall`, is not asked elsewhere.

### R3-C4 — Low
**`hold lost` is said to run before the record takes the request.**
- **The text.** design.md:425-428 says "Each check runs before the record takes the request:
  `maintenance over a held record` …; `hold lost` …". spec.md:168-171 says the same ("each check
  made before the record takes it"). ADR 0014:104-105 applies it to the first check only.
- **Read literally,** `hold lost` compares the model after a transition with the record before
  it.
  - Every correct publish, attach or restart of a record that is not held trips it, and so does
    every correct unhold or end.
  - `catches_restart_keeping_maintenance` could not fail: both sides read "maintenance".

Fix: `maintenance over a held record` runs before the record takes the request; `hold lost` runs
after it. Change the design and the spec to match the ADR.

## Nits
- **R3-N1.** design.md:449-451: the Scenarios line "the maintenance node's own restart of the
  volume, taking a client epoch" contradicts the Terms as R2-N3 fixed them. Those Terms say this
  restart is a start (spec.md:13-14). Say "the agent's `only_if_target` restart on the
  maintenance node".
- **R3-N2.** design.md:348-355: `on_ended(K, Epoch, End)` takes no `now`, yet `StartFailed`
  starts a 5-minute back-off in a sans-I/O machine. Add `now` to `on_ended`, or say that the
  back-off is timed from the next `on_list`.

Reviewed commit: a4c01a7e291a8a12d272efacf43e9517a93058cc (the diff 937f1e1..fdf6b6e; a4c01a7 adds only report.md)
