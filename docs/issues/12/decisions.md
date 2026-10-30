# Issue #12: the data plane in Kubernetes — decisions

These are settled. Each line gives the choice, then the reason.

## Scope

- **"Keeps a pod with a PVC running"** means two things. A pod runs without an I/O error while
  other nodes upgrade. On its own node's upgrade, it is evicted and restarts elsewhere with its
  data. Running through its own node's restart needs ublk user recovery.
- **ublk user recovery** is a new issue, whose text is in the intent. It needs the epoch
  re-taken through the agent identity, and its device state on the host.
- **#15 and #17 are not needed.** Only a pod's move needs a free node, and no replica moves.
- **The pod never formats.** `make csi-deploy` formats only blank disks, on the host. A restart
  can then never create a new store for a node the cluster knows.
- **The Mac demo is unchanged,** and keeps `make node-up`. It is a separate setup.
- **Benchmarks and checks keep the systemd unit,** and need `make csi-undeploy` on fl-a and
  fl-b. They read the unit's journal and flags.
- **Out of scope:**
  - separating the metadata members from the data nodes;
  - privilege separation;
  - #16;
  - a registry;
  - CPU pinning;
  - rotating the k3s server token.

  Each is a follow-up.

## Shape

- **One DaemonSet pod, `flaminio-dataplane`,** holds the agent's checks and claim (init) and
  the data plane. The claim's result is needed only at start.
- **The CSI node plugin keeps its own DaemonSet.** Its upgrades must not restart the data
  plane.
- **The socket path is unchanged** (`/run/flaminio/node.sock`), shared through the host. The
  node plugin, `node.sh` and the checks already use it.
- **Pod nodes are chosen by a `node-restriction.kubernetes.io/` label.** Kubelets cannot set
  it, and the label lets a node go back to the unit.
- **Tolerations are explicit:** out-of-service, not-ready, unreachable, unschedulable, and the
  pressures. Out-of-service is needed so the failover mark can be lifted; other taints keep
  their meaning.
- **The CSI node plugin does not tolerate out-of-service,** so nothing is staged on a node
  under the mark.
- **Device ids are chosen by the kernel.** Leaked ids are reclaimed at reboot. A crash's
  leftover then cannot block an attach, and no cleanup code is needed.
- **There is no dead-device cleanup,** for the reason above. User recovery will need those
  devices.
- **`hostPort` 7411 is declared,** so a successor cannot run beside its predecessor.
- **`flaminio-io` does not retry `EBUSY` on the disk.** The claim does, and a restarted
  container is restarted again by Kubernetes.
- **An `emptyDir` at `/run/ublksrvd`:** libublk writes there, and the root filesystem is
  read-only.
- **`terminationGracePeriodSeconds: 3600`** bounds the preStop wait, which is long enough to
  move a pod by hand.
- **SIGTERM:** the data plane exits at once with nothing attached or attaching. Otherwise it
  exits after the next detach. Pod deletions then end in seconds, and never stop a served
  volume early.
- **The handler lives in `flaminio-uring`,** which allows `unsafe`, unlike `flaminio-io`.
  `should_exit` is a pure function, so it can be tested.
- **`status` reports an attach in progress.** An attach that is queued or running counts as
  attached for the upgrade, the preStop hook and SIGTERM.
- **Attaching lasts until the main thread ends the attach,** not until the caller's limit. A
  late attach must not look like none.
- **One attach at a time, checked and set under one lock** (`begin_attach`), for the socket
  and the startup attach alike. Only the main thread ends an attach. Concurrent attaches would
  otherwise both be sent.
- **`ok attaching VOLUME`** is told apart from an attached answer, whose second word starts
  with `/dev/`. An old CSI plugin answers `ResourceExhausted`, which kubelet retries. No volume
  is named `attaching` (`pvc-<uuid>`).
- **SIGTERM is blocked, and taken by a `sigwait` thread,** so no ublk control command is
  interrupted.
  - `install()` runs first in `node::main`, not in `main.rs`: the other subcommands, `serve`
    among them, keep SIGTERM's default.
  - Its test is a harness-less test binary, as libtest's threads would take the signal.
- **`status --none` passes when no socket answers,** so a preStop hook does not wait 3600 s for
  a data plane that never came up.
- **`k8s-down` and `k8s-uninstall` ask the socket over ssh, not through Kubernetes.** They
  refuse only on an answer naming an attach, and `FORCE=1` skips the check.
  - With Kubernetes down, a live data plane still blocks them.
  - A dead data plane does not.
- **`detach` while attaching answers an error,** which the plugin maps to `Unavailable`. It does
  not wait for the attach to end: that is simpler, and kubelet retries.
- **The units' `up` refuse on a pod node by its pod sandbox** (`crictl pods`), not by the
  socket. A stopped data plane must still block them.
- **The units' `down` deletes only a device its own active unit serves,** and refuses where the
  socket answers. They used to delete ids 1 and 0 whatever owned them.
- **The kernel checks are those of ADR 0003,** `io_uring_disabled` must be 0 included. The ADR
  stays as it is.
- **`hostPID` is false.** The kernel checks the ublk server's pid in its own namespace. The
  fallback is `hostPID: true`.

## Upgrade

- **It is an ordered upgrade, with `OnDelete` and `make node-upgrade`, not `RollingUpdate`.**
  `RollingUpdate` ignores attached volumes, and counts a pod available before resync.
- **The steps:** cordon, checks, evict, a bounded wait for nothing attached, checks again
  without waiting, delete, a bounded wait for Ready and full ISRs, uncordon. It is simple, and
  a rerun starts over.
- **No hold, no instance id, no nonce, no annotations.** Their machinery carried most of the
  findings. The bounded waits and the re-check are enough for the lab.
- **The upgrade uncordons only if it cordoned.** An administrator's cordon survives.
- **A refusal after the eviction leaves the node cordoned,** and prints the
  `kubectl uncordon` to run. A rerun does not uncordon what it did not cordon.
- **Step 5's re-checks:**
  - `status --none`;
  - no VolumeAttachment;
  - ISRs full, and `minIsr`;
  - the same pod UID and revision.

  The delete carries a UID precondition. This closes the gap between the check and the
  deletion, without a hold.
- **The checks:** RF 1, every ISR full, ISR − 1 ≥ `minIsr`, and room for an evicted pod. They
  are what an eviction or a restart needs. All of them cover served volumes only: an unserved
  volume has no I/O to lose, and unserved RF 1 volumes would block every upgrade.
- **Nodes without a Flaminio volume go first,** which saves moves.
- **"Needs an upgrade"** is the pod's `controller-revision-hash` against the newest revision.
  A change to the manifest alone rolls out too.
- **The image is named by a content-derived tag,** from its inputs: the binaries, the base's
  digest and `image.sh`. It is computed before the build, as the tarball's digest is not known
  yet. `ctr import` names by tag only, and an unchanged build then changes nothing.
- **The pod is deleted with a UID precondition** (`kubectl delete --raw` with `DeleteOptions`),
  not through a proxy.
- **The k3s token never goes on an installer's command line.** It goes in `config.yaml`
  (0600), written before the install.
- **`kubectl rollout status` is not used:** it fails for `OnDelete`. The deploy polls
  `observedGeneration` and `numberReady` instead.
- **No `O_EXCL` change in `flaminio-disk format`:** no rule needs it.

## Security

- **The data-plane pod is privileged** (ADR 0007). There is no privilege separation yet.
- **Pod Security:** a k3s default of `baseline`, with only `kube-system` and `flaminio-system`
  exempt. Baseline refuses every hostPath and privileged pod.
- **No admission policy at all,** neither a ValidatingAdmissionPolicy nor a PV check. The
  policy was bypassable. Baseline covers pods, but not PersistentVolumes: PV creators are an
  accepted risk.
- **The socket is unauthorized beyond its mode:** `detach` and `delete` take no token, as
  today. Whoever reaches it can write the raw disk anyway. A token on `detach` could wedge a
  volume, and the lab cleans leftovers with `delete`.
- **The CSI node plugin stores no token.**
- **`node.sh` writes token files 0600, and deletes them on `detach` and `down`.** The
  directory is 0700.
- **PKI:** four files, read-only, `type: File`. They limit mistakes; the boundary is the host.
- **The k3s agents join with a separate agent token,** not the server's (cluster-admin) token.
  It is in the full `K10<CA hash>::node:<secret>` form, which keeps the CA pinning, and its
  files are 0600.

## Accepted risks

- **A compromised data-plane pod has the node, and in the lab the cluster.** The pod nodes hold
  the token key, the meta and etcd keys, and the administrator's key on fl-a. Fixing it needs
  separate metadata nodes, and it may block any use beyond the lab.
- **The k3s server token was on the data nodes** until this change. Rotation is out of scope.
- **Capability tokens appear in `/proc/*/cmdline`** on the VMs, as today. The readers are
  root-equivalent.
- **Who reaches the socket:**
  - host root;
  - pods in `kube-system` and `flaminio-system`;
  - anyone who may create or relabel namespaces, `exec` or `attach` into `flaminio-system`
    pods, use `nodes/proxy`, update the DaemonSets, or create HelmChart objects;
  - the privileged CSI node plugin.

  All of them are administrators, in the lab.
- **The failover controller's service account may patch nodes,** including the pod-node label
  and taints. It needs to, for its taint.
- **A compromised data plane can read the kubelet's credentials** and label its node a witness,
  drawing the controllers and their admin identity. It already has the cluster's keys here.
- **The image is built on a data VM** (`flaminio-agent` on the operator's machine). A
  compromised VM can poison it. There is no registry or signing yet.
- **A deleter that shortens the grace period** bypasses the preStop wait. That is a crash under
  the volume.
- **PersistentVolumes:** a hostPath PV reaches the socket from any namespace. PVs are created
  by the cluster's administrators and by `flaminio-csi-controller`.
- **A reboot under an attached volume is a crash.** The lab's k3s has no graceful node
  shutdown.
- **Concurrent `make node-upgrade` runs** are not locked. One run at a time is documented.
- **Attaches outside Kubernetes during an upgrade** (`node.sh`, `make vol-attach`) are not
  stopped. The bounded wait and the re-check catch most; the rest is a crash risk the lab
  accepts.
