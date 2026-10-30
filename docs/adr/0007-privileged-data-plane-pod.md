# 0007. A privileged data-plane pod, until privilege separation

- **Status:** Proposed
- **Date:** 2026-10-30

## Context

ARCHITECTURE §5.1 and §15.5 call for an unprivileged `flaminio-io`, with a small privileged
helper that opens the raw devices and the ublk control device and passes their descriptors.
There is no such helper yet.

Issue #12 runs the data plane in a Kubernetes pod. What binds is device access:
- the raw disk, opened with `O_DIRECT | O_EXCL`;
- `/dev/ublk-control`;
- `/dev/ublkcN` and `/dev/ublkbN`, which appear after the container starts.

The device cgroup of an unprivileged container admits only devices known when it starts.
io_uring is a smaller matter: kubelet's default seccomp profile is `Unconfined` unless
`seccompDefault` is set, and Flaminio's own profile is for later (§15.5).

## Decision

The data-plane pod (`flaminio-dataplane`), its init container included, runs privileged, with
the host's `/dev`, until privilege separation lands. The init container reads and opens the
disks to claim one.

**Rejected:**
- **Seccomp `Unconfined` with `CAP_SYS_ADMIN` and device-cgroup rules,** without privileged.
  Kubernetes has no field for device-cgroup rules. Each runtime has its own way, and the
  capability gives most of root anyway.
- **Unprivileged ublk** (`UBLK_F_UNPRIVILEGED_DEV`). The devices it adds belong to the user
  who added them, and they still need a device-cgroup rule to be opened. The lab's kernels and
  libublk usage have not been tried with it.

## Consequences

- **The node is the boundary.** A compromised data plane has the node: the host's disk, and
  every key on it. `readOnlyRootFilesystem` and the four PKI files mounted one by one guard
  against mistakes, not attackers.
- **The control socket is full control of the node's volumes.** Whoever reaches
  `/run/flaminio/node.sock` can:
  - attach with a token;
  - detach any target;
  - delete any replica not attached there;
  - hold and release attaches;
  - read the status.

  These can reach it:
  - root on the host;
  - pods in the namespaces exempt from Pod Security (`kube-system`, `flaminio-system`);
  - anyone who may create a namespace or label one `privileged`.
- **In the lab, the node is the cluster.** fl-a and fl-b run etcd and the metadata service
  besides the data plane. So the pod nodes hold `token.key`, and the `meta`, `meta-etcd` and
  `etcd` keys (`scripts/lab.sh`, `cmd_pki`), and on fl-a the administrator's key. A
  compromised data-plane pod can then:
  - mint capability tokens;
  - change the metadata;
  - read or write any volume.
- **More ways in, and around:**
  - **Host networking:** the pod shares the node's network, and holds port 7411 on it.
  - **The exempt namespaces:** whoever can create pods in `flaminio-system` or `kube-system`
    gets the node, and here the cluster.
  - **The failover controller's service account** may patch nodes (`deploy/csi/rbac.yaml`).
    It can set or remove the pod-node label, taints, and the upgrade's annotation.
  - **The image** is built on a data VM. A compromised data VM can poison it for every node.
- **Keeping the keys off the pod nodes** means running the metadata members on nodes that run
  no data plane. That is a follow-up, and it may block any use beyond the lab.
- **Expected to be superseded** by an ADR for privilege separation, which also ships the data
  plane's seccomp and AppArmor profiles.
