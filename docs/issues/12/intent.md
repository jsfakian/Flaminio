# Issue #12: the data plane in Kubernetes — intent

[Issue #12](https://github.com/jsfakian/Flaminio/issues/12), follow-ups §9.

## Why

1. **The data plane runs beside Kubernetes, not in it.**
   - `make node-up` (`scripts/lab.sh`, `cmd_node`) runs `scripts/guest/node.sh up` in each data
     VM: `flaminio-io node` as the transient systemd unit `flaminio-node` (`systemd-run`,
     `Restart=no`). Its binary is built in the VM by `make deploy` (`scripts/guest/build.sh`).
   - Its flags come from `node_env`: the disk whose serial matches `data` (`common.sh`,
     `data_disk`), `--listen` on the overlay, `--meta`, `--pki /etc/flaminio/pki`,
     `--trust-domain`, `--id 1`, and the benchmarks' settings. Only fl-c's disk is claimed by
     the node agent (`cmd_data_node_up`).
   - `ublk_drv` already loads at boot. `scripts/templates/user-data.yaml` writes
     `/etc/modules-load.d/flaminio.conf`.
   - `make csi-deploy` needs `make node-up`. So does `make csi-failover`, which runs
     `cmd_node up` once the lost VM is back. So do the benchmarks and checks (`ensure_nodes`).
   - `node.sh attach` refuses unless the unit runs, and `node.sh delete` does nothing without
     it.
   - Only `node.sh up` with `ATTACH` writes a token file: in `/run/flaminio`, which it makes
     0700, with `sudo tee`, so 0644.
   - The token reaches `node.sh` and its Python on command lines (`lab.sh`, `vol attach`;
     `node.sh`, `control`), which `/proc/*/cmdline` shows.
2. **The node plugin already reaches the socket through the host.**
   - `deploy/csi/driver.yaml` mounts the host's `/run/flaminio` into the node plugin. The plugin
     sends its commands to `/run/flaminio/node.sock` (`control/csi/dataplane.go`).
   - The socket is mode 0600 (`control.rs`, `spawn`). Its directory is made 0755, by
     `create_dir_all` and by `DirectoryOrCreate`.
   - The lab's namespaces have no Pod Security default. The labelled ones:
     - `flaminio-test` and `flaminio-smoke` are `restricted`;
     - `flaminio-bench` is `baseline`;
     - `flaminio-system` is `privileged` (`rbac.yaml`).
   - Only `attach` needs a token. `detach` stops any target, and `delete VOLUME` deletes the
     node's replicas of any volume not attached there (`engine.rs`, `DeleteReplicas`). The lab
     uses `delete` to clean what moves and wiped metadata leave (`lab.sh`, `delete_wbench`).
3. **The node agent is two things today.**
   - `flaminio-agent` (`control/cmd/flaminio-agent`) is a command: `disks` and `claim`, the
     claim policy, and the format of a blank disk. Its `--serial` has no default.
   - `flaminio-disk format` opens the disk without `O_EXCL` (`flaminio-disktool`, `format`).
   - The CSI node plugin does the rest, "the node's agent for now" (`flaminio-csi`, `main.go`):
     - it takes each attach's epoch as `.../node/<node>/agent`;
     - it copies zone and rack;
     - it sweeps tombstones.
4. **A restart of the data plane ends the volume attached on that node.**
   - The ublk device is added without `UBLK_F_USER_RECOVERY` (`ublk.rs`, `add_device`).
     ARCHITECTURE §5.1, §11.1 and §18 and ADR 0003 count on it.
   - So the device's I/O fails when the process ends. The node plugin attaches only in
     NodeStageVolume (`node.go`, `device`).
   - On SIGKILL, libublk's `Drop` does not run (`ctrl.rs`), so the device is not deleted. With
     a fixed `--id`, the next attach waits for that id.
   - libublk writes each device's file under `/run/ublksrvd` (`ctrl.rs`, `flush_json`).
   - The data plane has no signal handler.
   - Replicas rejoin by resync after a restart (`make node-fault`, `make chaos`).
5. **The failover controller's mark needs the data plane.**
   - `control/failover` sets `node.kubernetes.io/out-of-service:NoExecute` on a lost node.
   - It removes the mark only once the node's data plane heartbeats again.
   - A data-plane pod that does not tolerate the mark would never start, so the mark would
     never go.
   - Its service account may patch nodes (`deploy/csi/rbac.yaml`).
6. **A target cannot move without its pod.**
   - ControllerPublishVolume makes the pod's node the target (`controller.go`).
   - `control/drain` moves replicas, and only of attached volumes (#17).
   - A node serves one volume at a time (`MaxVolumesPerNode: 1`; #15).
7. **The data plane has no image.**
   - `scripts/dev/image.sh` builds the CSI image without an engine, on an Ubuntu base pinned by
     tag. `make csi-deploy` imports it into each node's containerd. There is no registry.
   - The data plane's binaries are built only in the data VMs (`resolute`, as the base).
8. **The lab's data nodes hold the cluster's keys.**
   - fl-a and fl-b run etcd and the metadata service, as well as the data plane.
   - `cmd_pki` (`lab.sh`) puts on them:
     - the token key;
     - the `meta`, `meta-etcd` and `etcd` keys;
     - on fl-a, the administrator's key.
   - A privileged pod with the host's `/dev` can read them all.
   - The k3s agents on fl-a and fl-b join with the server's full token (`cmd_k8s`, `up`;
     `k3s.sh`, `agent`). It is passed on the ssh command line.

## What we want

1. On a Kubernetes data node, the node agent and the data plane run in one DaemonSet pod,
   `flaminio-dataplane`. Kubernetes starts it, also after a reboot or under the out-of-service
   mark.
2. The CSI node plugin stays a DaemonSet of its own, so that its upgrades do not restart the
   data plane. It reaches the data plane through the same socket.
3. An ordered upgrade, one node at a time:
   - it never stops a data plane under an attached volume;
   - it never takes a chunk's ISR below its minimum;
   - it goes on only once the node's replicas are back in every ISR.

   Other deletions of the pod wait, within a bound, while a volume is attached.
4. `make node-up` is not needed in the Kubernetes lab. The Mac demo is unchanged.
5. Nodes outside Kubernetes (fl-c) keep the systemd unit, and so do the lab's benchmarks.

**What "keeps a pod with a PVC running" means here** (decided):
- **While other nodes upgrade:** the pod runs on with no I/O error. Their replicas rejoin by
  resync.
- **When its own node upgrades:** the pod is evicted, and starts on another node with its
  data. This needs a node with no Flaminio volume (#15). #17 is not needed.
- **The same pod through its own node's upgrade** needs ublk user recovery (out of scope).

**Blast radius, for the spec gate.**
- The data-plane pod is privileged (ADR 0007, proposed, which this change adds).
- In this lab, its nodes hold the cluster's keys (Why, 8). So a compromised data-plane pod
  reaches every volume's data and the metadata, not one node.
- Whoever reaches the control socket controls the node's volumes. ADR 0007 lists who can.
- Keeping the keys off the pod nodes means separating the metadata members from the data
  nodes, which is a follow-up. It may block any use beyond the lab.

## Out of scope

- **ublk user recovery**, for a new issue:
  - **What:** a data plane that restarts under an attached volume, which the volume's pod sees
    as a pause. The device is added with `UBLK_F_USER_RECOVERY`, and the new process recovers
    it.
  - **The epoch:** a target never restarts under the epoch it had, and only the node's agent
    identity may take a new one (`node.go`, `device`). The recovered target must take one
    through that identity, not with `io`.
  - **The device:** its id and libublk's file must be kept in a host directory. Here,
    `/run/ublksrvd` is the pod's own.
  - **Done when:** an upgrade of the pod's own node keeps the pod, with a verified write load
    and no I/O error.
- **Separating metadata members from data nodes**, and the keys with them (Blast radius).
- **Privilege separation** (ARCHITECTURE §5.1, §15.5), and seccomp and AppArmor profiles.
  ADR 0007 records the deviation.
- **The operator** (phase 1 step 7+):
  - ordering upgrades by itself;
  - the storage disruption guard (§10.4);
  - the CRDs.
- **Formatting in the pod:** the pod only uses an existing Flaminio disk.
- **Certificates:** short-lived and rotated is #16.
- **Refusing a re-attach at the same epoch,** so that a stolen token cannot be replayed: issue
  #15's token check.
- **Several volumes per node** (#15), **draining unattached volumes** (#17), **the agent's
  sweeps** (#13, #14).
- **A registry, signed images and reproducible builds** (§15.6).
- **CPU pinning and resource requests.**
- **Rotating the k3s server token** that the agents held until now.
- **Running the benchmarks and checks in the pod.** They keep the unit, which needs
  `make csi-undeploy` first.
- **The Mac demo** (`make demo-up`): it keeps `make node-up`.
