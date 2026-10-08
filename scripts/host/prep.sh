#!/usr/bin/env bash
# One-time host preparation (idempotent): KVM/libvirt packages, group membership, storage pool,
# NAT network, and ufw rules so VMs on the libvirt bridge get DHCP, DNS and outbound traffic.
set -euo pipefail
export LC_ALL=C  # parsed command output must not be localized
# Note: `grep ... >/dev/null`, never `grep -q`, after a pipe: -q exits early and, with pipefail,
# the SIGPIPE in the writer makes a successful match look like a failure.
say() { printf '  [%s] %s\n' "$(hostname)" "$*"; }

[[ -e /dev/kvm ]] || { echo "no /dev/kvm on $(hostname): enable VT-x/AMD-V" >&2; exit 1; }

need=()
for p in qemu-system-x86 qemu-utils libvirt-daemon-system libvirt-clients cloud-image-utils; do
    dpkg -s "$p" >/dev/null 2>&1 || need+=("$p")
done
if ((${#need[@]})); then
    say "installing ${need[*]}"
    sudo apt-get update -qq
    sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends "${need[@]}" >/dev/null
fi

for g in libvirt kvm; do
    if ! id -nG "$USER" | grep -w "$g" >/dev/null; then
        sudo usermod -aG "$g" "$USER"
        say "added $USER to group $g"
    fi
done

V="sudo virsh -c qemu:///system"
if ! $V pool-info "$LIBVIRT_POOL" >/dev/null 2>&1; then
    say "defining storage pool $LIBVIRT_POOL at $LIBVIRT_POOL_PATH"
    $V pool-define-as "$LIBVIRT_POOL" dir --target "$LIBVIRT_POOL_PATH" >/dev/null
    $V pool-build "$LIBVIRT_POOL" >/dev/null
fi
$V pool-info "$LIBVIRT_POOL" | grep 'State:.*running' >/dev/null || $V pool-start "$LIBVIRT_POOL" >/dev/null
$V pool-autostart "$LIBVIRT_POOL" >/dev/null

$V net-info "$LIBVIRT_NET" >/dev/null 2>&1 || { echo "libvirt network $LIBVIRT_NET is missing" >&2; exit 1; }
$V net-info "$LIBVIRT_NET" | grep 'Active:.*yes' >/dev/null || $V net-start "$LIBVIRT_NET" >/dev/null
$V net-autostart "$LIBVIRT_NET" >/dev/null

if sudo ufw status 2>/dev/null | grep 'Status: active' >/dev/null; then
    sudo ufw allow in on "$LIBVIRT_BRIDGE" comment 'flaminio-lab: libvirt DHCP/DNS' >/dev/null
    if sudo ufw status verbose | grep 'deny (routed)' >/dev/null; then
        sudo ufw route allow in on "$LIBVIRT_BRIDGE" comment 'flaminio-lab: VM egress' >/dev/null
        sudo ufw route allow out on "$LIBVIRT_BRIDGE" comment 'flaminio-lab: VM return traffic' >/dev/null
    fi
    say "ufw: traffic on $LIBVIRT_BRIDGE allowed"
fi

mkdir -p "$HOME/$HOST_STATE_DIR"
say "ready: $(virsh --version 2>/dev/null || true) libvirt, pool $LIBVIRT_POOL, network $LIBVIRT_NET ($($V net-dumpxml "$LIBVIRT_NET" | grep -o "ip address='[^']*'" | head -1))"
