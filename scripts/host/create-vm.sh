#!/usr/bin/env bash
# Creates and starts one VM (idempotent). Expects user-data, meta-data and domain.xml in
# ~/$HOST_STATE_DIR/$VM/, uploaded by lab.sh.
set -euo pipefail
export LC_ALL=C  # parsed command output must not be localized
# Note: `grep ... >/dev/null`, never `grep -q`, after a pipe: -q exits early and, with pipefail,
# the SIGPIPE in the writer makes a successful match look like a failure.
say() { printf '  [%s] %s\n' "$(hostname)" "$*"; }
V="virsh -c qemu:///system"
dir="$HOME/$HOST_STATE_DIR/$VM"
have_vol() { $V vol-info --pool "$LIBVIRT_POOL" "$1" >/dev/null 2>&1; }

if $V dominfo "$VM" >/dev/null 2>&1; then
    say "$VM already exists ($($V domstate "$VM"))"
    exit 0
fi

base="flaminio-$UBUNTU_CODENAME-base.qcow2"
if ! have_vol "$base"; then
    img="$HOME/$HOST_STATE_DIR/$UBUNTU_CODENAME-cloudimg.img"
    if [[ ! -f $img ]]; then
        say "downloading $IMAGE_URL"
        curl -fsSL --retry 3 -o "$img.part" "$IMAGE_URL"
        mv "$img.part" "$img"
    fi
    say "uploading base image into pool $LIBVIRT_POOL"
    $V vol-create-as "$LIBVIRT_POOL" "$base" "$(stat -c %s "$img")" --format qcow2 >/dev/null
    $V vol-upload --pool "$LIBVIRT_POOL" "$base" "$img"
    $V pool-refresh "$LIBVIRT_POOL" >/dev/null
fi

if ! have_vol "$VM-root.qcow2"; then
    say "root disk: ${VM_ROOT_GIB} GiB qcow2 over $base"
    $V vol-create-as "$LIBVIRT_POOL" "$VM-root.qcow2" "${VM_ROOT_GIB}G" --format qcow2 \
        --backing-vol "$base" --backing-vol-format qcow2 >/dev/null
fi
if ! have_vol "$VM-data.img"; then
    say "data disk: ${VM_DATA_GIB} GiB raw, preallocated"
    $V vol-create-as "$LIBVIRT_POOL" "$VM-data.img" "${VM_DATA_GIB}G" --format raw >/dev/null
fi

cloud-localds "$dir/seed.iso" "$dir/user-data" "$dir/meta-data"
have_vol "$VM-seed.iso" && $V vol-delete --pool "$LIBVIRT_POOL" "$VM-seed.iso" >/dev/null
$V vol-create-as "$LIBVIRT_POOL" "$VM-seed.iso" "$(stat -c %s "$dir/seed.iso")" --format raw >/dev/null
$V vol-upload --pool "$LIBVIRT_POOL" "$VM-seed.iso" "$dir/seed.iso"

if ! $V net-dumpxml "$LIBVIRT_NET" | grep -i "mac='$VM_MAC'" >/dev/null; then
    say "DHCP reservation $VM_MAC -> $VM_IP"
    $V net-update "$LIBVIRT_NET" add-last ip-dhcp-host "<host mac='$VM_MAC' name='$VM' ip='$VM_IP'/>" --live --config >/dev/null
fi

$V define "$dir/domain.xml" >/dev/null
$V start "$VM" >/dev/null
say "$VM started: ${VM_CPUS} vCPU, ${VM_MEM_GIB} GiB, $VM_IP"
