# shellcheck shell=bash
# Prepended to every guest script (scripts/guest/*.sh) by run_on_vm; runs inside the lab VMs.
set -euo pipefail

log() { printf '\033[1;34m[%s]\033[0m %s\n' "$(hostname)" "$*" >&2; }
die() { printf '\033[1;31m[%s] error:\033[0m %s\n' "$(hostname)" "$*" >&2; exit 1; }

# The dedicated Flaminio data disk: the whole disk whose serial contains "data"
# (libvirt sets "flaminio-data", LXD sets "lxd_data").
data_disk() {
    local disk
    disk=$(lsblk -dn -o PATH,SERIAL | awk '$2 ~ /data/ {print $1; exit}')
    [[ -n $disk ]] || die "no data disk found (lsblk: $(lsblk -dn -o PATH,SERIAL,SIZE | tr '\n' ';'))"
    echo "$disk"
}

export PATH="$HOME/.cargo/bin:$PATH"
