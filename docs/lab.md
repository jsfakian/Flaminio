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
- **etcd** 3.7.2: the official release, checked against its published SHA-256 sums. It runs as
  one member per VM, listening on the overlay address only. Not yet TLS between members or
  clients: that is phase 1 step 3. The overlay already encrypts the traffic.
- **flaminio-meta** runs on every VM against all three etcd members. It is a sandboxed systemd
  service with a dynamic, unprivileged user. It is built on the workstation
  (`GOOS=linux`) and copied in by `make meta-up`.

```bash
make cluster-up      # fl-w, port forwards, overlay, etcd and flaminio-meta on all three
make mesh-check      # ping every VM pair over the overlay
make etcd-status     # one line per member: leader, raft term and index
make meta-smoke      # register nodes, create and attach a volume, lose one then two etcd members
```

`make meta-smoke` (2026-10-09):
1. Registers the three nodes, with fl-w as a witness.
2. Creates a 1 GiB volume (RF 2, 256 MiB chunks), with every replica on fl-a and fl-b.
3. Attaches it through fl-b's service (epoch 1).
4. Stops etcd on fl-w and attaches again: it works (epoch 2).
5. Stops etcd on fl-b as well: the service refuses, without quorum.
6. Restarts both members: the state is intact (epoch 2).

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
- Overlay traffic between the VMs (etcd, the metadata service) is encrypted by WireGuard. The
  forwarded UDP ports accept only the other server's address.
- Replication traffic in the MVP is **not yet encrypted**. mTLS comes with the replication
  protocol (ARCHITECTURE §15.2). Until then, don't put real data in the lab.

## Troubleshooting

| Symptom | Check |
|---|---|
| `bash >= 4.4 required` | `brew install bash`, and make sure `/opt/homebrew/bin` comes before `/bin` in `PATH` |
| `make net-check` fails | `scripts/lab.sh net-setup` again. Then `ENV_VARS=… scripts/host/net-forward.sh status` on rhythm-backup, or simply look for `flaminio-lab` in `sudo iptables -S FORWARD` |
| VM has no network | `make vm-console VM=a` (exit with `Ctrl-]`), and check `sudo ufw status` on the host |
| VM unreachable after it was recreated | It got a new host key. `make vm-destroy` cleans `.lab/known_hosts`; otherwise run `ssh-keygen -R <ip> -f .lab/known_hosts` |
