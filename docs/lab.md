# Test lab

Two servers, one KVM VM on each, driven from a workstation with `make`. The VMs are where the
data plane is built, tested and benchmarked on Linux (see [mvp-plan.md](mvp-plan.md)).

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
make bench-raw          # fio baseline of the raw data disks (destroys data on them)
make help               # everything else
```

Lab targets take `VM=a|b|all`. Settings live in [`scripts/lab.conf`](../scripts/lab.conf), and
any of them can be overridden from the environment (for example `VM_A_CPUS=16 make vm-create`).

## What the scripts change on the servers

All of it is idempotent, and everything Flaminio-specific is named or tagged `flaminio-lab`.

| Step | Change | Undo |
|---|---|---|
| `host-prep` | Installs `qemu-system-x86 qemu-utils libvirt-daemon-system libvirt-clients cloud-image-utils` where missing. Adds the user to the `libvirt` and `kvm` groups. Defines and autostarts the `default` storage pool and network if needed | Remove the packages manually |
| `host-prep` | ufw: allows traffic in on `virbr0` (DHCP/DNS for VMs) and, where routed traffic is denied by default, routes in and out of `virbr0` | `sudo ufw status numbered`, then delete the `flaminio-lab` rules |
| `vm-create` | Volumes `fl-*-root.qcow2`, `fl-*-data.img`, `fl-*-seed.iso` and the cached `flaminio-resolute-base.qcow2` in the `default` pool. A DHCP reservation in the `default` network. The domain itself. `~/flaminio-lab/` | `make vm-destroy CONFIRM=yes` (keeps the base image) |
| `net-setup` | On rhythm-backup: one `nat PREROUTING` DNAT rule and one `FORWARD` accept rule, both restricted to source 147.52.71.227 and port 7410 | `make net-teardown` |

The `net-setup` iptables rules are **not persistent**. After rhythm-backup reboots, run
`make net-setup` again (it is idempotent). `make net-check` tells you whether the path works.

## Security notes

- The replication port is reachable only from rhythm's address. This was checked: a
  connection from elsewhere does not get through.
- VMs accept SSH key logins only (no passwords). The `ubuntu` user has passwordless sudo inside
  the VM.
- The VMs sit on each host's NAT network, so nothing else on the campus network can reach them.
- Replication traffic in the MVP is **not yet encrypted**. mTLS comes with the replication
  protocol (ARCHITECTURE §15.2). Until then, don't put real data in the lab.

## Troubleshooting

| Symptom | Check |
|---|---|
| `bash >= 4.4 required` | `brew install bash`, and make sure `/opt/homebrew/bin` comes before `/bin` in `PATH` |
| `make net-check` fails | `scripts/lab.sh net-setup` again. Then `ENV_VARS=… scripts/host/net-forward.sh status` on rhythm-backup, or simply look for `flaminio-lab` in `sudo iptables -S FORWARD` |
| VM has no network | `make vm-console VM=a` (exit with `Ctrl-]`), and check `sudo ufw status` on the host |
| VM unreachable after it was recreated | It got a new host key. `make vm-destroy` cleans `.lab/known_hosts`; otherwise run `ssh-keygen -R <ip> -f .lab/known_hosts` |
