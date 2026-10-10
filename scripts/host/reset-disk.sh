#!/usr/bin/env bash
# The host's part of resetting a VM's data disk to an empty state (docs/performance-plan.md,
# W0 option 3), after the guest discarded the whole disk (QEMU punched holes in the image):
# trims the filesystem that holds the image, so the holes reach the drive, then preallocates the
# image again, as when the VM was created. Also reports the drive's counters.
# Usage: reset-disk.sh reset | counters        IMAGE: the image file
set -euo pipefail
export LC_ALL=C
: "${IMAGE:?}"
drive() { # the NVMe or SATA device under the image's filesystem
    local src
    src=$(findmnt -n -o SOURCE --target "$IMAGE")
    lsblk -no PKNAME "$src" | head -1
}
counters() {
    local d attrs
    d=/dev/$(drive)
    attrs=$(sudo -n smartctl -A "$d" 2>/dev/null || true)
    # NVMe: data units of 512 000 bytes; SATA: LBAs written (512 bytes), when reported.
    local units temp
    units=$(awk -F: '/^Data Units Written/ {gsub(/\[.*|[^0-9]/, "", $2); print $2; exit}' <<<"$attrs")
    temp=$(awk -F: '/^Temperature:/ {gsub(/[^0-9]/, "", $2); print $2; exit}' <<<"$attrs")
    echo "${units:--} ${temp:--}"
}
case "${1:-counters}" in
    reset)
        size=$(stat -c %s "$IMAGE")
        sudo -n fstrim "$(findmnt -n -o TARGET --target "$IMAGE")"
        sudo -n fallocate -l "$size" "$IMAGE"
        echo "reset $(basename "$IMAGE"): trimmed and preallocated again ($((size >> 30)) GiB)" >&2
        ;;
    counters) counters ;;
    *) echo "usage: reset-disk.sh reset|counters" >&2; exit 2 ;;
esac
