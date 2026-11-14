# Issue #82: Evidence-based node failover — intent

[Issue #82](https://github.com/jsfakian/Flaminio/issues/82), correctness-audit finding F07.
This plan covers preventing unsafe node-wide recovery and defining its evidence boundary.
Approved 2026-11-13; not implemented.

## Why

1. **Absence currently becomes a shutdown assertion.** `Controller.Step`
   (`control/failover/failover.go:63–95`) marks a storage node out of service after its Ready
   condition has been false for Grace, its Flaminio heartbeat has disappeared, and another
   storage node is Ready. A live partition produces exactly those observations. Neither the
   controller's Node input nor its Liveness interface carries shutdown evidence (`:31–59`).
2. **The action is node-wide.** `APIClient.SetOutOfService`
   (`control/failover/kube.go:167–194`) writes `node.kubernetes.io/out-of-service:NoExecute`.
   Kubernetes can then force-delete pods and immediately detach their storage, including
   volumes managed by other drivers. The current deployment starts this controller without
   a per-workload opt-in (`control/cmd/flaminio-csi/main.go:69–76`).
3. **Storage fencing is narrower.** Flaminio's new target must fence, wait when necessary,
   and reconcile under the replication protocol. That protects Flaminio block I/O; it does
   not stop a partitioned process, protect another driver's volume, or fence a pod's network
   side effects. Architecture §11.2 describes a per-StorageClass opt-in which code lacks.
4. **The decision is reproduced, not hypothetical.** The audit's
   `TestAuditPartitionDoesNotAssertNodeShutdown` presents only missing readiness/liveness and
   observes `mark a`. Existing `control/failover/failover_test.go:40–76` expects this action.
   Downstream data corruption was not induced. Kubernetes's node-shutdown guidance requires
   verifying shutdown/power-off before applying the node-wide taint.

## What we want

1. Missing readiness, expired heartbeat, partitions and a deleted Node object may identify a
   recovery candidate, but never count as proof the machine has stopped.
2. Node-wide force recovery requires trusted shutdown/power-fence evidence for the exact
   node incarnation. Missing, failed, stale or mismatched evidence leaves workloads alone.
3. Verified shutdown still permits recovery; restarts, retries and concurrent controllers
   do not authorize duplicate, stale or wrongly owned taint changes.
4. A future partition-recovery path, if selected, affects only explicitly opted-in Flaminio
   workloads, leaves unrelated storage alone, and states the limits on process side effects.
5. Tests cover live partitions, mixed storage, stale evidence, shutdown recovery and cleanup.

## Out of scope

- Choosing or implementing every cloud/BMC fencing provider, or adding STONITH requirements
  to ordinary Flaminio I/O. The first Proposed profile is manual physical lockout with terminal identity retirement;
  other providers remain unsupported.
- Changing epochs, ISR, leases, reconciliation or their current takeover gate. Proposed ADR
  0007 in the audit branch is context, not an accepted replacement for ADR 0005.
- Claiming that Kubernetes object deletion or storage fencing stops application processes.
- The operator (#43), live data-plane upgrades (#48), or the whole chaos suite (#49).
  Those issues consume this contract; this issue supplies its regression and integration cases.
- Running shared-lab shutdown, partition or detach commands while drafting these documents.
