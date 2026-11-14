# Issue #12: the data plane in Kubernetes — specification

Settled choices and accepted risks are in [decisions.md](decisions.md). The mechanisms are in
[design.md](design.md).

## Terms

- **Pod node:** a Kubernetes data node chosen by an administrator. Its data plane runs in the
  pod of the DaemonSet `flaminio-dataplane`. Every other data node runs the systemd unit, as
  today.
- **Attached:** the node's data plane serves a volume, or has an attach queued or running.
- **Served volume:** a volume attached on some data node.
- **Flaminio volume on a node:** a VolumeAttachment of `csi.flaminio.io` that names the node.
- **Every ISR full:** every chunk of every served volume has all its replicas in its ISR.

## Behaviour

**S1. The data plane runs in a pod.**
- On a pod node, Kubernetes runs one data-plane pod: the node agent's checks and claim, then
  `flaminio-io node`.
- No lab command starts it: not at deploy, not after a reboot, not under the failover
  controller's out-of-service mark.
- Two data planes never run on one node, whether pods or a pod and the unit.
- The CSI node plugin keeps its own DaemonSet. Its upgrades do not restart the data plane.

**S2. The checks and the claim.**
- The pod's data plane starts only when the node meets ADR 0003: kernel 6.6 or later,
  `ublk_drv` available, io_uring enabled. Otherwise it does not start, and its log names what
  is missing.
- It serves the node's existing Flaminio disk, chosen by the claim policy as now.
- The pod never formats a disk. A node with only a blank disk does not start, and its log says
  why.
- The data plane gets no more of the host than it needs:
  - its devices;
  - the control socket's directory;
  - its own identity (`.../node/<node>/io`) and the CA.

  It has no other key, and no Kubernetes credentials.
- It is privileged (ADR 0012). The boundary is the host, and in this lab, the cluster.

**S3. The control socket.**
- **Path:** unchanged, `/run/flaminio/node.sock` on the host. The CSI node plugin, `node.sh`
  and the lab's checks keep using it.
- **Commands:** unchanged, except:
  - `status` also reports an attach in progress;
  - a second attach is refused with `already attaching V` or `already attached to V`, naming
    the volume that holds the node (today: `V is attached here; one volume at a time`);
  - `detach V` while V is attaching answers `error: V is attaching`;
  - an attach after a SIGTERM answers `error: the node is shutting down`.
- **Who reaches it:** only root on the host and pods allowed to mount the host or run
  privileged.
  - Pod Security refuses such pods outside `kube-system` and `flaminio-system`.
  - Pod Security does not check PersistentVolumes. A hostPath PV reaches the socket from any
    namespace. The cluster's administrators can create one, and so can the CSI controller's
    service account (`flaminio-csi-controller`). This is an accepted risk (ADR 0012).
  - Its directory is root's, 0700, and the socket 0600.
  - Token files are 0600. `node.sh detach` deletes its volume's file, and `node.sh down`
    deletes them all once no data plane runs, a crashed unit's included. A detach by the CSI
    node plugin or by deposition leaves the file until then.
- **What it grants:** full control of the node's volumes, as today (ADR 0012).

**S4. Lifecycle.**
- **Ready:** once the socket answers. Until then, an attach on the node fails, and kubelet
  retries, as now.
- **A crash or restart with nothing attached:** Kubernetes restarts the data plane. Its replicas
  rejoin by resync.
- **A crash with a volume attached,** a reboot included:
  - the volume's I/O on that node fails;
  - the data plane comes back serving replicas;
  - once the volume's pod is deleted, the volume attaches again under a new epoch, wherever the
    pod goes, the same node included.

  A device left by the crash never blocks that attach.
- **A deletion of the pod while attached,** however it comes, waits up to the pod's grace
  period (3600 s) for nothing to be attached. The pod keeps serving meanwhile. If the grace
  runs out, that is a crash.
- **A deletion with nothing attached** ends within seconds. A data plane that does not answer
  counts as attached.
- **A SIGTERM while attached** is remembered, and the data plane exits after the next
  detach. It takes no new attach after a SIGTERM. A deletion of the pod signals it at the
  deletion's start, so it takes no new attach while the deletion waits.
- **The out-of-service mark:** the data-plane pod runs under it, so the failover controller can
  lift it. The CSI node plugin does not, so nothing is staged under the mark.
- **Lab commands that would kill containers without warning** refuse unless every pod node's
  socket, asked from the host, reports nothing attached. The answer is one of three: absent,
  none, or anything else, an error included. They refuse too when a node cannot be
  asked. This covers stopping or uninstalling k3s, and undeploying the driver. Removing the
  test namespace first is allowed.
- **The units' scripts** (`node.sh` and `ublk.sh`, `up` and `down`) never delete a ublk device
  that a live data plane serves. They delete only their own id, and only once no pod data plane
  can be on the node: no socket answers but their own unit's, and no data-plane sandbox is
  ready. A device left then is dead (#74 deletes such leftovers).

**S5. The ordered upgrade.** A change to the DaemonSet restarts nothing. `make node-upgrade`
replaces the data-plane pods that are not at the DaemonSet's newest revision, one node at a
time, nodes without a Flaminio volume first. For each node:
1. **Cordon,** unless the node is cordoned already.
2. **Check.** It refuses, uncordons what it cordoned, and names the cause, unless:
   - every ISR is full (it waits up to 900 s);
   - every chunk of a served volume with a replica on the node keeps `minIsr` without it. RF 1
     chunks fail this. Unserved volumes do not count;
   - every pod using a Flaminio volume on the node has a controller;
   - if the node has a Flaminio volume, another uncordoned data node has none (#15).
3. **Evict** the pods that use a Flaminio volume on the node. A refused eviction is a refusal.
4. **Wait,** up to 300 s, until nothing is attached on the node and no Flaminio volume names
   it. Otherwise it refuses, and leaves the node cordoned.
5. **Check again,** without waiting, then delete the pod.
6. **Wait,** up to 300 s, for the successor to be Ready, and up to 900 s for every ISR full.
   Otherwise it refuses, and leaves the node cordoned.
7. **Uncordon,** if it cordoned.

A rerun starts over, and skips the nodes already at the newest revision. Running one at a
time is documented, not enforced.

**S6. What a pod with a PVC sees.**
- **While another node upgrades:** no I/O error. Its target drops that node's replicas from the
  ISR, and they rejoin by resync. S5's checks keep every chunk at `minIsr` or above.
- **When its own node upgrades:** it is evicted, and starts on another node with its data. Its
  I/O stops until it is Ready there.

**S7. The Kubernetes lab.**
- **`make csi-deploy`** brings up the data plane in pods on fl-a and fl-b. `make node-up` is not
  needed.
  - It formats only a blank data disk, on the host, as `make data-node-up` does.
  - It refuses where the unit serves a volume.
  - Run again with the same build, it restarts no data plane.
  - **`DATAPLANE=unit`** deploys only the driver, over the units that `make node-up` started,
    as today (for #74's evidence under Kubernetes). It refuses where a node is a pod node.
- **`make csi-failover`** needs no `make node-up` after the VM returns.
- **The k3s agents** join with an agent token. The server's token is no longer given to the data
  nodes. Its copies in their logs from earlier installs stay, valid until it is rotated (out of
  scope).
- **The Mac demo is unchanged.**

**S8. Nodes outside Kubernetes.** Unchanged:
- fl-c and `make data-node-up`;
- `make node-up`;
- the benchmarks and checks that drive the unit. On fl-a and fl-b, they need
  `make csi-undeploy` first, and refuse on a pod node, the raw-disk ones included. Only a
  read-only raw run on an already preconditioned disk, as `make bench-pod`'s comparison, runs
  on a pod node.

**S9. Unchanged:**
- the control protocol's time limits;
- the CSI node plugin's work, identity, epochs and fencing;
- the claim policy's verdicts;
- the on-disk format;
- `flaminio-io node`'s flags.

## Tests

- **Unit:**
  - **The kernel checks:** each refusal, and a pass.
  - **The claim** never formats, and waits for a disk still held.
  - **The agent's `status`:** an attach in progress, nothing attached, and no socket. A socket
    that never answers, an error answer and a malformed one are not "nothing attached".
  - **The data plane:**
    - `status` reports a queued or running attach;
    - SIGTERM exits at once with nothing attached, and only after the detach while attached or
      attaching;
    - an attach after SIGTERM is refused;
    - the socket's directory is 0700.
  - **The manifests:** each S2–S4 property that a manifest carries (design, Tests).
  - **The scripts** (`node-scripts-test.sh`, and lab.sh's functions): each shell rule of S3
    (`k3s.sh server`'s Pod Security check included), S4 and S8, the PKI directory (S2), and
    `DATAPLANE=unit`'s refusal (S7) (design, Tests). The other S7 rules (the agent token, its
    files' modes, the restart on change) are shown in the lab.
- **Lab** (fl-a and fl-b; design, Tests):
  1. **From the unit to pods,** with no `make node-up`: both pods are Ready, the nodes are alive
     in the metadata service, and `make csi-test` passes. Kubernetes refuses what S3 says
     Pod Security refuses.
  2. **The claim in the pod** picks the same disk as on the host.
  3. **`make csi-failover`** passes without `make node-up`.
  4. **An ordered upgrade, with a load pod on fl-a:**
     - fl-b first: the load pod is not restarted, and sees no error;
     - then fl-a: the load pod moves to fl-b with its data verified;
     - every data-plane pod ends at the newest revision, with no restarts.
  5. **Each refusal of S5,** each with its own setup. No data-plane pod is deleted while
     attached.
  6. **Deletions while attached:** the pod keeps serving until the volume's pod has moved.
     Stopping k3s and undeploying refuse.
  7. **A crash while attached:** the volume's pod, deleted, comes back on the same node with its
     data, at a higher epoch.
  8. **One data plane per node:** the units' `up` refuse on a pod node, even with its data
     plane stopped, and the units' `down` leave a pod's volume attached.
  9. **`DATAPLANE=unit`:** over the units, `make csi-test` passes, and no data-plane pod runs.
  10. **`make bench-pod`** completes on pods, its raw comparison read only.
