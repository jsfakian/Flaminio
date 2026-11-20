# Issue #63: Autoscaler integration — intent

[Issue #63](https://github.com/jsfakian/Flaminio/issues/63), from the README roadmap (phase 5)
and ARCHITECTURE §1.1, §10.2 and §10.4. This change is the first half of the issue's step 1,
**the storage disruption guard** (§10.4): a Kubernetes drain of a storage node is held while
taking the node down would leave a chunk with fewer in-sync replicas than its minimum. Draining
a node's replicas before it is removed, steps 2 and 3, and all that needs an autoscaler follow:
issue #63 stays open.

The guard is not the cloud's. It serves `kubectl drain` before maintenance on bare metal, and
the lab can check it with no autoscaler. #48 lists it as its step 3, #43 beside its step 3, #12
under "the operator"; none builds it. By `docs/issues/README.md` (what several issues share is
done once) it belongs in an issue of its own, which #48's step 3 and #63 then depend on. This
intent is written to move there unchanged. It needs nothing of #12, #17, #45 or #61.

## Why

1. **An autoscaler removes a node as an administrator does.** It cordons the node, evicts its
   pods through the eviction API, which honours PodDisruptionBudgets, then deletes the node and
   its instance. `kubectl drain` before a reboot does the first two. Neither knows of replicas,
   and Flaminio hears of neither:
   - nothing reads a node's cordon, and nothing creates a PodDisruptionBudget (none in
     `deploy/`, `control/` or `scripts/`);
   - the one Kubernetes client is the failover controller's, written by hand, for nodes only:
     get, list, patch taints (`control/failover/kube.go:21`, `deploy/csi/rbac.yaml:123-129`).
     `go.mod` has no `client-go`.
2. **A drain stops no replica.** The data plane is a host unit, outside Kubernetes
   (`deploy/csi/driver.yaml:4`); #12 makes it a DaemonSet pod. `kubectl drain` leaves DaemonSet
   pods in place, and the cluster autoscaler never keeps a node for one. So a
   PodDisruptionBudget on the data-plane pod guards nothing. A drain moves the pods, and their
   targets with them, under new epochs (`ControllerPublishVolume`, `control/csi/controller.go:274`).
   The replicas are served until the machine stops. The drain is the last moment anyone is
   asked.
3. **What the stop then costs.** The metadata service refuses an ISR below the volume's minimum
   (`CheckIsrChange`, `meta/domain/domain.go:238`). A chunk left with fewer live in-sync
   members than that is unavailable until the node returns: its writes wait for the member
   (protocol §4), and a new target stays fencing (§6). If the node never returns and was the
   last in-sync member, the chunk is lost. The lab's StorageClass is RF 2, minimum 1
   (`driver.yaml:228`): one node may go, the second not until the first is back in sync.
4. **The check exists by hand only.** `vol_degraded` counts one volume's short ISRs
   (`scripts/lab.sh:740`). #12 plans the same test inside `make node-upgrade`, for the pods it
   deletes itself (#12 spec, S5). Nothing holds a drain that someone else starts.
5. **The records can answer, at a cost.**
   - `ListNodes` gives alive and draining; `ListVolumes` ids only
     (`meta/service/service.go:845`); `GetVolume` the minimum and every chunk's replicas and ISR
     (`readVolume`, `service.go:303`). Any authenticated identity may call them
     (`meta/service/auth.go:79`). The only identities are administrators and nodes
     (`pkg/pki/pki.go:193`): the failover controller runs as the CSI controller's administrator
     (`driver.yaml:220`).
   - One pass is two calls and one per volume, as the drain's (`Step`,
     `control/drain/drain.go:46-57`). There is no watch, no index by node, and no snapshot
     across volumes. Unmeasured beyond the lab's few volumes.
   - **An ISR in a record is not liveness.** With no target the record keeps a dead node; at
     the minimum a running target keeps it too (#45, Why 4). Liveness lapses 10 s after the
     last heartbeat (`meta/cmd/flaminio-meta/main.go:87`); a target removes a member after 3 s
     (`request_timeout`, `flaminio-repl/src/types.rs:60`).
   - A node's id is its Kubernetes name (`--node-id=$(NODE_NAME)`, `driver.yaml`; `alive[n.Name]`,
     `control/failover/failover.go:83`). Storage nodes carry `flaminio.io/storage=true`
     (`kube.go:19`). fl-c holds replicas and is no Kubernetes node.
6. **What a guard can hang on.**
   - **A pod of its own on each storage node, with a budget.** A drain evicts it like any pod,
     and a refused eviction keeps the drain from completing. This is §10.4's pattern. It needs
     Kubernetes objects only, and `kubectl drain` checks it in the lab. The pod needs a
     controller: a drain refuses bare pods. A held drain has still cordoned the node and
     evicted its other pods: only its completion is held.
   - **An autoscaler's own marks,** such as the node annotation
     `cluster-autoscaler.kubernetes.io/scale-down-disabled`. Only that autoscaler reads them,
     `kubectl drain` does not, and no autoscaler is here to check them.
   - **An admission webhook on evictions.** A server and certificates that every eviction in
     the cluster then waits on.
   - **A caveat, not run.** The cluster autoscaler does not remove a node whose pod cannot run
     elsewhere, whatever its budget (its FAQ, "pods that cannot be moved elsewhere due to
     scheduling constraints"). A guard pod bound to its node therefore ends that autoscaler's
     scale-down of storage nodes: safe, and not yet the issue's "done when".
7. **A guard cannot make a removal safe, and need not.** It reads, then Kubernetes acts. An ISR
   can shrink after the reading, and a crash shows only seconds later (5). Kubernetes counts
   evictions against a budget atomically; nothing ties that count to the records. Safety stays
   where it is: a write is acknowledged once durable on every in-sync replica, no ISR goes
   below its minimum, a deposed target is fenced (ARCHITECTURE §2, §8). A guard that allows
   wrongly costs what a node failure costs.
8. **Scale-up, for the later steps.** The agent is a command an operator runs, for one disk
   (`flaminio-agent disks|claim`; `Pick`, `control/agent/disks.go:161`). No rebalancer exists:
   `Place` puts new volumes on the least loaded nodes, and nothing moves a chunk but a drain
   (`meta/domain/domain.go:359`). No capacity is recorded: `Node` has none (`meta.proto:63`),
   and the CSI controller declares create, publish and multi-writer, not `GET_CAPACITY`
   (`controller.go:88`). No request removes a node's record.
9. **No autoscaler and no cloud.** The lab is fixed VMs: k3s with its server on fl-w and agents
   on fl-a and fl-b (`docs/lab.md`).

## What we want

1. **Every Kubernetes storage node is guarded** with no operator action: an eviction that a
   drain of the node must get past, kept by a guard controller, made when the node appears and
   removed with it.
2. **A drain is held while it is unsafe.** A storage node's drain does not complete while some
   chunk with a replica on the node would keep fewer in-sync members than its volume's minimum,
   counting only members on other nodes that are alive and not being drained. Otherwise the
   same `kubectl drain` goes through, with no Flaminio command.
3. **One reading never allows two drains.** A node whose drain was allowed counts as gone until
   it is schedulable and alive again. Two drains started together are not both allowed where
   together they would break (2): by Kubernetes' own count, not the guard's timing.
4. **Not knowing holds.** While the guard does not run, or cannot read the metadata service, no
   storage node's drain is allowed. Kubernetes' own override stays (`kubectl drain
   --disable-eviction`), and is documented.
5. **The guard says why.** For a held node: which volumes and chunks hold it, where the
   administrator looks (Kubernetes), and in the guard's log.
6. **Availability only.** The guard reads the metadata service and writes its own Kubernetes
   objects. No ISR, epoch, replica set, move or drain mark changes because of it. Wrong, late or
   gone, it costs a held drain or a chunk that waits for a node, never an acknowledged write.
7. **A lab check,** by an operator, on an RF 2 volume on fl-a and fl-b holding verified data:
   - fl-a's data plane stopped: `kubectl drain fl-b` does not complete, and the guard names
     the volume;
   - fl-a back and every ISR full: the same drain completes;
   - fl-b still cordoned: `kubectl drain fl-a` is held; after fl-b's uncordon it completes;
   - the data verifies at the end.
8. **Docs:** which part of §10.4 exists, `docs/lab.md`, the manifests and their permissions.

## Out of scope

- **The rest of step 1: draining before removal.** Nothing ties `flaminio-meta drain` to a
  cordon or to an autoscaler's intent, and the eviction API cannot tell a reboot from a
  removal. Until then a removed node lowers redundancy, within the minimum, and #45 restores it.
  It needs #17 (unattached volumes) and #13 (the sources).
- **Letting an autoscaler remove a storage node when it is safe** (Why 6, the caveat): each
  autoscaler's own marks and taints. It needs an autoscaler to check.
- **A removed node's record,** and a node that returns under another name. Later, with #45's
  states.
- **Step 2: scale-up.** Claiming a new node's disk at boot (#61's step 2) and the rebalancer
  (§10.2), which does not exist (Why 8).
- **Step 3: storage capacity.** `GetCapacity` and `CSIStorageCapacity` need capacity accounting
  first (#45, Why 6). The autoscaler's FAQ warns that per-node capacity objects can themselves
  block a scale-up.
- **Removals that ask nobody:** `kubectl delete node`, a reboot with no drain, a reclaimed spot
  instance. They are failures: #45, #61.
- **Holding the node's other pods.** A held drain has evicted them already (Why 6). Their
  volumes fail over, as in any drain.
- **Maintenance mode** (§10.4's other half): #44's step 3.
- **Ordered upgrades:** `make node-upgrade` (#12) keeps its own checks; it deletes DaemonSet
  pods, which is no eviction. #43's and #48's steps 3 may use the guard later.
- **Nodes outside Kubernetes** (fl-c). Nothing drains them through Kubernetes. They count as
  members.
- **A check at the moment of eviction** (a webhook), and **a metadata query for one node in one
  snapshot.** Recorded for when a pass's cost is measured (Why 5).
- **The metadata service, the replication protocol and the on-disk format:** unchanged.
- **A check with a real autoscaler.** It needs a cloud account or a provider for the lab's VMs.
  The issue's "done when" can only be shown there.
