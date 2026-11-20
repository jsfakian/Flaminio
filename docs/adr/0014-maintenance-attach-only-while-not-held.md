# 0014. A target that no client asked for is admitted only while no client holds the volume

- **Status:** Proposed
- **Date:** 2026-11-12. Renumbered from 0007 and amended on 2026-11-14, in rounds 1 to 3 and 8
  of the plan review of #17 ([decisions](../issues/17/decisions.md)).

## Context

A chunk's replica moves only through its volume's target (replication-protocol.md §7, moves), and
a volume gets a target only where a consumer attaches it. So a drain of a node that holds
replicas of unattached volumes never completes
([issue #17](https://github.com/jsfakian/Flaminio/issues/17),
[intent](../issues/17/intent.md), follow-ups §11).

The fix needs a target that no client asked for: a **maintenance target**. Two things stand in
its way today:

- **The record cannot tell a live client from a past one.** Every attach sets the target and it
  is never cleared (`Header.Attach`, `meta/domain/domain.go`). An attach for maintenance, as
  attaches are today, would depose a pod's target whenever the volume was in use after all.
- **Protocol §2 has two transitions,** `Attach` and `ChangeIsr`, and both sides implement that
  table (`meta/domain`, `flaminio-repl/src/meta.rs`). Which node becomes a target follows from
  where a consumer runs; the threat model counts on it ("volumes it does not host targets for",
  ARCHITECTURE §15.1).

[Spec](../issues/17/spec.md), [design](../issues/17/design.md).

## Decision

- **The volume's record says whether a client holds it:** from a client attach until an unhold.
  A record that names a target and has no mark, or a mark this version does not know, counts as
  held.
- **A maintenance attach commits only while the volume is not held,** in one conditional write
  of the header. A client attach always commits, whatever the record says: the metadata
  service never refuses it and never makes it wait for maintenance. On the maintenance node,
  the client's device waits for the maintenance target to end, at most 30 s (#15's bound for a
  detach); past that it is refused with nothing reserved, and kubelet retries. The node agent's
  restart of its node's target
  (`only_if_target`) on a maintenance record that names that node is a client attach: it takes a
  new epoch, and deposes the maintenance target.
- **A maintenance target is a target:** one per epoch, fenced by the next, holding leases as ADR
  0005 says, its epoch's run mark durable on its node before its first `Hello` (#15).
  Maintenance adds no way to read or write a volume but a target's, and no rule that rests on a
  timeout.
- **A maintenance record ends with its work.** When its target ends, when a start fails after
  taking its epoch, or when it finds no work, the maintenance node ends the record's
  maintenance, at that epoch only: the record then names it
  no longer as maintenance node, and no later move starts a target there without a new ask.
- **Rights are split three ways.** Only an administrator asks for a maintenance attach or
  records an unhold. Only the named node's data plane takes the epoch and its token, and ends
  its own maintenance, for itself. No node makes itself a maintenance node, and (with #13) no
  forgotten node is one.
- **Protocol §2 gains four transitions,** `Unhold`, `AttachMaintenance`, `StartMaintenance` and
  `EndMaintenance`, and the attach's three modes set the mark: `Attach`; `Publish`
  (`unless_target`), which now takes a new epoch for a record that is not held, even one naming
  its node; `Restart` (`only_if_target`). The table stays one table, in the Rust model and the
  Go service, checked by shared vectors.

## Options

1. **The record, with an unhold (chosen).**
   - For: the check and the attach are one transaction; it works without Kubernetes (fl-c).
   - Against: an unhold is a new call that every detach path must make; a missed one leaves a
     volume held, and a drain then waits for it, as today.
2. **Read Kubernetes VolumeAttachments at drain time.**
   - For: no new state.
   - Against: not atomic with the attach, so a pod can be deposed; needs the API server; blind
     to consumers outside Kubernetes.
3. **Move replicas without a target** (a copy driven by the metadata service or the drain).
   - For: no new kind of target.
   - Against: a second writer of chunk data outside epochs and fencing; it would contradict the
     rule that only the target changes a chunk's replicas (§7).
4. **Let maintenance attach as any attach, and accept a failover.**
   - Against: it ends a pod's I/O, and locks the pod out of its node (`only_if_target`).

## Consequences

- **A maintenance attach never takes a volume from a client, and never keeps one from a
  client.** The cost to a client is one takeover at most, as after any failover.
- **Risk accepted: the mark can be wrong in the unsafe direction only by procedure.** An unhold
  recorded for a volume still in use (a forced detach of a lost node; an operator's mistake)
  lets a maintenance attach depose a live target. Data stays safe: it is a failover. I/O on the
  old node fails. If the maintenance node is the client's own, the client's next restart there
  takes the volume back. If it is another node, that restart is refused (`only_if_target`, as
  after any attach elsewhere) until an administrator attaches the volume to the client's node
  again or Kubernetes publishes it again: a lockout, by procedure only.
- **Risk accepted: a wider set of target nodes.** A node becomes a target where an administrator
  says, not only where a consumer runs. The drain names only nodes that store the volume or are
  about to; the service does not enforce that rule. The choice lasts as long as the work: the
  record's maintenance ends with it.
- **Rollout order matters:** unholds start only once every metadata instance is new. An older
  instance answers an unhold `Unimplemented`, which the CSI controller retries as unavailable
  rather than lose. Its publish of a record that is not held, naming the publishing node,
  changes nothing and answers without `held`; the CSI controller refuses such an answer as
  unavailable too, so a publish is retried until a new instance answers. Within one call, the
  CSI controller moves past an older instance's answer to the next instance. Rolling back goes
  the other way: the CSI controller and `lab.sh` before the metadata service, since a metadata
  service older than the CSI controller fails every publish and unpublish.
- **Volumes detached before this change** stay held until one unhold by hand.
- **More transitions to keep in step** between the model, the service, the vectors and the
  simulator.

## Confirmation

- **Vectors:** `make contracts` regenerates the transition vectors from the Rust model, and the
  Go domain replays them.
- **Simulator:** with clients that attach by publish, restart and attach, and maintenance
  attaches and unholds made whatever the record says, `make sim` checks against its own record
  of holds, which takes each request as the model should admit it, that no maintenance attach,
  start or end commits while the volume is held (checked before the record takes the request),
  and that the model's held agrees with that record (checked after it takes the request); #15's
  check that no epoch runs twice covers maintenance targets. It must catch seven injected bugs:
  a maintenance attach that ignores held, a start that ignores the hold, an unhold applied for
  a node that is not the holder, a publish that leaves a record not held, an end of
  maintenance applied whatever the record, a restart that leaves a maintenance record not held,
  a maintenance start that skips the run mark.
- **Authorization:** `meta/service/auth_test.go` covers each new method for each identity.
- **Mixed versions:** `control/csi/csi_test.go` covers a publish answered without `held` and an
  unhold answered `Unimplemented`, and both done through a second instance when the first is
  old.
- **Review:** a new caller of `AttachMaintenance`, or a new way to clear the mark, needs a
  superseding ADR.
