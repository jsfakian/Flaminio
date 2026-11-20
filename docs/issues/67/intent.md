# Issue #67: RWX through an NFS gateway — intent

[Issue #67](https://github.com/jsfakian/Flaminio/issues/67), from the README roadmap (phase 6)
and ARCHITECTURE §17.1 and §20.7. This change is the issue's step 1 only: the gateway itself, for
one volume, created by a lab command, and mounted by pods on two nodes. Failover on node loss
(step 2), CSI `ReadWriteMany` (step 3) and question §20.7 (step 4) come later: issue #67 stays
open after this change.

## Why

1. **A volume is served from one node, and nothing else is accepted.**
   - `checkCapabilities` (`control/csi/controller.go:104`) refuses every multi-node access mode:
     `InvalidArgument ... a volume is served from one node at a time`. `CreateVolume`,
     `ControllerPublishVolume`, `NodeStageVolume` and `NodePublishVolume` all call it
     (tested: `control/csi/csi_test.go:125`).
   - Publishing to a second node does not share the volume: it moves it. `Attach` gives the
     node the target under a new epoch (`meta/service/service.go:723`), which deposes the first
     node's target on every replica (`csi_test.go:154`). ARCHITECTURE §8.1 promises block
     semantics "for one attached target".
   - So one filesystem on several nodes needs something above the block device. The README and
     §1 rule out a distributed filesystem; §17.1 names an NFS gateway.
2. **Nothing of a gateway exists.** No NFS server, image, manifest, lab command or document
   beyond the three lines of §1, §17.1 and §20.7.
3. **What a gateway can build on works today.** A pod with a filesystem volume is all it needs
   below it.
   - The node plugin formats (ext4 only, and only a device without a signature) and mounts the
     device (`control/csi/node.go:81`).
   - The device has no volatile write cache: a completed write is durable on the whole ISR
     (`dataplane/crates/flaminio-io/src/ublk.rs:9`, `:952`). A filesystem's `fsync` that
     returns is therefore on every in-sync replica.
   - A pod that restarts, or moves, gets its volume under a new epoch (`node.go:70`,
     `controller.go:281`), so two gateways can never both write the device. `make csi-failover`
     measured a node powered off to its pod Ready elsewhere, data verified, in 126 s
     (`docs/phase1-plan.md:200`).
4. **What the block layer does not cover is the gateway's own memory.**
   - Data an NFS server has taken but not yet written back is lost with it. NFS allows that
     for unstable writes (the client resends them when the server's write verifier changes),
     but not for a `COMMIT` or a stable write it answered. An export in `async` mode answers
     before the data is on the device.
   - A client's handles, opens and locks live in the server. They survive a restart only if
     the export's identity does not depend on the node or the device number (`/dev/ublkbN`
     differs between nodes), and if the server's record of its clients is kept where the next
     gateway finds it.
5. **A node serves one attached volume** (`MaxVolumesPerNode: 1`, `node.go:36`; `one volume at
   a time`, `flaminio-io/src/control.rs:95`; issue #15). A gateway takes its node's only
   attachment. In the lab that leaves one node for a second volume, and the node the gateway
   moves to must be free.
6. **An export is the first Flaminio port that workloads are meant to use.**
   - §15.1's control against a tenant pod is "no data-plane network reachability from pods".
     No NetworkPolicy is shipped (`deploy/` has none), though §15.5 says they are.
   - NFS with `AUTH_SYS` authenticates nobody: whoever reaches the port mounts the export, as
     any uid. A capability token has no counterpart here.
   - Kubelet mounts NFS itself, for any pod whose spec names a server (`nfs:` volumes), unless
     the pod's namespace forbids them (the `restricted` Pod Security level does). An export
     that nodes can reach is then reachable by such a pod, whatever its namespace.
   - "Every connection is mutual TLS" (§2, §15.2). The link from an NFS client to the gateway
     is a new one. In the lab, traffic between the servers is inside WireGuard
     (`scripts/guest/k3s.sh:36`); elsewhere it is not.
7. **The server and its mounts are new dependencies.**
   - The images are built by appending binaries to a pinned Ubuntu base, which brings `mount`,
     `blkid` and `mkfs.ext4` only (`scripts/dev/image.sh:3`): no NFS server, no `mount.nfs`.
   - The lab VMs install no NFS package (`scripts/guest/provision.sh:10`), so kubelet's own
     NFS mount has no `mount.nfs` there.
   - The kernel NFS server needs the `nfsd` module on the host and a privileged pod. A
     user-space server (NFS-Ganesha) needs neither, and is a new C dependency. No ADR covers
     either.
   - On Ubuntu 6.8 `nfs`, `nfsv4` and `nfsd` are in the base modules package (`ublk_drv` is
     not: `provision.sh:19`). The lab's 7.0 kernel is not checked yet: `modinfo nfsv4 nfsd`
     on fl-a and fl-b.
8. **Kubernetes workloads run on two nodes only.** fl-w is tainted and has no node plugin
   (`k3s.sh:40`, `deploy/csi/driver.yaml:100`); fl-c has no kubelet. "Two pods on two nodes"
   means fl-a and fl-b, and one of them also runs the gateway.
9. **Nothing in Flaminio creates Kubernetes workloads.** The CSI controller makes no Kubernetes
   call, its role holds only what the sidecars need (`deploy/csi/rbac.yaml:26`), and `go.mod`
   has no Kubernetes client. The operator (#43) is planned; its first step creates no
   per-volume object.

## What we want

1. A gateway: one pod that mounts one ReadWriteOnce Flaminio filesystem volume and exports it
   over NFS. One gateway per volume; it serves nothing else.
2. Pods on two supported workload hosts mount the export at the same time, write the same
   tree, and each reads what the other wrote. The original fl-a/fl-b VM layout is historical
   old-baseline evidence only once #83 lands: its composed success case requires admitted
   physical workload hosts, and its excluded VMs refuse before setup (design/qualification).
3. What the gateway acknowledged as stable is on the volume. After the gateway is killed
   without warning and starts again, every such write is there, and clients go on without
   remounting: their handles stay valid, at an address that did not change.
4. The gateway keeps nothing that a restart needs outside its volume, on whichever node it
   starts.
5. Only a pod that holds the volume's claim gets the export. No pod reaches the export's port
   itself, and a pod of another namespace cannot have a node mount it on its behalf.
6. The gateway holds no Flaminio identity or key and no access to a control socket, and runs
   with the privileges written down, as few as its server allows. It never runs in a tenant's
   namespace.
7. The choice of NFS server, and whether the NFS link is encrypted, are recorded in an ADR.
8. A lab check for 2 to 5, and `docs/lab.md` saying how to run it and what it found. The
   existing checks (`make csi-test`, `make csi-failover`) pass unchanged.

## Out of scope

- **Step 2: failover of the gateway on node loss.** The block layer already fences the old
  gateway. Left: the check with the gateway's node powered off, the pause clients see (126 s
  for a pod today, against NFS's grace period and lease), reclaim of opens and locks on another
  node, and what a client that still reaches a cut-off gateway reads from its cache, which the
  replicas' lease does not cover.
- **Step 3: CSI `ReadWriteMany`.** A claim creates the backing volume and its gateway and
  deletes them; the node plugin mounts the export. `checkCapabilities` stays as it is here. It
  needs a decision on who creates gateways: the CSI controller with new Kubernetes rights, or
  the operator (#43). Whether an export counts against `MaxVolumesPerNode` is settled there.
- **Step 4: question §20.7,** whether a gateway is enough. It needs the measurements below.
- **Performance of the export:** throughput and latency against the block volume, and the cost
  of a client on the gateway's own node. Measured once step 2 is in.
- **Several volumes per node** (#15). Until then a gateway's node serves no other volume.
- **The operator** (#43), a resource for gateways, and gateways outside the lab's script.
- **NFSv3, Kerberos, per-user access rules, quotas, snapshots of an export,** and xfs (the node
  plugin formats ext4 only).
- **Clients outside Kubernetes,** and on nodes without the node plugin (fl-w).
- **The NetworkPolicies of the data plane and the metadata service** (§15.5). This step adds
  the gateway's only.
- **`nosuid` and `nodev` on the node plugin's block-volume mounts** (§15.1 names them;
  `node.go:124` and `:174` pass none). The export's mounts get them.
