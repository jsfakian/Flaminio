# Issue #17: Draining volumes that are not attached — specification

[Intent](intent.md), [decisions](decisions.md), [design](design.md). Assumes #15 as planned: the
engine thread (its S3), the attach's token check, epoch rules and run mark (its S11).
Changed after approval, in rounds 1 to 9 of the plan review (2026-11-14; see decisions.md):
the Terms, S2–S5, S7, S9, S11–S13, S15, S17–S20 and the Tests. #17's "release" is now "unhold"
throughout, since #13 keeps "release" for copies to delete; the approval lines below keep the
old word.

## Terms

- **Client attach:** an attach for a consumer of the volume: the CSI controller's publish, the
  node agent's restart of its node's target (`only_if_target`), `flaminio-meta attach`. The
  maintenance node's own restart of a maintenance target is not one: it is a start (S4).
- **Held:** the volume's record says a client attach stands: from a client attach until its
  unhold.
- **Unhold:** the record that a client's attach on a node has ended. New.
- **Maintenance attach:** an attach the drain asks for, of a volume that is not held, to a
  **maintenance node**.
- **Maintenance target:** the target a maintenance node starts: no device, no client I/O.
- **Kind:** client or maintenance. Each epoch has one kind.
- **Work:** a move pending on a chunk of the volume.

## The invariant

A maintenance attach never takes a volume from a client, and never keeps one from a client: it
commits only while the volume is not held, and a client attach always commits. A maintenance
target is a target: one per epoch, fenced by the next (replication-protocol.md §3, §6). It adds
no way to read or write a volume but a target's.

## Behaviour

### The metadata service

**S1. Held.** A client attach makes the volume held, in the same write as its epoch. A client
attach of a volume that is not held takes a new epoch, even to the node the record names. A
repeated publish of a held volume to its target changes nothing, as today.

**S2. Unhold.** It names the volume and a node, and applies only if that node is the record's
target and the volume is held: a late unhold for another node changes nothing. It changes no
epoch, target or chunk, and neither fences nor stops a target. A target still running on a lost
node is deposed by the next attach, as today.

**S3. A maintenance attach** names a volume and a node. In one conditional write: a new epoch,
the node as target, the kind maintenance. It is refused, with nothing changed and the reason
given, when the volume is held, or is being created or deleted, or (with #13) the node is
forgotten. It gives the asker no token.

**S4. Starting.** The maintenance node takes a new epoch and its token for itself, only while
the record names it as the maintenance target and (with #13) it is not forgotten. Its restart
of a maintenance target (S13) is such a start, and keeps the kind maintenance. Only a client
attach (S5), the agent's restart of its node's target included, makes the kind client; only S3
makes it maintenance. (Amended after approval: it said a node's restart of its target keeps
the kind, which refused a client attach that S5 commits.)

**S5. A client attach wins.** It is applied whatever the kind: the metadata service never
refuses it or makes it wait for maintenance. The maintenance target is then an older target:
deposed by the existing rules, its half-done move resumed by the client's target (protocol §7).
On the maintenance node, the client's device waits for that target to end, at most 30 s (S12).
(Amended after approval, round 8: it said the attach is never made to wait, which cannot hold
on the node, where the maintenance target must end first.)

**S6. Records without the mark.** A volume whose record names a target and predates this
change counts as held, until a client attach or an unhold. A volume never attached is not held.

**S7. Callers.** A maintenance attach and an unhold: an administrator. S4's start and S11's end
of the record's maintenance: the named node's own identity, for itself only. No node makes
itself a maintenance node. `ChangeIsr` stays the current target's. Each maintenance attach,
unhold and end of maintenance is logged with its caller, volume, node and epoch.

**S8. Unchanged:** `ChangeIsr`, `MoveReplica`, and `DeleteVolume`, which fences a volume under
maintenance as any other (docs/issues/8): its target finds the volume gone and ends.

### The maintenance node

**S9. Starting a target.** The node starts a maintenance target only when all of these hold:
the record names it as the maintenance target, at the token's epoch; the volume has work; the
node runs no target of the volume; it is below its limit (S14). The target has no device, and
takes none of #15's device slots or ids: S14's limit alone counts it. Its epoch's run mark (#15,
S11) is durable before its first `Hello`, as any target's. A maintenance epoch never starts a
device, and a client epoch never starts a target without one (issue #6, S3, stays for client
attaches).

**S10. What it does.** What any target does: it fences, reconciles, resyncs replicas outside
the ISR, and carries out the moves, with a target's token, sessions and leases. Nothing can
submit client I/O to it. Other volumes on the node are affected as by any other volume (#15,
S5).

**S11. Ending.** It ends, as a detach does, when the volume has no work left in a read of its
record at its epoch; when it is deposed; when the volume is gone; when root or a client attach
ends it (S12, S14). It then leaves nothing running. Unless it was deposed or the volume is gone,
the node then ends the record's maintenance, at that epoch only, so that the record no longer
names it: no later move starts a target there without a new maintenance attach. It does the
same for a record that names it with no work, and starts nothing for it; and for a start that
took its epoch and failed, after which it starts that volume again no sooner than 5 minutes
later.

**S12. A client attach on the maintenance node** ends the maintenance target, waits for it to
end, at most 30 s, and then goes on within its usual limits. Past 30 s it is refused with
nothing reserved, `the maintenance target did not end within 30 s`, which the CSI node plugin
gives as unavailable, so kubelet retries. A token at or below the maintenance target's epoch is
refused for its epoch, as #15's S11 refuses one, and the target keeps running. (Amended after
approval, round 8: it said the attach is not refused for the maintenance target, which cannot
hold past #15's 30-s bound for a detach.)

**S13. Crash and restart.** A maintenance target ends with its process. After a restart the
node starts one again, under a new epoch, if S9 still holds; never under an epoch it had (its
run mark, S9).

**S14. Limit and visibility.** A node runs at most 2 maintenance targets at a time (to be
approved); the rest wait. The node's status lists them, and root can end one.
Settled at approval, 2026-11-13: at most 2 (see decisions.md, Settled at approval).

### The drain

**S15. Which volumes.** For each volume with a replica on the draining node, after requesting
its moves as today (each destination chosen in one place, which #18 changes; the source's copy
stays until #13):
- **held:** left to its client's target, as today;
- **not held, with work:** the drain asks for a maintenance attach, unless one stands (S17),
  whoever asked for it. A refusal because the volume became held is not an error;
- **not held, without work** (its chunks wait on #13's releases): nothing is asked, and the
  report gives #13's reason.

**S16. Which node.** Alive, not draining, not a witness, and either holding a replica of the
volume or the destination of one of its moves: no node reads a volume it neither stores nor is
about to. With none, the volume is reported with the reason, the drain goes on with the
others, and does not complete.

**S17. A maintenance target that dies or stalls.** A maintenance attach stands while its record
names a node that S16 allows, whoever asked for it. The drain asks again, for another node S16
allows if there is one: when that node is no longer alive or no longer allowed; when it ended
the record's maintenance while a move of the volume was pending (a failed start, S11); or when
5 minutes (to be approved) have passed both since the drain first saw a maintenance record
naming that node, or last asked that node itself (a time the node's own new epoch does not
reset), and since the volume's replicas on the draining node last went down, and the node has
taken no epoch since that time. The drain counts no epochs beyond that: its own
asks and a start retried after a lost answer raise them too. A node that is alive and started
is not asked again, however long its copy takes: asking again would restart that copy. Asking
again is an attach: safe at any moment. The limit serves progress only, never safety. A drain
whose node is no longer draining asks nothing.
Settled at approval, 2026-11-13: 5 minutes (see decisions.md, Settled at approval).

**S18. Progress and completion.** Each report names, per volume left: in use (held) and moved
by its target; under maintenance, and where; or waiting, and why (no node, no move pending);
and, once its replicas on the node have not gone down for 5 minutes, that it makes no progress.
It keeps #13's and #78's counts and reasons. Once #78 lands, the drain completes as #78 says
(its validated check); until then, when no chunk replica is left on the node, as today.

### Rollout and docs

**S19. Rollout.** The metadata service first, then the data planes and the callers of unhold,
then the drain, each step after the whole of the one before. Mixed versions stay safe: an old
metadata instance refuses a maintenance attach and an unhold, which the CSI controller retries
rather than lose; a header it rewrites loses the mark, which S6 then counts as held; its
publish of a record that is not held and names the publishing node changes nothing and answers
without the mark, which the CSI controller refuses as unavailable, so the publish is retried
until a new instance answers (over another node it rewrites the header: held, by S6); never
the reverse. Within one call, an old instance's answer to an unhold or a publish moves on to
the next instance. An old data plane named as maintenance node starts nothing, and S17 moves
on. Rolling back goes the other way: the CSI controller and `lab.sh` before the metadata
service, since a metadata service older than the CSI controller fails every publish and
unpublish.

**S20. Docs.** Protocol §2 (unhold, the maintenance attach and its end) and §7 (its last line);
`meta.proto`'s caller rules; `docs/lab.md` (drains, `vol detach` unholds); follow-ups §11.

## Tests

- **S1–S8, S19** (service, in memory and on etcd; `auth_test.go`; the model's vectors, `make
  contracts`; the CSI controller): each refusal; an unhold for another node; a client attach
  over maintenance, the agent's restart included; a maintenance attach racing a publish, in
  both orders; the end of maintenance at its epoch and at another; a header without the mark,
  old or rewritten by an old instance; a publish and an unhold answered by an old instance,
  retried, and with two instances, the first old, done through the second in one call; a
  forgotten node (with #13); every caller rule; the log.
- **Simulator** (the model's transitions in `flaminio-repl`): chaos adds unholds naming any
  node and maintenance attaches, both whatever the record says, and targets without clients,
  among failovers (by publish and restart as well as attach) and moves. Linearizability and
  replica agreement hold as before. New checks, against the simulator's own record of holds,
  which takes each request as the model should admit it: no maintenance attach, start or end
  commits while the volume is held, checked before the record takes the request, so no
  client's target is deposed by maintenance; the model's held agrees with that record, checked
  after the record takes the request. Injected bugs, each caught in some run: a maintenance
  attach that ignores held; a start that ignores the hold; an unhold applied for a node that is
  not the holder; a publish that leaves a record not held; an end of maintenance applied
  whatever the record; a restart that leaves a maintenance record not held; a maintenance start
  that skips the run mark.
- **S9–S14** (engine and node, fake metadata client): each condition of S9; no device and no
  slot taken; the run mark durable before the first `Hello`; client I/O impossible; the end at
  no work, on deposition and on a gone volume, and the record's maintenance ended; a failed
  start ends it too, once, reported once, and takes no second epoch for 5 minutes; the entry
  and its place freed at every end; every order of the main thread, the engine, root's end and
  a client attach that the design's lifecycle table lists, walked row by row by a table-driven
  test of the node, and each way a start or target ends checked in the real engine, together
  keeping one end, one place freed, no client attach refused or made to wait except by the 30-s
  bound (S12) and no client's target ended; root's end and a client attach answered with an
  error, and nothing reserved, when the maintenance target has not ended within 30 s, and
  answered as usual when it ends as the 30 s expire; no restart without work; a stale token that
  leaves the target running; a restart takes a new epoch; the limit; a listed volume id that is
  not valid.
- **S15–S18** (`control/drain`, in-memory service): held and not held volumes; a volume whose
  chunks all wait on releases; the node rule, and none eligible; a volume held meanwhile; a dead
  node, one that took no epoch, and one whose start failed, asked again; one that started, one
  re-asked after an end at no work, one whose start was retried after a lost answer, and one
  asked whose record still stands at the next step, not asked again while they copy or start;
  two drains sharing a volume, and a drain started over a standing
  maintenance attach; the reports, no progress included; completion.
- **Lab, a later step** (not run here): fl-b drained into fl-c while it holds a volume written
  with verified data then detached, and one never attached; each then attaches and verifies
  from the new nodes. A pod started on a volume under maintenance mounts and verifies. The
  maintenance node's data plane killed mid-move and started again: the drain completes. `make
  drain-test` passes.

## Open questions

1. **#15 first.** Its plan lets a target run without a device (its S3), but gives no way to
   start, limit or end one: S9–S14 here. It is not built. *Recommended:* build this after #15,
   not on today's frontend, where a device's queue thread drives the engine.
   Settled at approval, 2026-11-13: built after #15 (see decisions.md, Settled at approval).
2. **Recording held needs an unhold.** It changes `ControllerUnpublishVolume` and the lab's
   detach. The alternative, reading Kubernetes VolumeAttachments, needs the operator and cannot
   be made atomic with an attach. *Recommended:* the record (S1, S2).
   Settled at approval, 2026-11-13: the record, with a release (see decisions.md, Settled at approval).
3. **Who starts the target on the node (S4, S9).** (a) The data plane itself, reading the
   metadata service as its own identity, as #13's sweep will: it works on fl-c. (b) A
   control-socket command, as #15's intent expected, driven by the CSI node plugin: nothing
   drives it on fl-c (#14). *Recommended:* (a).
   Settled at approval, 2026-11-13: (a), the data plane itself (see decisions.md, Settled at approval).
4. **Existing volumes (S6).** Held: safe, but a volume detached before this change needs one
   unhold by hand before a drain moves it. Not held: drained at once, and a client's live
   target may be deposed once. *Recommended:* held.
   Settled at approval, 2026-11-13: held (see decisions.md, Settled at approval).
5. **A forced detach of a lost node** unholds a volume whose target may still run; a
   maintenance attach then deposes it, as the pod's next attach would. *Recommended:* accept.
   Settled at approval, 2026-11-13: accepted (see decisions.md, Settled at approval).
6. **The node rule (S16)** leaves an RF 1 volume one candidate: its move's destination.
   *Recommended:* keep it; widen only on need.
   Settled at approval, 2026-11-13: kept; widened only on need (see decisions.md, Settled at approval).
7. **The token.** S9 needs the kind of an epoch. *Recommended:* from the record at the token's
   epoch, not a new claim: no token format change.
   Settled at approval, 2026-11-13: from the record at the token's epoch; no token format change (see decisions.md, Settled at approval).
8. **The operator.** Nothing here needs it; `control/drain` is enough. *Recommended:* drop it
   from the issue's dependencies, keeping #15.
   Settled at approval, 2026-11-13: dropped from the dependencies, keeping #15 (see decisions.md, Settled at approval).
9. **The numbers:** 2 maintenance targets per node (S14); 5 minutes without progress (S17).
   Settled at approval, 2026-11-13: 2 maintenance targets per node; 5 minutes without progress (see decisions.md, Settled at approval).
10. **ADRs:** none is contradicted (a maintenance target holds leases as ADR 0005 says).
    Protocol §2's "only two kinds of transition" gains four (unhold, and the maintenance
    attach, start and end), and the attach's three modes set the mark (count corrected after
    approval): say if that wants an ADR.
    Settled at approval, 2026-11-13: yes, ADR 0014 (numbered 0007 then), which stays Proposed in docs/adr/ until accepted in its own step (see decisions.md, Settled at approval).
