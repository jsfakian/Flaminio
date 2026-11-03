# Test lab

Two servers with KVM VMs, driven from a workstation with `make`. The VMs are where the data
plane is built, tested and benchmarked on Linux (see [mvp-plan.md](mvp-plan.md)). Phase 1 adds a
small witness VM and a metadata cluster (see [below](#phase-1-witness-overlay-and-metadata)).

```
 workstation ── ssh ──▶ rhythm (147.52.71.227)              rhythm-backup (147.52.71.226)
   make …                 └─ fl-a  192.168.122.50 (NAT)        └─ fl-b  192.168.122.51 (NAT)
                             8 vCPU, 16 GiB                        4 vCPU, 8 GiB
                             /dev/vdb 100 GiB on SATA SSD          /dev/vdb 100 GiB on NVMe

 replication path:  fl-a ──NAT──▶ 147.52.71.226:7410 ──DNAT──▶ fl-b:7410
                    (rhythm-backup forwards the port only for traffic from 147.52.71.227)
```

Both VMs run Ubuntu 26.04 (kernel 7.0, the same as the hosts) with `ublk` and `io_uring`
available. Both use the same libvirt definition
([`scripts/templates/libvirt-domain.xml`](../scripts/templates/libvirt-domain.xml)): q35,
host-passthrough CPU, and a virtio-blk data disk backed by a preallocated raw file with
`cache=none` and `io=native`. So the two sides differ only in hardware.

## Requirements on the workstation

- SSH aliases `rhythm` and `rhythm-backup` in `~/.ssh/config`, with key login.
- **rhythm accepts SSH only from 147.52.71.0/24.** From anywhere else (off campus, no VPN), relay
  through rhythm-backup: `export HOST_A_JUMP=rhythm-backup`, then use `make` as usual.
- **bash ≥ 4.4.** macOS ships bash 3.2: run `brew install bash`. The scripts refuse to run on
  older versions.
- Optional: `shellcheck` (used by `make lint`).

Your public key (`~/.ssh/id_rsa.pub`, overridable with `SSH_PUBKEY_FILE`) is injected into the
VMs, which are reached through the hosts with SSH `ProxyJump`. VM host keys are kept in
`.lab/known_hosts`, not in your `~/.ssh/known_hosts`.

## Quick start

```bash
make lab-check          # reachability, sudo, KVM, libvirt
make lab-up             # host-prep → vm-create → net-setup → provision → deploy → net-check
make vm-status
make vm-test            # test suite on Linux, in fl-a
make bench-raw          # fio read baseline of the raw data disks (JOBS=. adds writes)
make bench-store        # replica-store read benchmark (FORMAT=0 reuses the formatted disk)
make help               # everything else
```

Lab targets take `VM=a|b|w|all|cluster` (`all` is the two data VMs, `cluster` adds fl-w). Settings live in [`scripts/lab.conf`](../scripts/lab.conf), and
any of them can be overridden from the environment (for example `VM_A_CPUS=16 make vm-create`).

## What the scripts change on the servers

All of it is idempotent, and everything Flaminio-specific is named or tagged `flaminio-lab`.

| Step | Change | Undo |
|---|---|---|
| `host-prep` | Installs `qemu-system-x86 qemu-utils libvirt-daemon-system libvirt-clients cloud-image-utils` where missing. Adds the user to the `libvirt` and `kvm` groups. Defines and autostarts the `default` storage pool and network if needed | Remove the packages manually |
| `host-prep` | ufw: allows traffic in on `virbr0` (DHCP/DNS for VMs) and, where routed traffic is denied by default, routes in and out of `virbr0` | `sudo ufw status numbered`, then delete the `flaminio-lab` rules |
| `vm-create` | Volumes `fl-*-root.qcow2`, `fl-*-data.img`, `fl-*-seed.iso` and the cached `flaminio-resolute-base.qcow2` in the `default` pool. A DHCP reservation in the `default` network. The domain itself. `~/flaminio-lab/` | `make vm-destroy CONFIRM=yes` (keeps the base image) |
| `net-setup` | `nat PREROUTING` DNAT rules with matching `FORWARD` accept rules, each restricted to the other server's address:<br>• rhythm-backup: TCP 7410 to fl-b (replication) and UDP 51822 to fl-b's WireGuard port<br>• rhythm: UDP 51821 and 51823 to fl-a and fl-w | `make net-teardown` |
| `mesh-up` | In the VMs: `wireguard-tools`, `/etc/wireguard/wg0.{key,conf}`, `wg-quick@wg0` enabled | `make mesh-down` |
| `pki-issue` | In the VMs: keys and certificates in `/etc/flaminio/pki` (root only). On the workstation: the two lab CAs and the token key in `.lab/pki` | Delete the directories |
| `etcd-up` | In the VMs: etcd in `/usr/local/bin`, `flaminio-etcd.service`, data in `/var/lib/flaminio-etcd` | `make etcd-down`; `lab.sh etcd wipe cluster` with `CONFIRM=yes` deletes the data |
| `meta-up` | In the VMs: `/usr/local/bin/flaminio-meta`, `flaminio-meta.service` | `make meta-down` |

`net-setup` makes the tagged rules exactly the configured set: it removes every `flaminio-lab`
rule, then adds them again. The rules are **not persistent**: after a server reboots, run
`make net-setup` again. `make net-check` and `make mesh-check` tell you whether the paths work.

## Phase 1: witness, overlay and metadata

```
 rhythm                                              rhythm-backup
   fl-a 192.168.122.50  wg 10.77.0.1  ◀── UDP ──▶       fl-b 192.168.122.51  wg 10.77.0.2
   fl-w 192.168.122.52  wg 10.77.0.3  (witness)
   each VM:  etcd :2379/:2380  ·  flaminio-meta :7400   (bound to the overlay address)
```

- **fl-w** (2 vCPU, 4 GiB) is the third etcd member: the 2-node + witness topology of
  ARCHITECTURE §1.2. It is registered with the label `flaminio.io/witness`, so placement never
  puts volume data on it. It shares rhythm with fl-a, so losing rhythm loses two of three
  members: it demonstrates quorum behavior, not failure independence.
- **Overlay.** The two servers' libvirt networks are separate NATs on the same subnet. A
  WireGuard mesh (10.77.0.0/24) joins the three VMs:
  - fl-a and fl-w reach each other directly on rhythm's bridge;
  - across the servers, each VM's WireGuard port is forwarded from its host's uplink, for the
    other server's address only;
  - all inter-VM traffic of the cluster (etcd, metadata service, later Kubernetes) is
    encrypted.

  Each private key is generated in its VM and never leaves it.
- **Identities** (`make pki-issue`). Two lab CAs live on the workstation in `.lab/pki`:
  - a **workload CA** for the metadata service, the data planes and the administrator;
  - an **etcd CA** that signs only etcd members and the metadata service's etcd clients. A
    node's workload certificate can therefore never talk to etcd.

  Each VM makes its own keys (ECDSA P-256) and sends only certificate requests. Identities are
  SPIFFE IDs in the trust domain `flaminio-lab` (for example
  `spiffe://flaminio-lab/flaminio/node/fl-b/io`). Certificates last 30 days (`CERT_DAYS`); in a
  cluster the operator will issue 24-hour certificates and rotate them. The lab administrator's
  key is on fl-a. `.lab/pki` also holds the Ed25519 key that signs capability tokens.
- **etcd** 3.7.2: the official release, checked against its published SHA-256 sums. It runs as
  one member per VM, on the overlay address only, with TLS 1.3 and client certificates
  required from peers and clients.
- **flaminio-meta** runs on every VM against all three etcd members:
  - it requires mutual TLS and checks each method's caller rule (`meta.proto`);
  - `Attach` issues signed capability tokens;
  - it is a sandboxed systemd service with a dynamic, unprivileged user, and gets its keys as
    systemd credentials;
  - it is built on the workstation (`GOOS=linux`) and copied in by `make meta-up`.

```bash
make cluster-up      # fl-w, port forwards, overlay, identities, etcd and flaminio-meta
make mesh-check      # ping every VM pair over the overlay
make etcd-status     # one line per member: leader, raft term and index
make meta-smoke      # register nodes, create and attach a volume, lose one then two etcd members
```

`make meta-smoke` (2026-10-09):
1. Registers the three nodes, with fl-w as a witness.
2. Creates a 1 GiB volume (RF 2, 256 MiB chunks), with every replica on fl-a and fl-b.
3. Attaches it through fl-b's service (epoch first+1; a volume starts at its first epoch, the
   revision of its claim: #30).
4. Stops etcd on fl-w and attaches again: it works (epoch first+2).
5. Stops etcd on fl-b as well: the service refuses, without quorum.
6. Restarts both members: the state is intact (epoch first+2).
7. Checks refusals:
   - a plain-text client;
   - a node identity creating a volume;
   - etcd without a client certificate;
   - etcd with a node's workload certificate.

### Phase-1 nodes and a replicated volume

```bash
make deploy                       # build flaminio-io on fl-a and fl-b
FORMAT=1 make node-up             # both nodes serve replicas (FORMAT=1 reformats the data disks)
make vol-create                   # vol1: 16 GiB, RF 2, placed by the metadata service
make vol-attach VM=a              # fl-a becomes vol1's target: /dev/ublkb1
make vol-show                     # epoch, target, ISR per chunk
make node-fault                   # verified writes through a replica restart, then failover
```

A node is `flaminio-io node` as the systemd unit `flaminio-node`:
- it replicates on port 7411 of the overlay, over mutual TLS with the identities in
  `/etc/flaminio/pki`;
- a volume target serves ublk id 1 (`/dev/ublkb1`), so it never collides with the MVP
  commands, which use id 0;
- `vol attach` works in three steps:
  1. It detaches the volume from the other data VMs.
  2. It asks the metadata service, as the lab administrator, for the attach.
  3. It hands the capability token to the running node through its control socket,
     `/run/flaminio/node.sock`, with the line `attach VOLUME TOKEN-BASE64`.

  Nothing restarts, so the replicas keep their state.

### A third data node, and drains (step 6)

```bash
make data-node-up                 # fl-c: VM, overlay, identities, build; its disk claimed by the agent
make agent-disks VM=c             # every device of a VM, with the claim policy's verdict
make drain-test                   # drain fl-b into fl-c under a verified write load; back again
```

**fl-c** is a data VM on rhythm, beside fl-a and fl-w: 4 vCPU, 8 GiB, a 100 GiB data disk. It
gives drains a destination; it adds no failure independence. It is on the overlay, but it is
not an etcd member.

**The node agent** (`flaminio-agent`, [`control/agent`](../control/agent)) finds the node's
block devices and applies the claim policy:
- it **refuses** anything read-only, removable, virtual (loop, device mapper, ublk…), mounted,
  partitioned, in use by another device, smaller than 1 GiB, or not matching the serial
  selector (`data` in the lab);
- it **refuses any device that holds anything**: a signature lsblk or blkid knows
  (filesystems, partition tables, RAID, LVM), or any non-zero byte in the first or last MiB;
- a device with a Flaminio superblock is **ours**, used as it is;
- otherwise the one blank device is formatted, and the node starts on it.

**Drains** (`flaminio-meta drain <node>`, [`control/drain`](../control/drain)):
1. The node is marked draining, so no new volume is placed on it.
2. A move is requested for each of its chunk replicas, to the least loaded node that may take
   it.
3. Each volume's target carries the moves out, under its epoch: it adds the destination,
   resyncs it, and drops the source once the destination is in sync
   ([protocol](replication-protocol.md) §7, moves).

The command waits until the node holds nothing. A volume that is not attached has no target to
move it, and waits for its next attach.

**`make drain-test`:**
1. fio writes 4 GiB of verified data to vol1 on fl-a while fl-b is drained into fl-c.
2. fl-a's and fl-b's nodes stop, vol1 fails over to fl-c, and fio verifies every byte there,
   from fl-c's replicas alone.
3. Everything restarts, and fl-c is drained back into fl-b, so vol1 is back on fl-a and fl-b.

A drained node's chunks stay on its disk until replica garbage collection exists
([follow-ups](followups.md) item 8).

### Deleting volumes, and fl-c's tombstones

```bash
make meta-up                              # flaminio-meta with the tombstones and confirm commands
make vol-delete                           # vol1, and its replicas on every data node
scripts/lab.sh vol-confirm wbench fl-c    # confirm a deletion for fl-c by hand, after checks there
```

A deleted volume leaves a tombstone that names the nodes its chunks were on. Each of them
confirms once it has deleted its replicas; the last confirmation removes the tombstone, and until
then the volume's id cannot be created again. There is exactly one confirmer per node:
- **fl-a and fl-b:** the CSI node plugin's sweep (`make csi-deploy`), every 5 s;
- **fl-c,** which is not a Kubernetes node: an operator, through `lab.sh vol-confirm`, the only
  supported way to confirm by hand.

The harness never confirms. `make vol-delete`, and the write campaign's deletions (wbench at
every reset, vol1 at its start):
1. detach the volume on every data node, and delete it from the metadata service;
2. start every data node that is not running, and delete the volume's replicas on each. The
   campaign's node starts with its store formatted (the campaign formats it anyway); any other
   node as it is;
3. stop at once when the volume is pending for fl-c, with the procedure: `vol-confirm`, then the
   interrupted command again (`RESUME=<dir> make bench-writes`, or `make vol-delete`);
4. otherwise wait, at most `TOMBSTONE_WAIT` seconds (120), for the sweeps to remove the
   tombstone.

So they need the new `flaminio-meta` (`make meta-up`), fl-c's node (`make data-node-up` once,
then `make node-up VM=c`), and the CSI node plugins on fl-a and fl-b (`make csi-deploy`). A
campaign of RF 1 on fl-b never stops for fl-c. One that places wbench or vol1 on fl-c (RF 2, or
`NODE=c`) stops at every deletion until an operator runs `vol-confirm`. A campaign also stops at
its start when a volume other than wbench and vol1 has a replica on its node, whose disk every
reset formats.

fl-c counts as a data node only when the checkout that runs the lab has `.lab/fl-c/created`,
which `make data-node-up` writes. A new checkout or worktree of a lab where fl-c exists needs it
written by hand (`mkdir -p .lab/fl-c && touch .lab/fl-c/created`): without it, deletions skip
fl-c, and `vol-confirm` refuses it.

**`lab.sh vol-confirm VOLUME NODE`,** run with no campaign or `vol` command running, confirms
only when all of these hold, checked in this order:
1. NODE is one data node, not a metadata cluster member;
2. NODE's tombstones list the volume (otherwise there is nothing to do);
3. nothing on NODE can sweep: no CSI plugin, no node agent, no k3s;
4. the volume is attached nowhere, and no stopped node's data plane still runs;
5. NODE's data plane is its `flaminio-node` unit, on the disk the claim policy calls ours; then
   its replicas of the volume are deleted;
6. the volume is still attached nowhere, and a second delete finds no replica;
7. NODE's own agent confirms, and finds the volume still pending.

A wrong confirmation is worse than a blocked id: a node confirmed while it still holds replicas
keeps them. A volume created again with that name drops them at its first session there
([#30](https://github.com/jsfakian/Flaminio/issues/30)); until then they hold space, and the old
volume's tokens still open sessions on them. A volume created by an old metadata service (a
rolling upgrade, a downgrade) is still deposed by them.

`flaminio-meta tombstones [node]` lists the tombstones pending for one node, or for every
registered node. `flaminio-meta confirm VOLUME NODE --replicas-deleted` is `vol-confirm`'s last
step: it refuses any caller but NODE's own agent, before any call. The metadata service logs
every new confirmation: who, from where, with which certificate.

### Partition test (step 5)

```bash
make partition-test               # HOLD=30: seconds the partition lasts after fl-b took over
```

It needs the nodes and vol1 from the section above, and the administrator identity on fl-w
(`make pki-issue VM=w`; the test issues it if it is missing). Starting nodes that are down
is part of the test; running nodes are left alone. The test:

1. Makes fl-a the target, waits for full ISRs, and writes `gen-1` at 1 GiB.
2. Starts two probes on fl-a's `/dev/ublkb1`, one O_DIRECT 4 KiB operation at a time, each
   logged with its start and end time:
   - a reader at 1 GiB;
   - a writer of `writer-1`, `writer-2`, … at 2 GiB.
3. Cuts fl-a off the overlay with iptables on `wg0` (both ways; SSH is unaffected). fl-a keeps
   its local etcd member and metadata service, which have no quorum alone.
4. From fl-w, as the administrator, attaches vol1 to fl-b. fl-b must:
   - read `gen-1`;
   - read at 2 GiB the last write fl-a acknowledged, or the one in flight at the cut;
   - write `gen-2` and read it back.
5. Heals the overlay after `HOLD` seconds. fl-a must be told it was deposed, and then rejoin
   every ISR as a replica.

Checks, each on one VM's clock:
- no read on fl-a that started 5 s (the lease) or more after the cut succeeded;
- no write on fl-a that started after the cut was acknowledged.

The probe logs are kept in `.lab/partition-{reader,writer}.log`. Afterwards vol1's target is
fl-b; `make vol-attach VM=a` moves it back.

## Investor / colleague demo (Mac control plane)

For a talk with the k3s server on this Mac (Lima guest, NoSchedule) and workers on fl-a / fl-b,
see [demo.md](demo.md) and [demo-competitors.md](demo-competitors.md):

```bash
export DEMO_CP_SRC_IP=…           # this Mac as the lab hosts see it
make demo-up && make csi-deploy && make demo-obs && make demo
```

The day-to-day lab control plane remains fl-w (`make k8s-up` below). `make demo-up` moves the
API to the Mac; `RESTORE_LAB_K8S=1 make demo-down` puts it back.

## Kubernetes (k3s)

```bash
make k8s-up                       # server on fl-w, agents on fl-a and fl-b (K3S_VERSION in lab.conf)
make k8s-status                   # k3s per VM, nodes, pods
make k8s-smoke                    # cross-node pod traffic, DNS, services, Pod Security, witness taint
make kubectl ARGS='get nodes -o wide'
make k8s-down                     # stop k3s and its pods (state kept; k8s-up starts it again)
CONFIRM=yes make k8s-uninstall    # remove k3s and its state
```

Requires the overlay (`make mesh-up`). Layout:
- **Server (control plane) on the witness fl-w.** It is tainted `NoSchedule` and labelled
  `flaminio.io/witness=true`, so no workload, CSI node plugin included, runs where there is no
  volume data. It uses k3s's SQLite datastore, which is separate from the metadata etcd.
- **Agents on fl-a and fl-b**, labelled `flaminio.io/storage=true`.
- **Addresses:** every node address is its overlay address. The API server listens only on
  10.77.0.3:6443, and flannel's VXLAN runs over `wg0`, so pod traffic between the servers is
  encrypted.
- **Disabled:** Traefik, ServiceLB and local-path storage. Storage will be Flaminio's.
- **Secrets** are encrypted at rest (`secrets-encryption`). The kubeconfig is readable by root
  only.
- **Version:** k3s is pinned (`K3S_VERSION`), installed by that release's own install script,
  which verifies the binary's checksum.

`make kubectl` runs kubectl on fl-w and needs nothing on the workstation. For a local kubectl:

```bash
make k8s-tunnel                   # in one terminal: 127.0.0.1:16443 -> fl-w's API server
export KUBECONFIG=$PWD/.lab/kubeconfig
kubectl get nodes
```

### CSI driver

```bash
make node-up                      # the data plane on fl-a and fl-b (outside Kubernetes, for now)
make csi-deploy                   # build the image, import it on every VM, apply deploy/csi
make csi-status                   # pods, StorageClass, PVs, VolumeAttachments
make csi-test                     # a PVC is written on one node, then verified on the other
make csi-failover                 # a data VM powered off: the pod comes back on the other node
make csi-logs
```

The driver is one binary, `flaminio-csi` ([`control/csi`](../control/csi),
[`control/failover`](../control/failover)), in three roles:
- **Controller** (a Deployment on fl-w). It creates, deletes and publishes volumes in the
  metadata service as the identity `.../admin/csi-controller`. Its key is made on fl-w and
  kept in a Secret.
  - Publishing makes the pod's node the volume's target; publishing again to the current
    target changes nothing.
  - Unpublishing does nothing: the next attach fences the old target.
- **Node plugin** (a DaemonSet on fl-a and fl-b). For NodeStage it:
  1. asks the metadata service for a new epoch for its own node, as the node's agent
     identity, which may do only that;
  2. hands the token to `flaminio-io node` through its control socket;
  3. formats the device with ext4 only if it carries no signature at all;
  4. mounts it.

  A node serves one volume at a time (`MaxVolumesPerNode: 1`), so `csi-test` first detaches
  the lab volume vol1.
- **Failover controller** (a Deployment on fl-w). It sets the standard
  `node.kubernetes.io/out-of-service` taint on a data node only when both signals agree:
  - Kubernetes has seen the node not Ready for 40 s;
  - the node's data plane has stopped heartbeating to the metadata service.

  Kubernetes then deletes the node's pods and detaches their volumes, and they start on the
  other node. The controller removes its taint once the node is back. Fencing is what makes
  this safe for the data, whatever the lost node still does (see `make partition-test`).

The StorageClass `flaminio` takes `replicas` (default 2), `minIsr` (1), `chunkSize` (256Mi),
`failureDomain` (`node`|`rack`|`zone`) and `failureDomainPolicy` (`strict`|`bestEffort`).
Label storage nodes with `topology.kubernetes.io/zone` and `flaminio.io/rack` so zone/rack
placement has distinct domains (see docs/followups.md §4). Volumes are `ReadWriteOnce` or
`ReadWriteOncePod`, as a filesystem or a raw block device. The image is built without a
container engine (`crane`, on a pinned Ubuntu base) and imported into each node's containerd;
nothing is pushed to a registry.

### Phase-1 benchmark and chaos run (step 8)

```bash
make bench-pod                    # NODE=fl-b RUNTIME=30: reads from a pod, against the raw disk
make chaos                        # ROUNDS=4: faults under a verified write load, then a crash
```

**`make bench-pod`.** A pod on `NODE` gets a 16 GiB Flaminio volume as a raw block device.
1. With topology (`WaitForFirstConsumer`), the CSI controller learns the pod's node, and the
   metadata service puts a replica of every chunk there, so reads are local.
2. The pod writes the whole volume once (never-written blocks would read as zeros), then runs
   the read jobs of `make bench-raw`.
3. The same jobs then read the node's raw data disk, read-only.

The table shows both, and the pod's share of raw. fio in the pod is built statically from the
`FIO_VERSION` tag of fio's repository (in fl-a), on the same pinned base image as the CSI
driver.

**`make chaos`.** `ROUNDS` rounds, alternating the target between fl-a and fl-b. Each round:
1. fio writes 4 GiB of verified data on the target.
2. Meanwhile, random faults hit the rest, one after another, until the load ends:
   - a restart of the replica's node;
   - the replica cut off the overlay for 15 s;
   - fl-w's etcd member and metadata service down for 20 s;
   - 100 ms added to every packet of the replica's for 20 s.

   The load must finish with no errors.
3. Once every ISR is full again, the target's node is killed with SIGKILL, like a crash. The
   volume fails over to the other node, and fio verifies every byte there.

## Running the tests yourself

| Command | Where | Duration | What it checks |
|---|---|---|---|
| `make test` | workstation | 2 min | Rust and Go unit and integration tests |
| `make sim` | workstation | 5 min | 20,000 simulated runs of the replication protocol (`SEEDS`), then every injected bug must be caught |
| `make meta-interop` | workstation | 1 min | the Rust metadata client against the Go service, over mutual TLS |
| `make check` | workstation | 10 min | all of the above, plus `make lint` |
| `make vm-test` | fl-a | 5 min | the test suite on Linux, including io_uring and ublk |
| `make meta-smoke` | lab | 2 min | metadata through etcd member loss; refusal of unauthenticated callers |
| `make node-fault` | lab | 5–10 min | verified writes through a replica restart; ISR restored; failover |
| `make partition-test` | lab | 3–5 min | the old target, cut off, serves no stale reads and loses no acknowledged write |
| `make bench-pod` | lab | 10 min | reads from a pod with a Flaminio volume against the raw disk |
| `make bench-reads` | lab | 15 min per run | `VOL` (vol1) against raw by the [performance plan](performance-plan.md)'s rules: raw reads where the volume's data is on the disk, found by tracing a read of it (bpftrace); ends with fio's check of every block. `MODE=zero-copy\|verify`, `QUEUES`, `POLL_US`, `NUMJOBS` |
| `make chaos` | lab | 20–30 min | random faults under a verified write load; each round ends with a crashed target and a verified failover |
| `make drain-test` | lab | 15–25 min | a node drained under a verified write load; the destination then serves every byte alone (needs `make data-node-up`) |
| `make k8s-smoke` | lab | 1 min | Kubernetes over the overlay |
| `make csi-test` | lab | 3 min | a PVC provisioned, formatted, written, moved to the other node, verified |
| `make csi-failover` | lab | 10 min | a data VM powered off: the failover controller marks it, the pod moves with its data; the VM comes back and rejoins |
| `make lab-test` | lab | about 30 min | every lab test above, in order |

From an empty lab, before the lab tests:

```bash
make lab-up && make cluster-up             # VMs, overlay, identities, etcd, metadata service
make deploy && FORMAT=1 make node-up       # nodes on fl-a and fl-b
make vol-create && make vol-attach VM=a    # the test volume
make k8s-up && make csi-deploy             # Kubernetes and the CSI driver
```

## Long-running jobs

`bench-raw` and `bench-store` run inside the VM as a transient systemd unit. The workstation
only polls for output, so a dropped SSH connection (laptop sleep, network change) does not kill
the run. If the connection drops anyway, rerun the target, or read the log in the VM at
`~/.flaminio-lab/<unit>.log`.

## Health checks

- `make vm-status` runs a trivial process on **every vCPU** of each VM and reports any vCPU
  where processes fail, plus whether CET is on.
- `make vm-console-log VM=a` shows a VM's console (kernel messages, panics), which is logged on
  its host in `/var/log/libvirt/qemu/<vm>-console.log`.
- `make vm-restart VM=b FORCE=1` hard-resets a hung guest.

## Known issues

### 2026-10-08: CET (shadow stack) failures in guests

**Symptoms.** About an hour after boot, every user process scheduled on **vCPU 2** of fl-a was
killed with `segfault at 7726019fffb0 ... error 46`, while the other seven vCPUs were fine
(`taskset -c 2 /bin/true` failed 5 times out of 5; every other vCPU succeeded 5 out of 5). Page-fault error
`0x46` is `WRITE | USER | SHSTK`: a shadow-stack write. The fault address was the same in every
process, including processes that had never enabled shadow stacks, which points to a stale
shadow-stack pointer on that vCPU rather than to anything in the processes or on disk. In the
same period fl-b hung: vCPU 0 spinning in the guest kernel (privilege level 0, not halted, interrupts
enabled), with no console log to say why.

**Environment.** Host and guest kernels 7.0.0-34. Host-passthrough CPUs (Xeon 6527P and Xeon
E-2436, both with CET). Both guests reported `user_shstk`.

**Workaround (lab only).** The guests boot with `clearcpuid=user_shstk ibt=off`
(`GUEST_KERNEL_ARGS` in `scripts/lab.conf`, applied by `make provision`), and consoles are now
logged so a recurrence leaves evidence. This disables a security feature **inside the lab VMs
only**. It is a workaround for KVM/CET on these kernels, not a product decision. Before
re-enabling it (`GUEST_KERNEL_ARGS=""`), retest with newer host kernels and QEMU, and consider
reporting upstream with the evidence above. The failing processes were unrelated programs
(`systemd`, `sshd`, `lsblk`, `dash`). The first fault (uptime 3304 s) came about 20 minutes into
a store benchmark run, so the benchmark load may have triggered the bug. It is not known
whether it did. Per-vCPU CPU state is kernel/KVM territory, which a userspace program cannot
corrupt without a kernel bug.

## Security notes

- The replication port is reachable only from rhythm's address. This was checked: a
  connection from elsewhere does not get through.
- VMs accept SSH key logins only (no passwords). The `ubuntu` user has passwordless sudo inside
  the VM.
- The VMs sit on each host's NAT network, so nothing else on the campus network can reach them.
- Overlay traffic between the VMs is encrypted by WireGuard, and etcd and the metadata
  service add mutual TLS on top. The forwarded UDP ports accept only the other server's
  address.
- Replication traffic in the MVP is **not yet encrypted**. mTLS comes with the replication
  protocol (ARCHITECTURE §15.2). Until then, don't put real data in the lab.

## Troubleshooting

| Symptom | Check |
|---|---|
| `bash >= 4.4 required` | `brew install bash`, and make sure `/opt/homebrew/bin` comes before `/bin` in `PATH` |
| `make net-check` fails | `scripts/lab.sh net-setup` again. Then `ENV_VARS=… scripts/host/net-forward.sh status` on rhythm-backup, or simply look for `flaminio-lab` in `sudo iptables -S FORWARD` |
| VM has no network | `make vm-console VM=a` (exit with `Ctrl-]`), and check `sudo ufw status` on the host |
| VM unreachable after it was recreated | It got a new host key. `make vm-destroy` cleans `.lab/known_hosts`; otherwise run `ssh-keygen -R <ip> -f .lab/known_hosts` |
