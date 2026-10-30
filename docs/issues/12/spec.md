# Issue #12: the data plane in Kubernetes — specification

## Terms

- **Data-plane pod:** the pod of the DaemonSet `flaminio-dataplane` on a node: the node agent's
  checks and claim, then `flaminio-io node`.
- **Pod node:** a Kubernetes node labelled
  `node-restriction.kubernetes.io/flaminio-dataplane=pod`.
  - A kubelet cannot set this label on its own node (NodeRestriction).
  - An administrator can set it, and so can the failover controller's service account, which
    may patch nodes (ADR 0007).
  - Every other data node runs the systemd unit `flaminio-node`, as today.
- **Attached volume:** a volume whose target runs on the node. The control socket's `status`
  names it, or answers `ok none`.
- **Served volume:** a volume attached on some data node, in Kubernetes or not.
- **Flaminio volume on a node:** a VolumeAttachment of `csi.flaminio.io` that names the node.
- **Evict:** to delete a pod through the Eviction API. The upgrade evicts pods. It moves no
  replica (`control/drain` is not used): they rejoin by resync.
- **Every ISR full:** every chunk of every served volume has its ISR equal to its replicas.
  - A volume without a target is left out: nothing changes its ISR before its next attach.
  - `wait_isr_full` checks only `$VOL` today. The upgrade checks every served volume.
- **Ordered upgrade:** replacing the data-plane pods with `make node-upgrade` (S6), not a
  DaemonSet rolling update.

## Behaviour

**S1. Deployment shape.** The DaemonSet `flaminio-dataplane` is in `flaminio-system`, in a
manifest of its own under `deploy/`.
- **Nodes:** its node selector is the pod-node label and `flaminio.io/storage=true`.
- **The CSI node plugin** keeps its own DaemonSet, unchanged. Its upgrades restart no data
  plane.

The pod:
1. **Checks and claim** (init container).
   - The kernel checks of S2.
   - `flaminio-agent claim --serial <selector> --no-format`: the claim policy as now, but it
     never formats.
     - A blank disk is refused: it must be formatted first (S8).
     - Once the policy picks the node's Flaminio disk, the claim opens that disk with
       `O_EXCL`. While that fails with `EBUSY`, as when the previous pod's data plane is still
       exiting, it waits and tries again.
   - The claimed path goes to the container through a directory in the pod.
   - On a refusal, the data plane does not start. The log gives the reason and each device's
     verdict, and Kubernetes tries again.
2. **The data plane** (container).
   - **The command:** `sh -c` reads the claimed path, then `exec`s `flaminio-io node`, so that
     the data plane is the container's PID 1 and gets its signals. Its flags:
     - `--device`: the claimed disk;
     - `--node`: the Kubernetes node's name, which is also the node's name in the metadata
       service;
     - `--listen`: `status.hostIP`, port 7411. On the lab, that is the overlay address
       (`k3s.sh`);
     - `--meta`, `--pki /etc/flaminio/pki`, `--trust-domain`, and
       `--control /run/flaminio/node.sock`.
   - **Never:** `--format`, `--force` or `--id`.
   - **The device id** is left to the kernel (the first free one). An id leaked by a crash
     cannot block the next attach. Leaked ids are reclaimed when the node reboots.
   - **Other settings** keep the node's defaults.
3. **The pod:**
   - **Network:** `hostNetwork`, with port 7411 declared as a host port, so that the scheduler
     never places two data-plane pods on one node. `hostPID` and `hostIPC` are false.
   - **Priority:** `system-node-critical`.
   - **Tolerations:** exactly these, each with `operator: Exists`:
     - `node.kubernetes.io/out-of-service`;
     - `not-ready`, `unreachable` and `unschedulable`;
     - `memory-pressure`, `disk-pressure` and `pid-pressure`.

     Any other `NoExecute` taint evicts the pod (S5). The label is the way to keep it off a
     node.
   - **Service account:** no token. The pod does not call the Kubernetes API.
   - **Root filesystem:** read-only in both containers. That is no boundary in a privileged
     container, only a guard against mistakes. An `emptyDir` is mounted at `/run/ublksrvd`,
     where libublk writes its device files.
   - **Image:** `imagePullPolicy: Never`.
   - **Updates:** `updateStrategy: OnDelete` (S6).
   - **Grace:** `terminationGracePeriodSeconds: 3600`, which bounds the preStop hook (S5). That
     is long enough for an administrator to move a volume's pod away by hand.
4. **`flaminio-agent` subcommands,** each with `[--socket PATH]`:
   - **`status`** sends `status` and prints the answer. It exits 0 when the socket answers, 1
     when it cannot connect, and 2 on an error answer.
   - **`status --none`** exits 0 only on `ok none`. No socket is not `none`: it exits 1.
   - **`hold`** prints the data plane's instance id, a number drawn when the process starts
     (S3).
   - **`release`** releases the hold.
   - **Exit codes:** `hold` and `release` exit 0 only on their `ok`.

   The readiness probe is `flaminio-agent status`.
5. **The image** holds `flaminio-io`, `flaminio-disk` and `flaminio-agent`.
   - **Where it is built:** from the release build of a data VM, on the CSI image's base, now
     pinned by digest (`scripts/dev/image.sh`; the CSI image gets the pin too). A compromised
     data VM could poison it (ADR 0007).
   - **The tag** is derived from the binaries' sha256 sums, so an unchanged build keeps its
     tag, and the template is unchanged.
   - **Which pods need an upgrade:** those whose `controller-revision-hash` label differs from
     the DaemonSet's newest ControllerRevision. So a change to the manifest alone rolls out
     too.

**S2. Privileges, host mounts and the kernel.**

**Privileged:** both containers (ADR 0007, proposed; this change adds it). Device access binds:
- the raw disk;
- `/dev/ublk-control`;
- device nodes that appear after the container starts.

The claim opens the disks too.

**Host mounts.** The pod mounts these host paths, and only these:
- `/dev`;
- `/run/udev`, read-only, for `lsblk` and `blkid`;
- `/run/flaminio` (S3);
- from `/etc/flaminio/pki`, read-only, each file with `type: File`: `io.crt`, `io.key`,
  `ca.crt` and `token.pub`.

**The boundary is the host, and in this lab, the cluster** (ADR 0007).
- Mounting four files keeps the other keys out of the pod's view, not out of its reach.
- In this lab the pod nodes hold the token key, the `meta`, `meta-etcd` and `etcd` keys, and,
  on fl-a, the administrator's key (intent, Why 8). A compromised data-plane pod reaches every
  volume and the metadata.
- Keeping these keys off the pod nodes means separating the metadata members from the data
  nodes: a follow-up, flagged in the intent.

**`ublk_drv`** loads at boot, as now. The pod loads no module.

**Kernel checks** (ADR 0003 gives them to the node agent at its start). They refuse, naming
what is missing:
- a kernel older than 6.6;
- no `/dev/ublk-control`;
- `kernel.io_uring_disabled` other than 0, or missing (ADR 0003).

**`hostPID`** is not needed. Lab test 1 confirms it. The fallback is `hostPID: true`.

**S3. The shared socket.**
- **Path:** unchanged, the host's `/run/flaminio/node.sock`.
- **Modes:**
  - The init container makes `/run/flaminio` root's, 0700. `flaminio-io` creates it 0700.
  - The socket stays 0600.
  - `node.sh up` writes its token file 0600. `node.sh detach` and `node.sh down` delete it.
  - The CSI node plugin stores no token, as now.
- **Tokens off command lines:** `lab.sh` passes a token to `node.sh` on stdin, and `node.sh`
  passes the command to its Python on stdin too.
- **The socket is full control of the node's volumes.** Whoever reaches it can:
  - attach with a token;
  - detach any target;
  - delete any replica not attached there;
  - hold and release;
  - read the status.

  `delete` and `detach` stay unauthorized, as now. The lab relies on `delete` to clean
  leftovers (`delete_wbench`). A check there would be no boundary: whoever reaches the socket
  can also write the raw disk.
- **Who reaches it:**
  - **The Pod Security default:** the k3s server gets a Pod Security admission configuration
    (`pod-security-admission-config-file`). It makes `baseline` the default, which refuses
    privileged pods and every hostPath. Only `kube-system` and `flaminio-system` are exempt.
  - **Labelled namespaces** keep their labels:
    - `flaminio-test` and `flaminio-smoke`, `restricted`;
    - `flaminio-bench`, `baseline`;
    - `flaminio-system`, `privileged`.
  - **What remains:**
    - root on the host;
    - anyone who may create pods in `kube-system` or `flaminio-system`;
    - anyone who may create a namespace, or label one `privileged`. Only the cluster's
      administrators may, in the lab (lab test 1).
- **Commands:**
  - **`hold`** answers `ok held <instance id>`. From then on, the data plane refuses new
    attaches, with that reason, until `release`. An attach that has begun completes.
    - A hold lives in the process: a restart loses it, and the new process has a new instance
      id.
    - `flaminio-agent release` ends a hold that a failed upgrade left. The upgrade's own
      recovery is in S6.
  - **`attach`, `detach`, `delete` and `status`** are unchanged.
- **A socket left by a previous run** is removed at start, as now.

**S4. Identity.**
- **The data plane** is `.../flaminio/node/<node>/io`, as under the unit.
- **The CSI node plugin** keeps `.../node/<node>/agent`.
- **The checks and claim** use none.
- **A reissued certificate** takes effect when the pod next starts (#16).

**S5. Lifecycle.**
- **Start.**
  - Checks, claim, then the data plane, which registers and heartbeats as under the unit.
  - The pod is Ready once `flaminio-agent status` succeeds.
  - Until then, the node plugin answers NodeStageVolume `Unavailable`, as now.
- **A reboot, or the out-of-service mark.**
  - The pod starts with no lab command.
  - It tolerates the mark, and its heartbeats let the failover controller remove it.
  - The CSI node plugin does not tolerate the mark, so nothing is staged until the mark is
    gone.
  - **A reboot under an attached volume is a crash.** The lab's k3s has no graceful node
    shutdown, so no hook runs.
- **A crash or a restart without an attached volume:** Kubernetes restarts the container. The
  replicas rejoin by resync.
- **A crash with an attached volume:**
  - The volume's I/O on the node fails, and the data plane comes back serving replicas.
  - Once the volume's pod is deleted, the volume is staged again, under a new epoch, wherever
    the pod is scheduled.
- **Any deletion of the pod** runs the preStop hook. It covers:
  - removing the label;
  - deleting the DaemonSet;
  - `kubectl delete`;
  - eviction by a `NoExecute` taint the pod does not tolerate.

  The hook loops until `flaminio-agent status --none` succeeds. On each turn it sends `hold`
  again, so that a `release` from someone else lasts one turn at most. The grace period
  (3600 s) bounds the wait. If it runs out, the pod is killed, which is a crash with an
  attached volume.
- **SIGTERM:** `flaminio-io` now handles it.
  - With no volume attached, it exits at once: it stops the engine and removes the socket.
  - With a volume attached, it ignores SIGTERM. The preStop hook has waited; a SIGTERM that
    comes anyway leaves the volume served until the grace period's SIGKILL, which is a crash
    under the volume.
- **`make k8s-down` and `make k8s-uninstall`** kill the containers without hooks
  (`k3s-killall.sh`). Both now refuse while any pod node has an attached volume.

**S6. The ordered upgrade.** With `OnDelete`, a change to the template restarts nothing.
`make node-upgrade` replaces the data-plane pods one node at a time.
- **Order:** first the nodes without a Flaminio volume, then the others.
- **Skipped:** a node whose pod has the DaemonSet's newest revision (S1).
- **Each run** draws a nonce.

**The checks.** They run at step 2, and without waiting at step 6. They pass when:
- every ISR is full. Step 2 waits for this up to 900 s, as `wait_isr_full`; step 6 does not
  wait;
- every chunk with a replica on the node keeps `minIsr` without it (ISR − 1 ≥ `minIsr`), in
  every served volume. A chunk with its only replica there (RF 1) fails this;
- every pod that uses a Flaminio volume on the node has a controller;
- while the node has a Flaminio volume, another uncordoned `flaminio.io/storage` node has
  none (#15).

**A refusal:** the upgrade sends `release`, undoes its own cordon, and stops, naming the cause.
An `EXIT` trap does the same when the upgrade is interrupted.

**Its cordon.**
- **Marking:** the upgrade cordons a node only if it is schedulable, and annotates it
  `flaminio.io/upgrade-cordon=<nonce>`.
- **Uncordoning:** only if the annotation still holds this run's nonce. An administrator who
  wants to keep the cordon changes or removes the annotation. A cordon made before the run is
  never touched.

**A rerun** resumes every node with the annotation:
1. It sends `release`, and writes its own nonce into the annotation.
2. If the node's pod has the newest revision, it uncordons the node once every ISR is full.
3. Otherwise, it upgrades the node from step 2.

For each node, it:
1. **Cordons** the node (above).
2. **Runs the checks,** on the pods it lists after cordoning.
3. **Holds** the data plane: `kubectl exec` of `flaminio-agent hold`. It keeps the instance id.
4. **Evicts** every pod that uses a Flaminio volume on the node. A refused eviction (a
   PodDisruptionBudget, say) is a refusal.
5. **Waits** up to 300 s (`csi_test`'s limit for a pod to start elsewhere) until `status` is
   `ok none` and no Flaminio volume is on the node. Running out of time is a refusal. So is a
   volume attached outside Kubernetes.
6. **Before deleting,** requires all of these, or refuses:
   - `hold` again, answered with the instance id of step 3;
   - `status` `ok none`;
   - no Flaminio volume on the node;
   - the checks, without waiting;
   - the new image present in the node's containerd (`k3s ctr images ls`).

   Then it deletes the pod, and waits for the successor, with the newest revision, to be Ready.
7. **Waits** until every ISR is full again.
8. **Uncordons** the node (above).

**Why not `RollingUpdate`:** it deletes a pod whatever its node serves. It also counts the new
pod as available before the replicas are back in sync. This ordering is the operator's job
later (ARCHITECTURE §18).

**S7. What a pod with a PVC sees.**
- **While other nodes upgrade:** no I/O error. Its target drops the restarting node's
  replicas after its request timeout (3 s), and they rejoin by resync. The checks keep every
  chunk at `minIsr` or above.
- **When its own node upgrades:** it is evicted, and starts on another node with its data. Its
  I/O stops until it is Ready there.

**S8. The Kubernetes lab.** None of these steps runs in demo mode (`demo_active`): the demo
keeps `make node-up`.
- **`make k8s-up`:**
  - **Pod Security:** it writes the configuration of S3 into the k3s server's configuration.
  - **Agent token:** it gives the server an `agent-token`, and the agents on fl-a and fl-b join
    with it instead of the server's full token. The token goes to them on stdin.
  - **An existing lab:** `install_k3s` returns early while the right version runs (`k3s.sh`).
    Now, when the configuration changes, `k8s-up` restarts k3s on that VM. Restarting the
    service leaves the pods running.
- **`make csi-deploy`** no longer needs `make node-up`; it needs `make deploy`. It:
  1. runs the agent's claim on each data node's host (`cmd_agent claim`), which formats only a
     blank disk, as for fl-c;
  2. stops a running unit on fl-a and fl-b. It refuses where the unit has an attached volume;
  3. builds the image, imports it on fl-a and fl-b, and labels them as pod nodes;
  4. applies the DaemonSet, and waits for its pods to be Ready;
  5. deploys the CSI driver, as now.

  Run again, it applies the new template and restarts nothing.
- **`make node-upgrade`:** S6.
- **`make csi-failover`** waits for the lost VM's data-plane pod, instead of `cmd_node up`.
- **`make csi-undeploy`** deletes the test namespace, then the DaemonSet and the labels.
- **`node.sh`** asks the socket, not the unit, whether a data plane runs (`attach`, `delete`,
  `status`).
- **`deploy/lab/upgrade-load.yaml`:** a StatefulSet of one pod with a Flaminio PVC. It runs fio
  in the lab's fio image, writing with verification continuously. It keeps fio's verify state
  on the PVC (`verify_state_save`, `verify_state_load`), so that after a move it verifies
  what was written before it.
- **`docs/lab.md`:** the CSI section without `make node-up`, and the two kinds of node.

**S9. Nodes outside Kubernetes.** Unchanged:
- `node.sh` (apart from S3 and S8), `make node-up` and `make data-node-up` (fl-c);
- the benchmarks and checks that drive the unit. On fl-a and fl-b, they need
  `make csi-undeploy` first.

One data plane per node:
- `node.sh up` refuses where the socket answers and the unit is not active.
- `flaminio-disk format` opens a block device with `O_EXCL`, as `uring.rs` does. A regular file
  is opened as now: it may be created or resized.

**S10. Unchanged:**
- the control protocol's commands and time limits;
- the CSI node plugin's work, its identity, epochs and fencing;
- the claim policy's verdicts;
- the on-disk format;
- `flaminio-io node`'s flags.

## Tests

- **Go:**
  - **The kernel checks of S2** (`control/agent`), against a given version and a temporary
    directory. Each refusal names what is missing:
    - 6.5;
    - no `/dev/ublk-control`;
    - `io_uring_disabled` at 1, at 2, and missing;
    - all present.
  - **The claim:**
    - `--no-format` refuses a blank disk;
    - `EBUSY` on the picked disk is retried until it is free.
  - **`flaminio-agent`** against a test socket:
    - **`status`:** 0 on `ok none` and on `ok vol /dev/ublkb3`, 1 with no socket, 2 on
      `error`;
    - **`status --none`:** non-zero on a volume, and with no socket;
    - **`hold`:** prints the instance id from `ok held <id>`;
    - **`hold` and `release`:** 0 only on their `ok`.
  - **The image tag:** the same binaries give the same tag, and a changed one a new tag.
  - **The manifests:** a test renders `deploy/` with placeholder values and checks:
    - `OnDelete`, `imagePullPolicy: Never`, and the pod-node selector;
    - the tolerations exactly as S1. `out-of-service` is tolerated, and the CSI node plugin
      does not tolerate it;
    - the host paths exactly as S2, and the PKI files read-only, `type: File`;
    - host port 7411, `hostPID` and `hostIPC` false, `readOnlyRootFilesystem`, and the
      `emptyDir` at `/run/ublksrvd`;
    - `terminationGracePeriodSeconds: 3600`;
    - the probe and the preStop hook use `flaminio-agent`;
    - the container's command `exec`s `flaminio-io`;
    - `/run/flaminio` mounted only by `flaminio-dataplane` and `flaminio-csi-node`;
    - the data plane's `--control` equals the node plugin's `--control-socket`;
    - no `--format`, `--force` or `--id`, and no service-account token.
- **Rust:**
  - **`hold`:**
    - it answers `ok held <id>`, with the same id each time in one process;
    - `attach` is refused with its reason, while `status`, `detach` and `delete` work;
    - `release` lets attaches through.
  - **The socket's directory** is created 0700.
  - **SIGTERM:**
    - with no volume attached, the process exits 0 within a second;
    - with one attached (a test device), it keeps serving, and exits only on SIGKILL.
- **`flaminio-disk`:** `format` of a block device held with `O_EXCL` fails with `EBUSY` (lab,
  as it needs a block device).
- **shellcheck** covers the changed scripts.
- **Lab,** once the design is approved, on fl-a and fl-b:
  1. **No `make node-up`.** With `make node-down`, then `make k8s-up` (on the existing lab),
     `make deploy` and `make csi-deploy`:
     - k3s restarted with the new configuration, and the agents' environment holds the agent
       token, not the server's;
     - both data-plane pods are Ready, without `hostPID`;
     - fl-a and fl-b are alive in the metadata service;
     - `make csi-test` passes;
     - **Kubernetes refusals:**
       - a kubelet cannot set the pod-node label on its own node;
       - in a new unlabelled namespace, a privileged pod and a pod with a hostPath at
         `/run/flaminio` are refused;
       - a service account that is not an administrator cannot create a namespace, nor label
         one `privileged` (`kubectl auth can-i`, and a try).
     - **What is accepted** (ADR 0007):
       - a namespace the administrator labels `privileged` admits a privileged pod;
       - the failover controller's service account can patch the pod-node label.
  2. **The claim in the pod** picks the same disk, with the same claim state for each device,
     as `make agent-disks` with the same `--serial`. The reasons may differ: mount points come
     from the container's mount namespace.
  3. **A reboot:** `make csi-failover` passes without `cmd_node up`. The data-plane pod starts
     under the out-of-service mark, and the mark comes off.
  4. **An ordered upgrade.** The pod of `upgrade-load.yaml` runs on fl-a. Then
     `make csi-deploy` again with a changed build, then `make node-upgrade`. A rerun of
     `make csi-deploy` with an unchanged build changes no revision.
     - **fl-b first:**
       - the load pod keeps its UID, with a restart count of 0;
       - fio reports no error;
       - fl-b's data-plane pod has the newest revision, with a restart count of 0;
       - every ISR is full again.
     - **then fl-a:**
       - the load pod is evicted after fl-a is cordoned and held;
       - it is Ready on fl-b within 300 s, and fio verifies what was written before;
       - fl-a's old data-plane pod is deleted only after `ok none`, and exits within seconds;
       - the DaemonSet controller creates the successor while the old pod terminates. It
         starts once the old one is gone, with a restart count of 0;
       - fl-a is uncordoned.
     - **At the end:** every data-plane pod has the newest revision.
  5. **Refusals.** Each stops, naming the cause. No data-plane pod is deleted under an
     attached volume, and the node is released, and not left cordoned by the upgrade:
     - a second PVC pod on fl-b (#15);
     - vol1 attached by `make vol-attach VM=a` (step 5);
     - a PodDisruptionBudget (`minAvailable: 1`) on the load pod;
     - a bare pod with a PVC on fl-a;
     - an RF 1 volume served from fl-b, with chunks only on fl-a;
     - `minIsr: 2` on an RF 2 volume served from fl-b;
     - fl-b's data plane restarted after fl-a's step 2: step 6 finds the ISRs not full, and
       does not wait;
     - fl-a's data plane restarted between steps 3 and 6: step 6 gets another instance id;
     - the new image removed from fl-a's containerd before step 6;
     - **a pod placed with `spec.nodeName: fl-a`** after step 3: it is not attached, its
       VolumeAttachment stops the upgrade at step 5 or 6, and once released it is attached
       and its data verifies;
     - **fl-a cordoned beforehand by the administrator:** the upgrade leaves it cordoned;
     - **the annotation changed during the run:** fl-a is left cordoned;
     - **the upgrade killed after the hold:** the trap releases. A rerun after a `kill -9`
       releases first, then resumes the node.
  6. **Deletions under an attached volume,** with `make csi-test`'s workload (`KEEP=1`) on
     fl-a:
     - removing fl-a's label leaves the pod terminating and serving, with the workload
       unharmed. It ends within seconds once the workload is moved away;
     - a `release` sent by hand during that wait: within one turn, `hold` is back. No attach
       reaches fl-a meanwhile;
     - the same with the DaemonSet deleted;
     - SIGTERM sent to `flaminio-io` by hand: the volume stays served;
     - `make k8s-down` and `make k8s-uninstall` refuse.
  7. **A crash under an attached volume.** fl-a's data-plane container is killed (SIGKILL),
     with that workload on fl-a. Then:
     - the container restarts, and fl-a is alive again;
     - the workload pod, deleted and scheduled back on fl-a (fl-b cordoned), is Ready with
       "verified on fl-a", at a higher epoch, on a new device id.
  8. **The socket:**
     - `/run/flaminio` is 0700;
     - a token file is gone after `node.sh detach` and after `node.sh down`;
     - no token appears in `ps` output during `make vol-attach`.
  9. **One data plane per node:** `node.sh up` on fl-a refuses while its pod runs.
