# Issue #17: Draining volumes that are not attached — intent

[Issue #17](https://github.com/jsfakian/Flaminio/issues/17), follow-ups §11. Depends on #15
(several volumes per node), planned on its own branch and not built. Related: #13 (a move's
source is deleted), #18 (where a move goes).

## Why

A drain only requests moves (`MoveReplica`). Each volume's target carries them out, under its
epoch (replication-protocol.md §7, "Moves"). "A move needs a running target; a volume that is
not attached moves when it is next attached."

1. **A drain of a node with an unattached volume never completes.** `flaminio-meta drain` steps
   until no chunk replica is left on the node (`Run`, `control/drain/drain.go`), for 30 minutes
   by default. In the lab, a leftover 1 GiB volume that was never attached held the first drain
   until it was deleted (phase 1 plan, step 6 result).
2. **The drain lists only volumes that were never attached.** It takes a volume as unattached
   when its record names no target (`stepVolume`, `drain.go:113`). But the target is set by
   every attach and never cleared (`Header.Attach`, `meta/domain/domain.go:217`, its only
   writer):
   - `ControllerUnpublishVolume` "changes nothing" (`control/csi/controller.go:290`);
   - `NodeUnstageVolume` stops the target through the node's control socket only
     (`control/csi/node.go:140`);
   - the metadata service has no detach.

   So a volume whose pod is gone (a StatefulSet scaled to zero) still names its last node. The
   drain counts its replicas as moving, lists it nowhere, and waits for nothing.
3. **The metadata service cannot tell a client's live target from a past one.** Every attach
   takes the target under a new epoch (`Attach`, `meta/service/service.go:723`), and the target
   it replaces is deposed: at its next ISR change, at a replica's `Stale`, or at its 2 s poll
   (`on_meta_inner`, `flaminio-repl/src/target.rs`). An attach for maintenance, as attaches are
   today, would end a pod's I/O whenever the volume was in use after all.
4. **It would also lock the client out.** A node starts its target with `only_if_target`
   (`control/csi/node.go:70`), refused when the record names another node (`service.go:714`).
   Kubernetes publishes once per VolumeAttachment. A pod that restarts on its node while a
   maintenance attach holds the target elsewhere would never stage again.
5. **Nothing can start a target without a device.**
   - The frontend forbids it: an attach that times out is dropped, "an answer that comes later
     must not start a target that no device serves" (`flaminio-io/src/ublk.rs:866`, issue #6).
   - The control socket's attach always adds a ublk device, and its detach refuses a volume
     with no device (`control.rs:193`).
   - The socket is local and root-only. The drain runs as an administrator, anywhere: it
     reaches the metadata service, and no node's data plane.
   - On a data node outside Kubernetes (fl-c), nothing drives the socket (issue #14).
6. **What exists and works.**
   - The engine keeps its targets in a map, and its tests run them with no device
     (`flaminio-node/tests/engine.rs`).
   - A target carries out moves with no client I/O: it polls its volume every 2 s. The next
     target resumes a move a dead one left (protocol §7). The simulator checks both.
   - A volume deleted during a move is fenced by its header (docs/issues/8); a target that
     finds its volume gone is deposed.
7. **What #15 gives, and what it does not.** Its plan (`docs/issues/15/` on its branch, 72
   commits behind main) gives an engine thread that runs "from the node's start to its end,
   devices or not" (spec S3), so a target's turns no longer come from a device's queue thread.
   It checks an attach's token (S11). It leaves to this issue "an attach without a device on the
   control socket, and the drain's flow" (intent, out of scope; decisions F13). Its registry
   entries, limit and identities are all per device. So #15 makes a target without a device
   able to run; starting, limiting and ending one is here.
8. **The operator.** None is in the tree, and no issue plans it. The issue names it because a
   drain will one day be a reconciler. Nothing here needs Kubernetes: the drain controller
   (`control/drain`, run by `flaminio-meta drain`) is enough, and the operator can call it
   later.
9. **A maintenance target is a target.** It takes an epoch and a token, fences the replicas,
   holds leases, and reads and writes the volume's data on its node. Today a node becomes a
   target only where a consumer attaches the volume; with this, wherever the drain says. The
   threat model's "volumes it does not host targets for" (ARCHITECTURE §15.1) widens unless the
   choice of node is narrow and only an administrator makes it.

## What we want

1. **The metadata service knows whether a client holds a volume:** from its attach until an
   unhold, which the CSI controller's unpublish and the lab's detach record. (Called a release
   until the plan review's round 1, 2026-11-14: #13 keeps that word for copies to delete.)
2. **A maintenance attach,** asked by the drain: applied only while no client holds the volume,
   in one conditional write. A client's attach always wins over it, at the cost of a failover
   at most.
3. **A maintenance target** on the node the drain named: no device, no client I/O. It fences,
   reconciles, resyncs and moves like any target, and ends by itself when no move is left.
4. **A drain that survives it failing:** a maintenance target that dies mid-move is started
   again, there or elsewhere, and the move resumes.
5. **Narrow rights:** only an administrator asks; only the named node starts; the node already
   stores the volume, or is about to.
6. **Done when:** draining a node that holds replicas of unattached volumes completes, and
   attaching those volumes afterwards finds their data on the new nodes.

## Out of scope

- **#15:** the engine thread, several devices, the token check of an attach. Assumed done as
  planned.
- **Where a move goes** (#18). The drain keeps choosing each destination in one place; this
  issue does not change the choice.
- **Deleting a move's source** (#13). After a drain here, the drained node still holds its
  copies, as today.
- **The operator, CRDs, `kubectl flaminio node drain`** (ARCHITECTURE §10.3). The drain stays
  `flaminio-meta drain`.
- **Restoring redundancy of an unattached volume** whose replica is outside the ISR, with no
  drain. A maintenance target resyncs while it runs, but nothing asks for one then. The same
  attach would serve a rebuild controller later.
- **A held volume whose target does not run** (published and not staged, or its node lost and
  not failed over). Its moves wait for its target, as today; the drain now names it.
- **Moving targets off a draining node** (follow-ups §9, upgrades).
- **Cancelling a requested move.**
- **Encryption at the target** (phase 3): which keys a maintenance target needs, if any.
