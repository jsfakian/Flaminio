#!/usr/bin/env bash
# Removes one VM and its disks, seed and DHCP reservation. Keeps the cached base image.
set -euo pipefail
export LC_ALL=C  # parsed command output must not be localized
# Note: `grep ... >/dev/null`, never `grep -q`, after a pipe: -q exits early and, with pipefail,
# the SIGPIPE in the writer makes a successful match look like a failure.
say() { printf '  [%s] %s\n' "$(hostname)" "$*"; }
V="virsh -c qemu:///system"

if $V dominfo "$VM" >/dev/null 2>&1; then
    [[ $($V domstate "$VM") == "shut off" ]] || $V destroy "$VM" >/dev/null
    $V undefine "$VM" >/dev/null
    say "undefined $VM"
fi
for vol in "$VM-root.qcow2" "$VM-data.img" "$VM-seed.iso"; do
    if $V vol-info --pool "$LIBVIRT_POOL" "$vol" >/dev/null 2>&1; then
        $V vol-delete --pool "$LIBVIRT_POOL" "$vol" >/dev/null
        say "deleted volume $vol"
    fi
done
if $V net-dumpxml "$LIBVIRT_NET" | grep -i "mac='$VM_MAC'" >/dev/null; then
    $V net-update "$LIBVIRT_NET" delete ip-dhcp-host "<host mac='$VM_MAC' name='$VM' ip='$VM_IP'/>" --live --config >/dev/null
    say "removed DHCP reservation for $VM_MAC"
fi
rm -rf "${HOME:?}/$HOST_STATE_DIR/$VM"
