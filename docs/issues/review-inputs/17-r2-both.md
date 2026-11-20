# Plan review: issue #17, round 2 (incremental), design-code + design-security

Scope: `git diff b904a18..c25679e -- docs/issues/17/ docs/adr/ ':!docs/issues/17/report.md'`
on issue/17-drain-unattached-volumes. Checked against the code in that worktree, #15's plan
(flaminio-15, review-clear), #13's plan (flaminio-13, review-clear) and #78's plan.

## Earlier findings
- R1-C1: fixed. `Output::Maintain`, a maintenance registry entry and the run mark before `Input::Marked`. The new path opens R2-B1 and R2-C2.
- R1-C2: fixed for `maintenance_ignores_held` and `unhold_any_node` (random chaos, late delivery). The new `end_any_hold` has the same flaw (R2-C1).
- R1-C3: fixed. Any record stands while its node is eligible, and the per-run ranking is gone.
- R1-C4: fixed. Fields 16 and 17, in the proto and in `pb.rs`.
- R1-C5: fixed. #78's `CheckDrain`, `PendingCreations` and `Incoming` are kept, and the package `Step` stays.
- R1-C6: fixed. Unhold and `in use`; #13's `Held` and `Released` kept; `no move pending`; forgotten nodes refused.
- R1-C7: fixed. A node that took an epoch is not asked again. This opens R2-B1: an epoch taken is not a target running.
- R1-C8: fixed. S5 is kept and S4 amended; the transition count is corrected. The CSI publish checks `held`, and S19 holds. The fix opens R2-C4 (Low).
- R1-C9: fixed. `EndMaintenance`.
- R1-C10: fixed. `Since`, and the "no progress for D" clause.
- R1-C11: fixed.
- R1-C12: fixed. ADR 0014, with its README row; no stale reference remains.
- R1-S1: fixed. `start_ignores_hold` and `publish_keeps_unheld`; Publish and Restart in the chaos; the check is by transition.
- R1-S2: fixed (as R1-C1).
- R1-S3: fixed (as R1-C3).
- R1-S4: fixed (as R1-C9).
- R1-S5: fixed (as R1-C8).
- R1-S6: fixed (as R1-C4).
- R1-S7: fixed. `Held()` fails closed.
- R1-S8: fixed. Only a verified token above the entry's epoch ends the entry.
- R1-S9: fixed. `ValidID` drop, and a cap of 1024.
- R1-S10: fixed. #13's mark is compared in AttachMaintenance and StartMaintenance.
- R1-S11: fixed.
- Nits (both reports): fixed.

### R2-C1 — High
**`catches_end_over_held` cannot fail: the simulator's hold record applies the very end that the bug applies.**
- **What the record does.** The shadow record is "kept from the requests the model applied". In
  it, "an unhold by the holder, a maintenance attach and an end of maintenance leave it not
  held" (design.md:391-395). Only the unhold is conditional (on the holder), and that is why
  `unhold_any_node` trips `hold lost`.
- **The two checks.** `maintenance over a held record` looks only at `AttachMaintenance` and
  `StartMaintenance`. `hold lost` compares `held()` with the record after the record has taken
  the request.
- **With `end_any_hold`** ("EndMaintenance applied for its node whatever the record's hold and
  epoch", design.md:407-408), a late end from the Maintainer reaches a record that the node's
  `Restart` made client.
  - The model applies it, and the hold becomes unheld.
  - The record applies "an end of maintenance" too, and becomes not held.
  - The two agree, so `hold lost` stays silent. Neither check covers an end.
- **Nothing later catches it.** A later random `AttachMaintenance` applies on a record that both
  call not held. It deposes the live client target by fencing, so linearizability and replica
  agreement still pass.
- **Consequence.** ADR 0014's guard on the new transition has no working simulator check, and
  the mutation test would fail in `make sim`. This is the R1-C2 class of defect, on the new bug.

Evidence: design.md:391-397, :407-408; spec.md:159-165; ADR 0014:97-104; AGENTS.md Principles.

Fix:
- Add `EndMaintenance` to `maintenance over a held record`: an `AttachMaintenance`,
  `StartMaintenance` or `EndMaintenance` that applies while the record says held.
- Say that each check runs before the record takes the request. Alternatively, make the
  record's end rule conditional ("an end at the node and epoch it last recorded as
  maintenance").
- Update the spec's Tests and the ADR's Confirmation to match.

### R2-B1 — Medium
**A start that takes an epoch and then fails is never reported back, and the drain counts that epoch as progress. On an alive node, the drain waits forever and the node takes an epoch at every scan.**
- **The start path.** `StartMaintenance` commits a new epoch before the main thread checks the
  token and the registry. "A refusal drops the token, logged" (design.md:144-147). A refusal
  happens when the volume has any entry, at the limit, or at the run mark. An `AttachFailed`,
  a failed mark or a time-out ends the entry. Then "the next scan decides again" (design.md:158).
- **No failure reaches the `Maintainer`.** Its inputs (`on_list`, `on_view`, `on_started`,
  `on_ended`, `busy`) and `End::{NoWork, Deposed, Gone, Detached}` have no failed start, and a
  registry refusal sends the engine nothing (design.md:320-325). So either:
  - the `Maintainer` counts the start forever, and two refusals use up the default limit of 2 for
    good; or
  - it calls `StartMaintenance` at every 5 s scan, a new epoch each time.
- **The drain then never asks elsewhere.** S17 does not ask again a node "that took an epoch
  since this run first saw the record ... while it stays alive and eligible" (design.md:221-223;
  spec.md:124-126). "Alive" is the data plane engine's own heartbeat
  (`engine.rs` `next_heartbeat`; `service.go:220-229`), and that engine keeps running.
- **Persistent triggers, from #15:**
  - a deposed device entry, which stays "stopped" with its fd until a detach (#15
    design.md:228-229). This is exactly ADR 0014's accepted risk: maintenance on the client's
    own node after a wrong unhold deposes its device;
  - a read-only or full state directory: each mark write fails the start (#15 design.md:553-554);
  - a malformed mark, which "refuses its volume's attaches" (#15 design.md:569).
- **What it costs.** The drain never completes, and the volume's header is rewritten every 5 s.
  A truly dead node does not cause this: its heartbeat stops and S17 asks again at once. A
  compromised or broken `io` can do the same on purpose.
- The Risks call the scan's pass-over a mere saving of a `StartMaintenance` (design.md:510-512).
  It is not.

Evidence: design.md:139-158, :215-223, :320-325, :510-512; spec.md:120-127; #15 design.md:228-229,
:553-554, :569.

Fix:
- **Feed every failed start back to the `Maintainer`** (the main thread's refusal, `AttachFailed`,
  a failed mark, a time-out) through an `Input`, for example `End::StartFailed`. Give it a
  back-off before that volume's next `StartMaintenance`.
- **End the record on a failed start:** `EndMaintenance` at the epoch it took, so that the drain
  asks again.
- **In S17, count a risen epoch as progress once per sighting.** For example: ask another
  candidate when, for `Stall`, the replicas did not go down and the epoch rose more than once.
- **Tests:**
  - engine: a Maintain refused over a stopped device entry takes no second epoch before the
    back-off;
  - engine: a failed mark ends the record's maintenance;
  - drain: a node whose epoch rises at every step without progress is asked elsewhere after
    `Stall`.

### R2-C2 — Medium
**Nothing ends a maintenance entry on `Detached`. A leaked entry blocks the volume's client attach on that node.**
- **After `Attached`, nobody waits on the entry.** The design says "on `Attached`, it spawns
  nothing: the entry runs until its `Detached`", and the `Detached` "go[es] to it as a device
  entry's do" (design.md:154-156, :162-164, :169-170).
  - In #15 the entry's waiter is read only by the main thread during an attach (#15
    design.md:39-40, :79-89). The main thread then moves on.
  - A device entry is ended by its device thread after `run_target` (#15 design.md:226-233).
    A maintenance entry has no such thread.
  - So no thread is named to remove the entry from the registry, or to free its
    `--max-maintenance` slot.
- **A leaked entry breaks S12 and S5.** "Refused if the volume has any entry" (design.md:145).
  S12's client attach "waits up to 30 s for the entry to end, and reserves its own"
  (design.md:181-183), so it fails every time. A client attach on the maintenance node is then
  refused until the data plane restarts.
- **During a start, the main thread does not stop on `Detached`.** #15's main thread stops only
  on `MarkEpoch`, `Attached`, `AttachFailed` or its time-out. A client that detaches a start in
  flight therefore waits up to `METADATA_LIMIT`, 30 s (`ublk.rs:697`), against its own 30 s.

Evidence: design.md:145, :154-156, :162-164, :169-170, :178-183; #15 design.md:39-40, :79-89,
:226-233; `dataplane/crates/flaminio-io/src/ublk.rs:697`.

Fix:
- Say who ends the entry. For example: the router, when it routes a maintenance entry's
  `Detached` after the engine's lock is let go, ends the entry under the registry's lock.
- The main thread's start wait ends at once on its generation's `Detached`.
- Test that the entry and its slot are gone after each kind of end, and that a client attach
  over a start in flight reserves without waiting out the start.

### R2-C3 — Low
**With step 2 no longer waiting for step 1, unholds that reach an older instance are lost without a trace.**
- **The rollout change.** The design now lets the callers of `UnholdVolume` roll out while some
  metadata instances are old (design.md:235-241; decisions.md:111-122).
- **How an unhold is lost.** `ControllerUnpublishVolume` counts `Unimplemented` as "success,
  logged" (design.md:120-121). So an unpublish that reaches an old instance leaves the volume
  held for good.
- **What follows.** Every drain then reports the volume `in use on NODE`, for a client that is
  gone, until an unhold by hand. Nothing lists such volumes.
- **Before the change,** `Unimplemented` meant a misordered rollout. Now it is routine during
  the rolling window.

Evidence: design.md:120-121, :235-241; decisions.md:111-122.

Fix: either keep unholds after every instance, or answer `Unimplemented` as `Unavailable` so that
Kubernetes retries. Otherwise, state the residue in Rollout and in `docs/lab.md`, with how to find
such volumes (held, and no device on the named node) and unhold them.

### R2-C4 — Low
**The changed `Restart` row has no injected bug.**
- **The change.** Round 1 reversed the row: `Restart` on a maintenance record naming its node
  now makes the volume held (design.md:64, :80-84).
- **Its regression is the approved design itself.** A `Restart` that keeps hold = maintenance
  leaves a client target on a record that is not held. The next maintenance attach then deposes
  it.
- **The `Publish` path has its injected bug (`publish_keeps_unheld`); the `Restart` path has
  none.** `hold lost` would catch the regression, but no mutation proves it, against AGENTS.md
  ("a simulator check and an injected bug" per new safety rule).

Evidence: design.md:64, :80-84, :400-412; spec.md:162-165; ADR 0014:101-104.

Fix: add `meta::Bugs::restart_keeps_maintenance`, with its first seed recorded, as
`catches_restart_keeping_maintenance`. List it in the spec's Tests and in the ADR's Confirmation
(seven bugs).

## Nits
- spec.md:5-6: the list of rules changed after approval leaves out two:
  - S2, whose condition went from "the kind is client" to "the volume is held". This change has
    an effect: an unhold now applies to a record from before this change and to an unknown
    value;
  - S20, which gains "its end".

  decisions.md does not record the S2 change either.
- spec.md:140-142 (S19): "its publish of a record that is not held changes nothing" holds only
  for a record that names the publishing node. Over another node, an older instance writes the
  header, which is then held by S6. The design says "naming the node" (design.md:118, :238).
- spec.md:11-12: the Terms call "a node's own restart of its target" a client attach. Yet S4
  (spec.md:48-50) makes the node's restart of a maintenance target (S13) a maintenance start.
  Name the agent's `only_if_target` restart in the Terms.
- design.md:193 and :215-216: "timed from when this run first saw its record at its epoch" reads
  as a clock and an epoch that restart at each new epoch. That would ask a node again mid-copy,
  against :221-223. Say that both belong to this run's first sight of a maintenance record
  naming that node.

Reviewed commit: 937f1e199a7a376ca80cee818e742ec0f91f2fb5 (the diff b904a18..c25679e; 937f1e1 adds only report.md)
