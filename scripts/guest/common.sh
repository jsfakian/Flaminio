# shellcheck shell=bash
# Prepended to every guest script (scripts/guest/*.sh) by run_on_vm and run_detached; runs inside
# the lab VMs.
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

# Sends one command to the node's control socket and prints the answer. The socket's path goes to
# python3 as an argument: sudo resets the environment. CONTROL_SOCKET overrides it only under
# FLAMINIO_SCRIPT_TEST=1 (scripts/tools/node-scripts-test.sh).
control() {
    local sock=/run/flaminio/node.sock
    [[ ${FLAMINIO_SCRIPT_TEST:-0} != 1 ]] || sock=${CONTROL_SOCKET:-$sock}
    sudo python3 -c 'import socket, sys
s = socket.socket(socket.AF_UNIX)
s.settimeout(180)
s.connect(sys.argv[2])
s.sendall((sys.argv[1] + "\n").encode())
print(s.makefile().readline().strip())' "$1" "$sock"
}

export PATH="$HOME/.cargo/bin:$PATH"
